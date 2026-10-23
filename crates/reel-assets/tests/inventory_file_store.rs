use reel_assets::{
    inventory_evidence::{
        self as evidence,
        sidecar::{
            self, Store,
            file_store::{FileStore, Limits, ReadOnlyStore},
        },
    },
    object,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn blob(bytes: &[u8]) -> evidence::Blob {
    evidence::Blob {
        sha256: hash(bytes),
        bytes: bytes.len() as u64,
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    cache: PathBuf,
    staging: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().canonicalize().unwrap();
        let staging = cache.join("existing-staging");
        fs::create_dir(&staging).unwrap();
        Self {
            _dir: dir,
            cache,
            staging,
        }
    }
    fn begin(&self, id: &str) -> FileStore {
        FileStore::begin(&self.cache, &self.staging, id, Limits::default()).unwrap()
    }
}
fn read(store: &mut FileStore, b: &evidence::Blob) -> Vec<u8> {
    let mut raw = Vec::new();
    store.open(b).unwrap().read_to_end(&mut raw).unwrap();
    raw
}
fn prepared() -> evidence::PreparedInventory {
    let rows:Vec<_>=(0..1000).map(|i|json!({"sha256":hash(format!("fixture-{i}").as_bytes()),"bytes":i,"registered":true,"copies":[]})).collect();
    let raw = serde_json::to_vec(
        &json!({"schema":evidence::SOURCE_SCHEMA,"project_id":"synthetic",
        "profile_sha256":hash(b"profile"),"registry_generation":1,"tiers":[],"objects":rows}),
    )
    .unwrap();
    let source = evidence::Source {
        encoded: blob(&raw),
        decoded: blob(&raw),
        encoding: evidence::Encoding::Identity,
        schema: evidence::SOURCE_SCHEMA.into(),
        project_id: "synthetic".into(),
        authority_namespace: "fixture".into(),
        adapter_version: evidence::ADAPTER_VERSION,
    };
    evidence::prepare(raw.as_slice(), &source, evidence::Limits::default()).unwrap()
}

#[test]
fn publish_seal_drop_resume_and_query_use_the_existing_cache_layout() {
    let f = Fixture::new();
    let mut store = f.begin("build");
    let index = prepared();
    let source = index.source().clone();
    let root = sidecar::publish(&mut store, &index, sidecar::BuildLimits::default()).unwrap();
    assert!(object(&f.cache, &root.sha256).is_file());
    assert_eq!(store.staged_nodes(), 18);
    store.seal(&root).unwrap();
    store.seal(&root).unwrap();
    assert!(store.publish(b"late write").is_err());
    drop(store);
    drop(index);
    let mut reopened = FileStore::resume(&f.cache, &f.staging, "build").unwrap();
    assert!(reopened.sealed_root() == Some(&root));
    let result = sidecar::lookup(
        &mut reopened,
        &root,
        &source,
        &hash(b"fixture-713"),
        sidecar::ReadLimits::default(),
    )
    .unwrap();
    assert_eq!(result.nodes_read, 3);
    assert_eq!(result.observation.unwrap().source_record, 713);
}

#[test]
fn read_only_lookup_does_not_resume_staging_or_acquire_writer_ownership() {
    let f = Fixture::new();
    let mut writer = f.begin("writer");
    let index = prepared();
    let root = sidecar::publish(&mut writer, &index, sidecar::BuildLimits::default()).unwrap();
    // An invalid unrelated staging session must not affect exact CAS reads.
    fs::create_dir(f.staging.join("unrelated-broken-session")).unwrap();
    fs::write(
        f.staging.join("unrelated-broken-session/session.json"),
        b"partial",
    )
    .unwrap();
    let mut reader = ReadOnlyStore::new(&f.cache, 256 * 1024).unwrap();
    let result = sidecar::lookup(
        &mut reader,
        &root,
        index.source(),
        &hash(b"fixture-713"),
        sidecar::ReadLimits::default(),
    )
    .unwrap();
    assert_eq!(result.nodes_read, 3);
    assert!(result.rows_inspected <= 128);
    assert!(reader.publish(b"no write").is_err());
    assert!(
        writer
            .publish(b"writer still owns its independent session")
            .is_ok()
    );
}

#[test]
fn named_recovery_enforces_budgets_and_rejects_unexpected_staged_entries() {
    for case in ["count", "bytes", "unknown", "directory"] {
        let f = Fixture::new();
        let store = FileStore::begin(
            &f.cache,
            &f.staging,
            "bounded",
            Limits {
                nodes: 1,
                staged_bytes: 4,
                ..Limits::default()
            },
        )
        .unwrap();
        let objects = store.staging_path().join("objects");
        match case {
            "count" => {
                fs::write(objects.join(hash(b"one")), b"one").unwrap();
                fs::write(objects.join(hash(b"two")), b"two").unwrap();
            }
            "bytes" => fs::write(objects.join(hash(b"large")), b"large").unwrap(),
            "unknown" => fs::write(objects.join("unrecognized"), b"x").unwrap(),
            _ => fs::create_dir(objects.join(hash(b"directory"))).unwrap(),
        }
        drop(store);
        assert!(
            FileStore::resume(&f.cache, &f.staging, "bounded").is_err(),
            "{case}"
        );
        assert!(objects.is_dir());
    }
}

#[test]
fn sessions_are_independent_and_same_session_has_one_owner() {
    let f = Fixture::new();
    let mut first = f.begin("first");
    let mut second = f.begin("second");
    assert!(FileStore::begin(&f.cache, &f.staging, "first", Limits::default()).is_err());
    assert!(FileStore::resume(&f.cache, &f.staging, "first").is_err());
    let a = first.publish(b"same").unwrap();
    let b = second.publish(b"same").unwrap();
    assert!(a == b);
    assert_eq!(first.staged_nodes(), 1);
    assert_eq!(second.staged_nodes(), 1);
    assert_eq!(read(&mut first, &a), b"same");
    assert_eq!(read(&mut second, &b), b"same");
    drop(first);
    let mut first = FileStore::resume(&f.cache, &f.staging, "first").unwrap();
    assert!(first.publish(b"another").is_ok());
    assert!(second.publish(b"other session remains usable").is_ok());
}

#[test]
fn duplicate_publication_is_budgeted_once_and_survives_resume() {
    let f = Fixture::new();
    let limits = Limits {
        node_bytes: 256,
        staged_bytes: 4,
        nodes: 1,
    };
    let mut store = FileStore::begin(&f.cache, &f.staging, "bounded", limits).unwrap();
    let b = store.publish(b"same").unwrap();
    assert!(store.publish(b"same").unwrap() == b);
    assert_eq!(store.staged_bytes(), 4);
    assert_eq!(store.staged_nodes(), 1);
    drop(store);
    let mut store = FileStore::resume(&f.cache, &f.staging, "bounded").unwrap();
    assert!(store.publish(b"same").unwrap() == b);
    assert!(store.publish(b"x").is_err());
    assert!(!object(&f.cache, &hash(b"x")).exists());
    assert!(store.publish(b"same").is_err()); // failure requires explicit recovery
}

#[test]
fn stage_protects_data_even_if_the_cas_name_is_removed() {
    let f = Fixture::new();
    let mut store = f.begin("protected");
    let b = store.publish(b"protected bytes").unwrap();
    let staged = store.staging_path().join("objects").join(&b.sha256);
    // Fault injection only inside this synthetic temporary cache.
    fs::remove_file(object(&f.cache, &b.sha256)).unwrap();
    assert_eq!(fs::read(&staged).unwrap(), b"protected bytes");
    assert!(store.open(&b).is_err()); // no silent staging fallback
    drop(store);
    let mut store = FileStore::resume(&f.cache, &f.staging, "protected").unwrap();
    assert!(store.publish(b"protected bytes").unwrap() == b);
    assert_eq!(read(&mut store, &b), b"protected bytes");
}

#[test]
fn preexisting_corrupt_winner_is_preserved_and_session_requires_recovery() {
    let f = Fixture::new();
    let expected = blob(b"expected");
    let path = object(&f.cache, &expected.sha256);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"corrupt!").unwrap();
    let mut store = f.begin("corrupt");
    assert!(store.publish(b"expected").is_err());
    assert_eq!(fs::read(&path).unwrap(), b"corrupt!");
    assert!(store.publish(b"anything else").is_err());
    assert!(store.open(&expected).is_err());
}

