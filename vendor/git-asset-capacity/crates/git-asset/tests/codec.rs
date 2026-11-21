mod common;
use common::Fixture;
use git_asset::{
    cache::{CacheBackend, Layout, Profile, ReadNodes},
    pointer::{Pointer, PointerError},
    protocol::{self, AgentError, DownloadBackend},
};
use git_asset_core::Object;
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Cursor, Write},
    path::PathBuf,
};
fn messages(f: &Fixture) -> Vec<u8> {
    format!("{{\"event\":\"init\",\"operation\":\"download\"}}\n{{\"event\":\"download\",\"oid\":\"{}\",\"size\":{}}}\n{{\"event\":\"terminate\"}}\n",f.objects[0].sha256.as_str(),f.objects[0].bytes).into_bytes()
}
fn replies(bytes: &[u8]) -> Vec<Value> {
    std::str::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
#[test]
fn plain_and_legacy_pointer_roundtrip_preserves_exact_identity() {
    let object = Object::from_bytes(b"AAA");
    let p = Pointer::new(object.clone());
    assert_eq!(Pointer::parse(&p.to_bytes()).unwrap(), p);
    let old = String::from_utf8(p.to_bytes())
        .unwrap()
        .replace("git-lfs.github.com", "hawser.github.com");
    let legacy = Pointer::parse(old.as_bytes()).unwrap();
    assert_eq!(legacy.object(), &object);
    assert_eq!(legacy.to_bytes(), old.as_bytes());
}
#[test]
fn malformed_unsupported_and_ambiguous_pointers_reject() {
    let p = String::from_utf8(Pointer::new(Object::from_bytes(b"AAA")).to_bytes()).unwrap();
    for text in [
        p.replace("size 3", "size 03"),
        p.replace("size 3", "size -3"),
        p.replace("size 3", "size 18446744073709551616"),
        p.replace('\n', "\r\n"),
        p.trim_end().into(),
        format!("{p}ext-0-name ignored\n"),
        p.replace("sha256:", "sha512:"),
        p.replace("spec/v1", "spec/V1"),
    ] {
        assert!(Pointer::parse(text.as_bytes()).is_err());
    }
    assert_eq!(
        Pointer::parse(&vec![b' '; 1025]).unwrap_err(),
        PointerError::Budget
    );
    assert!(Pointer::parse(&[]).is_err());
}
#[test]
fn protocol_returns_disposable_path_and_leaves_source_intact() {
    let f = Fixture::new(&[b"AAA"]);
    let mut backend = CacheBackend::new(f.profile.clone()).unwrap();
    let mut output = Vec::new();
    protocol::serve(Cursor::new(messages(&f)), &mut output, &mut backend).unwrap();
    let r = replies(&output);
    assert_eq!(r.len(), 3);
    assert_eq!(r[0], json!({}));
    assert_eq!(r[1]["bytesSoFar"], 3);
    let alias = PathBuf::from(r[2]["path"].as_str().unwrap());
    assert_ne!(alias, f.object_path(0));
    assert_eq!(fs::read(&alias).unwrap(), b"AAA");
    fs::remove_file(alias).unwrap();
    assert_eq!(fs::read(f.object_path(0)).unwrap(), b"AAA");
}
#[test]
fn upload_init_is_explicitly_disabled_without_aliases() {
    let f = Fixture::new(&[b"AAA"]);
    let mut backend = CacheBackend::new(f.profile.clone()).unwrap();
    let mut output = Vec::new();
    protocol::serve(
        Cursor::new(b"{\"event\":\"init\",\"operation\":\"upload\"}\n"),
        &mut output,
        &mut backend,
    )
    .unwrap();
    assert!(replies(&output)[0]["error"].is_object());
    assert!(f.aliases().is_empty());
}
#[test]
fn per_file_error_does_not_end_the_protocol_or_expose_request_secrets() {
    let f = Fixture::new(&[b"AAA"]);
    let unknown = Object::from_bytes(b"not admitted");
    let mut input = format!(
        "{{\"event\":\"init\",\"operation\":\"download\"}}\n{{\"event\":\"download\",\"oid\":\"{}\",\"size\":{}}}\n",
        unknown.sha256.as_str(),
        unknown.bytes
    );
    input.push_str(
        std::str::from_utf8(&messages(&f))
            .unwrap()
            .split_once('\n')
            .unwrap()
            .1,
    );
    let mut backend = CacheBackend::new(f.profile.clone()).unwrap();
    let mut output = Vec::new();
    protocol::serve(Cursor::new(input), &mut output, &mut backend).unwrap();
    let r = replies(&output);
    assert!(r[1]["error"].is_object());
    assert!(r[3]["path"].is_string());
    let input = format!(
        "{{\"event\":\"init\",\"operation\":\"download\"}}\n{{\"event\":\"download\",\"oid\":\"{}\",\"size\":3,\"action\":{{\"header\":{{\"Authorization\":\"secret-test-value\"}}}}}}\n",
        f.objects[0].sha256.as_str()
    );
    let mut output = Vec::new();
    protocol::serve(
        Cursor::new(input),
        &mut output,
        &mut CacheBackend::new(f.profile.clone()).unwrap(),
    )
    .unwrap();
    assert!(
        !String::from_utf8(output)
            .unwrap()
            .contains("secret-test-value")
    );
}
#[test]
fn protocol_framing_state_and_concurrency_limits_reject_before_transfer() {
    let f = Fixture::new(&[b"AAA"]);
    for input in [
        b"{\"event\":\"download\"}\n".to_vec(),
        vec![b'x'; 65_538],
        b"{\"event\":\"init\",\"event\":\"download\"}\n".to_vec(),
        b"{\"event\":\"init\",\"operation\":\"download\",\"concurrenttransfers\":100}\n".to_vec(),
    ] {
        assert!(
            protocol::serve(
                Cursor::new(input),
                Vec::new(),
                &mut CacheBackend::new(f.profile.clone()).unwrap()
            )
            .is_err()
        );
        assert!(f.aliases().is_empty());
    }
}
#[test]
fn corrupt_higher_priority_candidate_is_not_hidden_by_fallback() {
    let mut f = Fixture::new(&[b"AAA"]);
    let fallback = f.temp.path().join("fallback");
    let path = Layout::ReelCas.object_path(&fallback, &f.objects[0].sha256);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"AAA").unwrap();
    let mut second = f.profile.places[0].clone();
    second.root = fallback;
    f.profile.places.push(second);
    fs::write(f.object_path(0), b"bad").unwrap();
    let mut backend = CacheBackend::new(f.profile.clone()).unwrap();
    assert!(matches!(
        backend.download(&f.objects[0]),
        Err(AgentError::Integrity)
    ));
    assert!(f.aliases().is_empty());
}
#[test]
fn missing_disabled_places_and_each_supported_layout_have_explicit_order() {
    for layout in [Layout::ReelCas, Layout::GitLfs, Layout::Flat] {
        let mut f = Fixture::new(&[b"AAA"]);
        let root = f.temp.path().join("alternate");
        let path = layout.object_path(&root, &f.objects[0].sha256);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"AAA").unwrap();
        let mut live = f.profile.places[0].clone();
        live.root = root;
        live.layout = layout;
        f.profile.places[0].root = f.temp.path().join("offline");
        let mut disabled = f.profile.places[0].clone();
        disabled.enabled = false;
        disabled.root = f.object_path(0); // A file is invalid as a live root.
        f.profile.places.insert(0, disabled);
        f.profile.places.push(live);
        let mut backend = CacheBackend::new(f.profile.clone()).unwrap();
        assert!(backend.download(&f.objects[0]).is_ok());
        assert!(f.aliases().is_empty());
    }
}
#[test]
fn object_authorization_requires_exact_size_and_verified_metadata_nodes() {
    let f = Fixture::new(&[b"AAA"]);
    let mut wrong = f.objects[0].clone();
    wrong.bytes += 1;
    assert!(matches!(
        CacheBackend::new(f.profile.clone())
            .unwrap()
            .download(&wrong),
        Err(AgentError::Authority)
    ));
    fs::write(
        f.profile
            .authority_nodes
            .join(f.profile.authority_root.sha256.as_str()),
        b"{}",
    )
    .unwrap();
    assert!(
        CacheBackend::new(f.profile.clone())
            .unwrap()
            .initialize()
            .is_err()
    );
    assert!(f.aliases().is_empty());
}
#[test]
fn profile_bounds_closed_jobs_and_session_limits_do_not_bypass_admission() {
    let mut f = Fixture::new(&[b"AAA"]);
    for cap in [0, 2_000_001] {
        let mut invalid = f.profile.clone();
        invalid.max_authority_entries = cap;
        assert!(CacheBackend::new(invalid).is_err());
    }
    let mut legacy = serde_json::to_value(&f.profile).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("max_authority_entries");
    let legacy = Profile::from_json(&serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(legacy.max_authority_entries, 500_000);
    assert!(legacy.authority_checkpoint.is_none());
    let mut invalid = f.profile.clone();
    invalid.max_object_bytes = u64::MAX;
    assert!(CacheBackend::new(invalid).is_err());
    assert!(Profile::from_json(&vec![b' '; 65_537]).is_err());
    f.profile.max_requests = 1;
    let mut b = CacheBackend::new(f.profile.clone()).unwrap();
    drop(b.download(&f.objects[0]).unwrap());
    assert!(matches!(b.download(&f.objects[0]), Err(AgentError::Limit)));
    let a = &f.profile.places[0].admission;
    let mut ledger =
        git_asset_store::ledger::CapacityLedger::open(&a.ledger_root, &a.checkpoint).unwrap();
    ledger.finish(common::key("finish"), &a.job).unwrap();
    assert!(
        CacheBackend::new(f.profile.clone())
            .unwrap()
            .download(&f.objects[0])
            .is_err()
    );
    assert!(f.aliases().is_empty());
}
#[test]
fn unexposed_paths_are_cleaned_but_ambiguous_complete_ownership_is_preserved() {
    struct FailsAfter {
        lines: usize,
        complete: bool,
    }
    impl Write for FailsAfter {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            if (self.complete && self.lines >= 2 && !b.is_empty())
                || (!self.complete && self.lines >= 1 && !b.is_empty())
            {
                Err(io::ErrorKind::BrokenPipe.into())
            } else {
                if b.contains(&b'\n') {
                    self.lines += 1;
                }
                Ok(b.len())
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let f = Fixture::new(&[b"AAA"]);
    assert!(
        protocol::serve(
            Cursor::new(messages(&f)),
            FailsAfter {
                lines: 0,
                complete: false
            },
            &mut CacheBackend::new(f.profile.clone()).unwrap()
        )
        .is_err()
    );
    assert!(f.aliases().is_empty());
    assert!(
        protocol::serve(
            Cursor::new(messages(&f)),
            FailsAfter {
                lines: 0,
                complete: true
            },
            &mut CacheBackend::new(f.profile.clone()).unwrap()
        )
        .is_err()
    );
    assert_eq!(f.aliases().len(), 1);
    assert_eq!(fs::read(f.object_path(0)).unwrap(), b"AAA");
}
#[test]
fn metadata_adapter_is_readonly_and_bounded() {
    use git_asset_core::tree::NodeStore;
    let f = Fixture::new(&[b"AAA"]);
    let mut nodes = ReadNodes::new(f.profile.authority_nodes.clone());
    assert!(nodes.get(&f.profile.authority_root).unwrap().is_some());
    assert!(nodes.put(b"no write").is_err());
    let mut too_large = f.profile.authority_root.clone();
    too_large.bytes = 65_537;
    assert!(nodes.get(&too_large).is_err());
}
#[cfg(unix)]
#[test]
fn cross_filesystem_handoff_never_falls_back_to_a_full_copy() {
    let Ok(alias) = tempfile::tempdir_in("/dev/shm") else {
        return;
    };
    let mut f = Fixture::new(&[b"AAA"]);
    f.profile.places[0].handoff_directory = alias.path().to_owned();
    assert!(
        CacheBackend::new(f.profile.clone())
            .unwrap()
            .download(&f.objects[0])
            .is_err()
    );
    assert_eq!(fs::read_dir(alias.path()).unwrap().count(), 0);
    assert_eq!(fs::read(f.object_path(0)).unwrap(), b"AAA");
}
