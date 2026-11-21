use git_asset_core::Object;
use git_asset_store::{local::*, *};
use std::{
    fs,
    io::{BufRead, BufReader, Cursor, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};
use tempfile::TempDir;

const READY: &str = "GIT_ASSET_FENCE_READY";

fn key(value: &str) -> Key {
    Key::parse(value).unwrap()
}
fn request(id: &str, bytes: &[u8]) -> Request {
    Request {
        operation: key(id),
        owner: key("owner"),
        object: Object::from_bytes(bytes),
        required: AckLevel::ProcessRestart,
    }
}
struct Fixture {
    _temp: TempDir,
    root: PathBuf,
    request: Request,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("store");
        let mut store = LocalStore::create(&root, key("place")).unwrap();
        let req = request("base", b"AAA");
        publish(&mut store, &req, &mut Cursor::new(b"AAA")).unwrap();
        Self {
            _temp: temp,
            root,
            request: req,
        }
    }
    fn open(&self) -> LocalStore {
        LocalStore::open(&self.root).unwrap()
    }
}

#[test]
fn multiple_readers_coexist_and_exclusive_waits_for_the_last_reader() {
    let f = Fixture::new();
    let s = f.open();
    let a = s.read_object(&f.request.object).unwrap();
    let mut b = s.read_object(&f.request.object).unwrap();
    assert!(matches!(
        s.try_exclusive_object(&f.request.object),
        Err(Error::Busy)
    ));
    drop(a);
    assert!(matches!(
        s.try_exclusive_object(&f.request.object),
        Err(Error::Busy)
    ));
    let mut bytes = Vec::new();
    b.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"AAA");
    assert_eq!(b.object(), &f.request.object);
    drop(b);
    let exclusive = s.try_exclusive_object(&f.request.object).unwrap();
    assert_eq!(exclusive.object(), &f.request.object);
    assert_eq!(exclusive.place(), &key("place"));
}
#[test]
fn exclusive_object_blocks_only_that_objects_read_and_publication() {
    let f = Fixture::new();
    let s = f.open();
    let held = s.try_exclusive_object(&f.request.object).unwrap();
    assert!(matches!(
        f.open().read_object(&f.request.object),
        Err(Error::Busy)
    ));
    assert_eq!(
        publish(&mut f.open(), &f.request, &mut Cursor::new(b"AAA")),
        Err(Error::Busy)
    );
    let mut direct = f.open();
    assert_eq!(direct.protect(&f.request), Err(Error::Busy));
    assert_eq!(direct.verified_object(&f.request.object), Err(Error::Busy));
    assert_eq!(
        direct.publish_object(&f.request, &mut Cursor::new(b"AAA")),
        Err(Error::Busy)
    );
    assert_eq!(direct.commit(&f.request), Err(Error::Busy));
    let other = request("other", b"BBB");
    publish(&mut f.open(), &other, &mut Cursor::new(b"BBB")).unwrap();
    assert!(f.open().read_object(&other.object).is_ok());
    drop(held);
    assert!(f.open().read_object(&f.request.object).is_ok());
}
#[test]
fn aliases_of_the_same_local_root_share_the_same_os_fence() {
    let f = Fixture::new();
    let s = f.open();
    let held = s.read_object(&f.request.object).unwrap();
    let canonical = fs::canonicalize(&f.root).unwrap();
    let alias = LocalStore::open(&canonical).unwrap();
    assert!(matches!(
        alias.try_exclusive_object(&f.request.object),
        Err(Error::Busy)
    ));
    drop(held);
    assert!(alias.try_exclusive_object(&f.request.object).is_ok());
}
#[test]
fn size_mismatch_cannot_escape_the_same_hash_fence_or_leak_a_guard() {
    let f = Fixture::new();
    let s = f.open();
    let held = s.read_object(&f.request.object).unwrap();
    let mut wrong = f.request.object.clone();
    wrong.bytes += 1;
    assert!(matches!(s.try_exclusive_object(&wrong), Err(Error::Busy)));
    drop(held);
    assert!(matches!(s.read_object(&wrong), Err(Error::Integrity)));
    assert!(s.try_exclusive_object(&f.request.object).is_ok());
}
#[test]
fn failed_publication_releases_only_its_temporary_os_guard() {
    let f = Fixture::new();
    let req = request("bad", b"new");
    assert_eq!(
        publish(&mut f.open(), &req, &mut Cursor::new(b"bad")),
        Err(Error::Integrity)
    );
    assert!(f.open().try_exclusive_object(&req.object).is_ok());
    // No deletion is attempted: persistent ingest intent is a separate obligation.
    assert!(recover(&mut f.open(), &req).unwrap().is_none());
}
#[test]
fn stable_anchors_survive_close_reopen_without_timeout_or_unlink() {
    let f = Fixture::new();
    let anchor = f.root.join("fences").join(f.request.object.sha256.as_str());
    assert!(anchor.exists());
    for _ in 0..3 {
        let guard = f.open().try_exclusive_object(&f.request.object).unwrap();
        drop(guard);
        assert!(anchor.exists());
    }
    assert_eq!(fs::metadata(anchor).unwrap().len(), 0);
}
#[test]
fn malformed_anchor_is_not_cleared_as_a_stale_lock() {
    let f = Fixture::new();
    let anchor = f.root.join("fences").join(f.request.object.sha256.as_str());
    fs::write(&anchor, b"foreign bytes").unwrap();
    assert!(matches!(
        f.open().read_object(&f.request.object),
        Err(Error::InvalidRoot)
    ));
    assert_eq!(fs::read(anchor).unwrap(), b"foreign bytes");
}
#[test]
fn old_experimental_manifest_rejects_without_automatic_setup() {
    let f = Fixture::new();
    for entry in fs::read_dir(f.root.join("fences")).unwrap() {
        fs::remove_file(entry.unwrap().path()).unwrap();
    }
    fs::remove_dir(f.root.join("fences")).unwrap();
    fs::write(
        f.root.join("store.json"),
        br#"{"place":"place","schema":"tig.local-store.design-1"}"#,
    )
    .unwrap();
    assert!(LocalStore::open(&f.root).is_err());
    assert!(!f.root.join("fences").exists());
    assert_eq!(
        fs::read(
            f.root
                .join("objects")
                .join(f.request.object.sha256.as_str())
        )
        .unwrap(),
        b"AAA"
    );
}

