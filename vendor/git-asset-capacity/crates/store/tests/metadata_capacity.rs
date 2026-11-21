use git_asset_core::{Digest, Object, tree::NodeStore};
use git_asset_store::metadata::{
    self, Checkpoint, Limits, MetadataReader, MetadataStore,
    capacity::{self as cap, Plan, Request, Stage, Status},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Barrier},
    thread,
};

fn limits() -> Limits {
    Limits {
        nodes: 32,
        bytes: 32_768,
        database_bytes: 262_144,
        min_free_bytes: 65_536,
    }
}
fn larger() -> Limits {
    Limits {
        nodes: 64,
        bytes: 65_536,
        database_bytes: 524_288,
        ..limits()
    }
}
struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    original: Checkpoint,
    objects: Vec<Object>,
}
impl Fixture {
    fn new() -> Self {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("metadata");
        let mut s = MetadataStore::create(&root, limits()).unwrap();
        let objects = [
            b"first retained tree".as_slice(),
            b"second retained tree".as_slice(),
        ]
        .iter()
        .map(|b| s.put(b).unwrap())
        .collect();
        let original = s.finish().unwrap();
        Self {
            _temp: t,
            root,
            original,
            objects,
        }
    }
    fn plan(&self, op: &str) -> Plan {
        cap::plan(
            &self.root,
            Request {
                operation: Digest::blob(op.as_bytes()),
                expected: self.original.clone(),
                expected_limits: limits(),
                limits: larger(),
            },
        )
        .unwrap()
        .plan
    }
    fn verify(&self, checkpoint: &Checkpoint) {
        let r = MetadataReader::open(&self.root, checkpoint).unwrap();
        for o in &self.objects {
            assert_eq!(Object::from_bytes(&r.get(o).unwrap().unwrap()), *o);
        }
    }
}
fn inject() -> cap::Error {
    cap::Error::Provider(metadata::Error::Injected)
}

