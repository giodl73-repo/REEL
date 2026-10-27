#![cfg(feature = "tig-publication")]
use reel::tig_publication::{ExistingPublisher, Profile};
use reel_assets::{Config, PathPolicy};
use std::{
    fs,
    io::{self, Cursor, Read},
    path::Path,
};
use tig_core::{Digest, Object};
use tig_store::{
    AckLevel, Error, Key, PublicationBackend, Request,
    existing::{self, RequiredProtection},
};

fn key(value: &str) -> Key {
    Key::parse(value).unwrap()
}

fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("cache/objects/sha256")).unwrap();
    fs::create_dir(temp.path().join("coordination")).unwrap();
    fs::create_dir(temp.path().join("artifact")).unwrap();
    temp
}
fn profile(root: &Path) -> Profile {
    let mut storage = Config {
        path_policy: PathPolicy::ConfiguredLocalRoots,
        cache_roots: vec![root.join("cache")],
        coordination_root: Some(root.join("coordination")),
        ..Config::default()
    };
    storage.capacity.minimum_free_bytes = 0;
    storage.capacity.minimum_free_basis_points = 0;
    storage.capacity.max_object_bytes = 64 * 1024 * 1024;
    Profile {
        artifact_root: root.join("artifact"),
        namespace: key("fixture"),
        storage,
        destination: root.join("cache"),
        reader_limits: existing::Limits::default(),
        protection: if cfg!(windows) {
            RequiredProtection::PreventMutation
        } else {
            RequiredProtection::CooperativeImmutable
        },
    }
}
fn request(id: &str, bytes: &[u8]) -> Request {
    Request {
        operation: key(id),
        owner: key("owner"),
        object: Object::from_bytes(bytes),
        required: AckLevel::ProcessRestart,
    }
}
struct NoRead;
impl Read for NoRead {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        panic!("source must not be read");
    }
}
fn files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files(&path));
        } else {
            found.push(path);
        }
    }
    found
}

#[test]
fn publishes_large_stream_without_extra_source_copy_and_recovers_after_reopen() {
    let temp = fixture();
    let root = temp.path();
    let bytes = vec![0x6d; 40 * 1024 * 1024];
    let req = request("large-master", &bytes);
    assert!(req.object.bytes > tig_store::local::MAX_OBJECT_BYTES);
    let mut publisher = ExistingPublisher::open(profile(root)).unwrap();
    let receipt = tig_store::publish(&mut publisher, &req, &mut Cursor::new(&bytes)).unwrap();
    assert_eq!(receipt.request(), &req);
    assert_eq!(receipt.level(), AckLevel::ProcessRestart);
    let final_path = reel_assets::object(&root.join("cache"), req.object.sha256.as_str());
    assert_eq!(fs::metadata(&final_path).unwrap().len(), req.object.bytes);
    assert_eq!(files(&root.join("cache")), vec![final_path]);
    let promotions: serde_json::Value = serde_json::from_str(
        fs::read_to_string(root.join("artifact/.local/cache-promotions.jsonl"))
            .unwrap()
            .trim(),
    )
    .unwrap();
    assert_eq!(promotions["source"]["kind"], "caller-reader");
    assert_eq!(
        promotions["creative_authority"],
        "not-granted-by-this-receipt"
    );
    drop(publisher);
    let mut reopened = ExistingPublisher::open(profile(root)).unwrap();
    assert!(tig_store::recover(&mut reopened, &req).unwrap().is_some());
    tig_store::publish(&mut reopened, &req, &mut NoRead).unwrap();
    let mut foreign = req.clone();
    foreign.owner = key("foreign");
    assert!(matches!(
        tig_store::publish(&mut reopened, &foreign, &mut NoRead),
        Err(Error::OperationConflict)
    ));
    let foreign = request("large-master", b"different bytes same operation");
    assert!(matches!(
        tig_store::publish(&mut reopened, &foreign, &mut NoRead),
        Err(Error::OperationConflict)
    ));
}

