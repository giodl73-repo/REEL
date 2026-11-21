//! Real sockets and ephemeral synthetic TLS; no live endpoint/profile/credential lookup.
mod common;
#[path = "remote_lfs/support.rs"]
mod support;
use std::{io::Read, time::Duration};
use support::{Reply, Server, batch};
use tig_core::{Digest, Object};
use tig_git::remote_lfs::{Config, Limits, Secret, Source};
use tig_store::{
    AckLevel, Key, Request, accounted,
    acquire::{self, ObjectSource, SourceError},
    ledger::{CapacityLedger, JournalRef},
    local::LocalStore,
    provider::publication,
};
fn k(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn d(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn source(server: &Server, limits: Limits) -> Source {
    let config = if let Some(cert) = &server.cert {
        Config::https(d("remote"), &server.endpoint(), limits)
            .unwrap()
            .trust_pem(cert.as_bytes())
            .unwrap()
    } else {
        Config::loopback_http(d("remote"), &server.endpoint(), limits).unwrap()
    };
    config
        .batch_authorization(Secret::new("Bearer synthetic-batch").unwrap())
        .connect()
        .unwrap()
}
struct Destination {
    temp: tempfile::TempDir,
    store: LocalStore,
    ledger: CapacityLedger,
    job: JournalRef,
    plan: accounted::PublicationPlan,
    request: Request,
}
impl Destination {
    fn new(object: Object) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let store = LocalStore::create(&temp.path().join("store"), k("M")).unwrap();
        let request = Request {
            operation: k("remote-copy"),
            owner: k("fixture"),
            object,
            required: AckLevel::ProcessRestart,
        };
        let plan = accounted::plan(&store, &request).unwrap();
        let mut ledger =
            CapacityLedger::create(&temp.path().join("ledger"), k("volume"), k("epoch"), 0)
                .unwrap();
        let p = ledger.begin_probe(k("probe")).unwrap();
        ledger.observe(k("sample"), p.reference(), 1 << 30).unwrap();
        let job = ledger
            .reserve(k("job"), k("fixture"), plan.required_peak())
            .unwrap()
            .reference()
            .clone();
        Self {
            temp,
            store,
            ledger,
            job,
            plan,
            request,
        }
    }
    fn publish(
        &mut self,
        source: &mut Source,
    ) -> Result<
        publication::Published<accounted::AccountedReceipt>,
        acquire::AcquireError<publication::Error>,
    > {
        let mut publisher = publication::native::Accounted::new(
            &self.store,
            &mut self.ledger,
            &self.job,
            &self.plan,
        );
        acquire::publish_placed(source, &mut publisher, self.store.identity(), &self.request)
    }
    fn reopen(&mut self) {
        self.store = LocalStore::open(&self.temp.path().join("store")).unwrap();
        self.ledger =
            CapacityLedger::open(&self.temp.path().join("ledger"), &self.ledger.checkpoint())
                .unwrap();
    }
}
#[test]
fn verified_https_streams_into_original_native_accounting_and_warm_reopen_uses_zero_network() {
    let bytes = vec![0x5a; 1024 * 1024 + 17];
    let object = Object::from_bytes(&bytes);
    let server = Server::good(bytes.clone(), true);
    let mut remote = source(&server, Limits::default());
    assert_eq!(server.count(), 0);
    assert_eq!(remote.stats().unwrap().requests_charged, 0);
    let mut destination = Destination::new(object.clone());
    let published = destination.publish(&mut remote).unwrap();
    assert_eq!(
        published.evidence.publication().request(),
        &destination.request
    );
    assert_eq!(remote.stats().unwrap().requests_charged, 2);
    assert_eq!(remote.stats().unwrap().transfer_bytes_charged, object.bytes);
    assert_eq!(remote.stats().unwrap().active_streams, 0);
    {
        let r = server.requests.lock().unwrap();
        assert_eq!(r[0].method, "POST");
        assert_eq!(r[0].path, "/objects/batch");
        let body: serde_json::Value = serde_json::from_slice(&r[0].body).unwrap();
        assert_eq!(body["objects"][0]["oid"], object.sha256.as_str());
        assert_eq!(r[0].headers["authorization"], "Bearer synthetic-batch");
        assert_eq!(r[1].headers["authorization"], "Bearer synthetic-download");
    }
    let mut actual = Vec::new();
    destination
        .store
        .read_object(&object)
        .unwrap()
        .read_to_end(&mut actual)
        .unwrap();
    assert_eq!(actual, bytes);
    let endpoint = server.endpoint();
    let cert = server.cert.clone().unwrap();
    drop(server);
    destination.reopen();
    let mut offline = Config::https(
        d("remote"),
        &endpoint,
        Limits {
            max_object_bytes: 0,
            ..Limits::default()
        },
    )
    .unwrap()
    .trust_pem(cert.as_bytes())
    .unwrap()
    .connect()
    .unwrap();
    assert_eq!(
        destination.publish(&mut offline).unwrap().version,
        published.version
    );
    assert_eq!(offline.stats().unwrap().requests_charged, 0);
}
#[test]
fn public_catalogue_git_snapshot_downloads_exact_selected_bytes_into_m() {
    use tig_core::LogicalPath;
    use tig_git::{
        pointer::Pointer,
        source::{GitId, Repository, Scope},
    };
    let catalogue: serde_json::Value =
        serde_json::from_str(include_str!("../../../examples/teaching/catalogue.json")).unwrap();
    let asset = catalogue["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "fixed")
        .unwrap();
    let bytes = asset["content"].as_str().unwrap().as_bytes().to_vec();
    let object = Object::from_bytes(&bytes);
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    common::git(root, &["-c", "init.templateDir=", "init", "-b", "main"]);
    common::git(root, &["config", "core.hooksPath", "no-hooks"]);
    common::git(root, &["config", "user.name", "Synthetic fixture"]);
    common::git(root, &["config", "user.email", "fixture@example.invalid"]);
    std::fs::create_dir(root.join("assets")).unwrap();
    let path = root.join("assets/opening.bin");
    std::fs::write(&path, Pointer::new(object.clone()).to_bytes()).unwrap();
    common::git(root, &["add", "assets"]);
    common::git(root, &["commit", "-m", "synthetic golden"]);
    let head = common::git(root, &["rev-parse", "HEAD"]);
    let id = GitId::parse(std::str::from_utf8(&head.stdout).unwrap().trim()).unwrap();
    let reader = Repository::new(
        common::git_executable(),
        root.into(),
        tig_git::source::Limits::default(),
    )
    .unwrap();
    let snapshot = reader.commit(id).unwrap();
    std::fs::write(&path, b"uncommitted unrelated bytes").unwrap();
    let selected = reader
        .extract(
            &snapshot,
            Scope::new([LogicalPath::parse("assets").unwrap()]).unwrap(),
        )
        .unwrap();
    assert!(selected.complete());
    assert_eq!(selected.bindings().len(), 1);
    assert_eq!(selected.bindings()[0].object, object);
    let server = Server::good(bytes, true);
    let mut remote = source(&server, Limits::default());
    let mut destination = Destination::new(selected.bindings()[0].object.clone());
    destination.publish(&mut remote).unwrap();
    destination.reopen();
    let mut actual = Vec::new();
    destination
        .store
        .read_object(&object)
        .unwrap()
        .read_to_end(&mut actual)
        .unwrap();
    assert_eq!(Object::from_bytes(&actual), object);
    assert_eq!(server.count(), 2);
}
#[test]
fn untrusted_tls_refuses_and_explicit_fixture_trust_keeps_verification_enabled() {
    let bytes = b"public TLS bytes".to_vec();
    let object = Object::from_bytes(&bytes);
    let server = Server::good(bytes, true);
    let mut untrusted = Config::https(d("remote"), &server.endpoint(), Limits::default())
        .unwrap()
        .connect()
        .unwrap();
    assert!(untrusted.open(&object).is_err());
    assert_eq!(server.count(), 0);
    let mut trusted = source(&server, Limits::default());
    let mut actual = Vec::new();
    trusted
        .open(&object)
        .unwrap()
        .read_to_end(&mut actual)
        .unwrap();
    assert_eq!(Object::from_bytes(&actual), object);
}
#[test]
fn cross_origin_actions_and_redirects_never_receive_batch_credentials_without_admission() {
    let bytes = b"public origin bytes".to_vec();
    let object = Object::from_bytes(&bytes);
    let trap = Server::good(bytes.clone(), false);
    for redirect in [false, true] {
        let target = format!("{}/trap", trap.root);
        let object = object.clone();
        let server = Server::new(false, move |_, root| {
            if redirect {
                let mut r = Reply::bytes(vec![]);
                r.status = 302;
                r.headers.push(("Location".into(), target.clone()));
                r
            } else {
                let mut value = batch(root, &object);
                value["objects"][0]["actions"]["download"]["href"] = target.clone().into();
                Reply::json(value)
            }
        });
        let mut remote = source(&server, Limits::default());
        assert!(remote.open(&Object::from_bytes(&bytes)).is_err());
        assert_eq!(server.count(), 1);
        assert_eq!(trap.count(), 0);
    }
}
#[test]
fn explicitly_admitted_download_origin_uses_its_own_credentials() {
    let bytes = b"public origin bytes".to_vec();
    let object = Object::from_bytes(&bytes);
    let trap = Server::good(bytes.clone(), false);
    let target = format!("{}/download", trap.root);
    let object2 = object.clone();
    let server = Server::new(false, move |_, root| {
        let mut value = batch(root, &object2);
        let row = &mut value["objects"][0];
        row["authenticated"] = false.into();
        row["actions"]["download"]["href"] = target.clone().into();
        row["actions"]["download"]
            .as_object_mut()
            .unwrap()
            .remove("header");
        Reply::json(value)
    });
    let mut remote = Config::loopback_http(d("remote"), &server.endpoint(), Limits::default())
        .unwrap()
        .batch_authorization(Secret::new("Bearer synthetic-batch").unwrap())
        .allow_download_origin(&trap.root)
        .unwrap()
        .download_authorization(
            &trap.root,
            Secret::new("Bearer independently-admitted-download").unwrap(),
        )
        .unwrap()
        .connect()
        .unwrap();
    let mut actual = Vec::new();
    remote
        .open(&object)
        .unwrap()
        .read_to_end(&mut actual)
        .unwrap();
    assert_eq!(actual, bytes);
    assert_eq!(
        trap.requests.lock().unwrap()[0].headers["authorization"],
        "Bearer independently-admitted-download"
    );
}
#[test]
fn malformed_identity_algorithms_duplicate_metadata_and_expired_actions_refuse_before_get() {
    let bytes = b"public metadata bytes".to_vec();
    let object = Object::from_bytes(&bytes);
    for case in 0..9 {
        let object = object.clone();
        let server = Server::new(false, move |_, root| {
            let mut value = batch(root, &object);
            match case {
                0 => value["objects"][0]["oid"] = d("wrong").as_str().into(),
                1 => value["objects"][0]["size"] = (object.bytes + 1).into(),
                2 => value["transfer"] = "other".into(),
                3 => value["hash_algo"] = "sha512".into(),
                4 => value["objects"][0]["actions"]["download"]["expires_in"] = 0.into(),
                5 => {
                    value["objects"][0]["actions"]["download"]
                        .as_object_mut()
                        .unwrap()
                        .remove("expires_in");
                    value["objects"][0]["actions"]["download"]["expires_at"] =
                        "1970-01-01T00:00:00Z".into();
                }
                6 => {
                    let mut r = Reply::json(value);
                    r.body = b"{\"objects\":[],\"objects\":[]}".to_vec();
                    return r;
                }
                7 => {
                    value["objects"][0]["actions"]["download"]["header"] =
                        serde_json::json!({"Host":"evil.invalid"})
                }
                _ => {
                    value["objects"][0]["actions"]["download"]["header"] =
                        serde_json::json!({"Authorization":"safe","authorization":"conflicting"})
                }
            }
            Reply::json(value)
        });
        assert!(
            source(&server, Limits::default())
                .open(&Object::from_bytes(&bytes))
                .is_err()
        );
        assert_eq!(server.count(), 1);
    }
}
#[test]
fn status_denial_unavailability_and_per_object_errors_do_not_become_absence_or_custody() {
    let bytes = b"public status bytes".to_vec();
    let object = Object::from_bytes(&bytes);
    for (code, expected) in [
        (401, SourceError::Denied),
        (403, SourceError::Denied),
        (404, SourceError::Unavailable),
        (410, SourceError::Unavailable),
        (503, SourceError::Unavailable),
    ] {
        for per_object in [false, true] {
            let object = object.clone();
            let server = Server::new(false, move |_, _| {
                if per_object {
                    Reply::json(
                        serde_json::json!({"objects":[{"oid":object.sha256.as_str(),"size":object.bytes,"error":{"code":code,"message":"synthetic private message must not escape"}}]}),
                    )
                } else {
                    let mut r = Reply::bytes(vec![]);
                    r.status = code;
                    r
                }
            });
            let result = source(&server, Limits::default()).open(&Object::from_bytes(&bytes));
            assert!(
                matches!(result,Err(e) if e==expected || (per_object&&code==503&&e==SourceError::Unknown))
            );
            assert_eq!(server.count(), 1);
        }
    }
}
#[test]
fn corruption_short_and_excess_streams_never_acknowledge_a_complete_native_object() {
    let bytes = b"public verified bytes".to_vec();
    let object = Object::from_bytes(&bytes);
    for mode in 0..3 {
        let object = object.clone();
        let bytes = bytes.clone();
        let server = Server::new(false, move |r, root| {
            if r.method == "POST" {
                return Reply::json(batch(root, &object));
            }
            let mut r = Reply::bytes(bytes.clone());
            match mode {
                0 => r.body[0] ^= 1,
                1 => {
                    r.declared = Some(r.body.len());
                    r.body.pop();
                }
                _ => {
                    r.body.push(1);
                    r.chunked = true;
                }
            }
            r
        });
        let mut dest = Destination::new(Object::from_bytes(b"public verified bytes"));
        assert!(
            dest.publish(&mut source(&server, Limits::default()))
                .is_err()
        );
        assert!(!matches!(
            tig_store::recover(&mut dest.store, &dest.request),
            Ok(Some(_))
        ));
    }
}

#[test]
fn whole_object_admission_stream_slots_and_read_chunks_are_bounded() {
    let bytes = vec![0x5a; 200_003];
    let object = Object::from_bytes(&bytes);
    let server = Server::good(bytes.clone(), false);
    let limits = Limits {
        max_streams: 1,
        max_total_bytes: object.bytes + 1,
        ..Limits::default()
    };
    let mut remote = source(&server, limits);
    let mut reader = remote.open(&object).unwrap();
    assert_eq!(remote.stats().unwrap().active_streams, 1);
    assert!(matches!(remote.open(&object), Err(SourceError::Busy)));
    assert_eq!(server.count(), 2);
    let mut buffer = vec![0; 200_003];
    let n = reader.read(&mut buffer).unwrap();
    assert!(n <= 65_536);
    let mut rest = Vec::new();
    reader.read_to_end(&mut rest).unwrap();
    let mut actual = buffer[..n].to_vec();
    actual.extend(rest);
    assert_eq!(actual, bytes);
    drop(reader);
    assert_eq!(remote.stats().unwrap().transfer_bytes_charged, object.bytes);
    assert!(matches!(remote.open(&object), Err(SourceError::Budget)));
    assert_eq!(server.count(), 2);
    assert_eq!(remote.stats().unwrap().active_streams, 0);
}

#[test]
fn known_size_request_metadata_and_expired_session_budgets_refuse_without_hidden_retry() {
    let bytes = b"public resource bytes".to_vec();
    let object = Object::from_bytes(&bytes);
    for case in 0..4 {
        let server = Server::good(bytes.clone(), false);
        let mut limits = Limits::default();
        match case {
            0 => limits.max_object_bytes = object.bytes - 1,
            1 => limits.max_requests = 1,
            2 => limits.metadata_bytes = 16,
            _ => {
                limits.request_timeout = Duration::from_nanos(1);
                limits.session_timeout = Duration::from_nanos(1);
            }
        }
        let mut remote = source(&server, limits);
        assert!(matches!(remote.open(&object), Err(SourceError::Budget)));
        assert_eq!(server.count(), if case == 1 || case == 2 { 1 } else { 0 });
        assert_eq!(remote.stats().unwrap().active_streams, 0);
        assert_eq!(remote.stats().unwrap().transfer_bytes_charged, 0);
    }
}

#[test]
fn batch_latency_does_not_refresh_action_expiry_and_relative_expiry_takes_precedence() {
    let bytes = b"public expiry bytes".to_vec();
    let object = Object::from_bytes(&bytes);
    let object2 = object.clone();
    let server = Server::new(false, move |_, root| {
        let mut r = Reply::json(batch(root, &object2));
        r.delay = Duration::from_millis(150);
        r
    });
    let mut remote = source(
        &server,
        Limits {
            action_ttl: Duration::from_millis(50),
            ..Limits::default()
        },
    );
    assert_eq!(remote.open(&object).err(), Some(SourceError::Stale));
    assert_eq!(server.count(), 1);
    for absolute_only in [false, true] {
        let object2 = object.clone();
        let bytes = bytes.clone();
        let server = Server::new(false, move |r, root| {
            if r.method != "POST" {
                return Reply::bytes(bytes.clone());
            }
            let mut value = batch(root, &object2);
            value["objects"][0]["actions"]["download"]["expires_at"] = if absolute_only {
                "9999-12-31T23:59:59Z"
            } else {
                "1970-01-01T00:00:00Z"
            }
            .into();
            if absolute_only {
                value["objects"][0]["actions"]["download"]
                    .as_object_mut()
                    .unwrap()
                    .remove("expires_in");
            }
            Reply::json(value)
        });
        let mut actual = Vec::new();
        source(&server, Limits::default())
            .open(&object)
            .unwrap()
            .read_to_end(&mut actual)
            .unwrap();
        assert_eq!(Object::from_bytes(&actual), object);
    }
}

#[test]
fn body_deadlines_content_encoding_and_download_redirects_fail_closed() {
    let bytes = b"public timeout bytes".to_vec();
    let object = Object::from_bytes(&bytes);
    for case in 0..4 {
        let object2 = object.clone();
        let bytes = bytes.clone();
        let server = Server::new(false, move |r, root| {
            if r.method == "POST" {
                return Reply::json(batch(root, &object2));
            }
            let mut r = Reply::bytes(bytes.clone());
            match case {
                0 => r.body_delay = Duration::from_secs(1),
                1 => r.headers.push(("Content-Encoding".into(), "gzip".into())),
                2 => {
                    r.status = 302;
                    r.headers
                        .push(("Location".into(), format!("{root}/redirected")));
                }
                _ => r.headers.push((
                    "Content-Type".into(),
                    "text/plain; charset=iso-8859-1".into(),
                )),
            }
            r
        });
        let mut remote = source(
            &server,
            Limits {
                request_timeout: Duration::from_millis(300),
                ..Limits::default()
            },
        );
        if case == 0 {
            let mut reader = remote.open(&object).unwrap();
            let error = reader.read(&mut [0]).unwrap_err();
            assert!(!error.to_string().contains("synthetic-action"));
        } else {
            assert!(matches!(
                remote.open(&object),
                Err(SourceError::Unsupported)
            ));
        }
        assert_eq!(server.count(), 2);
    }
}

#[test]
fn endpoint_header_and_secret_profiles_are_explicit_and_redacted() {
    for endpoint in [
        "http://example.invalid/objects/batch",
        "http://localhost/objects/batch",
        "https://user:pass@example.invalid/objects/batch",
        "https://example.invalid/objects/batch#fragment",
        "https://example.invalid:99999/objects/batch",
    ] {
        assert!(Config::https(d("remote"), endpoint, Limits::default()).is_err());
    }
    assert!(
        Config::loopback_http(
            d("remote"),
            "http://[::1]:1/objects/batch",
            Limits::default()
        )
        .is_ok()
    );
    let secret = Secret::new("Bearer synthetic-profile-secret").unwrap();
    assert!(!format!("{secret:?}").contains("synthetic-profile-secret"));
    let config = Config::https(
        d("remote"),
        "https://example.invalid/objects/batch",
        Limits::default(),
    )
    .unwrap();
    assert!(config.allow_action_header("Host").is_err());
    let source = Config::https(
        d("remote"),
        "https://example.invalid/objects/batch",
        Limits::default(),
    )
    .unwrap()
    .batch_authorization(secret)
    .connect()
    .unwrap();
    assert!(!format!("{source:?}").contains("example.invalid"));
    assert_eq!(source.stats().unwrap().requests_charged, 0);
}