#[test]
fn planning_is_read_only_and_headroom_is_not_reserved() {
    let f = Fixture::new();
    let config = fs::read(f.root.join("metadata.json")).unwrap();
    let db = fs::read(f.root.join("nodes.sqlite")).unwrap();
    let files = fs::read_dir(&f.root).unwrap().count();
    let preview = cap::plan(
        &f.root,
        Request {
            operation: Digest::blob(b"plan"),
            expected: f.original.clone(),
            expected_limits: limits(),
            limits: larger(),
        },
    )
    .unwrap();
    assert!(!preview.reserved_capacity);
    assert_eq!(preview.remaining_nodes, 62);
    assert_eq!(
        preview.remaining_body_bytes,
        larger().bytes - f.original.bytes()
    );
    assert_eq!(
        cap::status(&f.root, &preview.plan).unwrap(),
        Status::NotApplied
    );
    assert_eq!(fs::read(f.root.join("metadata.json")).unwrap(), config);
    assert_eq!(fs::read(f.root.join("nodes.sqlite")).unwrap(), db);
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), files);
}
#[test]
fn growth_preserves_history_fences_cached_writers_and_rebinds_explicitly() {
    let f = Fixture::new();
    let reader = MetadataReader::open(&f.root, &f.original).unwrap();
    let mut old = MetadataStore::open(&f.root, &f.original).unwrap();
    old.put(b"uncommitted stale writer").unwrap();
    let p = f.plan("growth");
    let receipt = cap::apply(&f.root, &p).unwrap();
    assert_ne!(receipt.before.identity(), receipt.after.identity());
    assert_eq!(receipt.after.nodes(), receipt.before.nodes());
    assert_eq!(receipt.after.bytes(), receipt.before.bytes());
    assert_eq!(receipt.after.generation(), receipt.before.generation() + 1);
    assert_eq!(old.finish().unwrap_err(), metadata::Error::Corrupt);
    assert_eq!(
        MetadataReader::open(&f.root, &f.original).err(),
        Some(metadata::Error::Foreign)
    );
    assert_eq!(
        reader.get(&f.objects[0]).unwrap().unwrap(),
        b"first retained tree"
    );
    let rebound = cap::rebind(&f.root, &f.original, &receipt.after, 1).unwrap();
    assert_eq!(rebound.generation(), f.original.generation());
    assert_eq!(rebound.nodes(), f.original.nodes());
    f.verify(&rebound);
    let mut new = MetadataStore::open(&f.root, &receipt.after).unwrap();
    assert_eq!(new.limits(), larger());
    new.put(b"new retained history").unwrap();
    let current = new.finish().unwrap();
    assert_eq!(cap::apply(&f.root, &p).unwrap(), receipt);
    assert_eq!(
        cap::status(&f.root, &p).unwrap(),
        Status::Applied { receipt }
    );
    f.verify(&current);
}
#[test]
fn every_crash_boundary_reconciles_the_original_operation() {
    for stage in [
        Stage::BeforeCommit,
        Stage::AfterCommit,
        Stage::BeforePublish,
        Stage::AfterPublish,
    ] {
        let f = Fixture::new();
        let p = f.plan("interrupted");
        let err = cap::apply_hook(
            &f.root,
            &p,
            |s| if s == stage { Err(inject()) } else { Ok(()) },
        )
        .unwrap_err();
        let state = cap::status(&f.root, &p).unwrap();
        match stage {
            Stage::BeforeCommit => {
                assert_eq!(state, Status::NotApplied);
                assert_eq!(err, inject());
                f.verify(&f.original);
            }
            Stage::AfterCommit | Stage::BeforePublish => {
                assert!(matches!(state, Status::Pending { .. }));
                assert_eq!(err, cap::Error::Uncertain);
            }
            Stage::AfterPublish => {
                assert!(matches!(state, Status::Applied { .. }));
                assert_eq!(err, cap::Error::Uncertain);
            }
        }
        let r = cap::apply(&f.root, &p).unwrap();
        assert_eq!(cap::apply(&f.root, &p).unwrap(), r);
        f.verify(&r.after);
    }
}
#[test]
fn stale_plan_and_reused_operation_never_adopt_other_state() {
    let f = Fixture::new();
    let p = f.plan("same");
    let other = cap::plan(
        &f.root,
        Request {
            operation: p.operation().clone(),
            expected: f.original.clone(),
            expected_limits: limits(),
            limits: Limits {
                nodes: 80,
                ..larger()
            },
        },
    )
    .unwrap()
    .plan;
    let mut s = MetadataStore::open(&f.root, &f.original).unwrap();
    s.put(b"advance before admission").unwrap();
    let current = s.finish().unwrap();
    assert_eq!(cap::apply(&f.root, &p).unwrap_err(), cap::Error::Stale);
    f.verify(&current);
    let f = Fixture::new();
    let p = f.plan("same");
    let changed = cap::plan(
        &f.root,
        Request {
            operation: p.operation().clone(),
            expected: f.original.clone(),
            expected_limits: limits(),
            limits: other.limits(),
        },
    )
    .unwrap()
    .plan;
    let r = cap::apply(&f.root, &p).unwrap();
    assert_eq!(
        cap::apply(&f.root, &changed).unwrap_err(),
        cap::Error::Conflict
    );
    f.verify(&r.after);
}
#[test]
fn pending_config_is_explicit_and_read_only_inspection_never_repairs() {
    let f = Fixture::new();
    let p = f.plan("pending");
    let prior = fs::read(f.root.join("metadata.json")).unwrap();
    assert_eq!(
        cap::apply_hook(&f.root, &p, |s| if s == Stage::AfterCommit {
            Err(inject())
        } else {
            Ok(())
        })
        .unwrap_err(),
        cap::Error::Uncertain
    );
    let db = fs::read(f.root.join("nodes.sqlite")).unwrap();
    assert!(matches!(
        cap::status(&f.root, &p).unwrap(),
        Status::Pending { .. }
    ));
    assert_eq!(fs::read(f.root.join("metadata.json")).unwrap(), prior);
    assert_eq!(fs::read(f.root.join("nodes.sqlite")).unwrap(), db);
    assert_eq!(
        cap::plan(
            &f.root,
            Request {
                operation: Digest::blob(b"other"),
                expected: f.original.clone(),
                expected_limits: limits(),
                limits: larger()
            }
        )
        .unwrap_err()
        .code(),
        "needs_recovery"
    );
    cap::apply(&f.root, &p).unwrap();
}
#[test]
fn foreign_corrupt_and_absent_providers_refuse_without_initialization() {
    let f = Fixture::new();
    let p = f.plan("foreign");
    let g = Fixture::new();
    assert_eq!(cap::apply(&g.root, &p).unwrap_err().code(), "foreign");
    let absent = f._temp.path().join("absent");
    assert!(cap::apply(&absent, &p).is_err());
    assert!(!absent.exists());
    fs::write(f.root.join("metadata.json"), b"{broken").unwrap();
    assert_eq!(cap::status(&f.root, &p).unwrap_err().code(), "corrupt");
    let g = Fixture::new();
    let p = g.plan("record");
    cap::apply(&g.root, &p).unwrap();
    let conn = rusqlite::Connection::open(g.root.join("nodes.sqlite")).unwrap();
    conn.execute("UPDATE metadata_capacity SET plan='{}'", [])
        .unwrap();
    assert_eq!(cap::status(&g.root, &p).unwrap_err().code(), "corrupt");
}
#[test]
fn contraction_never_discards_nodes_or_revives_old_writer_identity() {
    let f = Fixture::new();
    let up = f.plan("up");
    let first = cap::apply(&f.root, &up).unwrap();
    let down = cap::plan(
        &f.root,
        Request {
            operation: Digest::blob(b"down"),
            expected: first.after.clone(),
            expected_limits: larger(),
            limits: limits(),
        },
    )
    .unwrap()
    .plan;
    let r = cap::apply(&f.root, &down).unwrap();
    assert_ne!(r.after.identity(), f.original.identity());
    assert_ne!(r.after.identity(), first.after.identity());
    let config = fs::read(f.root.join("metadata.json")).unwrap();
    assert_eq!(cap::apply(&f.root, &up).unwrap(), first);
    assert_eq!(fs::read(f.root.join("metadata.json")).unwrap(), config);
    assert_eq!(
        MetadataStore::open(&f.root, &r.after).unwrap().limits(),
        limits()
    );
    f.verify(&r.after);
    for denied in [
        Limits {
            nodes: 1,
            ..limits()
        },
        Limits {
            bytes: 1,
            ..limits()
        },
        Limits {
            database_bytes: 16_384,
            ..limits()
        },
    ] {
        assert_eq!(
            cap::plan(
                &f.root,
                Request {
                    operation: Digest::blob(b"denied"),
                    expected: r.after.clone(),
                    expected_limits: limits(),
                    limits: denied
                }
            )
            .unwrap_err()
            .code(),
            "admission_denied"
        );
        f.verify(&r.after);
    }
    assert_eq!(
        cap::rebind(&f.root, &f.original, &r.after, 1)
            .unwrap_err()
            .code(),
        "admission_denied"
    );
    f.verify(&cap::rebind(&f.root, &f.original, &r.after, 2).unwrap());
}
#[test]
fn held_sql_writer_returns_busy_and_parallel_plans_have_one_winner() {
    let f = Fixture::new();
    let p = f.plan("busy");
    let conn = rusqlite::Connection::open(f.root.join("nodes.sqlite")).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(cap::apply(&f.root, &p).unwrap_err().code(), "busy");
    conn.execute_batch("ROLLBACK").unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let plans = [f.plan("one"), f.plan("two")];
    let handles: Vec<_> = plans
        .iter()
        .cloned()
        .map(|p| {
            let root = f.root.clone();
            let b = barrier.clone();
            thread::spawn(move || {
                b.wait();
                cap::apply(&root, &p)
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for e in results.iter().filter_map(|r| r.as_ref().err()) {
        assert!(matches!(
            e.code(),
            "busy" | "stale" | "needs_recovery" | "uncertain"
        ));
    }
    // No per-round fairness is promised. Serial recovery uses the original IDs.
    let settled: Vec<_> = plans.iter().map(|p| cap::apply(&f.root, p)).collect();
    assert_eq!(settled.iter().filter(|r| r.is_ok()).count(), 1);
    let receipt = settled.into_iter().find_map(Result::ok).unwrap();
    f.verify(&receipt.after);
}

// The harness child executes the real provider and exits without unwinding.
#[test]
fn capacity_crash_child() {
    let Some(root) = std::env::var_os("GIT_ASSET_CAPACITY_CRASH_ROOT") else {
        return;
    };
    let plan: Plan =
        serde_json::from_slice(&fs::read(Path::new(&root).join("plan.json")).unwrap()).unwrap();
    let stage = std::env::var("GIT_ASSET_CAPACITY_CRASH_STAGE").unwrap();
    cap::apply_hook(Path::new(&root).join("metadata").as_path(), &plan, |s| {
        if format!("{s:?}") == stage {
            std::process::exit(73);
        }
        Ok(())
    })
    .unwrap();
    panic!("crash boundary not reached");
}
#[test]
fn real_process_exits_recover_before_and_after_commit_and_publication() {
    for stage in [
        "BeforeCommit",
        "AfterCommit",
        "BeforePublish",
        "AfterPublish",
    ] {
        let f = Fixture::new();
        let p = f.plan(stage);
        fs::write(
            f._temp.path().join("plan.json"),
            serde_json::to_vec(&p).unwrap(),
        )
        .unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "capacity_crash_child", "--test-threads=1"])
            .env("GIT_ASSET_CAPACITY_CRASH_ROOT", f._temp.path())
            .env("GIT_ASSET_CAPACITY_CRASH_STAGE", stage)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(73),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let receipt = cap::apply(&f.root, &p).unwrap();
        assert_eq!(cap::apply(&f.root, &p).unwrap(), receipt);
        f.verify(&receipt.after);
    }
}

#[test]
#[ignore = "explicit pinned legacy executable and owned Windows/NTFS qualification"]
fn pinned_legacy_writer_is_fenced_at_near_production_metadata_capacity() {
    use std::time::{Duration, Instant};
    let exe = std::env::var_os("GIT_ASSET_METADATA_LEGACY_EXE").expect("pinned tool required");
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("metadata");
    let old_limits = Limits {
        nodes: 100_000,
        bytes: 33_554_432,
        database_bytes: 67_108_864,
        min_free_bytes: 65_536,
    };
    let mut s = MetadataStore::create(&root, old_limits).unwrap();
    for i in 0u64..92_256 {
        let mut bytes = vec![0u8; 347];
        bytes[..8].copy_from_slice(&i.to_be_bytes());
        s.put(&bytes).unwrap();
    }
    let cp = s.finish().unwrap();
    drop(s);
    assert_eq!(cp.nodes(), 92_256);
    assert_eq!(cp.bytes(), 92_256 * 347);
    let profile = t.path().join("minimum.json");
    fs::write(&profile, serde_json::to_vec(&cp).unwrap()).unwrap();
    let ready = t.path().join("ready");
    let release = t.path().join("release");
    let mut child = Command::new(exe)
        .arg(&root)
        .arg(profile)
        .arg(&ready)
        .arg(&release)
        .spawn()
        .unwrap();
    let start = Instant::now();
    while !ready.exists() {
        assert!(start.elapsed() < Duration::from_secs(30));
        thread::sleep(Duration::from_millis(10));
    }
    let p = cap::plan(
        &root,
        Request {
            operation: Digest::blob(b"qualified-growth"),
            expected: cp.clone(),
            expected_limits: old_limits,
            limits: Limits {
                nodes: 200_000,
                bytes: 67_108_864,
                database_bytes: 134_217_728,
                ..old_limits
            },
        },
    )
    .unwrap()
    .plan;
    let receipt = cap::apply(&root, &p).unwrap();
    fs::write(release, b"release").unwrap();
    assert!(child.wait().unwrap().success());
    let rebound = cap::rebind(&root, &cp, &receipt.after, 1).unwrap();
    let reader = MetadataReader::open(&root, &rebound).unwrap();
    for i in 0u64..92_256 {
        let mut bytes = vec![0u8; 347];
        bytes[..8].copy_from_slice(&i.to_be_bytes());
        assert_eq!(
            reader.get(&Object::from_bytes(&bytes)).unwrap().unwrap(),
            bytes
        );
    }
    assert_eq!(reader.checkpoint().nodes(), cp.nodes());
    assert_eq!(reader.checkpoint().bytes(), cp.bytes());
    assert_eq!(cap::apply(&root, &p).unwrap(), receipt);
}
