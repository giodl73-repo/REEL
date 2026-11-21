use super::*;
use crate::local::removal::{Protection, ProtectionLease};
use std::collections::BTreeSet;
#[path = "protection/adoption_tests.rs"]
mod adoption_tests;
#[path = "protection/batch_tests.rs"]
mod batch_tests;
#[path = "protection/effects_tests.rs"]
mod effects_tests;
#[path = "protection/work_tests.rs"]
mod work_tests;
fn d(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn object(i: usize) -> Object {
    Object::from_bytes(format!("public indexed asset {i}").as_bytes())
}
fn p(s: &str) -> LogicalPath {
    LogicalPath::parse(s).unwrap()
}
fn nodes(root: &Path) -> metadata::MetadataStore {
    metadata::MetadataStore::create(
        root,
        metadata::Limits {
            nodes: 100_000,
            bytes: 64 * 1024 * 1024,
            database_bytes: 128 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap()
}
struct Fixture {
    temp: tempfile::TempDir,
    index: Index,
    nodes: metadata::MetadataStore,
    main: BTreeMap<LogicalPath, Object>,
}
impl Fixture {
    fn new(count: usize) -> Self {
        Self::configured(count, false, false)
    }
    fn routed(count: usize) -> Self {
        Self::configured(count, true, false)
    }
    fn bound(count: usize) -> Self {
        Self::configured(count, true, true)
    }
    fn configured(count: usize, routed: bool, bound: bool) -> Self {
        Self::configured_effects(count, routed, bound, false)
    }
    fn configured_effects(count: usize, routed: bool, bound: bool, effects: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut nodes = nodes(&temp.path().join("nodes"));
        let main = (0..count)
            .map(|i| (p(&format!("assets/a-{i:05}")), object(i)))
            .collect::<BTreeMap<_, _>>();
        let empty = Tree::empty(&mut nodes).unwrap();
        let (tree, _) = Tree::build(&main, &mut nodes, limits()).unwrap();
        nodes.finish().unwrap();
        let mut stores = vec![d("F"), d("D"), d("M")];
        stores.sort();
        let mut main_stores = vec![d("F"), d("M")];
        main_stores.sort();
        let mut history_stores = vec![d("D"), d("M")];
        history_stores.sort();
        let actors = vec![
            Actor {
                id: key("history"),
                owner: key("source-owner"),
                stores: history_stores,
            },
            Actor {
                id: key("jobs"),
                owner: key("source-owner"),
                stores: stores.clone(),
            },
            Actor {
                id: key("main"),
                owner: key("source-owner"),
                stores: main_stores,
            },
        ];
        let profile = Enrollment {
            owner: key("owner"),
            subject: d("subject"),
            policies: vec![d("policy")],
            stores,
            actors,
            max_bindings: 5000,
        };
        let mut index = if effects {
            Index::create_effects(
                &temp.path().join("index"),
                key("epoch"),
                profile,
                routed.then(|| Routing {
                    source: key("main"),
                    actor: key("main"),
                }),
            )
        } else if routed {
            let create = if bound {
                Index::create_recoverable
            } else {
                Index::create_routed
            };
            create(
                &temp.path().join("index"),
                key("epoch"),
                profile,
                Routing {
                    source: key("main"),
                    actor: key("main"),
                },
            )
        } else {
            Index::create(&temp.path().join("index"), key("epoch"), profile)
        }
        .unwrap();
        assert!(matches!(
            index.inspect(&mut nodes, &object(0), &d("F")),
            Err(Error::Unknown)
        ));
        index
            .bootstrap(
                [
                    (
                        "main".into(),
                        Selection {
                            root: tree.root().clone(),
                            context: d("main"),
                        },
                    ),
                    (
                        "history".into(),
                        Selection {
                            root: empty.root().clone(),
                            context: d("history"),
                        },
                    ),
                    (
                        "jobs".into(),
                        Selection {
                            root: empty.root().clone(),
                            context: d("jobs"),
                        },
                    ),
                ]
                .into(),
                &mut nodes,
            )
            .unwrap();
        Self {
            temp,
            index,
            nodes,
            main,
        }
    }
    fn request(
        &mut self,
        actor: &str,
        op_id: &str,
        changes: BTreeMap<LogicalPath, Option<Object>>,
        ready: bool,
    ) -> Update {
        let state = self.index.actor(&mut self.nodes, &key(actor)).unwrap();
        let old = Tree::from_root(state.selection.root.clone());
        let (next, _) = old
            .edit(old.root(), &changes, &mut self.nodes, limits())
            .unwrap();
        Update {
            operation: key(op_id),
            actor: key(actor),
            owner: key("source-owner"),
            expected_revision: state.revision,
            selection: Selection {
                root: next.root().clone(),
                context: d(op_id),
            },
            changes,
            ready,
        }
    }
    fn reopen(&mut self) {
        self.nodes.finish().unwrap();
        let minimum = self.index.checkpoint();
        self.index = Index::open(&self.temp.path().join("index"), &minimum).unwrap();
    }
}
#[test]
fn exact_index_queries_do_not_traverse_thousands_of_source_bindings() {
    let mut f = Fixture::new(2000);
    let known = f
        .index
        .inspect(&mut f.nodes, &object(1987), &d("F"))
        .unwrap();
    assert_eq!(known.protected_by, vec![key("main")]);
    assert!(known.metadata_reads < 80, "{} reads", known.metadata_reads);
    assert!(
        f.index
            .inspect(&mut f.nodes, &object(1987), &d("D"))
            .unwrap()
            .protected_by
            .is_empty()
    );
    let missing = f
        .index
        .inspect(&mut f.nodes, &object(99999), &d("F"))
        .unwrap();
    assert!(missing.protected_by.is_empty());
    assert!(missing.metadata_reads < 80);
    println!(
        "INDEX 2000 bindings: known={} metadata reads, absent={} metadata reads; payload IO not used",
        known.metadata_reads, missing.metadata_reads
    );
    let mut raw = SourceJournal::open(&f.temp.path().join("index"), &f.index.checkpoint()).unwrap();
    assert_eq!(
        raw.apply(
            key("bypass"),
            Command::Withdraw {
                source: key("authority"),
                owner: key("owner"),
                expected_revision: 2
            }
        )
        .unwrap_err(),
        super::super::Error::Restricted
    );
    assert!(!raw.obligations(16).unwrap().complete_claim_coverage());
    assert_eq!(f.main.len(), 2000);
}
#[test]
fn aliases_and_other_actors_stay_protected_through_incremental_edits() {
    let mut f = Fixture::new(3);
    let r = f.request(
        "main",
        "alias",
        [(p("assets/alias"), Some(object(0)))].into(),
        true,
    );
    f.index.update(&r, &mut f.nodes).unwrap();
    let r = f.request(
        "main",
        "one-alias-removed",
        [(p("assets/a-00000"), None)].into(),
        true,
    );
    f.index.update(&r, &mut f.nodes).unwrap();
    assert_eq!(
        f.index
            .inspect(&mut f.nodes, &object(0), &d("F"))
            .unwrap()
            .protected_by,
        vec![key("main")]
    );
    let r = f.request(
        "jobs",
        "job",
        [(p("jobs/active"), Some(object(0)))].into(),
        true,
    );
    f.index.update(&r, &mut f.nodes).unwrap();
    let r = f.request(
        "main",
        "last-alias-removed",
        [(p("assets/alias"), None)].into(),
        true,
    );
    f.index.update(&r, &mut f.nodes).unwrap();
    assert_eq!(
        f.index
            .inspect(&mut f.nodes, &object(0), &d("F"))
            .unwrap()
            .protected_by,
        vec![key("jobs")]
    );
    f.reopen();
    assert_eq!(
        f.index
            .inspect(&mut f.nodes, &object(0), &d("M"))
            .unwrap()
            .protected_by,
        vec![key("jobs")]
    );
}
#[test]
fn mismatched_root_owner_revision_and_expanded_retry_refuse_without_adoption() {
    let mut f = Fixture::new(3);
    let original = f.index.checkpoint();
    let mut r = f.request(
        "main",
        "edit",
        [(p("assets/a-00000"), Some(object(10)))].into(),
        true,
    );
    r.selection.root = f
        .index
        .actor(&mut f.nodes, &key("main"))
        .unwrap()
        .selection
        .root;
    assert!(matches!(
        f.index.update(&r, &mut f.nodes),
        Err(Error::Invalid)
    ));
    assert_eq!(original, f.index.checkpoint());
    let mut r = f.request(
        "main",
        "edit",
        [(p("assets/a-00000"), Some(object(10)))].into(),
        true,
    );
    r.owner = key("wrong");
    assert!(matches!(
        f.index.update(&r, &mut f.nodes),
        Err(Error::Invalid)
    ));
    r.owner = key("source-owner");
    let saved = f.index.update(&r, &mut f.nodes).unwrap();
    assert!(f.index.update(&r, &mut f.nodes).unwrap().replayed);
    assert_eq!(
        f.index.update(&r, &mut f.nodes).unwrap().checkpoint,
        saved.checkpoint
    );
    r.ready = false;
    assert!(matches!(
        f.index.update(&r, &mut f.nodes),
        Err(Error::Conflict)
    ));
}
#[test]
fn prepared_union_preserves_old_and_new_protection_and_resumes_exactly() {
    let mut f = Fixture::new(3);
    let r = f.request(
        "main",
        "pending",
        [(p("assets/a-00000"), Some(object(10)))].into(),
        true,
    );
    assert!(matches!(
        f.index
            .update_hook(&r, &mut f.nodes, |stage| if stage == Stage::Prepared {
                Err(Error::Injected)
            } else {
                Ok(())
            }),
        Err(Error::Injected)
    ));
    f.reopen();
    for o in [object(0), object(10)] {
        assert_eq!(
            f.index
                .inspect(&mut f.nodes, &o, &d("F"))
                .unwrap()
                .protected_by,
            vec![key("main")]
        );
    }
    f.index.resume(&r.operation, &mut f.nodes).unwrap();
    assert!(
        f.index
            .inspect(&mut f.nodes, &object(0), &d("F"))
            .unwrap()
            .protected_by
            .is_empty()
    );
    assert_eq!(
        f.index
            .inspect(&mut f.nodes, &object(10), &d("F"))
            .unwrap()
            .protected_by,
        vec![key("main")]
    );
    assert!(f.index.resume(&r.operation, &mut f.nodes).unwrap().replayed);
}
#[test]
fn incomplete_coverage_and_missing_metadata_are_not_zero_protection() {
    let mut f = Fixture::new(3);
    let r = f.request("jobs", "unknown", BTreeMap::new(), false);
    f.index.update(&r, &mut f.nodes).unwrap();
    assert!(matches!(
        f.index.inspect(&mut f.nodes, &object(99), &d("F")),
        Err(Error::Unknown)
    ));
    let r = f.request("jobs", "ready", BTreeMap::new(), true);
    f.index.update(&r, &mut f.nodes).unwrap();
    struct Offline;
    impl NodeStore for Offline {
        fn get(&self, _: &Object) -> std::result::Result<Option<Vec<u8>>, TreeError> {
            Ok(None)
        }
        fn put(&mut self, _: &[u8]) -> std::result::Result<Object, TreeError> {
            Err(TreeError::Budget)
        }
    }
    assert!(matches!(
        f.index.inspect(&mut Offline, &object(99), &d("F")),
        Err(Error::Unknown)
    ));
}
#[test]
fn stale_handles_reject_changes_and_trusted_minimum_rejects_rollback() {
    let mut f = Fixture::new(3);
    let old = f.index.checkpoint();
    let root = f.temp.path().join("index");
    let mut peer = Index::open(&root, &old).unwrap();
    let request = f.request(
        "main",
        "first",
        [(p("assets/a-00000"), Some(object(10)))].into(),
        true,
    );
    f.index.update(&request, &mut f.nodes).unwrap();
    let stale = Update {
        operation: key("stale"),
        ..request.clone()
    };
    assert!(matches!(
        peer.update(&stale, &mut f.nodes),
        Err(Error::Stale)
    ));
    let mut connection = rusqlite::Connection::open(root.join("claims.sqlite")).unwrap();
    let old_hash: String = connection
        .query_row(
            "SELECT hash FROM events WHERE sequence=?1",
            [old.sequence() as i64],
            |r| r.get(0),
        )
        .unwrap();
    let old_bytes: i64 = connection
        .query_row(
            "SELECT SUM(length(body)) FROM events WHERE sequence<=?1",
            [old.sequence() as i64],
            |r| r.get(0),
        )
        .unwrap();
    let tx = connection.transaction().unwrap();
    tx.execute(
        "DELETE FROM events WHERE sequence>?1",
        [old.sequence() as i64],
    )
    .unwrap();
    tx.execute(
        "UPDATE meta SET sequence=?1,hash=?2,bytes=?3 WHERE id=1",
        rusqlite::params![old.sequence() as i64, old_hash, old_bytes],
    )
    .unwrap();
    tx.commit().unwrap();
    assert!(matches!(
        Index::open(&root, &f.index.checkpoint()),
        Err(Error::Journal(super::super::Error::Rollback))
    ));
}

#[test]
fn incremental_update_cost_and_historical_trim_do_not_disturb_pending_work() {
    let mut f = Fixture::new(2000);
    let first = f.request(
        "main",
        "first",
        [(p("assets/a-00000"), Some(object(9000)))].into(),
        true,
    );
    f.nodes.reset_metrics();
    f.index.update(&first, &mut f.nodes).unwrap();
    let metrics = f.nodes.metrics();
    assert!(metrics.node_gets < 400, "{} gets", metrics.node_gets);
    assert!(metrics.puts < 150, "{} puts", metrics.puts);
    println!(
        "INDEX one change among 2000 bindings: {} metadata gets, {} puts",
        metrics.node_gets, metrics.puts
    );
    let next = f.request(
        "main",
        "next",
        [(p("assets/a-00001"), Some(object(9001)))].into(),
        true,
    );
    assert!(matches!(
        f.index
            .update_hook(&next, &mut f.nodes, |s| if s == Stage::Prepared {
                Err(Error::Injected)
            } else {
                Ok(())
            }),
        Err(Error::Injected)
    ));
    let pending = f.index.checkpoint();
    assert!(
        f.index
            .resume(&first.operation, &mut f.nodes)
            .unwrap()
            .replayed
    );
    assert_eq!(pending, f.index.checkpoint());
    f.index.resume(&next.operation, &mut f.nodes).unwrap();
    assert_eq!(
        f.index
            .inspect(&mut f.nodes, &object(9001), &d("F"))
            .unwrap()
            .protected_by,
        vec![key("main")]
    );
}

#[derive(Serialize, Deserialize)]
struct ChildStart {
    journal: Checkpoint,
    metadata: metadata::Checkpoint,
    request: Update,
    phase: String,
}
#[test]
fn index_child() {
    let Some(root) = std::env::var_os("TIG_PUBLIC_INDEX_CHILD") else {
        return;
    };
    let root = PathBuf::from(root);
    let start: ChildStart =
        serde_json::from_slice(&std::fs::read(root.join("start.json")).unwrap()).unwrap();
    let mut index = Index::open(&root.join("index"), &start.journal).unwrap();
    let mut nodes = metadata::MetadataStore::open(&root.join("nodes"), &start.metadata).unwrap();
    index
        .update_hook(&start.request, &mut nodes, |s| {
            if format!("{s:?}") == start.phase {
                std::process::exit(71);
            }
            Ok(())
        })
        .unwrap();
    panic!("child must exit at its admitted boundary");
}
#[test]
fn actual_child_exits_recover_metadata_prepare_and_adopt_without_root_handles() {
    for phase in ["MetadataReady", "Prepared", "Adopted"] {
        let mut f = Fixture::new(3);
        let r = f.request(
            "main",
            "child-update",
            [(p("assets/a-00000"), Some(object(10)))].into(),
            true,
        );
        f.nodes.finish().unwrap();
        let start = ChildStart {
            journal: f.index.checkpoint(),
            metadata: f.nodes.checkpoint(),
            request: r.clone(),
            phase: phase.into(),
        };
        std::fs::write(f.temp.path().join("start.json"), encode(&start).unwrap()).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "source_claims::protection::tests::index_child",
                "--nocapture",
            ])
            .env("TIG_PUBLIC_INDEX_CHILD", f.temp.path())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(71));
        f.index = Index::open(&f.temp.path().join("index"), &start.journal).unwrap();
        f.nodes =
            metadata::MetadataStore::open(&f.temp.path().join("nodes"), &start.metadata).unwrap();
        if phase == "MetadataReady" {
            assert!(matches!(
                f.index.resume(&r.operation, &mut f.nodes),
                Err(Error::Unknown)
            ));
            f.index.update(&r, &mut f.nodes).unwrap();
        } else {
            f.index.resume(&r.operation, &mut f.nodes).unwrap();
        }
        assert!(
            f.index
                .inspect(&mut f.nodes, &object(0), &d("F"))
                .unwrap()
                .protected_by
                .is_empty()
        );
        assert_eq!(
            f.index
                .inspect(&mut f.nodes, &object(10), &d("F"))
                .unwrap()
                .protected_by,
            vec![key("main")]
        );
        assert_eq!(
            f.index.actor(&mut f.nodes, &key("main")).unwrap().revision,
            2
        );
    }
}