#[test]
fn failures_preserve_source_and_never_clobber_or_repair_completed_corruption() {
    let temp = fixture();
    let root = temp.path();
    let req = request("broken-source", b"expected payload");
    let mut publisher = ExistingPublisher::open(profile(root)).unwrap();
    for input in [
        b"short".as_slice(),
        b"expected payload plus unexpected bytes".as_slice(),
    ] {
        assert!(tig_store::publish(&mut publisher, &req, &mut Cursor::new(input)).is_err());
        assert!(files(&root.join("cache")).is_empty());
    }
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("injected source failure"))
        }
    }
    assert!(tig_store::publish(&mut publisher, &req, &mut Broken).is_err());
    assert!(files(&root.join("cache")).is_empty());
    tig_store::publish(&mut publisher, &req, &mut Cursor::new(b"expected payload")).unwrap();
    drop(publisher);
    let path = reel_assets::object(&root.join("cache"), req.object.sha256.as_str());
    fs::write(&path, b"corrupt").unwrap();
    let mut reopened = ExistingPublisher::open(profile(root)).unwrap();
    assert!(matches!(
        tig_store::publish(&mut reopened, &req, &mut NoRead),
        Err(Error::Integrity)
    ));
    assert_eq!(fs::read(&path).unwrap(), b"corrupt");
    drop(reopened);
    fs::remove_file(&path).unwrap();
    let mut reopened = ExistingPublisher::open(profile(root)).unwrap();
    assert!(matches!(
        tig_store::publish(&mut reopened, &req, &mut NoRead),
        Err(Error::MissingAcknowledgedObject)
    ));
    assert!(!path.exists());
}

#[test]
fn rejects_missing_roots_durable_levels_and_capacity_denial_before_stream_io() {
    let temp = fixture();
    let root = temp.path();
    let mut bad = profile(root);
    bad.destination = root.join("missing-cache");
    assert!(ExistingPublisher::open(bad).is_err());
    assert!(!root.join("missing-cache").exists());
    let mut bad = profile(root);
    bad.storage.coordination_root = None;
    assert!(ExistingPublisher::open(bad).is_err());
    let mut bad = profile(root);
    bad.reader_limits.requests = 0;
    assert!(ExistingPublisher::open(bad).is_err());
    let mut publisher = ExistingPublisher::open(profile(root)).unwrap();
    let mut req = request("unsupported", b"small");
    req.required = AckLevel::ProviderDurable;
    assert!(matches!(
        tig_store::publish(&mut publisher, &req, &mut NoRead),
        Err(Error::Unsupported)
    ));
    assert!(!root.join("coordination/tig-publications").exists());
    drop(publisher);
    let mut denied = profile(root);
    denied.storage.capacity.minimum_free_bytes = u64::MAX;
    let mut publisher = ExistingPublisher::open(denied).unwrap();
    let req = request("capacity-denied", b"small");
    assert!(matches!(
        tig_store::publish(&mut publisher, &req, &mut NoRead),
        Err(Error::Budget)
    ));
    assert!(files(&root.join("cache")).is_empty());
}

#[test]
fn only_same_destination_object_is_fenced_and_missing_intent_is_not_repaired() {
    let temp = fixture();
    let root = temp.path();
    let req = request("held", b"one");
    let mut first = ExistingPublisher::open(profile(root)).unwrap();
    let guard = first.publication_guard(&req.object).unwrap();
    let mut second = ExistingPublisher::open(profile(root)).unwrap();
    assert!(matches!(
        tig_store::publish(&mut second, &req, &mut NoRead),
        Err(Error::Busy)
    ));
    let other = request("independent", b"two");
    tig_store::publish(&mut second, &other, &mut Cursor::new(b"two")).unwrap();
    drop(guard);
    tig_store::publish(&mut first, &req, &mut Cursor::new(b"one")).unwrap();
    drop(first);
    drop(second);
    let operation = Digest::blob(req.operation.as_str().as_bytes());
    let intent = files(&root.join("coordination/tig-publications/operations"))
        .into_iter()
        .find(|path| {
            path.file_name().unwrap() == format!("{}.intent.json", operation.as_str()).as_str()
        })
        .unwrap();
    fs::remove_file(intent).unwrap();
    let mut reopened = ExistingPublisher::open(profile(root)).unwrap();
    assert!(matches!(
        tig_store::publish(&mut reopened, &req, &mut NoRead),
        Err(Error::CorruptRecord)
    ));
}

#[test]
fn interrupted_source_reopens_without_acknowledgement_then_publishes_exact_retry() {
    let temp = fixture();
    let root = temp.path();
    let req = request("interrupted", b"retained source");
    let mut publisher = ExistingPublisher::open(profile(root)).unwrap();
    assert!(tig_store::publish(&mut publisher, &req, &mut Cursor::new(b"short")).is_err());
    drop(publisher);
    let mut reopened = ExistingPublisher::open(profile(root)).unwrap();
    assert!(tig_store::recover(&mut reopened, &req).unwrap().is_none());
    assert!(files(&root.join("cache")).is_empty());
    tig_store::publish(&mut reopened, &req, &mut Cursor::new(b"retained source")).unwrap();
    drop(reopened);
    let mut reopened = ExistingPublisher::open(profile(root)).unwrap();
    assert!(tig_store::recover(&mut reopened, &req).unwrap().is_some());
}

