use git_asset_core::{Digest, Object};
use git_asset_store::{local::*, *};
use std::{
    fs,
    io::{self, Cursor, Read},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Barrier},
};
use tempfile::TempDir;

struct Fixture {
    _temp: TempDir,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("store");
        LocalStore::create(&root, key("test-place")).unwrap();
        Self { _temp: temp, root }
    }
    fn open(&self) -> LocalStore {
        LocalStore::open(&self.root).unwrap()
    }
    fn op(&self, operation: &str, suffix: &str) -> PathBuf {
        self.root.join("operations").join(format!(
            "{}.{suffix}",
            Digest::blob(operation.as_bytes()).as_str()
        ))
    }
    fn object(&self, object: &Object) -> PathBuf {
        self.root.join("objects").join(object.sha256.as_str())
    }
    fn count(&self, name: &str) -> usize {
        fs::read_dir(self.root.join(name)).unwrap().count()
    }
}
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn request(operation: &str, bytes: &[u8]) -> Request {
    Request {
        operation: key(operation),
        owner: key("worker-1"),
        object: Object::from_bytes(bytes),
        required: AckLevel::ProcessRestart,
    }
}
fn put(store: &mut LocalStore, request: &Request, bytes: &[u8]) -> Receipt {
    publish(store, request, &mut Cursor::new(bytes)).unwrap()
}
struct NoRead;
impl Read for NoRead {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        panic!("source must not be read")
    }
}

