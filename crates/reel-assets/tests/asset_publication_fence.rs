#![cfg(windows)]
use reel_assets::inventory_evidence::sidecar::Store;
use reel_assets::inventory_evidence::sidecar::file_store::collector::fence::{Domain, Mode};
use reel_assets::inventory_evidence::sidecar::file_store::{FileStore, Limits};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn hash(prefix: char) -> String {
    prefix.to_string().repeat(64)
}
fn subjects(prefix: char) -> Vec<String> {
    vec![hash(prefix)]
}

#[test]
fn enrolled_session_ordinary_publication_and_recovery_cannot_skip_its_fence() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().canonicalize().unwrap();
    fs::create_dir(cache.join("staging")).unwrap();
    fs::create_dir(cache.join("fence")).unwrap();
    let domain = Domain::initialize(&cache.join("fence")).unwrap();
    let mut writer = FileStore::begin_fenced(
        &cache,
        &cache.join("staging"),
        "session",
        Limits::default(),
        &domain,
    )
    .unwrap();
    let data = b"enrolled ordinary node";
    let subject = format!("{:x}", Sha256::digest(data));
    let removed = domain
        .acquire(std::slice::from_ref(&subject), Mode::Remove, Duration::ZERO)
        .unwrap();
    assert!(writer.publish(data).is_err());
    assert!(!reel_assets::object(&cache, &subject).exists());
    let unrelated = writer.publish(b"unrelated enrolled node").unwrap();
    let intent: Value =
        serde_json::from_slice(&fs::read(writer.staging_path().join("session.json")).unwrap())
            .unwrap();
    assert_eq!(intent["schema"], "reel.inventory-sidecar.session.v2");
    assert!(intent["publication_fence"]["identity"].is_string());
    drop(writer);
    let mut resumed = FileStore::resume(&cache, &cache.join("staging"), "session").unwrap();
    assert!(resumed.publish(data).is_err());
    assert!(reel_assets::object(&cache, &unrelated.sha256).exists());
    drop(removed);
    assert_eq!(resumed.publish(data).unwrap().sha256, subject);
}

#[test]
fn enrolled_session_rejects_a_second_domain_even_inside_the_same_cache() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().canonicalize().unwrap();
    for name in ["staging", "one", "two"] {
        fs::create_dir(cache.join(name)).unwrap();
    }
    let one = Domain::initialize(&cache.join("one")).unwrap();
    let two = Domain::initialize(&cache.join("two")).unwrap();
    let mut writer = FileStore::begin_fenced(
        &cache,
        &cache.join("staging"),
        "session",
        Limits::default(),
        &one,
    )
    .unwrap();
    assert!(writer.publish_fenced(&two, b"node").is_err());
    assert_eq!(writer.staged_nodes(), 0);
    writer.publish_fenced(&one, b"node").unwrap();
}

#[test]
fn enrolled_recovery_rejects_missing_and_recreated_identical_header_controls() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().canonicalize().unwrap();
    for name in ["staging", "fence"] {
        fs::create_dir(cache.join(name)).unwrap();
    }
    let domain = Domain::initialize(&cache.join("fence")).unwrap();
    let mut writer = FileStore::begin_fenced(
        &cache,
        &cache.join("staging"),
        "session",
        Limits::default(),
        &domain,
    )
    .unwrap();
    let node = writer.publish(b"preserved staged node").unwrap();
    let intent_path = writer.staging_path().join("session.json");
    let intent = fs::read(&intent_path).unwrap();
    drop((writer, domain));
    let control = cache.join("fence/asset-publication-fence.v1.lock");
    // Keep the old physical file alive so its identity cannot be reused.
    fs::rename(&control, cache.join("old-fence-control")).unwrap();
    assert!(FileStore::resume(&cache, &cache.join("staging"), "session").is_err());
    let replacement = Domain::initialize(&cache.join("fence")).unwrap();
    assert!(FileStore::resume(&cache, &cache.join("staging"), "session").is_err());
    assert_eq!(fs::read(intent_path).unwrap(), intent);
    assert_eq!(
        fs::read(reel_assets::object(&cache, &node.sha256)).unwrap(),
        b"preserved staged node"
    );
    drop(replacement);
}