#[test]
fn malformed_and_oversized_operation_records_refuse_before_source_io() {
    let temp = fixture();
    let root = temp.path();
    let req = request("record-validation", b"retained payload");
    let mut publisher = ExistingPublisher::open(profile(root)).unwrap();
    assert!(tig_store::publish(&mut publisher, &req, &mut Cursor::new(b"short")).is_err());
    drop(publisher);
    let intent = files(&root.join("coordination/tig-publications/operations"))
        .into_iter()
        .find(|path| path.to_string_lossy().ends_with(".intent.json"))
        .unwrap();
    let original = fs::read(&intent).unwrap();
    let mut unknown: serde_json::Value = serde_json::from_slice(&original).unwrap();
    unknown["unexpected"] = serde_json::json!(true);
    let mut schema: serde_json::Value = serde_json::from_slice(&original).unwrap();
    schema["schema"] = serde_json::json!("wrong schema");
    let duplicate = format!(
        "{{\"schema\":\"duplicate\",{}",
        std::str::from_utf8(&original)
            .unwrap()
            .trim_start_matches('{')
    );
    for bytes in [
        b"not json".to_vec(),
        vec![b' '; 65_537],
        serde_json::to_vec(&unknown).unwrap(),
        serde_json::to_vec(&schema).unwrap(),
        duplicate.into_bytes(),
    ] {
        fs::write(&intent, bytes).unwrap();
        let mut reopened = ExistingPublisher::open(profile(root)).unwrap();
        assert!(matches!(
            tig_store::publish(&mut reopened, &req, &mut NoRead),
            Err(Error::CorruptRecord)
        ));
        assert!(files(&root.join("cache")).is_empty());
    }
    fs::write(&intent, original).unwrap();
    let mut reopened = ExistingPublisher::open(profile(root)).unwrap();
    tig_store::publish(&mut reopened, &req, &mut Cursor::new(b"retained payload")).unwrap();
}

#[test]
fn independent_writer_finishes_while_another_stream_is_paused() {
    use std::sync::mpsc;
    use std::time::Duration;
    struct Paused {
        started: Option<mpsc::Sender<()>>,
        resume: mpsc::Receiver<()>,
        bytes: Cursor<Vec<u8>>,
    }
    impl Read for Paused {
        fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
            if let Some(started) = self.started.take() {
                started.send(()).map_err(io::Error::other)?;
                self.resume
                    .recv_timeout(Duration::from_secs(20))
                    .map_err(io::Error::other)?;
            }
            self.bytes.read(target)
        }
    }
    let temp = fixture();
    let root = temp.path().to_owned();
    let worker_root = root.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let req = request("paused-stream", b"first payload");
        let mut publisher = ExistingPublisher::open(profile(&worker_root)).unwrap();
        let mut source = Paused {
            started: Some(started_tx),
            resume: resume_rx,
            bytes: Cursor::new(b"first payload".to_vec()),
        };
        tig_store::publish(&mut publisher, &req, &mut source).is_ok()
    });
    started_rx.recv_timeout(Duration::from_secs(20)).unwrap();
    let mut publisher = ExistingPublisher::open(profile(&root)).unwrap();
    let req = request("independent-stream", b"second payload");
    let independent = tig_store::publish(&mut publisher, &req, &mut Cursor::new(b"second payload"));
    resume_tx.send(()).unwrap();
    let first = worker.join().unwrap();
    assert!(independent.is_ok(), "{independent:?}");
    assert!(first);
    assert_eq!(files(&root.join("cache")).len(), 2);
}

#[test]
fn reader_writer_preserves_normal_and_canonical_destination_paths() {
    let temp = fixture();
    let root = temp.path();
    let cache = root.join("cache");
    for (index, destination) in [cache.clone(), cache.canonicalize().unwrap()]
        .iter()
        .enumerate()
    {
        let bytes = format!("reader path {index}");
        let object = Object::from_bytes(bytes.as_bytes());
        let path = reel_assets::recover_from_reader(
            &root.join("artifact"),
            &mut Cursor::new(bytes.as_bytes()),
            destination,
            object.sha256.as_str(),
            object.bytes,
        )
        .unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes.as_bytes());
    }
}

#[cfg(unix)]
#[test]
fn symlink_shard_is_refused_before_reading_source_or_writing_outside_cache() {
    let temp = fixture();
    let root = temp.path();
    let outside = root.join("outside");
    fs::create_dir(&outside).unwrap();
    let req = request("escaped-shard", b"payload");
    let shard = root
        .join("cache/objects/sha256")
        .join(&req.object.sha256.as_str()[..2]);
    std::os::unix::fs::symlink(&outside, shard).unwrap();
    assert!(
        reel_assets::recover_from_reader(
            &root.join("artifact"),
            &mut NoRead,
            &root.join("cache"),
            req.object.sha256.as_str(),
            req.object.bytes,
        )
        .is_err()
    );
    assert!(files(&outside).is_empty());
}