#[test]
fn explicit_fresh_root_and_reopen_preserve_place_identity() {
    let f = Fixture::new();
    assert_eq!(f.open().place(), &key("test-place"));
    assert!(LocalStore::create(&f.root, key("other")).is_err());
    assert!(LocalStore::open(Path::new("relative")).is_err());
}
#[test]
fn keys_cannot_supply_filesystem_paths() {
    for bad in ["", "../escape", "a/b", "C:root", "a\\b", "with space"] {
        assert!(Key::parse(bad).is_err());
    }
    assert!(Key::parse(&"a".repeat(129)).is_err());
}
#[test]
fn publish_then_reopen_recovers_exact_receipt_and_bytes() {
    let f = Fixture::new();
    let req = request("one", b"verified bytes");
    let receipt = put(&mut f.open(), &req, b"verified bytes");
    assert_eq!(receipt.level(), AckLevel::ProcessRestart);
    assert_eq!(receipt.request(), &req);
    assert_eq!(recover(&mut f.open(), &req).unwrap(), Some(receipt));
    assert_eq!(fs::read(f.object(&req.object)).unwrap(), b"verified bytes");
    assert_eq!(f.count("staging"), 0);
}
#[test]
fn empty_and_multibuffer_payloads_have_exact_streamed_identity() {
    let f = Fixture::new();
    for (n, bytes) in [Vec::new(), vec![0x5a; 150_003]].into_iter().enumerate() {
        let req = request(&format!("object-{n}"), &bytes);
        put(&mut f.open(), &req, &bytes);
        assert_eq!(fs::read(f.object(&req.object)).unwrap(), bytes);
    }
}
#[test]
fn identical_retry_reuses_verified_bytes_without_requiring_source_again() {
    let f = Fixture::new();
    let req = request("one", b"same");
    let receipt = put(&mut f.open(), &req, b"same");
    assert_eq!(publish(&mut f.open(), &req, &mut NoRead).unwrap(), receipt);
    assert_eq!(f.count("objects"), 1);
    assert_eq!(f.count("operations"), 2);
}
#[test]
fn unsupported_durability_and_oversize_reject_before_intent_or_source_io() {
    let f = Fixture::new();
    let mut req = request("one", b"x");
    req.required = AckLevel::ProviderDurable;
    assert_eq!(
        publish(&mut f.open(), &req, &mut NoRead),
        Err(Error::Unsupported)
    );
    req.required = AckLevel::ProcessRestart;
    req.object.bytes = MAX_OBJECT_BYTES + 1;
    assert_eq!(
        publish(&mut f.open(), &req, &mut NoRead),
        Err(Error::Budget)
    );
    assert_eq!(f.count("operations"), 0);
    assert_eq!(f.count("staging"), 0);
}
#[test]
fn requested_weaker_ack_is_not_silently_upgraded_to_stronger_custody() {
    let f = Fixture::new();
    let mut req = request("one", b"x");
    req.required = AckLevel::VerifiedLocal;
    assert_eq!(
        put(&mut f.open(), &req, b"x").level(),
        AckLevel::VerifiedLocal
    );
}
#[test]
fn operation_reuse_with_different_owner_object_or_level_rejects() {
    let f = Fixture::new();
    let req = request("one", b"first");
    put(&mut f.open(), &req, b"first");
    let mut owner = req.clone();
    owner.owner = key("worker-2");
    let mut object = req.clone();
    object.object = Object::from_bytes(b"second");
    let mut level = req.clone();
    level.required = AckLevel::VerifiedLocal;
    for changed in [owner, object, level] {
        assert_eq!(
            publish(&mut f.open(), &changed, &mut NoRead),
            Err(Error::OperationConflict)
        );
    }
    assert_eq!(fs::read(f.object(&req.object)).unwrap(), b"first");
}
#[test]
fn short_long_and_same_size_wrong_inputs_never_publish_complete_objects() {
    for input in [b"ab".as_slice(), b"abcd", b"xyz"] {
        let f = Fixture::new();
        let req = request("one", b"abc");
        assert_eq!(
            publish(&mut f.open(), &req, &mut Cursor::new(input)),
            Err(Error::Integrity)
        );
        assert_eq!(f.count("objects"), 0);
        assert_eq!(f.count("staging"), 0);
        assert_eq!(recover(&mut f.open(), &req).unwrap(), None);
    }
}
#[test]
fn interrupted_source_reads_retry_but_real_read_failure_keeps_only_intent() {
    struct InterruptedOnce {
        first: bool,
        inner: Cursor<Vec<u8>>,
    }
    impl Read for InterruptedOnce {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            if self.first {
                self.first = false;
                return Err(io::ErrorKind::Interrupted.into());
            }
            self.inner.read(b)
        }
    }
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::Other.into())
        }
    }
    let f = Fixture::new();
    let req = request("one", b"abc");
    assert!(publish(&mut f.open(), &req, &mut Broken).is_err());
    assert_eq!(recover(&mut f.open(), &req).unwrap(), None);
    assert_eq!(f.count("staging"), 0);
    assert!(
        publish(
            &mut f.open(),
            &req,
            &mut InterruptedOnce {
                first: true,
                inner: Cursor::new(b"abc".to_vec())
            }
        )
        .is_ok()
    );
}
#[test]
fn corrupt_existing_object_is_not_overwritten_or_accepted() {
    let f = Fixture::new();
    let req = request("one", b"abc");
    fs::write(f.object(&req.object), b"xyz").unwrap();
    assert_eq!(
        publish(&mut f.open(), &req, &mut NoRead),
        Err(Error::Integrity)
    );
    assert_eq!(fs::read(f.object(&req.object)).unwrap(), b"xyz");
    assert!(!f.op("one", "done").exists());
}
#[test]
fn missing_acknowledged_object_is_an_integrity_incident_not_a_normal_miss() {
    let f = Fixture::new();
    let req = request("one", b"abc");
    put(&mut f.open(), &req, b"abc");
    fs::remove_file(f.object(&req.object)).unwrap();
    assert_eq!(
        recover(&mut f.open(), &req),
        Err(Error::MissingAcknowledgedObject)
    );
    assert!(f.op("one", "done").exists());
}
#[test]
fn replacement_keeps_both_versions_as_immutable_objects() {
    let f = Fixture::new();
    let one = request("v1", b"old");
    let two = request("v2", b"new");
    put(&mut f.open(), &one, b"old");
    put(&mut f.open(), &two, b"new");
    assert_eq!(f.count("objects"), 2);
    assert!(recover(&mut f.open(), &one).unwrap().is_some());
    assert!(recover(&mut f.open(), &two).unwrap().is_some());
}
#[test]
fn torn_intent_and_completion_are_not_treated_as_missing_or_success() {
    for suffix in ["intent", "done"] {
        let f = Fixture::new();
        let req = request("one", b"abc");
        put(&mut f.open(), &req, b"abc");
        fs::write(f.op("one", suffix), b"{").unwrap();
        assert_eq!(recover(&mut f.open(), &req), Err(Error::CorruptRecord));
        assert_eq!(fs::read(f.op("one", suffix)).unwrap(), b"{");
    }
}
#[test]
fn missing_completion_reconciles_only_after_actual_payload_verification() {
    let f = Fixture::new();
    let req = request("one", b"abc");
    put(&mut f.open(), &req, b"abc");
    fs::remove_file(f.op("one", "done")).unwrap();
    assert!(recover(&mut f.open(), &req).unwrap().is_some());
    assert!(f.op("one", "done").exists());
}
#[test]
fn surviving_completion_prevents_foreign_rebinding_after_missing_intent() {
    let f = Fixture::new();
    let req = request("one", b"abc");
    put(&mut f.open(), &req, b"abc");
    fs::remove_file(f.op("one", "intent")).unwrap();
    let mut foreign = req.clone();
    foreign.owner = key("worker-2");
    assert_eq!(
        recover(&mut f.open(), &foreign),
        Err(Error::OperationConflict)
    );
    assert!(!f.op("one", "intent").exists());
    assert!(recover(&mut f.open(), &req).unwrap().is_some());
}
#[test]
fn duplicate_or_unknown_record_fields_are_rejected() {
    let f = Fixture::new();
    let req = request("one", b"abc");
    put(&mut f.open(), &req, b"abc");
    fs::write(f.op("one", "intent"), br#"{"schema":"x","schema":"y"}"#).unwrap();
    assert_eq!(recover(&mut f.open(), &req), Err(Error::CorruptRecord));
    fs::write(
        f.root.join("store.json"),
        br#"{"place":"test-place","schema":"tig.local-store.design-1","unknown":1}"#,
    )
    .unwrap();
    assert!(LocalStore::open(&f.root).is_err());
}
#[test]
fn wrong_root_types_and_symlink_objects_cannot_be_admitted() {
    let f = Fixture::new();
    fs::remove_dir(f.root.join("objects")).unwrap();
    fs::write(f.root.join("objects"), b"file").unwrap();
    assert!(matches!(LocalStore::open(&f.root), Err(Error::InvalidRoot)));
    #[cfg(unix)]
    {
        let f = Fixture::new();
        let req = request("one", b"abc");
        let other = f._temp.path().join("outside");
        fs::write(&other, b"abc").unwrap();
        std::os::unix::fs::symlink(&other, f.object(&req.object)).unwrap();
        assert_eq!(
            publish(&mut f.open(), &req, &mut NoRead),
            Err(Error::InvalidRoot)
        );
        assert_eq!(fs::read(other).unwrap(), b"abc");
    }
}

struct Faults {
    inner: LocalStore,
    step: usize,
    cut: usize,
    exit: bool,
}
impl Faults {
    fn tick(&mut self) -> Result<()> {
        self.step += 1;
        if self.step == self.cut {
            if self.exit {
                std::process::exit(77);
            }
            return Err(Error::Io(
                io::ErrorKind::Other,
                "injected ordering fault".into(),
            ));
        }
        Ok(())
    }
}
impl PublicationBackend for Faults {
    fn publication_guard(&mut self, object: &Object) -> Result<Box<dyn PublicationGuard>> {
        self.inner.publication_guard(object)
    }
    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }
    fn protect(&mut self, r: &Request) -> Result<State> {
        self.tick()?;
        let s = self.inner.protect(r)?;
        self.tick()?;
        Ok(s)
    }
    fn verified_object(&mut self, o: &Object) -> Result<bool> {
        self.tick()?;
        let b = self.inner.verified_object(o)?;
        self.tick()?;
        Ok(b)
    }
    fn publish_object(&mut self, r: &Request, s: &mut dyn Read) -> Result<()> {
        self.tick()?;
        self.inner.publish_object(r, s)?;
        self.tick()
    }
    fn commit(&mut self, r: &Request) -> Result<()> {
        self.tick()?;
        self.inner.commit(r)?;
        self.tick()
    }
}
#[test]
fn every_protocol_boundary_recovers_without_false_ack_or_object_loss() {
    for cut in 1..=10 {
        let f = Fixture::new();
        let req = request("one", b"abc");
        let mut backend = Faults {
            inner: f.open(),
            step: 0,
            cut,
            exit: false,
        };
        assert!(
            publish(&mut backend, &req, &mut Cursor::new(b"abc")).is_err(),
            "cut {cut}"
        );
        drop(backend);
        let recovered = recover(&mut f.open(), &req).unwrap();
        assert_eq!(recovered.is_some(), cut >= 6, "cut {cut}");
        put(&mut f.open(), &req, b"abc");
        assert_eq!(fs::read(f.object(&req.object)).unwrap(), b"abc");
        assert_eq!(f.count("objects"), 1);
        assert_eq!(f.count("staging"), 0);
    }
}
fn concurrent(same_operation: bool, same_bytes: bool) {
    let f = Fixture::new();
    let barrier = Arc::new(Barrier::new(2));
    let mut workers = Vec::new();
    for n in 0..2 {
        let root = f.root.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            let bytes = if same_bytes || n == 0 { b"aaa" } else { b"bbb" };
            let op = if same_operation {
                "shared".into()
            } else {
                format!("worker-{n}")
            };
            let req = request(&op, bytes);
            let mut store = LocalStore::open(&root).unwrap();
            barrier.wait();
            publish(&mut store, &req, &mut Cursor::new(bytes))
        }));
    }
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    if same_operation && !same_bytes {
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, Err(Error::OperationConflict)))
        );
    } else {
        assert!(results.iter().all(|r| r.is_ok()), "{results:?}");
    }
    assert_eq!(f.count("objects"), 1);
    assert_eq!(f.count("staging"), 0);
}
#[test]
fn parallel_operations_share_one_complete_object_without_global_lock() {
    concurrent(false, true);
}
#[test]
fn identical_operation_retries_are_safe_when_concurrent() {
    concurrent(true, true);
}
#[test]
fn concurrent_operation_collision_has_one_exact_winner() {
    concurrent(true, false);
}