#[test]
fn incomplete_staged_write_blocks_resume_without_deleting_or_repairing() {
    let f = Fixture::new();
    let store = f.begin("partial");
    let b = blob(b"complete");
    let path = store.staging_path().join("objects").join(&b.sha256);
    fs::write(&path, b"par").unwrap();
    drop(store);
    assert!(FileStore::resume(&f.cache, &f.staging, "partial").is_err());
    assert_eq!(fs::read(path).unwrap(), b"par");
    assert!(!object(&f.cache, &b.sha256).exists());
}

#[test]
fn complete_stage_before_cas_publication_is_resumable_by_exact_retry() {
    let f = Fixture::new();
    let store = f.begin("interrupted");
    let b = blob(b"complete");
    fs::write(
        store.staging_path().join("objects").join(&b.sha256),
        b"complete",
    )
    .unwrap();
    drop(store);
    let mut store = FileStore::resume(&f.cache, &f.staging, "interrupted").unwrap();
    assert!(store.open(&b).is_err());
    assert!(store.publish(b"complete").unwrap() == b);
}

#[test]
fn missing_and_corrupt_seals_never_release_staged_nodes() {
    let f = Fixture::new();
    let mut store = f.begin("seal");
    let b = store.publish(b"node").unwrap();
    let staged = store.staging_path().join("objects").join(&b.sha256);
    assert!(store.seal(&blob(b"unrelated")).is_err());
    assert!(staged.is_file());
    fs::write(store.staging_path().join("root.json"), b"partial").unwrap();
    drop(store);
    assert!(FileStore::resume(&f.cache, &f.staging, "seal").is_err());
    assert_eq!(fs::read(staged).unwrap(), b"node");
}

