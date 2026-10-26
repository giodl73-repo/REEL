use super::*;
use crate::publish;
use std::{
    fs,
    io::{BufRead, BufReader, Cursor, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tempfile::TempDir;
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
struct Fixture {
    temp: TempDir,
    stores: Vec<LocalStore>,
    requests: Vec<Request>,
}
impl Fixture {
    fn new(contents: &[&[u8]]) -> Self {
        Self::with_level(contents, AckLevel::ProcessRestart)
    }
    fn with_level(contents: &[&[u8]], level: AckLevel) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut stores = Vec::new();
        let mut requests = Vec::new();
        for (i, bytes) in contents.iter().enumerate() {
            let mut store =
                LocalStore::create(&temp.path().join(format!("store-{i}")), key("place")).unwrap();
            let request = Request {
                operation: key("base"),
                owner: key("owner"),
                object: tig_core::Object::from_bytes(bytes),
                required: level,
            };
            publish(&mut store, &request, &mut Cursor::new(bytes)).unwrap();
            stores.push(store);
            requests.push(request);
        }
        Self {
            temp,
            stores,
            requests,
        }
    }
    fn root(&self, i: usize) -> PathBuf {
        self.temp.path().join(format!("store-{i}"))
    }
    fn object(&self, i: usize) -> PathBuf {
        self.root(i)
            .join("objects")
            .join(self.requests[i].object.sha256.as_str())
    }
    fn operation(&self, i: usize, suffix: &str) -> PathBuf {
        self.root(i)
            .join("operations")
            .join(format!("{}.{suffix}", Digest::blob(b"base").as_str()))
    }
    fn verify(&self, i: usize) -> VerifiedReplica {
        verify(&self.stores[i], &self.requests[i]).unwrap()
    }
    fn enroll(&self, domains: &[&str]) -> Vec<ReplicaEnrollment> {
        domains
            .iter()
            .enumerate()
            .map(|(i, d)| ReplicaEnrollment {
                store: self.stores[i].identity().clone(),
                failure_domain: key(d),
            })
            .collect()
    }
}
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(root: &Path, p: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for e in fs::read_dir(p).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out)
            } else {
                out.push((
                    p.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(p).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}
#[test]
fn completed_copy_verifies_and_reads_without_metadata_mutation() {
    for payload in [b"AAA".to_vec(), Vec::new(), vec![0x5a; 150_003]] {
        let f = Fixture::new(&[&payload]);
        let before = snapshot(f.temp.path());
        let mut v = f.verify(0);
        assert_eq!(v.observation().store(), f.stores[0].identity());
        assert_eq!(v.observation().publication(), &f.requests[0]);
        assert_eq!(v.observation().level(), AckLevel::ProcessRestart);
        let mut bytes = Vec::new();
        v.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, payload);
        assert_eq!(snapshot(f.temp.path()), before);
    }
}
#[test]
fn bare_file_without_completed_publication_is_not_a_verified_replica() {
    let f = Fixture::new(&[b"AAA"]);
    fs::remove_file(f.operation(0, "done")).unwrap();
    let before = snapshot(f.temp.path());
    assert!(matches!(
        verify(&f.stores[0], &f.requests[0]),
        Err(ReplicaError::MissingCompletion)
    ));
    assert_eq!(snapshot(f.temp.path()), before);
}
#[test]
fn missing_anchor_is_not_recreated_to_satisfy_verification() {
    let f = Fixture::new(&[b"AAA"]);
    let anchor = f
        .root(0)
        .join("fences")
        .join(f.requests[0].object.sha256.as_str());
    fs::remove_file(&anchor).unwrap();
    assert!(verify(&f.stores[0], &f.requests[0]).is_err());
    assert!(!anchor.exists());
}
#[test]
fn wrong_length_or_hash_never_produces_live_evidence() {
    for bytes in [b"bad".as_slice(), b"AA".as_slice(), b"AAAA".as_slice()] {
        let f = Fixture::new(&[b"AAA"]);
        fs::write(f.object(0), bytes).unwrap();
        assert!(verify(&f.stores[0], &f.requests[0]).is_err());
        assert_eq!(fs::read(f.object(0)).unwrap(), bytes);
    }
}
#[test]
fn owner_operation_size_and_ack_are_bound_to_publication() {
    let f = Fixture::new(&[b"AAA"]);
    for kind in 0..4 {
        let mut req = f.requests[0].clone();
        match kind {
            0 => req.owner = key("other"),
            1 => req.operation = key("other"),
            2 => req.object.bytes += 1,
            _ => req.required = AckLevel::VerifiedLocal,
        };
        assert!(verify(&f.stores[0], &req).is_err());
    }
}
#[test]
fn unsupported_requests_reject_without_io_changes() {
    let f = Fixture::new(&[b"AAA"]);
    let before = snapshot(f.temp.path());
    let mut req = f.requests[0].clone();
    req.required = AckLevel::ProviderDurable;
    assert!(matches!(
        verify(&f.stores[0], &req),
        Err(ReplicaError::Store(Error::Unsupported))
    ));
    req.required = AckLevel::ProcessRestart;
    req.object.bytes = MAX_OBJECT_BYTES + 1;
    assert!(matches!(
        verify(&f.stores[0], &req),
        Err(ReplicaError::Store(Error::Budget))
    ));
    assert_eq!(snapshot(f.temp.path()), before);
}
#[test]
fn damaged_intent_completion_or_store_is_preserved_without_repair() {
    for name in ["intent", "done", "store"] {
        let f = Fixture::new(&[b"AAA"]);
        let path = if name == "store" {
            f.root(0).join("store.json")
        } else {
            f.operation(0, name)
        };
        fs::write(&path, b"{}").unwrap();
        let before = snapshot(f.temp.path());
        assert!(verify(&f.stores[0], &f.requests[0]).is_err());
        assert_eq!(snapshot(f.temp.path()), before);
    }
}
#[test]
fn live_session_fences_only_its_object_and_drop_releases() {
    let mut f = Fixture::new(&[b"AAA"]);
    let v = f.verify(0);
    assert!(matches!(
        f.stores[0].try_exclusive_object(&f.requests[0].object),
        Err(Error::Busy)
    ));
    let other = Request {
        operation: key("other"),
        owner: key("owner"),
        object: tig_core::Object::from_bytes(b"BBB"),
        required: AckLevel::ProcessRestart,
    };
    publish(&mut f.stores[0], &other, &mut Cursor::new(b"BBB")).unwrap();
    drop(v);
    assert!(
        f.stores[0]
            .try_exclusive_object(&f.requests[0].object)
            .is_ok()
    );
}
#[test]
fn failed_revalidation_invalidates_evidence_but_keeps_the_guard() {
    let f = Fixture::new(&[b"AAA", b"AAA"]);
    let source = f.verify(0);
    let mut retained = f.verify(1);
    fs::write(f.object(1), b"bad").unwrap();
    assert!(retained.revalidate(&f.stores[1]).is_err());
    assert!(matches!(
        coverage(
            &source,
            &[&retained],
            &f.enroll(&["one", "two"]),
            1,
            AckLevel::ProcessRestart
        ),
        Err(ReplicaError::Invalidated)
    ));
    assert!(matches!(
        f.stores[1].try_exclusive_object(&f.requests[1].object),
        Err(Error::Busy)
    ));
    fs::write(f.object(1), b"AAA").unwrap();
    retained.revalidate(&f.stores[1]).unwrap();
    assert!(
        coverage(
            &source,
            &[&retained],
            &f.enroll(&["one", "two"]),
            1,
            AckLevel::ProcessRestart
        )
        .unwrap()
        .meets_declared_coverage()
    );
}
#[test]
fn expiry_rejects_coverage_without_expiry_based_unlock() {
    let f = Fixture::new(&[b"AAA", b"AAA"]);
    let source = f.verify(0);
    let mut retained = f.verify(1);
    retained.checked = Instant::now() - Duration::from_secs(6);
    assert!(matches!(
        coverage(
            &source,
            &[&retained],
            &f.enroll(&["one", "two"]),
            1,
            AckLevel::ProcessRestart
        ),
        Err(ReplicaError::Stale)
    ));
    assert!(matches!(
        f.stores[1].try_exclusive_object(&f.requests[1].object),
        Err(Error::Busy)
    ));
    retained.revalidate(&f.stores[1]).unwrap();
    assert!(
        coverage(
            &source,
            &[&retained],
            &f.enroll(&["one", "two"]),
            1,
            AckLevel::ProcessRestart
        )
        .is_ok()
    );
}
#[test]
fn historical_record_roundtrip_never_substitutes_for_missing_current_bytes() {
    let f = Fixture::new(&[b"AAA"]);
    let v = f.verify(0);
    let encoded = v.observation().to_json().unwrap();
    let record = ReplicaObservation::from_json(&encoded).unwrap();
    assert_eq!(record.to_json().unwrap(), encoded);
    drop(v);
    fs::remove_file(f.object(0)).unwrap();
    assert!(record.verify_current(&f.stores[0]).is_err());
}
#[test]
fn declared_domains_count_once_and_never_authorize_eviction() {
    let f = Fixture::new(&[b"AAA", b"AAA", b"AAA", b"AAA"]);
    let s = f.verify(0);
    let a = f.verify(1);
    let b = f.verify(2);
    let c = f.verify(3);
    let one = coverage(
        &s,
        &[&a, &b, &c],
        &f.enroll(&["source", "other", "other", "source"]),
        2,
        AckLevel::ProcessRestart,
    )
    .unwrap();
    assert_eq!(one.retained_stores(), 3);
    assert_eq!(one.declared_independent_domains(), 1);
    assert!(!one.meets_declared_coverage());
    let two = coverage(
        &s,
        &[&a, &b],
        &f.enroll(&["source", "other", "third", "source"]),
        2,
        AckLevel::ProcessRestart,
    )
    .unwrap();
    assert!(two.meets_declared_coverage());
    assert!(!two.retention_established());
    assert!(!two.authorizes_eviction());
    let json = serde_json::to_value(two).unwrap();
    assert_eq!(json["eviction_authority"], false);
    assert_eq!(json["retention_established"], false);
}
#[test]
fn source_and_aliases_cannot_be_counted_as_another_replica() {
    let f = Fixture::new(&[b"AAA", b"AAA"]);
    let s = f.verify(0);
    let a = f.verify(1);
    let alias = LocalStore::open(&f.root(1)).unwrap();
    let b = verify(&alias, &f.requests[1]).unwrap();
    let e = f.enroll(&["one", "two"]);
    assert!(matches!(
        coverage(&s, &[&s], &e, 1, AckLevel::ProcessRestart),
        Err(ReplicaError::DuplicateStore)
    ));
    assert!(matches!(
        coverage(&s, &[&a, &b], &e, 1, AckLevel::ProcessRestart),
        Err(ReplicaError::DuplicateStore)
    ));
}
#[test]
fn missing_or_duplicate_enrollments_are_not_inferred_from_paths_or_labels() {
    let f = Fixture::new(&[b"AAA", b"AAA"]);
    let s = f.verify(0);
    let a = f.verify(1);
    assert!(matches!(
        coverage(&s, &[&a], &[], 1, AckLevel::ProcessRestart),
        Err(ReplicaError::MissingEnrollment)
    ));
    let mut e = f.enroll(&["one", "two"]);
    e.push(e[1].clone());
    assert!(matches!(
        coverage(&s, &[&a], &e, 1, AckLevel::ProcessRestart),
        Err(ReplicaError::ConflictingEnrollment)
    ));
}
#[test]
fn different_objects_and_inadequate_acknowledgements_reject() {
    let f = Fixture::new(&[b"AAA", b"BBB"]);
    let s = f.verify(0);
    let a = f.verify(1);
    assert!(matches!(
        coverage(
            &s,
            &[&a],
            &f.enroll(&["one", "two"]),
            1,
            AckLevel::ProcessRestart
        ),
        Err(ReplicaError::ObjectMismatch)
    ));
    let f = Fixture::with_level(&[b"AAA", b"AAA"], AckLevel::VerifiedLocal);
    let s = f.verify(0);
    let a = f.verify(1);
    assert_eq!(a.observation().level(), AckLevel::VerifiedLocal);
    assert!(matches!(
        coverage(
            &s,
            &[&a],
            &f.enroll(&["one", "two"]),
            1,
            AckLevel::ProcessRestart
        ),
        Err(ReplicaError::InsufficientAcknowledgement)
    ));
}
#[test]
fn coverage_limits_and_unknown_retained_copies_are_explicit() {
    let f = Fixture::new(&[b"AAA", b"AAA"]);
    let s = f.verify(0);
    let a = f.verify(1);
    let e = f.enroll(&["one", "two"]);
    for required in [0, 17] {
        assert!(matches!(
            coverage(&s, &[&a], &e, required, AckLevel::ProcessRestart),
            Err(ReplicaError::Budget)
        ));
    }
    assert!(matches!(
        coverage(&s, &[&a; 17], &e, 1, AckLevel::ProcessRestart),
        Err(ReplicaError::Budget)
    ));
    assert!(matches!(
        coverage(
            &s,
            &[&a],
            &vec![e[0].clone(); 18],
            1,
            AckLevel::ProcessRestart
        ),
        Err(ReplicaError::Budget)
    ));
    let empty = coverage(&s, &[], &e, 1, AckLevel::ProcessRestart).unwrap();
    assert!(!empty.meets_declared_coverage());
    assert_eq!(empty.retained_stores(), 0);
}
#[test]
fn portable_records_reject_malformed_or_overstated_capabilities() {
    let f = Fixture::new(&[b"AAA"]);
    let v = f.verify(0);
    let bytes = v.observation().to_json().unwrap();
    assert!(ReplicaObservation::from_json(&vec![b' '; 65_537]).is_err());
    assert!(ReplicaObservation::from_json(br#"{"schema":"a","schema":"b"}"#).is_err());
    for kind in 0..4 {
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        match kind {
            0 => value["extra"] = true.into(),
            1 => value["schema"] = "future".into(),
            2 => value["level"] = "provider_durable".into(),
            _ => {
                value["level"] = "provider_durable".into();
                value["publication"]["required"] = "provider_durable".into();
            }
        }
        assert!(ReplicaObservation::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}
#[test]
fn wrong_store_revalidation_does_not_silently_rebind_a_session() {
    let f = Fixture::new(&[b"AAA", b"AAA"]);
    let mut s = f.verify(0);
    assert!(matches!(
        s.observation().verify_current(&f.stores[1]),
        Err(ReplicaError::WrongStore)
    ));
    assert!(matches!(
        s.revalidate(&f.stores[1]),
        Err(ReplicaError::WrongStore)
    ));
    assert_eq!(s.observation().store(), f.stores[0].identity());
    assert!(matches!(s.fresh(), Err(ReplicaError::Invalidated)));
    assert!(matches!(
        f.stores[0].try_exclusive_object(&f.requests[0].object),
        Err(Error::Busy)
    ));
}
#[test]
fn replica_child() {
    let Ok(root) = std::env::var("TIG_REPLICA_TEST_ROOT") else {
        return;
    };
    let store = LocalStore::open(Path::new(&root)).unwrap();
    let request = Request {
        operation: key("base"),
        owner: key("owner"),
        object: tig_core::Object::from_bytes(b"AAA"),
        required: AckLevel::ProcessRestart,
    };
    let _session = verify(&store, &request).unwrap();
    println!("READY");
    io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}
#[test]
fn process_exit_drops_live_guard_but_does_not_create_a_retention_promise() {
    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let f = Fixture::new(&[b"AAA"]);
    let mut child = ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "replica::tests::replica_child", "--nocapture"])
            .env("TIG_REPLICA_TEST_ROOT", f.root(0))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (ready, wait) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut output = BufReader::new(stdout);
        let mut line = String::new();
        while output.read_line(&mut line).unwrap_or(0) > 0 {
            if line.contains("READY") {
                let _ = ready.send(());
                break;
            }
            line.clear();
        }
    });
    wait.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(matches!(
        f.stores[0].try_exclusive_object(&f.requests[0].object),
        Err(Error::Busy)
    ));
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    reader.join().unwrap();
    assert!(
        f.stores[0]
            .try_exclusive_object(&f.requests[0].object)
            .is_ok()
    );
    let reopened = LocalStore::open(&f.root(0)).unwrap();
    assert!(verify(&reopened, &f.requests[0]).is_ok());
}