#[test]
fn native_job_index_fences_real_unlink_and_preserves_unrelated_admission() {
    use crate::{
        local::removal, retention::release::tests::Fixture as RemovalFixture,
        retirement_intent::progress,
    };
    let mut f = RemovalFixture::new_removable();
    let object = f.grant.request().publication.object.clone();
    let store = f.store.identity().clone();
    let root = f.temp.path().join("protection");
    let meta_root = f.temp.path().join("protection-nodes");
    let mut n = nodes(&meta_root);
    let empty = Tree::empty(&mut n).unwrap();
    let paths = [(p("jobs/current"), object.clone())].into();
    let (jobs, _) = Tree::build(&paths, &mut n, limits()).unwrap();
    n.finish().unwrap();
    let enrollment = Enrollment {
        owner: f.ctx.owner().clone(),
        subject: f.ctx.review().owner().clone(),
        policies: vec![f.ctx.review().policy_identity().clone()],
        stores: vec![store.clone()],
        actors: vec![Actor {
            id: key("jobs"),
            owner: key("job-owner"),
            stores: vec![store.clone()],
        }],
        max_bindings: 100,
    };
    let mut index = Index::create(&root, key("epoch"), enrollment).unwrap();
    index
        .bootstrap(
            [(
                "jobs".into(),
                Selection {
                    root: jobs.root().clone(),
                    context: d("job-frame"),
                },
            )]
            .into(),
            &mut n,
        )
        .unwrap();
    let attempt = f.start();
    let released = f.run(attempt).unwrap();
    f.finish(attempt, released.evidence);
    let attempt = progress::start(
        &mut f.journal,
        &mut f.nodes,
        &f.ctx,
        &object,
        progress::Action::Remove {
            store: store.clone(),
        },
    )
    .unwrap()
    .attempt;
    let plan = removal::plan(&f.store, &f.ctx, &object, attempt).unwrap();
    let mut gate = index
        .protection(&meta_root, &n.checkpoint(), &f.ctx, &plan)
        .unwrap();
    assert!(matches!(
        gate.acquire(plan.scope()),
        Err(crate::Error::Retained)
    ));
    let changes = [(p("jobs/current"), None)].into();
    let r = Update {
        operation: key("job-end"),
        actor: key("jobs"),
        owner: key("job-owner"),
        expected_revision: 1,
        selection: Selection {
            root: empty.root().clone(),
            context: d("empty"),
        },
        changes,
        ready: true,
    };
    index.update(&r, &mut n).unwrap();
    let mut lease = gate.acquire(plan.scope()).unwrap();
    let (with_other, _) = empty
        .edit(
            empty.root(),
            &[(p("jobs/other"), Some(super::tests::object(999)))].into(),
            &mut n,
            limits(),
        )
        .unwrap();
    let unrelated = Update {
        operation: key("other-job"),
        expected_revision: 2,
        selection: Selection {
            root: with_other.root().clone(),
            context: d("other-job"),
        },
        changes: [(p("jobs/other"), Some(super::tests::object(999)))].into(),
        ..r.clone()
    };
    index.update(&unrelated, &mut n).unwrap();
    assert!(lease.revalidate(plan.scope()).is_ok());
    let current = index.actor(&mut n, &key("jobs")).unwrap();
    let old = Tree::from_root(current.selection.root.clone());
    let changes = [(p("jobs/current"), Some(object.clone()))].into();
    let (target, _) = old.edit(old.root(), &changes, &mut n, limits()).unwrap();
    let blocked = Update {
        operation: key("new-protection"),
        expected_revision: 3,
        selection: Selection {
            root: target.root().clone(),
            context: d("new-job"),
        },
        changes,
        ..r.clone()
    };
    assert!(matches!(
        index.update(&blocked, &mut n),
        Err(Error::Store(crate::Error::Busy))
    ));
    let unknown = Update {
        operation: key("coverage-loss"),
        expected_revision: 3,
        selection: current.selection,
        changes: BTreeMap::new(),
        ready: false,
        ..r
    };
    assert!(matches!(
        index.update(&unknown, &mut n),
        Err(Error::Store(crate::Error::Busy))
    ));
    drop(lease);
    let result = removal::apply(
        removal::Execution {
            store: &f.store,
            ledger: &mut f.ledger,
            job: &f.job,
            journal: &mut f.journal,
            nodes: &f.nodes,
            intent: &f.ctx,
            protection: &mut gate,
        },
        &plan,
    )
    .unwrap();
    assert!(f.store.read_object(&object).is_err());
    progress::finish(
        &mut f.journal,
        &mut f.nodes,
        &f.ctx,
        &object,
        attempt,
        progress::Action::Remove { store },
        progress::Outcome::Removed {
            evidence: result.evidence,
            allocation: progress::Allocation {
                before: Some(result.file_data_before),
                reclaimed: None,
            },
        },
    )
    .unwrap();
    assert_eq!(
        f.ctx.decision().consent().accept_intentional_loss,
        BTreeSet::from([object])
    );
}
