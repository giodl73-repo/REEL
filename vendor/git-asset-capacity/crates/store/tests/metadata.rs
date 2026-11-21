use git_asset_core::{
    LogicalPath, Object,
    tree::{NodeStore, Tree, TreeError, TreeLimits},
};
use git_asset_store::metadata::{self, Checkpoint, Error, Limits, MetadataStore, Stage};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Barrier},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;
fn limits() -> Limits {
    Limits {
        nodes: 10_000,
        bytes: 4 * 1024 * 1024,
        database_bytes: 8 * 1024 * 1024,
        min_free_bytes: 65_536,
    }
}
fn fixture() -> (TempDir, PathBuf, MetadataStore) {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("metadata");
    let s = MetadataStore::create(&root, limits()).unwrap();
    (t, root, s)
}
fn path(s: &str) -> LogicalPath {
    LogicalPath::parse(s).unwrap()
}
fn lookup(t: &Tree, name: &str, s: &mut MetadataStore) -> Option<Object> {
    t.get(
        &path(name),
        s,
        TreeLimits {
            max_entries: 500_000,
            ..TreeLimits::default()
        },
    )
    .unwrap()
    .0
}

#[test]
fn immutable_nodes_finish_reopen_and_deduplicate_exact_bytes() {
    let (_t, root, mut s) = fixture();
    let first = s.put(b"metadata").unwrap();
    let minimum = s.checkpoint();
    let other = MetadataStore::open(&root, &minimum).unwrap();
    assert!(other.get(&first).unwrap().is_none());
    drop(other);
    let witness = s.finish().unwrap();
    assert_eq!(witness.nodes(), 1);
    drop(s);
    let mut s = MetadataStore::open(&root, &witness).unwrap();
    assert_eq!(s.metrics().sql_node_reads, 0);
    assert_eq!(s.get(&first).unwrap().unwrap(), b"metadata");
    assert_eq!(s.put(b"metadata").unwrap(), first);
    assert_eq!(s.finish().unwrap().nodes(), 1);
}
#[test]
fn validated_limits_inspection_is_read_only_and_detached_from_provider_policy() {
    use git_asset_store::metadata::MetadataReader;
    let (_t, root, mut store) = fixture();
    store.put(b"existing source metadata").unwrap();
    let checkpoint = store.finish().unwrap();
    assert_eq!(store.limits(), limits());
    drop(store);
    let header = fs::read(root.join("metadata.json")).unwrap();
    let database = fs::read(root.join("nodes.sqlite")).unwrap();
    let files = fs::read_dir(&root).unwrap().count();
    let reader = MetadataReader::open(&root, &checkpoint).unwrap();
    let metrics = reader.metrics();
    let mut detached = reader.limits();
    detached.bytes = 1;
    assert_ne!(detached, reader.limits());
    assert_eq!(reader.limits(), limits());
    assert_eq!(reader.checkpoint(), &checkpoint);
    assert_eq!(reader.metrics(), metrics);
    assert_eq!(fs::read(root.join("metadata.json")).unwrap(), header);
    assert_eq!(fs::read(root.join("nodes.sqlite")).unwrap(), database);
    assert_eq!(fs::read_dir(&root).unwrap().count(), files);
    drop(reader);
    let mut writer = MetadataStore::open(&root, &checkpoint).unwrap();
    assert_eq!(writer.limits(), limits());
    writer.put(b"next bounded source metadata").unwrap();
    assert_eq!(writer.finish().unwrap().nodes(), checkpoint.nodes() + 1);
}
#[test]
fn dropped_unflushed_nodes_never_replace_an_acknowledged_tree() {
    let (_t, root, mut s) = fixture();
    let (old, _) = Tree::build(
        &BTreeMap::from([(path("cel"), Object::from_bytes(b"old"))]),
        &mut s,
        TreeLimits::default(),
    )
    .unwrap();
    let witness = s.finish().unwrap();
    let changes = BTreeMap::from([(path("cel"), Some(Object::from_bytes(b"new")))]);
    let (new, _) = old
        .edit(old.root(), &changes, &mut s, TreeLimits::default())
        .unwrap();
    assert_ne!(old.root(), new.root());
    drop(s);
    let mut s = MetadataStore::open(&root, &witness).unwrap();
    assert_eq!(
        lookup(&old, "cel", &mut s),
        Some(Object::from_bytes(b"old"))
    );
    assert!(s.get(new.root()).unwrap().is_none());
}
#[test]
fn logical_limits_and_oversize_nodes_fail_without_overwriting_old_nodes() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("metadata");
    let mut s = MetadataStore::create(
        &root,
        Limits {
            nodes: 1,
            ..limits()
        },
    )
    .unwrap();
    let old = s.put(b"old").unwrap();
    let witness = s.finish().unwrap();
    assert!(matches!(
        s.put(&vec![0; metadata::MAX_NODE_BYTES + 1]),
        Err(TreeError::Budget)
    ));
    assert_eq!(s.get(&old).unwrap().unwrap(), b"old");
    s.put(b"new").unwrap();
    assert_eq!(s.finish().unwrap_err(), Error::Budget);
    assert_eq!(s.finish().unwrap_err(), Error::NeedsRecovery);
    drop(s);
    let s = MetadataStore::open(&root, &witness).unwrap();
    assert_eq!(s.get(&old).unwrap().unwrap(), b"old");
}
#[test]
fn physical_database_page_limit_rejects_before_logical_limit_and_recovers() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("metadata");
    let mut s = MetadataStore::create(
        &root,
        Limits {
            database_bytes: 16_384,
            ..limits()
        },
    )
    .unwrap();
    let old = s.put(b"old").unwrap();
    let witness = s.finish().unwrap();
    s.put(&vec![b'x'; 65_536]).unwrap();
    assert_eq!(s.finish().unwrap_err(), Error::Budget);
    drop(s);
    let s = MetadataStore::open(&root, &witness).unwrap();
    assert_eq!(s.get(&old).unwrap().unwrap(), b"old");
    assert!(fs::metadata(root.join("nodes.sqlite")).unwrap().len() <= 16_384);
}
#[test]
fn corrupt_wrong_size_and_missing_nodes_never_become_empty_success() {
    let (_t, root, mut s) = fixture();
    let node = s.put(b"metadata").unwrap();
    let witness = s.finish().unwrap();
    let mut wrong = node.clone();
    wrong.bytes += 1;
    assert!(matches!(s.get(&wrong), Err(TreeError::Integrity)));
    drop(s);
    let c = rusqlite::Connection::open(root.join("nodes.sqlite")).unwrap();
    c.execute(
        "UPDATE nodes SET body=x'6261646461746121' WHERE oid=?1",
        [node.sha256.as_str()],
    )
    .unwrap();
    drop(c);
    let s = MetadataStore::open(&root, &witness).unwrap();
    assert!(matches!(s.get(&node), Err(TreeError::Integrity)));
    drop(s);
    let c = rusqlite::Connection::open(root.join("nodes.sqlite")).unwrap();
    c.execute("DELETE FROM nodes WHERE oid=?1", [node.sha256.as_str()])
        .unwrap();
    drop(c);
    let mut s = MetadataStore::open(&root, &witness).unwrap();
    assert!(s.get(&node).unwrap().is_none());
    assert!(
        Tree::from_root(node)
            .get(&path("cel"), &mut s, TreeLimits::default())
            .is_err()
    );
}
#[test]
fn known_counter_rollback_and_foreign_identity_reject_without_a_census() {
    let (t, root, mut s) = fixture();
    let minimum = s.checkpoint();
    s.put(b"one").unwrap();
    let older = s.finish().unwrap();
    drop(s);
    let backup = t.path().join("backup.sqlite");
    fs::copy(root.join("nodes.sqlite"), &backup).unwrap();
    let mut s = MetadataStore::open(&root, &older).unwrap();
    s.put(b"two").unwrap();
    let latest = s.finish().unwrap();
    drop(s);
    fs::copy(backup, root.join("nodes.sqlite")).unwrap();
    assert!(matches!(
        MetadataStore::open(&root, &latest),
        Err(Error::Rollback)
    ));
    assert!(MetadataStore::open(&root, &minimum).is_ok());
    let (_other, other_root, _s) = fixture();
    assert!(matches!(
        MetadataStore::open(&other_root, &minimum),
        Err(Error::Foreign)
    ));
}
#[test]
fn concurrent_writers_deduplicate_and_capacity_checks_use_current_counters() {
    let (_t, root, s) = fixture();
    let witness = s.checkpoint();
    drop(s);
    let mut one = MetadataStore::open(&root, &witness).unwrap();
    let mut two = MetadataStore::open(&root, &witness).unwrap();
    let shared = one.put(b"shared").unwrap();
    assert_eq!(two.put(b"shared").unwrap(), shared);
    one.put(b"one").unwrap();
    two.put(b"two").unwrap();
    let gate = Arc::new(Barrier::new(2));
    let other = gate.clone();
    let work = |mut s: MetadataStore, g: Arc<Barrier>| {
        g.wait();
        let start = Instant::now();
        loop {
            match s.finish() {
                Err(Error::Busy) if start.elapsed() < Duration::from_secs(3) => {
                    thread::sleep(Duration::from_millis(2))
                }
                r => return r,
            }
        }
    };
    let a = thread::spawn(move || work(one, gate));
    let b = thread::spawn(move || work(two, other));
    a.join().unwrap().unwrap();
    b.join().unwrap().unwrap();
    let mut s = MetadataStore::open(&root, &witness).unwrap();
    assert_eq!(s.finish().unwrap().nodes(), 3);
    assert_eq!(s.get(&shared).unwrap().unwrap(), b"shared");
}
#[test]
fn injected_flush_boundaries_recover_without_acknowledging_a_partial_batch() {
    for stage in [Stage::BeforeRows, Stage::BeforeCommit, Stage::AfterCommit] {
        let (_t, root, mut s) = fixture();
        let old = s.put(b"old").unwrap();
        let witness = s.finish().unwrap();
        let new = s.put(b"new").unwrap();
        assert_eq!(
            s.finish_hook(|p| if p == stage {
                Err(Error::Injected)
            } else {
                Ok(())
            })
            .unwrap_err(),
            Error::Injected
        );
        assert_eq!(s.finish().unwrap_err(), Error::NeedsRecovery);
        drop(s);
        let mut s = MetadataStore::open(&root, &witness).unwrap();
        assert_eq!(s.get(&old).unwrap().unwrap(), b"old");
        assert_eq!(s.get(&new).unwrap().is_some(), stage == Stage::AfterCommit);
        s.put(b"new").unwrap();
        assert_eq!(s.finish().unwrap().nodes(), 2);
    }
}
#[test]
fn busy_batches_retain_private_nodes_and_can_retry_after_the_short_transaction() {
    let (_t, root, mut s) = fixture();
    let n = s.put(b"node").unwrap();
    let c = rusqlite::Connection::open(root.join("nodes.sqlite")).unwrap();
    c.execute_batch("BEGIN IMMEDIATE").unwrap();
    let start = Instant::now();
    assert_eq!(s.finish().unwrap_err(), Error::Busy);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(s.pending_nodes(), 1);
    c.execute_batch("ROLLBACK").unwrap();
    s.finish().unwrap();
    assert_eq!(s.get(&n).unwrap().unwrap(), b"node");
}
#[test]
fn invalid_limits_and_existing_roots_never_trigger_a_new_store() {
    let (_t, root, s) = fixture();
    let witness = s.checkpoint();
    drop(s);
    assert!(MetadataStore::create(&root, limits()).is_err());
    for bad in [
        Limits {
            nodes: 0,
            ..limits()
        },
        Limits {
            database_bytes: 16_385,
            ..limits()
        },
        Limits {
            bytes: u64::MAX,
            ..limits()
        },
    ] {
        assert!(MetadataStore::create(&root, bad).is_err());
    }
    fs::write(root.join("metadata.json"), b"{}").unwrap();
    assert!(MetadataStore::open(&root, &witness).is_err());
}

