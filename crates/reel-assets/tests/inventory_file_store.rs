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
    if mode == "retire-kill" {
        let index = prepared();
        let root = sidecar::publish(&mut store, &index, sidecar::BuildLimits::default()).unwrap();
        store.seal(&root).unwrap();
        let retained = cache.join("retained-indexes");
        store.handoff(&retained, &root, index.source()).unwrap();
        let plan = store
            .plan_retirement(&retained, &root, index.source())
            .unwrap();
        assert_eq!(
            store.apply_retirement(&plan, 5).unwrap().links_remaining,
            13
        );
        fs::write(
            cache.join(format!("{id}.published")),
            serde_json::to_vec(&json!({"sha256":root.sha256,"bytes":root.bytes})).unwrap(),
        )
        .unwrap();
        loop {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
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

fn capacity_config(f: &Fixture) -> reel_assets::Config {
    let coordination = f.cache.join("shared-coordination");
    fs::create_dir_all(&coordination).unwrap();
    serde_json::from_value(json!({"path_policy":"configured-local-roots","cache_roots":[f.cache],"archive_roots":[],
        "coordination_root":coordination,"capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0,"max_object_bytes":8u64*1024*1024*1024}})).unwrap()
}
fn admissions(config: &reel_assets::Config) -> usize {
    fs::read_dir(
        config
            .coordination_root
            .as_ref()
            .unwrap()
            .join("admissions"),
    )
    .unwrap()
    .map(|e| e.unwrap().path())
    .filter(|p| p.extension().is_some_and(|e| e == "json"))
    .count()
}
#[test]
fn admitted_sessions_share_the_existing_ledger_without_holding_its_lock() {
    let f = Fixture::new();
    let config = capacity_config(&f);
    let limits = Limits {
        nodes: 64,
        staged_bytes: 1024 * 1024,
        ..Limits::default()
    };
    let mut first = FileStore::begin_admitted(
        &f.cache,
        &config,
        &f.cache,
        &f.staging,
        "first",
        limits,
        64 * 4096,
    )
    .unwrap();
    let other_worktree = f.cache.join("another-worktree");
    fs::create_dir(&other_worktree).unwrap();
    let second = FileStore::begin_admitted(
        &other_worktree,
        &config,
        &f.cache,
        &f.staging,
        "second",
        limits,
        64 * 4096,
    )
    .unwrap();
    assert_eq!(admissions(&config), 2);
    let metadata =
        reel_assets::admission::lock(config.coordination_root.as_ref().unwrap()).unwrap();
    drop(metadata);
    assert!(first.publish(b"data").is_ok());
    drop(second);
    assert_eq!(admissions(&config), 1);
    drop(first);
    assert_eq!(admissions(&config), 0);
    let mut raw = FileStore::resume(&f.cache, &f.staging, "first").unwrap();
    assert!(raw.publish(b"bypass").is_err());
    drop(raw);
    let mut recovered =
        FileStore::resume_admitted(&f.cache, &config, &f.cache, &f.staging, "first").unwrap();
    assert!(recovered.publish(b"resumed").is_ok());
    assert_eq!(admissions(&config), 1);
}
#[test]
fn admission_denial_creates_no_staging_and_capacity_scope_cannot_change() {
    let f = Fixture::new();
    let mut config = capacity_config(&f);
    config.capacity.minimum_free_bytes = u64::MAX;
    let limits = Limits {
        nodes: 1,
        staged_bytes: 256,
        ..Limits::default()
    };
    assert!(
        FileStore::begin_admitted(
            &f.cache, &config, &f.cache, &f.staging, "denied", limits, 4096
        )
        .is_err()
    );
    assert!(!f.staging.join("denied").exists());
    config.capacity.minimum_free_bytes = 0;
    let writer = FileStore::begin_admitted(
        &f.cache, &config, &f.cache, &f.staging, "scope", limits, 4096,
    )
    .unwrap();
    drop(writer);
    let other = f.cache.join("other-coordination");
    fs::create_dir(&other).unwrap();
    config.coordination_root = Some(other);
    assert!(FileStore::resume_admitted(&f.cache, &config, &f.cache, &f.staging, "scope").is_err());
    config.coordination_root = None;
    assert!(
        FileStore::begin_admitted(
            &f.cache,
            &config,
            &f.cache,
            &f.staging,
            "not-shared",
            limits,
            4096
        )
        .is_err()
    );
    assert!(FileStore::capacity_budget(limits, 4095).is_err());
    assert!(FileStore::capacity_budget(limits, u64::MAX).is_err());
}
#[test]
fn a_live_existing_reservation_reduces_sidecar_capacity() {
    let f = Fixture::new();
    let config = capacity_config(&f);
    // Reserve nearly all currently free bytes without writing payloads. The
    // second request is deliberately larger than the small remaining allowance.
    let free = fs2::available_space(&f.cache).unwrap();
    let held = free.saturating_sub(8 * 1024 * 1024);
    let mut config = config;
    config.capacity.max_object_bytes = u64::MAX;
    let reservation =
        reel_assets::admission::reserve_on(&f.cache, &config, &f.cache, held).unwrap();
    let limits = Limits {
        nodes: 1,
        staged_bytes: 64 * 1024 * 1024,
        ..Limits::default()
    };
    assert!(
        FileStore::begin_admitted(
            &f.cache, &config, &f.cache, &f.staging, "competes", limits, 4096
        )
        .is_err()
    );
    assert!(!f.staging.join("competes").exists());
    drop(reservation);
    assert!(
        FileStore::begin_admitted(
            &f.cache, &config, &f.cache, &f.staging, "competes", limits, 4096
        )
        .is_ok()
    );
}
fn retained_parent(f: &Fixture) -> PathBuf {
    let root = f.cache.join("retained-indexes");
    fs::create_dir(&root).unwrap();
    root
}
fn sealed(f: &Fixture, id: &str) -> (FileStore, evidence::PreparedInventory, evidence::Blob) {
    let mut writer = f.begin(id);
    let index = prepared();
    let root = sidecar::publish(&mut writer, &index, sidecar::BuildLimits::default()).unwrap();
    writer.seal(&root).unwrap();
    (writer, index, root)
}
#[test]
fn whole_closure_handoff_dry_run_retire_and_reopen_preserve_queries() {
    let f = Fixture::new();
    let retained = retained_parent(&f);
    let (mut writer, index, root) = sealed(&f, "handoff");
    writer.handoff(&retained, &root, index.source()).unwrap();
    writer.handoff(&retained, &root, index.source()).unwrap();
    let before = writer.staged_nodes();
    let plan = writer
        .plan_retirement(&retained, &root, index.source())
        .unwrap();
    assert_eq!(plan.nodes(), 18);
    assert_eq!(plan.payload_bytes_reclaimed(), 0);
    assert_eq!(writer.staged_nodes(), before);
    assert!(plan.logical_bytes() > 0);
    assert!(!writer.staging_path().join("retirement.json").exists());
    let result = writer.apply_retirement(&plan, usize::MAX).unwrap();
    assert_eq!(result.links_removed, 18);
    assert_eq!(result.links_remaining, 0);
    assert_eq!(writer.staged_bytes(), 0);
    drop(writer);
    let mut recovered = FileStore::resume(&f.cache, &f.staging, "handoff").unwrap();
    assert!(recovered.sealed_root() == Some(&root));
    let retry = recovered
        .plan_retirement(&retained, &root, index.source())
        .unwrap();
    assert_eq!(
        recovered.apply_retirement(&retry, 1).unwrap().links_removed,
        0
    );
    let mut reader = ReadOnlyStore::new(&f.cache, 256 * 1024).unwrap();
    assert!(
        sidecar::lookup(
            &mut reader,
            &root,
            index.source(),
            &hash(b"fixture-713"),
            sidecar::ReadLimits::default()
        )
        .unwrap()
        .observation
        .is_some()
    );
    assert!(
        retained
            .join(&root.sha256)
            .join("objects")
            .join(&root.sha256)
            .is_file()
    );
}
#[test]
fn staged_retirement_in_blocks_resumes_without_republishing_or_releasing_other_sessions() {
    let f = Fixture::new();
    let retained = retained_parent(&f);
    let (mut writer, index, root) = sealed(&f, "blocks");
    let mut other = f.begin("other");
    let protected = other.publish(b"other-session").unwrap();
    writer.handoff(&retained, &root, index.source()).unwrap();
    let plan = writer
        .plan_retirement(&retained, &root, index.source())
        .unwrap();
    let progress = writer.apply_retirement(&plan, 5).unwrap();
    assert_eq!(progress.links_removed, 5);
    assert_eq!(progress.links_remaining, 13);
    drop(writer);
    let mut writer = FileStore::resume(&f.cache, &f.staging, "blocks").unwrap();
    let plan = writer
        .plan_retirement(&retained, &root, index.source())
        .unwrap();
    assert_eq!(
        writer.apply_retirement(&plan, 100).unwrap().links_remaining,
        0
    );
    assert_eq!(read(&mut other, &protected), b"other-session");
}
#[test]
fn absent_or_changed_retained_evidence_blocks_before_any_staging_unlink() {
    for bad in [
        "missing-receipt",
        "missing-link",
        "corrupt-link",
        "wrong-source",
    ] {
        let f = Fixture::new();
        let retained = retained_parent(&f);
        let (mut writer, index, root) = sealed(&f, "strict");
        writer.handoff(&retained, &root, index.source()).unwrap();
        let plan = writer
            .plan_retirement(&retained, &root, index.source())
            .unwrap();
        let anchor = retained.join(&root.sha256);
        let path = anchor.join("objects").join(&root.sha256);
        match bad {
            "missing-receipt" => fs::remove_file(anchor.join("root.json")).unwrap(),
            "missing-link" => fs::remove_file(&path).unwrap(),
            "corrupt-link" => {
                fs::remove_file(&path).unwrap();
                fs::write(&path, b"corrupt replacement").unwrap();
            }
            _ => {
                let mut value: serde_json::Value =
                    serde_json::from_slice(&fs::read(anchor.join("root.json")).unwrap()).unwrap();
                value["source"]["authority"] = json!("wrong");
                fs::remove_file(anchor.join("root.json")).unwrap();
                fs::write(
                    anchor.join("root.json"),
                    serde_json::to_vec(&value).unwrap(),
                )
                .unwrap();
            }
        }
        assert!(writer.apply_retirement(&plan, 100).is_err(), "{bad}");
        assert_eq!(writer.staged_nodes(), 18);
        assert_eq!(
            fs::read_dir(writer.staging_path().join("objects"))
                .unwrap()
                .count(),
            18
        );
    }
}
#[test]
fn retirement_requires_exact_session_closure_and_rejects_wrong_plans() {
    let f = Fixture::new();
    let retained = retained_parent(&f);
    let (mut first, index, root) = sealed(&f, "first");
    first.handoff(&retained, &root, index.source()).unwrap();
    let plan = first
        .plan_retirement(&retained, &root, index.source())
        .unwrap();
    let (mut second, _, _) = sealed(&f, "second");
    assert!(second.apply_retirement(&plan, 10).is_err());
    assert_eq!(second.staged_nodes(), 18);
    let extra = first
        .staging_path()
        .join("objects")
        .join(hash(b"unknown node"));
    fs::write(&extra, b"unknown node").unwrap();
    assert!(first.apply_retirement(&plan, 10).is_err());
    assert_eq!(first.staged_nodes(), 18);
    assert!(first.handoff(&retained, &root, index.source()).is_err());
    let mut wrong = index.source().clone();
    wrong.authority_namespace = "wrong".into();
    assert!(second.handoff(&retained, &root, &wrong).is_err());
    assert!(second.handoff(&f.staging, &root, index.source()).is_err());
    assert!(second.apply_retirement(&plan, 0).is_err());
}
#[test]
fn admitted_handoff_and_retirement_do_not_bypass_capacity_on_recovery() {
    let f = Fixture::new();
    let retained = retained_parent(&f);
    let config = capacity_config(&f);
    let index = prepared();
    let limits = Limits {
        nodes: 64,
        staged_bytes: 1024 * 1024,
        ..Limits::default()
    };
    let mut writer = FileStore::begin_admitted(
        &f.cache,
        &config,
        &f.cache,
        &f.staging,
        "admitted",
        limits,
        64 * 4096,
    )
    .unwrap();
    let root = sidecar::publish(&mut writer, &index, sidecar::BuildLimits::default()).unwrap();
    writer.seal(&root).unwrap();
    writer.handoff(&retained, &root, index.source()).unwrap();
    drop(writer);
    let mut raw = FileStore::resume(&f.cache, &f.staging, "admitted").unwrap();
    let plan = raw
        .plan_retirement(&retained, &root, index.source())
        .unwrap();
    assert!(raw.apply_retirement(&plan, 1).is_err());
    drop(raw);
    let mut writer =
        FileStore::resume_admitted(&f.cache, &config, &f.cache, &f.staging, "admitted").unwrap();
    let plan = writer
        .plan_retirement(&retained, &root, index.source())
        .unwrap();
    assert_eq!(
        writer.apply_retirement(&plan, 100).unwrap().links_remaining,
        0
    );
}

#[test]
fn killed_process_after_partial_retirement_can_resume_the_exact_remainder() {
    let f = Fixture::new();
    let retained = retained_parent(&f);
    let mut child = worker(&f, "retiring", "retire-kill");
    wait_for(|| f.cache.join("retiring.ready").exists());
    fs::write(f.cache.join("go"), b"go").unwrap();
    wait_for(|| f.cache.join("retiring.published").exists());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let v: serde_json::Value =
        serde_json::from_slice(&fs::read(f.cache.join("retiring.published")).unwrap()).unwrap();
    let root = evidence::Blob {
        sha256: v["sha256"].as_str().unwrap().into(),
        bytes: v["bytes"].as_u64().unwrap(),
    };
    let index = prepared();
    let mut store = FileStore::resume(&f.cache, &f.staging, "retiring").unwrap();
    assert_eq!(store.staged_nodes(), 13);
    let plan = store
        .plan_retirement(&retained, &root, index.source())
        .unwrap();
    assert_eq!(
        store.apply_retirement(&plan, 100).unwrap().links_remaining,
        0
    );
    let mut reader = ReadOnlyStore::new(&f.cache, 256 * 1024).unwrap();
    assert!(
        sidecar::lookup(
            &mut reader,
            &root,
            index.source(),
            &hash(b"fixture-713"),
            sidecar::ReadLimits::default()
        )
        .unwrap()
        .observation
        .is_some()
    );
}

#[test]
fn dry_run_does_not_create_missing_retention_controls_or_delete_nodes() {
    let f = Fixture::new();
    let retained = retained_parent(&f);
    let (writer, index, root) = sealed(&f, "dry");
    let anchor = retained.join(&root.sha256);
    fs::create_dir(&anchor).unwrap();
    assert!(
        writer
            .plan_retirement(&retained, &root, index.source())
            .is_err()
    );
    assert_eq!(fs::read_dir(&anchor).unwrap().count(), 0);
    assert_eq!(writer.staged_nodes(), 18);
}

#[test]
fn interrupted_handoff_can_retry_but_never_overwrites_a_bad_retained_winner() {
    let f = Fixture::new();
    let retained = retained_parent(&f);
    let (mut writer, index, root) = sealed(&f, "partial-handoff");
    let anchor = retained.join(&root.sha256);
    fs::create_dir(&anchor).unwrap();
    fs::create_dir(anchor.join("objects")).unwrap();
    let bad = anchor.join("objects").join(&root.sha256);
    fs::create_dir(&bad).unwrap();
    assert!(writer.handoff(&retained, &root, index.source()).is_err());
    assert!(bad.is_dir());
    assert_eq!(writer.staged_nodes(), 18);
    assert!(!anchor.join("root.json").exists());
    // Explicit reconciliation of the injected synthetic empty directory only.
    fs::remove_dir(&bad).unwrap();
    writer.handoff(&retained, &root, index.source()).unwrap();
    assert!(
        writer
            .plan_retirement(&retained, &root, index.source())
            .is_ok()
    );
}