#[test]
fn invalid_paths_ids_budgets_and_scope_are_rejected() {
    let f = Fixture::new();
    for id in ["", "../escape", "a/b", "a\\b", "a:b", ".", "CON."] {
        assert!(FileStore::begin(&f.cache, &f.staging, id, Limits::default()).is_err());
    }
    assert!(FileStore::begin(Path::new("relative"), &f.staging, "x", Limits::default()).is_err());
    assert!(FileStore::begin(&f.cache, &f.cache, "x", Limits::default()).is_err());
    assert!(FileStore::begin(&f.cache, &f.cache.join("missing"), "x", Limits::default()).is_err());
    for limits in [
        Limits {
            node_bytes: 0,
            ..Limits::default()
        },
        Limits {
            nodes: 0,
            ..Limits::default()
        },
        Limits {
            staged_bytes: 0,
            ..Limits::default()
        },
        Limits {
            node_bytes: 4 * 1024 * 1024 + 1,
            ..Limits::default()
        },
        Limits {
            nodes: 1_000_001,
            ..Limits::default()
        },
    ] {
        assert!(FileStore::begin(&f.cache, &f.staging, "invalid", limits).is_err());
    }
    let store = f.begin("scope");
    let intent = store.staging_path().join("session.json");
    drop(store);
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&intent).unwrap()).unwrap();
    value["id"] = json!("other");
    fs::write(&intent, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(FileStore::resume(&f.cache, &f.staging, "scope").is_err());
}

#[test]
fn invalid_descriptors_and_node_limits_fail_without_reading_other_paths() {
    let f = Fixture::new();
    let mut store = FileStore::begin(
        &f.cache,
        &f.staging,
        "small",
        Limits {
            node_bytes: 2,
            ..Limits::default()
        },
    )
    .unwrap();
    assert!(
        store
            .open(&evidence::Blob {
                sha256: "../outside".into(),
                bytes: 1
            })
            .is_err()
    );
    assert!(store.open(&blob(b"oversized")).is_err());
    assert!(store.publish(b"too big").is_err());
    assert_eq!(store.staged_nodes(), 0);
    assert_eq!(store.staged_bytes(), 0);
}

