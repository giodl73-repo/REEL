//! Owned HTTP/TLS protocol fixtures; service acceptance is a separate explicit run.
#[allow(dead_code)]
#[path = "../../tig-git/tests/remote_lfs/support.rs"]
mod wire;
use std::{io::Read, time::Duration};
use tig_core::{Digest, Object};
use tig_store::{
    AckLevel, Key, Request, accounted,
    acquire::{self, SourceError},
    ledger::CapacityLedger,
    local::LocalStore,
    provider::{
        self, ExactSource, Located, Lookup, ReadUse, Version,
        s3_read::{Config, Limits, Source},
    },
};
use wire::{Reply, Server};
fn d(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn k(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn reply(request: &wire::Request, bytes: &[u8], version: &str) -> Reply {
    assert!(request.path.starts_with("/bucket/object?versionId=v1"));
    assert_eq!(request.headers.get("accept-encoding").unwrap(), "identity");
    let mut r = Reply::bytes(if request.method == "HEAD" {
        vec![]
    } else {
        bytes.to_vec()
    });
    r.declared = Some(bytes.len());
    r.headers.push(("x-amz-version-id".into(), version.into()));
    r
}
fn config(s: &Server, object: Object, limits: Limits) -> Config {
    let url = format!(
        "{}/bucket/object?versionId=v1&X-Amz-Signature=synthetic",
        s.root
    );
    if let Some(cert) = &s.cert {
        Config::https(d("S3"), object, "v1", &url, &url, limits)
            .unwrap()
            .trust_pem(cert.as_bytes())
            .unwrap()
    } else {
        Config::loopback_http(d("S3"), object, "v1", &url, &url, limits).unwrap()
    }
}
fn source(s: &Server, bytes: &[u8], limits: Limits) -> Source {
    config(s, Object::from_bytes(bytes), limits)
        .connect()
        .unwrap()
}
fn open(s: &mut Source, bytes: &[u8]) -> Result<provider::s3_read::Stream, SourceError> {
    provider::open_exact(
        s,
        &d("S3"),
        &Object::from_bytes(bytes),
        ReadUse::Protected {
            allow_cooperative: false,
        },
    )
}
#[test]
fn https_exact_version_streams_to_native_accounting_and_warm_reopen_needs_zero_requests() {
    let data = vec![0x51; 1024 * 1024 + 17];
    let body = data.clone();
    let server = Server::new(true, move |r, _| reply(r, &body, "v1"));
    let mut source = source(&server, &data, Limits::default());
    assert_eq!(server.count(), 0);
    let temp = tempfile::tempdir().unwrap();
    let mut store = LocalStore::create(&temp.path().join("M"), k("M")).unwrap();
    let request = Request {
        operation: k("copy"),
        owner: k("fixture"),
        object: Object::from_bytes(&data),
        required: AckLevel::ProcessRestart,
    };
    let plan = accounted::plan(&store, &request).unwrap();
    let mut ledger =
        CapacityLedger::create(&temp.path().join("ledger"), k("volume"), k("epoch"), 0).unwrap();
    let probe = ledger.begin_probe(k("probe")).unwrap();
    ledger
        .observe(k("sample"), probe.reference(), 1 << 30)
        .unwrap();
    let job = ledger
        .reserve(k("job"), k("fixture"), plan.required_peak())
        .unwrap()
        .reference()
        .clone();
    let budget = provider::budget::Budget::new(provider::budget::Limits {
        calls: 2,
        read_calls: 1024,
        source_bytes: data.len() as u64 + 1,
        live_streams: 1,
        read_chunk: 65_536,
        elapsed: Duration::from_secs(60),
    })
    .unwrap();
    let mut cold_stats = None;
    let binding = provider::profile::Binding {
        place: d("S3"),
        authority: d("owned HTTPS fixture binding"),
        generation: 1,
    };
    let profile = provider::profile::Profile {
        schema: provider::profile::Schema::Experimental1,
        binding: binding.clone(),
        adapter: k("s3-read"),
        adapter_build: d("fixture adapter build"),
        provider: k("owned-https-fixture"),
        provider_build: d("fixture server build"),
        read: provider::profile::Qualification::Qualified {
            capabilities: source.describe(),
            evidence: d("owned fixture read evidence only"),
        },
        publish: provider::profile::Qualification::Unsupported,
        custody: provider::profile::Qualification::Unsupported,
        issue: provider::profile::Qualification::Unsupported,
        release: provider::profile::Qualification::Unsupported,
        remove: provider::profile::Qualification::Unsupported,
    };
    let guard = profile
        .admit(&profile.fingerprint().unwrap(), &binding)
        .unwrap();
    for warm in [false, true] {
        let mut destination =
            provider::publication::native::Accounted::new(&store, &mut ledger, &job, &plan);
        acquire::publish_placed(
            &mut provider::Source::new(
                &mut provider::budget::Source::new(
                    &mut provider::profile::Source::new(&mut source, &guard),
                    budget.clone(),
                ),
                d("S3"),
                ReadUse::Transfer,
            ),
            &mut destination,
            store.identity(),
            &request,
        )
        .unwrap();
        assert_eq!(source.stats().unwrap().requests_charged, 2);
        let stats = budget.stats().unwrap();
        assert_eq!(
            (
                stats.lookups,
                stats.opens,
                stats.source_bytes_observed,
                stats.source_bytes_charged,
                stats.live_streams
            ),
            (1, 1, data.len() as u64, data.len() as u64, 0)
        );
        if warm {
            assert_eq!(Some(stats), cold_stats);
        } else {
            cold_stats = Some(stats);
        }
        if !warm {
            store = LocalStore::open(&temp.path().join("M")).unwrap();
            ledger =
                CapacityLedger::open(&temp.path().join("ledger"), &ledger.checkpoint()).unwrap();
        }
    }
    assert_eq!(
        source.stats().unwrap().transfer_bytes_charged,
        data.len() as u64
    );
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "HEAD");
    assert_eq!(requests[1].method, "GET");
}
#[test]
fn version_changes_missing_tokens_and_partial_representations_refuse() {
    for mode in 0..6 {
        let server = Server::new(false, move |req, _| {
            let mut r = reply(
                req,
                b"abc",
                if req.method == "GET" && mode == 0 {
                    "v2"
                } else {
                    "v1"
                },
            );
            if mode == 1 {
                r.headers.clear()
            }
            if mode == 2 {
                r.headers
                    .push(("Content-Range".into(), "bytes 0-2/3".into()))
            }
            if mode == 3 {
                r.headers.push(("Content-Encoding".into(), "gzip".into()))
            }
            if mode == 4 {
                r.declared = Some(4)
            }
            if mode == 5 {
                r.headers
                    .push(("x-amz-delete-marker".into(), "true".into()))
            }
            r
        });
        let mut s = source(&server, b"abc", Limits::default());
        assert!(open(&mut s, b"abc").is_err());
        assert_eq!(s.stats().unwrap().active_streams, 0);
    }
}
#[test]
fn denied_absent_archive_busy_and_redirect_are_not_success_or_latest_fallback() {
    for (status, expected) in [
        (403, SourceError::Denied),
        (404, SourceError::Unavailable),
        (405, SourceError::Unavailable),
        (412, SourceError::Stale),
        (429, SourceError::Busy),
        (503, SourceError::Unavailable),
        (302, SourceError::Denied),
    ] {
        let server = Server::new(false, move |req, root| {
            let mut r = reply(req, b"abc", "v1");
            r.status = status;
            r.headers
                .push(("Location".into(), format!("{root}/latest")));
            r
        });
        let mut s = source(&server, b"abc", Limits::default());
        assert_eq!(s.lookup(&Object::from_bytes(b"abc")).unwrap_err(), expected);
        assert_eq!(server.count(), 1);
    }
}
#[test]
fn malformed_or_unversioned_private_bindings_refuse_without_io_and_debug_is_redacted() {
    let base = "https://s3.example/bucket/object";
    for query in [
        "",
        "?versionId=null",
        "?versionId=v2",
        "?versionId=v1&versionId=v1",
        "?%76ersionId=v1&versionId=v1",
    ] {
        assert!(
            Config::https(
                d("S3"),
                Object::from_bytes(b"abc"),
                "v1",
                &format!("{base}{query}"),
                &format!("{base}{query}"),
                Limits::default()
            )
            .is_err()
        );
    }
    let valid = format!("{base}?versionId=v1&X-Amz-Signature=PRIVATE");
    assert!(
        Config::https(
            d("S3"),
            Object::from_bytes(b"abc"),
            "null",
            &valid,
            &valid,
            Limits::default()
        )
        .is_err()
    );
    assert!(
        Config::loopback_http(
            d("S3"),
            Object::from_bytes(b"abc"),
            "v1",
            "http://example.com/o?versionId=v1",
            "http://example.com/o?versionId=v1",
            Limits::default()
        )
        .is_err()
    );
    let s = Config::https(
        d("S3"),
        Object::from_bytes(b"abc"),
        "v1",
        &valid,
        &valid,
        Limits::default(),
    )
    .unwrap()
    .connect()
    .unwrap();
    assert!(!format!("{s:?}").contains("PRIVATE"));
    assert_eq!(s.stats().unwrap().requests_charged, 0);
}
#[test]
fn budgets_foreign_locations_and_expiry_refuse_before_unbounded_io() {
    let server = Server::new(false, |r, _| reply(r, b"abc", "v1"));
    let mut s = source(
        &server,
        b"abc",
        Limits {
            max_requests: 1,
            ..Limits::default()
        },
    );
    assert_eq!(open(&mut s, b"abc").err().unwrap(), SourceError::Budget);
    assert_eq!(server.count(), 1);
    assert_eq!(s.stats().unwrap().active_streams, 0);
    let mut s = source(
        &server,
        b"abc",
        Limits {
            max_streams: 1,
            max_total_bytes: 3,
            ..Limits::default()
        },
    );
    let mut reader = open(&mut s, b"abc").unwrap();
    assert_eq!(open(&mut s, b"abc").err().unwrap(), SourceError::Busy);
    let mut data = Vec::new();
    reader.read_to_end(&mut data).unwrap();
    drop(reader);
    assert_eq!(data, b"abc");
    assert_eq!(open(&mut s, b"abc").err().unwrap(), SourceError::Budget);
    let mut s = source(&server, b"abc", Limits::default());
    let foreign = Located {
        place: d("other"),
        object: Object::from_bytes(b"abc"),
        version: Version::new(b"v1").unwrap(),
    };
    assert_eq!(s.open_version(&foreign).err().unwrap(), SourceError::Stale);
    assert_eq!(s.stats().unwrap().requests_charged, 0);
    let mut s = source(
        &server,
        b"abc",
        Limits {
            request_timeout: Duration::from_millis(1),
            session_timeout: Duration::from_millis(1),
            ..Limits::default()
        },
    );
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(
        s.lookup(&Object::from_bytes(b"abc")).unwrap_err(),
        SourceError::Budget
    );
}
#[test]
fn metadata_does_not_verify_bytes_and_truncation_is_not_clean_eof() {
    for truncated in [false, true] {
        let server = Server::new(false, move |req, _| {
            let mut r = reply(req, b"xyz", "v1");
            if truncated && req.method == "GET" {
                r.body.truncate(2)
            }
            r
        });
        let mut s = source(&server, b"abc", Limits::default());
        assert!(matches!(
            s.lookup(&Object::from_bytes(b"abc")).unwrap(),
            Lookup::Present(_)
        ));
        let mut stream = open(&mut s, b"abc").unwrap();
        let mut bytes = Vec::new();
        let result = stream.read_to_end(&mut bytes);
        if truncated {
            assert!(result.is_err())
        } else {
            assert_ne!(Object::from_bytes(&bytes), Object::from_bytes(b"abc"))
        }
        drop(stream);
        let temp = tempfile::tempdir().unwrap();
        let mut destination = LocalStore::create(&temp.path().join("M"), k("M")).unwrap();
        let request = Request {
            operation: k("bad-copy"),
            owner: k("fixture"),
            object: Object::from_bytes(b"abc"),
            required: AckLevel::ProcessRestart,
        };
        assert!(
            acquire::publish(
                &mut provider::Source::new(&mut s, d("S3"), ReadUse::Transfer),
                &mut destination,
                &request
            )
            .is_err()
        );
        assert!(
            !temp
                .path()
                .join("M/objects")
                .join(request.object.sha256.as_str())
                .exists()
        );
    }
}

#[test]
fn invalid_tls_and_stalled_payload_refuse_without_unbounded_retries() {
    let server = Server::new(true, |r, _| reply(r, b"abc", "v1"));
    let url = format!("{}/bucket/object?versionId=v1", server.root);
    let mut untrusted = Config::https(
        d("S3"),
        Object::from_bytes(b"abc"),
        "v1",
        &url,
        &url,
        Limits::default(),
    )
    .unwrap()
    .connect()
    .unwrap();
    assert_eq!(
        untrusted.lookup(&Object::from_bytes(b"abc")).unwrap_err(),
        SourceError::Unavailable
    );
    assert_eq!(untrusted.stats().unwrap().requests_charged, 1);
    let server = Server::new(false, |req, _| {
        let mut r = reply(req, b"abc", "v1");
        if req.method == "GET" {
            r.body_delay = Duration::from_millis(150)
        }
        r
    });
    let mut s = source(
        &server,
        b"abc",
        Limits {
            request_timeout: Duration::from_millis(50),
            ..Limits::default()
        },
    );
    let mut stream = open(&mut s, b"abc").unwrap();
    let mut data = Vec::new();
    assert!(stream.read_to_end(&mut data).is_err());
    drop(stream);
    assert_eq!(s.stats().unwrap().requests_charged, 2);
    assert_eq!(s.stats().unwrap().active_streams, 0);
}