#[test]
fn readonly_reader_sees_only_persisted_nodes_and_never_accepts_writes() {
    let (_t, root, mut s) = fixture();
    let minimum = s.checkpoint();
    let node = s.put(b"private").unwrap();
    let reader = git_asset_store::metadata::MetadataReader::open(&root, &minimum).unwrap();
    assert!(reader.get(&node).unwrap().is_none());
    drop(reader);
    let witness = s.finish().unwrap();
    drop(s);
    let mut reader = git_asset_store::metadata::MetadataReader::open(&root, &witness).unwrap();
    assert_eq!(reader.metrics().sql_node_reads, 0);
    assert_eq!(reader.get(&node).unwrap().unwrap(), b"private");
    assert!(reader.put(b"no write").is_err());
    assert_eq!(reader.metrics().sql_node_reads, 1);
    assert_eq!(fs::read_dir(root).unwrap().count(), 2);
}

#[test]
fn empty_finish_cannot_downgrade_a_known_checkpoint_after_counter_rollback() {
    let (_t, root, mut s) = fixture();
    s.put(b"known").unwrap();
    let known = s.finish().unwrap();
    let c = rusqlite::Connection::open(root.join("nodes.sqlite")).unwrap();
    c.execute("UPDATE counters SET generation=0,nodes=0,bytes=0", [])
        .unwrap();
    drop(c);
    assert_eq!(s.finish().unwrap_err(), Error::Rollback);
    assert_eq!(s.checkpoint(), known);
    assert_eq!(s.finish().unwrap_err(), Error::NeedsRecovery);
}
#[test]
fn metadata_child() {
    let Some(root) = std::env::var_os("GIT_ASSET_METADATA_TEST_ROOT") else {
        return;
    };
    let root = Path::new(&root);
    let witness: Checkpoint =
        serde_json::from_slice(&fs::read(root.parent().unwrap().join("minimum.json")).unwrap())
            .unwrap();
    let cut = std::env::var("GIT_ASSET_METADATA_TEST_STAGE").unwrap();
    let mut s = MetadataStore::open(root, &witness).unwrap();
    s.put(b"new").unwrap();
    s.finish_hook(|stage| {
        if format!("{stage:?}") == cut {
            std::process::exit(92);
        }
        Ok(())
    })
    .unwrap();
    panic!("child did not exit");
}
#[test]
fn actual_child_exits_recover_flush_and_preserve_old_acknowledgments() {
    for stage in [Stage::BeforeRows, Stage::BeforeCommit, Stage::AfterCommit] {
        let (t, root, mut s) = fixture();
        let old = s.put(b"old").unwrap();
        let witness = s.finish().unwrap();
        drop(s);
        fs::write(
            t.path().join("minimum.json"),
            serde_json::to_vec(&witness).unwrap(),
        )
        .unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "metadata_child", "--nocapture"])
            .env("GIT_ASSET_METADATA_TEST_ROOT", &root)
            .env("GIT_ASSET_METADATA_TEST_STAGE", format!("{stage:?}"))
            .spawn()
            .unwrap();
        let start = Instant::now();
        let status = loop {
            if let Some(s) = child.try_wait().unwrap() {
                break s;
            }
            if start.elapsed() > Duration::from_secs(10) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("child deadline");
            }
            thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(status.code(), Some(92));
        let mut s = MetadataStore::open(&root, &witness).unwrap();
        assert_eq!(s.get(&old).unwrap().unwrap(), b"old");
        assert_eq!(
            s.get(&Object::from_bytes(b"new")).unwrap().is_some(),
            stage == Stage::AfterCommit
        );
        s.put(b"new").unwrap();
        assert_eq!(s.finish().unwrap().nodes(), 2);
    }
}