const CHILD_BYTES: &[u8] = b"restart-fixture";
#[test]
fn publication_child() {
    let Some(root) = std::env::var_os("GIT_ASSET_PUBLICATION_TEST_ROOT") else {
        return;
    };
    let cut = std::env::var("GIT_ASSET_PUBLICATION_TEST_CUT")
        .unwrap()
        .parse()
        .unwrap();
    let req = request("child", CHILD_BYTES);
    let mut backend = Faults {
        inner: LocalStore::open(Path::new(&root)).unwrap(),
        step: 0,
        cut,
        exit: true,
    };
    if cut == 0 {
        struct CrashReader(bool);
        impl Read for CrashReader {
            fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
                if self.0 {
                    std::process::exit(77);
                }
                self.0 = true;
                b[..4].copy_from_slice(&CHILD_BYTES[..4]);
                Ok(4)
            }
        }
        let _ = publish(&mut backend, &req, &mut CrashReader(false));
    } else {
        let _ = publish(&mut backend, &req, &mut Cursor::new(CHILD_BYTES));
    }
    panic!("child did not hit its declared crash point");
}
#[test]
fn actual_process_exit_reopens_protection_or_verified_bytes_and_never_a_partial() {
    for cut in [0, 2, 6, 10] {
        let f = Fixture::new();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "publication_child", "--nocapture"])
            .env("GIT_ASSET_PUBLICATION_TEST_ROOT", &f.root)
            .env("GIT_ASSET_PUBLICATION_TEST_CUT", cut.to_string())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(77), "{output:?}");
        let req = request("child", CHILD_BYTES);
        assert_eq!(recover(&mut f.open(), &req).unwrap().is_some(), cut >= 6);
        if cut == 0 {
            assert_eq!(f.count("objects"), 0);
            assert_eq!(f.count("staging"), 1);
        }
        put(&mut f.open(), &req, CHILD_BYTES);
        assert_eq!(fs::read(f.object(&req.object)).unwrap(), CHILD_BYTES);
        // Abrupt-death partials are deliberately not treated as complete or swept.
        assert_eq!(f.count("staging"), usize::from(cut == 0));
    }
}