struct ChildScope(Child);
impl Drop for ChildScope {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn child(f: &Fixture, mode: &str) -> ChildScope {
    let process = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "fence_child", "--nocapture", "--quiet"])
        .env("GIT_ASSET_FENCE_TEST_ROOT", &f.root)
        .env("GIT_ASSET_FENCE_TEST_MODE", mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut scope = ChildScope(process);
    let stderr = scope.0.stderr.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let ready = BufReader::new(stderr.take(8192))
            .lines()
            .take(16)
            .any(|line| line.is_ok_and(|line| line.trim() == READY));
        let _ = send.send(ready);
    });
    assert!(
        receive
            .recv_timeout(std::time::Duration::from_secs(15))
            .unwrap_or(false),
        "child failed before holding its fence"
    );
    scope
}
fn exit_child(mut scope: ChildScope) {
    scope.0.stdin.take().unwrap().write_all(b"x").unwrap();
    assert_eq!(scope.0.wait().unwrap().code(), Some(77));
}
#[test]
fn fence_child() {
    let Some(root) = std::env::var_os("GIT_ASSET_FENCE_TEST_ROOT") else {
        return;
    };
    let mode = std::env::var("GIT_ASSET_FENCE_TEST_MODE").unwrap();
    let s = LocalStore::open(Path::new(&root)).unwrap();
    let object = Object::from_bytes(b"AAA");
    let _reader = if mode == "read" {
        Some(s.read_object(&object).unwrap())
    } else {
        None
    };
    let _exclusive = if mode == "exclusive" {
        Some(s.try_exclusive_object(&object).unwrap())
    } else {
        None
    };
    eprintln!("{READY}");
    std::io::stderr().flush().unwrap();
    let mut b = [0u8; 1];
    std::io::stdin().read_exact(&mut b).unwrap();
    std::process::exit(77);
}
#[test]
fn read_in_another_process_blocks_exclusive_until_process_exit() {
    let f = Fixture::new();
    let child = child(&f, "read");
    assert!(matches!(
        f.open().try_exclusive_object(&f.request.object),
        Err(Error::Busy)
    ));
    assert!(f.open().read_object(&f.request.object).is_ok());
    let other = request("parallel", b"BBB");
    publish(&mut f.open(), &other, &mut Cursor::new(b"BBB")).unwrap();
    exit_child(child);
    assert!(f.open().try_exclusive_object(&f.request.object).is_ok());
}
#[test]
fn exclusive_in_another_process_blocks_same_object_but_not_other_work() {
    let f = Fixture::new();
    let child = child(&f, "exclusive");
    assert!(matches!(
        f.open().read_object(&f.request.object),
        Err(Error::Busy)
    ));
    assert_eq!(
        publish(&mut f.open(), &f.request, &mut Cursor::new(b"AAA")),
        Err(Error::Busy)
    );
    let other = request("parallel", b"BBB");
    publish(&mut f.open(), &other, &mut Cursor::new(b"BBB")).unwrap();
    exit_child(child);
    assert!(f.open().read_object(&f.request.object).is_ok());
}