#[test]
fn enrolled_session_controls_reject_downgrade_and_partial_binding() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().canonicalize().unwrap();
    for name in ["staging", "fence"] {
        fs::create_dir(cache.join(name)).unwrap();
    }
    let domain = Domain::initialize(&cache.join("fence")).unwrap();
    let writer = FileStore::begin_fenced(
        &cache,
        &cache.join("staging"),
        "session",
        Limits::default(),
        &domain,
    )
    .unwrap();
    let path = writer.staging_path().join("session.json");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    drop(writer);
    for case in 0..5 {
        let mut changed = original.clone();
        match case {
            0 => {
                changed.as_object_mut().unwrap().remove("publication_fence");
            }
            1 => changed["schema"] = json!("reel.inventory-sidecar.session.v1"),
            2 => changed["publication_fence"]["schema"] = json!("unknown"),
            3 => changed["publication_fence"]["identity"] = json!("different"),
            _ => changed["publication_fence"]["directory"] = json!(cache.parent().unwrap()),
        }
        let raw = serde_json::to_vec(&changed).unwrap();
        fs::write(&path, &raw).unwrap();
        assert!(
            FileStore::resume(&cache, &cache.join("staging"), "session").is_err(),
            "case {case}"
        );
        assert_eq!(fs::read(&path).unwrap(), raw);
    }
    fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
    FileStore::resume(&cache, &cache.join("staging"), "session").unwrap();
}

#[test]
fn fenced_index_publication_refusal_is_retryable_and_other_nodes_proceed() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().canonicalize().unwrap();
    fs::create_dir(cache.join("staging")).unwrap();
    fs::create_dir(cache.join("fence")).unwrap();
    let domain = Domain::initialize(&cache.join("fence")).unwrap();
    let mut writer =
        FileStore::begin(&cache, &cache.join("staging"), "session", Limits::default()).unwrap();
    let data = b"synthetic index node";
    let subject = format!("{:x}", Sha256::digest(data));
    let remover = domain
        .acquire(std::slice::from_ref(&subject), Mode::Remove, Duration::ZERO)
        .unwrap();
    assert!(writer.publish_fenced(&domain, data).is_err());
    assert!(!reel_assets::object(&cache, &subject).exists());
    assert_eq!(
        fs::read_dir(writer.staging_path().join("objects"))
            .unwrap()
            .count(),
        0
    );
    let unrelated = writer
        .publish_fenced(&domain, b"unrelated index node")
        .unwrap();
    assert!(reel_assets::object(&cache, &unrelated.sha256).exists());
    drop(remover);
    let published = writer.publish_fenced(&domain, data).unwrap();
    assert_eq!(published.sha256, subject);
    assert_eq!(
        fs::read(reel_assets::object(&cache, &subject)).unwrap(),
        data
    );
    assert!(writer.staging_path().join("objects").join(subject).exists());
}

#[test]
fn fenced_index_publication_requires_the_same_cache_and_node_budget() {
    let temp = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let cache = temp.path().canonicalize().unwrap();
    fs::create_dir(cache.join("staging")).unwrap();
    fs::create_dir(cache.join("fence")).unwrap();
    let correct = Domain::initialize(&cache.join("fence")).unwrap();
    let foreign = Domain::initialize(other.path()).unwrap();
    let mut writer =
        FileStore::begin(&cache, &cache.join("staging"), "session", Limits::default()).unwrap();
    assert!(writer.publish_fenced(&foreign, b"node").is_err());
    assert!(
        writer
            .publish_fenced(
                &correct,
                &vec![0; Limits::default().node_bytes as usize + 1]
            )
            .is_err()
    );
    assert_eq!(
        fs::read_dir(writer.staging_path().join("objects"))
            .unwrap()
            .count(),
        0
    );
    writer.publish_fenced(&correct, b"node").unwrap();
}