#[cfg(windows)]
#[test]
fn directory_junction_in_cas_path_is_rejected() {
    let f = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    // Only synthetic temp directories; cmd is used for junction creation, never
    // recursive removal/moving. TempDir owns cleanup of the fixture afterward.
    let status = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(f.cache.join("objects"))
        .arg(outside.path())
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let mut store = f.begin("junction");
    assert!(store.publish(b"data").is_err());
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn symlink_in_cas_path_is_rejected() {
    let f = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), f.cache.join("objects")).unwrap();
    let mut store = f.begin("symlink");
    assert!(store.publish(b"data").is_err());
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}

struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn worker(f: &Fixture, id: &str, mode: &str) -> Worker {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", "subprocess_writer", "--nocapture"])
        .env("REEL_SIDECAR_TEST_CACHE", &f.cache)
        .env("REEL_SIDECAR_TEST_STAGING", &f.staging)
        .env("REEL_SIDECAR_TEST_ID", id)
        .env("REEL_SIDECAR_TEST_MODE", mode);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    Worker(cmd.spawn().unwrap())
}
fn wait_for(mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "fixture child timeout"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn subprocess_writer() {
    let Some(cache) = std::env::var_os("REEL_SIDECAR_TEST_CACHE") else {
        return;
    };
    let staging = PathBuf::from(std::env::var_os("REEL_SIDECAR_TEST_STAGING").unwrap());
    let cache = PathBuf::from(cache);
    let id = std::env::var("REEL_SIDECAR_TEST_ID").unwrap();
    let mode = std::env::var("REEL_SIDECAR_TEST_MODE").unwrap();
    let mut store = FileStore::begin(&cache, &staging, &id, Limits::default()).unwrap();
    fs::write(cache.join(format!("{id}.ready")), b"ready").unwrap();
    wait_for(|| cache.join("go").exists());
    let b = store.publish(b"concurrent metadata node").unwrap();
    if mode == "kill" {
        fs::write(cache.join(format!("{id}.published")), b"published").unwrap();
        // Parent kills this child after publication, without Rust Drop running.
        loop {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    store.seal(&b).unwrap();
}

#[test]
fn separate_processes_publish_same_and_independent_sessions_without_a_global_lock() {
    let f = Fixture::new();
    let mut first = worker(&f, "first", "finish");
    let mut second = worker(&f, "second", "finish");
    wait_for(|| f.cache.join("first.ready").exists() && f.cache.join("second.ready").exists());
    fs::write(f.cache.join("go"), b"go").unwrap();
    wait_for(|| first.0.try_wait().unwrap().is_some() && second.0.try_wait().unwrap().is_some());
    assert!(first.0.wait().unwrap().success());
    assert!(second.0.wait().unwrap().success());
    let b = blob(b"concurrent metadata node");
    for id in ["first", "second"] {
        let mut store = FileStore::resume(&f.cache, &f.staging, id).unwrap();
        assert!(store.sealed_root() == Some(&b));
        assert_eq!(read(&mut store, &b), b"concurrent metadata node");
    }
}

#[test]
fn killed_writer_retains_protection_and_releases_only_its_session_ownership() {
    let f = Fixture::new();
    let mut child = worker(&f, "crash", "kill");
    wait_for(|| f.cache.join("crash.ready").exists());
    assert!(FileStore::resume(&f.cache, &f.staging, "crash").is_err());
    let mut other = f.begin("unrelated");
    assert!(other.publish(b"independent").is_ok());
    fs::write(f.cache.join("go"), b"go").unwrap();
    wait_for(|| f.cache.join("crash.published").exists());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let mut store = FileStore::resume(&f.cache, &f.staging, "crash").unwrap();
    assert!(store.sealed_root().is_none());
    let b = store.publish(b"concurrent metadata node").unwrap();
    assert_eq!(store.staged_nodes(), 1);
    store.seal(&b).unwrap();
    assert!(other.publish(b"still independent").is_ok());
}