#[test]
fn half_million_binding_tree_persists_and_thirteen_edits_share_its_history() {
    let t = tempfile::tempdir().unwrap();
    assert!(
        fs2::available_space(t.path()).unwrap() > 2 * 1024 * 1024 * 1024,
        "scale fixture requires 2 GiB temporary headroom"
    );
    let root = t.path().join("metadata");
    let cap = Limits {
        nodes: 600_000,
        bytes: 384 * 1024 * 1024,
        database_bytes: 512 * 1024 * 1024,
        min_free_bytes: 64 * 1024 * 1024,
    };
    let mut s = MetadataStore::create(&root, cap).unwrap();
    let build_limits = TreeLimits {
        max_entries: 500_000,
        max_writes: 600_000,
        max_write_bytes: 384 * 1024 * 1024,
        ..TreeLimits::default()
    };
    let start = Instant::now();
    let bindings: BTreeMap<_, _> = (0..500_000)
        .map(|i| {
            (
                path(&format!("assets/item-{i:06}")),
                Object::from_bytes(format!("asset-{i}").as_bytes()),
            )
        })
        .collect();
    eprintln!("scale: generated 500000 synthetic bindings; building disk-backed tree");
    let (old, build) = Tree::build(&bindings, &mut s, build_limits).unwrap();
    let first = s.finish().unwrap();
    let physical_first = fs::metadata(root.join("nodes.sqlite")).unwrap().len();
    eprintln!(
        "scale: base persisted nodes={} logical_bytes={} database_bytes={} build_ms={}",
        first.nodes(),
        first.bytes(),
        physical_first,
        start.elapsed().as_millis()
    );
    drop(bindings);
    drop(s);
    let mut s = MetadataStore::open(&root, &first).unwrap();
    assert_eq!(s.metrics().sql_node_reads, 0);
    for i in [0, 1, 999, 55_555, 499_999] {
        assert_eq!(
            lookup(&old, &format!("assets/item-{i:06}"), &mut s),
            Some(Object::from_bytes(format!("asset-{i}").as_bytes()))
        );
    }
    assert!(s.metrics().sql_node_reads < 512);
    let changes: BTreeMap<_, _> = (0..13)
        .map(|i| {
            (
                path(&format!("assets/item-{:06}", i * 10_000)),
                Some(Object::from_bytes(format!("replacement-{i}").as_bytes())),
            )
        })
        .collect();
    s.reset_metrics();
    let edit_start = Instant::now();
    let (new, edit) = old
        .edit(
            old.root(),
            &changes,
            &mut s,
            TreeLimits {
                max_entries: 500_000,
                ..TreeLimits::default()
            },
        )
        .unwrap();
    let second = s.finish().unwrap();
    let delta_nodes = second.nodes() - first.nodes();
    let delta_bytes = second.bytes() - first.bytes();
    let edit_ms = edit_start.elapsed().as_millis();
    let database_growth = fs::metadata(root.join("nodes.sqlite")).unwrap().len() - physical_first;
    assert!(database_growth < 4 * 1024 * 1024);
    assert!(delta_nodes < 2048, "{delta_nodes}");
    assert!(delta_bytes < 1024 * 1024, "{delta_bytes}");
    assert!(edit.reads < 10_000 && edit.writes < 2048);
    assert_ne!(old.root(), new.root());
    drop(s);
    let mut s = MetadataStore::open(&root, &second).unwrap();
    assert_eq!(
        lookup(&old, "assets/item-000000", &mut s),
        Some(Object::from_bytes(b"asset-0"))
    );
    assert_eq!(
        lookup(&new, "assets/item-000000", &mut s),
        Some(Object::from_bytes(b"replacement-0"))
    );
    assert_eq!(
        lookup(&old, "assets/item-499999", &mut s),
        lookup(&new, "assets/item-499999", &mut s)
    );
    eprintln!(
        "scale: bindings=500000 nodes={} logical_bytes={} database_bytes={} build_writes={} edit_nodes={} edit_bytes={} edit_reads={} edit_writes={} elapsed_ms={} edit_ms={} database_growth={}",
        first.nodes(),
        first.bytes(),
        physical_first,
        build.writes,
        delta_nodes,
        delta_bytes,
        edit.reads,
        edit.writes,
        start.elapsed().as_millis(),
        edit_ms,
        database_growth
    );
}