#[test]
fn readers_share_exact_asset_while_remover_excludes_only_that_asset() {
    let temp = tempfile::tempdir().unwrap();
    let domain = Domain::initialize(temp.path()).unwrap();
    let before = fs::read_dir(temp.path())
        .unwrap()
        .map(|e| {
            let path = e.unwrap().path();
            (path.clone(), fs::read(path).unwrap())
        })
        .collect::<Vec<_>>();
    let first = domain
        .acquire(&subjects('a'), Mode::Publish, Duration::ZERO)
        .unwrap();
    let other_domain = Domain::open(temp.path()).unwrap();
    let second = other_domain
        .acquire(&subjects('a'), Mode::Publish, Duration::ZERO)
        .unwrap();
    assert!(
        domain
            .acquire(&subjects('a'), Mode::Remove, Duration::ZERO)
            .is_err()
    );
    let unrelated = domain
        .acquire(&subjects('b'), Mode::Remove, Duration::ZERO)
        .unwrap();
    drop(first);
    assert!(
        domain
            .acquire(&subjects('a'), Mode::Remove, Duration::ZERO)
            .is_err()
    );
    drop(second);
    let remover = domain
        .acquire(&subjects('a'), Mode::Remove, Duration::ZERO)
        .unwrap();
    assert!(
        domain
            .acquire(&subjects('a'), Mode::Publish, Duration::ZERO)
            .is_err()
    );
    assert!(
        domain
            .acquire(&subjects('a'), Mode::Remove, Duration::ZERO)
            .is_err()
    );
    drop((remover, unrelated));
    domain
        .acquire(&subjects('a'), Mode::Publish, Duration::ZERO)
        .unwrap();
    for (path, bytes) in before {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[test]
fn partial_batch_failure_releases_earlier_ranges_and_wait_is_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let domain = Domain::initialize(temp.path()).unwrap();
    let held = domain
        .acquire(&subjects('b'), Mode::Remove, Duration::ZERO)
        .unwrap();
    let started = Instant::now();
    assert!(
        domain
            .acquire(
                &[hash('a'), hash('b')],
                Mode::Remove,
                Duration::from_millis(30)
            )
            .is_err()
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    domain
        .acquire(&subjects('a'), Mode::Remove, Duration::ZERO)
        .unwrap();
    drop(held);
    domain
        .acquire(&[hash('b'), hash('a')], Mode::Remove, Duration::ZERO)
        .unwrap();
}

#[test]
fn collisions_are_conservative_and_high_offsets_never_extend_the_control() {
    let temp = tempfile::tempdir().unwrap();
    let domain = Domain::initialize(temp.path()).unwrap();
    let first = hash('f');
    let mut second = first.clone();
    second.replace_range(63..64, "e");
    let held = domain
        .acquire(std::slice::from_ref(&first), Mode::Remove, Duration::ZERO)
        .unwrap();
    assert!(
        domain
            .acquire(std::slice::from_ref(&second), Mode::Publish, Duration::ZERO)
            .is_err()
    );
    drop(held);
    domain
        .acquire(&[first, second], Mode::Remove, Duration::ZERO)
        .unwrap();
    let path = fs::read_dir(temp.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        fs::metadata(path).unwrap().len(),
        b"reel.asset-publication-fence.v1\n".len() as u64
    );
}

#[test]
fn invalid_requests_and_missing_or_corrupt_controls_never_initialize_implicitly() {
    let temp = tempfile::tempdir().unwrap();
    assert!(Domain::open(temp.path()).is_err());
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    assert!(Domain::initialize(&temp.path().join("absent")).is_err());
    let domain = Domain::initialize(temp.path()).unwrap();
    for request in [
        vec![],
        vec![hash('a'), hash('a')],
        vec!["bad".into()],
        vec![hash('A')],
        vec![hash('a'); 513],
    ] {
        assert!(
            domain
                .acquire(&request, Mode::Publish, Duration::ZERO)
                .is_err()
        );
    }
    assert!(
        domain
            .acquire(&subjects('a'), Mode::Publish, Duration::from_secs(3))
            .is_err()
    );
    let path = fs::read_dir(temp.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::write(&path, b"partial").unwrap();
    assert!(
        domain
            .acquire(&subjects('a'), Mode::Publish, Duration::ZERO)
            .is_err()
    );
    assert!(Domain::open(temp.path()).is_err());
    assert!(Domain::initialize(temp.path()).is_err());
    assert_eq!(fs::read(path).unwrap(), b"partial");
}

#[test]
fn pinned_domain_prevents_control_replacement_and_reinitialization_preserves_locks() {
    let temp = tempfile::tempdir().unwrap();
    let domain = Domain::initialize(temp.path()).unwrap();
    let held = domain
        .acquire(&subjects('a'), Mode::Remove, Duration::ZERO)
        .unwrap();
    let path = fs::read_dir(temp.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(fs::remove_file(&path).is_err());
    let second = Domain::initialize(temp.path()).unwrap();
    assert!(
        second
            .acquire(&subjects('a'), Mode::Publish, Duration::ZERO)
            .is_err()
    );
    second
        .acquire(&subjects('b'), Mode::Publish, Duration::ZERO)
        .unwrap();
    drop((held, domain, second));
    fs::remove_file(path).unwrap();
}

struct RunningChild(Child);
impl Drop for RunningChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn child(root: &Path, mode: &str) -> RunningChild {
    use std::os::windows::process::CommandExt;
    let process = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "subprocess_fence", "--nocapture"])
        .env("REEL_FENCE_TEST_DIRECTORY", root)
        .env("REEL_FENCE_TEST_MODE", mode)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()
        .unwrap();
    let mut process = RunningChild(process);
    let started = Instant::now();
    while !root.join("ready").exists() {
        assert!(
            process.0.try_wait().unwrap().is_none(),
            "child exited before readiness"
        );
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "child readiness timeout"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    process
}

#[test]
fn killed_publisher_releases_its_range_without_blocking_other_assets() {
    let temp = tempfile::tempdir().unwrap();
    let domain = Domain::initialize(temp.path()).unwrap();
    let mut publisher = child(temp.path(), "publish");
    domain
        .acquire(&subjects('a'), Mode::Publish, Duration::ZERO)
        .unwrap();
    assert!(
        domain
            .acquire(&subjects('a'), Mode::Remove, Duration::ZERO)
            .is_err()
    );
    domain
        .acquire(&subjects('b'), Mode::Remove, Duration::ZERO)
        .unwrap();
    publisher.0.kill().unwrap();
    publisher.0.wait().unwrap();
    domain
        .acquire(&subjects('a'), Mode::Remove, Duration::ZERO)
        .unwrap();
}

#[test]
fn killed_remover_releases_its_range_without_stealing_other_owners() {
    let temp = tempfile::tempdir().unwrap();
    let domain = Domain::initialize(temp.path()).unwrap();
    let unrelated = domain
        .acquire(&subjects('b'), Mode::Publish, Duration::ZERO)
        .unwrap();
    let mut remover = child(temp.path(), "remove");
    assert!(
        domain
            .acquire(&subjects('a'), Mode::Publish, Duration::ZERO)
            .is_err()
    );
    assert!(
        domain
            .acquire(&subjects('a'), Mode::Remove, Duration::ZERO)
            .is_err()
    );
    remover.0.kill().unwrap();
    remover.0.wait().unwrap();
    domain
        .acquire(&subjects('a'), Mode::Publish, Duration::ZERO)
        .unwrap();
    assert!(
        domain
            .acquire(&subjects('b'), Mode::Remove, Duration::ZERO)
            .is_err()
    );
    drop(unrelated);
    domain
        .acquire(&subjects('b'), Mode::Remove, Duration::ZERO)
        .unwrap();
}

#[test]
fn subprocess_fence() {
    let Some(root) = std::env::var_os("REEL_FENCE_TEST_DIRECTORY") else {
        return;
    };
    let root = PathBuf::from(root);
    let domain = Domain::open(&root).unwrap();
    let mode = match std::env::var("REEL_FENCE_TEST_MODE").unwrap().as_str() {
        "publish" => Mode::Publish,
        "remove" => Mode::Remove,
        _ => panic!("bad test mode"),
    };
    let _held = domain
        .acquire(&subjects('a'), mode, Duration::ZERO)
        .unwrap();
    fs::write(root.join("ready"), b"ready").unwrap();
    loop {
        std::thread::park_timeout(Duration::from_secs(1));
    }
}
