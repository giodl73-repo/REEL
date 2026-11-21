use super::produced_cases::{operation, pin, policies, recipe, subject};
use super::*;
use git_asset::source::native::{
    context::ProductionContext,
    job::{self, Admission, OutputEvidence, Role},
};
use git_asset_core::{
    Digest,
    tree::{MemoryNodes, NodeStore, Tree, TreeError, TreeLimits, TreeResult},
};
use git_asset_model::graph::recipe::{self, Plan, produced::Claim};
use git_asset_store::{
    Key,
    metadata::{Limits as StoreLimits, MetadataReader, MetadataStore},
    source_claims::{Command as OwnerCommand, Selection, SourceJournal},
};
use std::collections::BTreeMap;
#[path = "job_dependency_cases.rs"]
mod dependency_cases;
#[path = "job_protection_cases.rs"]
mod protection_cases;

fn fixture() -> (Fixture, AssetSnapshot, Plan, ProductionContext, MemoryNodes) {
    let f = Fixture::new();
    f.record("rows", "inputs/rows", true, json!([]), Some(b"rows-v1"));
    f.record(
        "report",
        "outputs/report",
        false,
        json!([]),
        Some(b"report-v1"),
    );
    let repository = reader(f.root());
    let mut source = capture(&repository, commit(f.root()));
    let plan = recipe(&mut source, &f.profiles, &policies(&f), &operation());
    let q = query(&source, "inputs/rows");
    let mut nodes = MemoryNodes::default();
    let context = source
        .freeze_production_context(&q, &f.profiles, b"{}", &mut nodes, TreeLimits::default())
        .unwrap();
    (f, source, plan, context, nodes)
}
fn names() -> BTreeMap<LogicalPath, Object> {
    BTreeMap::from([(
        LogicalPath::parse("encoder").unwrap(),
        Object::from_bytes(b"declared encoder"),
    )])
}
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn adopt(
    journal: &mut SourceJournal,
    id: &str,
    revision: u64,
    selection: Selection,
    operation: &str,
) {
    let pending = journal
        .apply(
            key(&format!("{operation}-prepare")),
            OwnerCommand::Prepare {
                source: key(id),
                owner: key("fixture"),
                expected_revision: revision,
                selection,
            },
        )
        .unwrap();
    journal
        .apply(
            key(&format!("{operation}-adopt")),
            OwnerCommand::Adopt {
                source: key(id),
                owner: key("fixture"),
                expected_revision: revision + 1,
                prepared: pending.reference,
            },
        )
        .unwrap();
}

#[test]
fn job_envelope_reuses_source_context_and_derives_deduplicated_typed_requirements() {
    let (_f, _source, plan, context, mut nodes) = fixture();
    let mut runtime = names();
    let primary = Object::from_bytes(b"rows-v1");
    runtime.insert(
        LogicalPath::parse("shared-runtime").unwrap(),
        primary.clone(),
    );
    let admission = Admission {
        context: &context,
        plan: &plan,
        runtime: &runtime,
        output: None,
    };
    let candidate = job::freeze(&admission, &mut nodes, TreeLimits::default()).unwrap();
    let (entries, _) = Tree::from_root(candidate.selection().root.clone())
        .entries(&mut nodes, TreeLimits::default())
        .unwrap();
    assert_eq!(entries.len(), 4 + runtime.len());
    assert_eq!(
        entries[&LogicalPath::parse("source/context").unwrap()],
        context.selection().root
    );
    let restored = job::restore(
        candidate.selection(),
        &admission,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    assert!(restored.of(Role::Primary).contains(&primary));
    assert!(restored.of(Role::Runtime).contains(&primary));
    assert!(
        restored
            .of(Role::Runtime)
            .contains(&plan.operation().producer)
    );
    assert!(
        restored
            .of(Role::Interpretation)
            .contains(&plan.operation().parameters)
    );
    assert_eq!(
        restored.logical_bytes(),
        restored.objects().iter().map(|o| o.bytes).sum::<u64>()
    );
    assert!(!restored.complete_claim_coverage());
    let again = job::freeze(&admission, &mut nodes, TreeLimits::default()).unwrap();
    assert_eq!(again.selection(), candidate.selection());
}

#[test]
fn job_claimed_output_handoff_survives_store_journal_restart_and_independent_main_change() {
    let (f, source, plan, context, initial) = fixture();
    let current = subject(&source, &f.profiles);
    let claim = Claim::create(&plan, pin(&context), vec![current.subject().clone()]).unwrap();
    let runtime = names();
    let attachments = BTreeMap::from([(
        LogicalPath::parse("native-receipt").unwrap(),
        Object::from_bytes(b"native receipt"),
    )]);
    let declared_admission = Admission {
        context: &context,
        plan: &plan,
        runtime: &runtime,
        output: None,
    };
    let claimed_admission = Admission {
        context: &context,
        plan: &plan,
        runtime: &runtime,
        output: Some(OutputEvidence {
            claim: &claim,
            attachments: &attachments,
        }),
    };
    let disk = tempfile::tempdir().unwrap();
    let metadata = disk.path().join("metadata");
    let mut store = MetadataStore::create(
        &metadata,
        StoreLimits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    for (_, bytes) in initial.objects() {
        store.put(bytes).unwrap();
    }
    let planned = job::freeze(&declared_admission, &mut store, TreeLimits::default()).unwrap();
    let claimed = job::freeze(&claimed_admission, &mut store, TreeLimits::default()).unwrap();
    assert_ne!(planned.selection(), claimed.selection());
    f.record("rows", "inputs/rows", true, json!([]), Some(b"rows-v2"));
    let repository = reader(f.root());
    let mut later = capture(&repository, commit(f.root()));
    let q = query(&later, "inputs/rows");
    let later_context = later
        .freeze_production_context(&q, &f.profiles, b"{}", &mut store, TreeLimits::default())
        .unwrap();
    let witness = store.finish().unwrap();
    drop(store);
    drop(source);
    drop(later);
    drop(repository);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut store = MetadataReader::open(&metadata, &witness).unwrap();
    let (mut restored, replay) = AssetSnapshot::from_production_context(
        context.selection(),
        &f.profiles,
        &mut store,
        Limits::default(),
        TreeLimits::default(),
        q.limits,
    )
    .unwrap();
    let restored_plan = recipe(&mut restored, &f.profiles, &policies(&f), &operation());
    let admission = Admission {
        context: &replay,
        plan: &restored_plan,
        runtime: &runtime,
        output: None,
    };
    let verified = job::restore(
        planned.selection(),
        &admission,
        &mut store,
        TreeLimits::default(),
    )
    .unwrap();
    let journal_path = disk.path().join("sources");
    let mut journal = SourceJournal::create(&journal_path, key("fixture"), key("epoch")).unwrap();
    adopt(
        &mut journal,
        "job",
        0,
        verified.selection().clone(),
        "job-start",
    );
    adopt(
        &mut journal,
        "peer-job",
        0,
        verified.selection().clone(),
        "peer-start",
    );
    adopt(
        &mut journal,
        "main",
        0,
        context.selection().clone(),
        "main-start",
    );
    adopt(
        &mut journal,
        "main",
        2,
        later_context.selection().clone(),
        "main-next",
    );
    journal
        .apply(
            key("main-retire"),
            OwnerCommand::Retire {
                source: key("main"),
                owner: key("fixture"),
                expected_revision: 4,
                predecessor_revision: 2,
            },
        )
        .unwrap();
    let admission = Admission {
        context: &replay,
        plan: &restored_plan,
        runtime: &runtime,
        output: Some(OutputEvidence {
            claim: &claim,
            attachments: &attachments,
        }),
    };
    let verified = job::restore(
        claimed.selection(),
        &admission,
        &mut store,
        TreeLimits::default(),
    )
    .unwrap();
    adopt(
        &mut journal,
        "job",
        2,
        verified.selection().clone(),
        "job-claimed",
    );
    let checkpoint = journal.checkpoint();
    drop(journal);
    let mut journal = SourceJournal::open(&journal_path, &checkpoint).unwrap();
    let obligations = journal.obligations(100).unwrap();
    assert_eq!(obligations.len(), 4);
    assert!(!obligations.complete_claim_coverage());
    assert!(
        obligations
            .iter()
            .any(|o| o.source == key("peer-job") && o.selection == *planned.selection())
    );
    assert!(
        obligations
            .iter()
            .any(|o| o.source == key("job") && o.selection == *planned.selection())
    );
    assert!(
        obligations
            .iter()
            .any(|o| o.source == key("job") && o.selection == *claimed.selection())
    );
    assert!(
        obligations
            .iter()
            .all(|o| o.source != key("main") || o.selection == *later_context.selection())
    );
    assert!(
        verified
            .of(Role::Primary)
            .contains(&Object::from_bytes(b"rows-v1"))
    );
    assert!(
        !later_context
            .requirements()
            .primary()
            .contains(&Object::from_bytes(b"rows-v1"))
    );
    assert!(
        verified
            .of(Role::ClaimedOutput)
            .contains(current.subject().object())
    );
    assert!(
        verified
            .of(Role::Evidence)
            .contains(&Object::from_bytes(b"native receipt"))
    );
    // No payload availability, producer execution or custody follows from pins.
    assert!(store.get(current.subject().object()).unwrap().is_none());
}

struct NoIo;
impl NodeStore for NoIo {
    fn get(&self, _: &Object) -> TreeResult<Option<Vec<u8>>> {
        panic!("unexpected metadata read")
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        panic!("unexpected metadata write")
    }
}
#[test]
fn complete_evidence_uses_total_entry_budget_and_preserves_sparse_prior_roots() {
    let (f, source, plan, context, mut nodes) = fixture();
    let current = subject(&source, &f.profiles);
    let claim = Claim::create(&plan, pin(&context), vec![current.subject().clone()]).unwrap();
    let runtime = names();
    let attachments: BTreeMap<_, _> = (0..1000)
        .map(|i| {
            (
                LogicalPath::parse(&format!("outputs/item-{i:04}")).unwrap(),
                Object::from_bytes(format!("retained output {i}").as_bytes()),
            )
        })
        .collect();
    let admission = Admission {
        context: &context,
        plan: &plan,
        runtime: &runtime,
        output: Some(OutputEvidence {
            claim: &claim,
            attachments: &attachments,
        }),
    };
    let limit = 4 + runtime.len() + 1 + attachments.len();
    let exact = TreeLimits {
        max_entries: limit,
        ..TreeLimits::default()
    };
    let short = TreeLimits {
        max_entries: limit - 1,
        ..exact
    };
    assert!(job::freeze(&admission, &mut NoIo, short).is_err());
    let original = job::freeze(&admission, &mut nodes, exact).unwrap();
    assert!(job::restore(original.selection(), &admission, &mut NoIo, short).is_err());
    let restored = job::restore(original.selection(), &admission, &mut nodes, exact).unwrap();
    assert_eq!(restored.of(Role::Evidence).len(), 1000);
    assert!(
        attachments
            .values()
            .all(|object| restored.of(Role::Evidence).contains(object))
    );
    assert!(
        restored
            .of(Role::Primary)
            .is_disjoint(restored.of(Role::Evidence))
    );
    assert!(
        restored
            .of(Role::ClaimedOutput)
            .contains(current.subject().object())
    );
    assert!(!restored.complete_claim_coverage());
    let mut changed = attachments.clone();
    let replacement = Object::from_bytes(b"one corrected output");
    changed.insert(
        LogicalPath::parse("outputs/item-0999").unwrap(),
        replacement.clone(),
    );
    let new_admission = Admission {
        context: &context,
        plan: &plan,
        runtime: &runtime,
        output: Some(OutputEvidence {
            claim: &claim,
            attachments: &changed,
        }),
    };
    let newer = job::freeze(&new_admission, &mut nodes, exact).unwrap();
    assert_ne!(original.selection(), newer.selection());
    let old_again = job::restore(original.selection(), &admission, &mut nodes, exact).unwrap();
    assert!(!old_again.of(Role::Evidence).contains(&replacement));
    assert!(job::restore(original.selection(), &new_admission, &mut nodes, exact).is_err());
    let conflicts = ["a", "a-b", "a/b"]
        .into_iter()
        .map(|name| {
            (
                LogicalPath::parse(name).unwrap(),
                Object::from_bytes(b"evidence"),
            )
        })
        .collect();
    assert!(
        job::freeze(
            &Admission {
                context: &context,
                plan: &plan,
                runtime: &runtime,
                output: Some(OutputEvidence {
                    claim: &claim,
                    attachments: &conflicts
                })
            },
            &mut NoIo,
            exact
        )
        .is_err()
    );
}
#[test]
fn unsafe_conflicting_overlapping_and_overbudget_jobs_refuse_before_metadata_io() {
    let (f, mut source, plan, context, _) = fixture();
    let mut runtime = names();
    let mut bad = Object::from_bytes(b"rows-v1");
    bad.bytes += 1;
    runtime.insert(LogicalPath::parse("conflict").unwrap(), bad);
    assert!(
        job::freeze(
            &Admission {
                context: &context,
                plan: &plan,
                runtime: &runtime,
                output: None
            },
            &mut NoIo,
            TreeLimits::default()
        )
        .is_err()
    );
    runtime = names();
    runtime.insert(
        LogicalPath::parse("unsafe").unwrap(),
        Object {
            sha256: Digest::blob(b"unsafe"),
            bytes: 9_007_199_254_740_992,
        },
    );
    assert!(
        job::freeze(
            &Admission {
                context: &context,
                plan: &plan,
                runtime: &runtime,
                output: None
            },
            &mut NoIo,
            TreeLimits::default()
        )
        .is_err()
    );
    runtime = ["a", "a-b", "a/b"]
        .into_iter()
        .map(|n| {
            (
                LogicalPath::parse(n).unwrap(),
                Object::from_bytes(b"runtime"),
            )
        })
        .collect();
    assert!(
        job::freeze(
            &Admission {
                context: &context,
                plan: &plan,
                runtime: &runtime,
                output: None
            },
            &mut NoIo,
            TreeLimits::default()
        )
        .is_err()
    );
    runtime = (0..65)
        .map(|i| {
            (
                LogicalPath::parse(&format!("item-{i}")).unwrap(),
                Object::from_bytes(b"runtime"),
            )
        })
        .collect();
    assert!(
        job::freeze(
            &Admission {
                context: &context,
                plan: &plan,
                runtime: &runtime,
                output: None
            },
            &mut NoIo,
            TreeLimits::default()
        )
        .is_err()
    );
    runtime = names();
    let admission = Admission {
        context: &context,
        plan: &plan,
        runtime: &runtime,
        output: None,
    };
    assert!(
        job::freeze(
            &admission,
            &mut NoIo,
            TreeLimits {
                max_entries: 0,
                ..TreeLimits::default()
            }
        )
        .is_err()
    );
    let mut wrong = operation();
    wrong.parameters = Object::from_bytes(b"different parameters");
    let wrong_plan = recipe(&mut source, &f.profiles, &policies(&f), &wrong);
    assert!(
        job::freeze(
            &Admission {
                context: &context,
                plan: &wrong_plan,
                runtime: &runtime,
                output: None
            },
            &mut NoIo,
            TreeLimits::default()
        )
        .is_err()
    );
    let current = subject(&source, &f.profiles);
    let claim = Claim::create(
        &plan,
        git_asset_model::graph::recipe::produced::ContextPin {
            root: Object::from_bytes(b"foreign context"),
            identity: Digest::blob(b"foreign identity"),
        },
        vec![current.subject().clone()],
    )
    .unwrap();
    let attachments = BTreeMap::new();
    assert!(
        job::freeze(
            &Admission {
                context: &context,
                plan: &plan,
                runtime: &runtime,
                output: Some(OutputEvidence {
                    claim: &claim,
                    attachments: &attachments
                })
            },
            &mut NoIo,
            TreeLimits::default()
        )
        .is_err()
    );
}

struct ReadOnly<'a> {
    base: &'a MemoryNodes,
    missing: Option<Object>,
    corrupt: Option<Object>,
}
impl NodeStore for ReadOnly<'_> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        if self.missing.as_ref() == Some(o) {
            return Ok(None);
        }
        if self.corrupt.as_ref() == Some(o) {
            return Ok(Some(b"changed metadata".to_vec()));
        }
        self.base.get(o)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        panic!("restore must not write")
    }
}
#[test]
fn job_restore_refuses_missing_corrupt_extra_and_substituted_metadata_without_writes() {
    let (f, source, plan, context, mut nodes) = fixture();
    let current = subject(&source, &f.profiles);
    let claim = Claim::create(&plan, pin(&context), vec![current.subject().clone()]).unwrap();
    let runtime = names();
    let attachments = BTreeMap::new();
    let admission = Admission {
        context: &context,
        plan: &plan,
        runtime: &runtime,
        output: Some(OutputEvidence {
            claim: &claim,
            attachments: &attachments,
        }),
    };
    let job = job::freeze(&admission, &mut nodes, TreeLimits::default()).unwrap();
    let (entries, _) = Tree::from_root(job.selection().root.clone())
        .entries(&mut nodes, TreeLimits::default())
        .unwrap();
    let header = entries[&LogicalPath::parse("job/identity").unwrap()].clone();
    for object in [&job.selection().root, &header, claim.object()] {
        for missing in [true, false] {
            let mut readonly = ReadOnly {
                base: &nodes,
                missing: missing.then(|| object.clone()),
                corrupt: (!missing).then(|| object.clone()),
            };
            assert!(
                job::restore(
                    job.selection(),
                    &admission,
                    &mut readonly,
                    TreeLimits::default()
                )
                .is_err()
            );
        }
    }
    let mut extra = entries.clone();
    extra.insert(
        LogicalPath::parse("extra/item").unwrap(),
        Object::from_bytes(b"unexpected"),
    );
    let tree = Tree::build(&extra, &mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    let changed = Selection {
        root: tree.root().clone(),
        context: job.selection().context.clone(),
    };
    assert!(
        job::restore(
            &changed,
            &admission,
            &mut ReadOnly {
                base: &nodes,
                missing: None,
                corrupt: None
            },
            TreeLimits::default()
        )
        .is_err()
    );
    let wrong = Selection {
        root: job.selection().root.clone(),
        context: Digest::blob(b"wrong identity"),
    };
    assert!(job::restore(&wrong, &admission, &mut NoIo, TreeLimits::default()).is_err());
}

struct Writes<'a> {
    base: &'a mut MemoryNodes,
    writes: usize,
    fail_after: Option<usize>,
}
impl NodeStore for Writes<'_> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.base.get(object)
    }
    fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
        if self.fail_after == Some(self.writes) {
            return Err(TreeError::Store("injected metadata interruption".into()));
        }
        self.writes += 1;
        self.base.put(bytes)
    }
}
#[test]
fn interrupted_job_freeze_keeps_old_context_and_exact_retry_root() {
    let (_f, _source, plan, context, mut nodes) = fixture();
    let runtime = names();
    let admission = Admission {
        context: &context,
        plan: &plan,
        runtime: &runtime,
        output: None,
    };
    let baseline: Vec<_> = nodes.objects().map(|(_, bytes)| bytes.to_vec()).collect();
    let expected = job::freeze(&admission, &mut nodes, TreeLimits::default()).unwrap();
    for fail_after in [0, 1, 2, 5] {
        let mut interrupted = MemoryNodes::default();
        for bytes in &baseline {
            interrupted.put(bytes).unwrap();
        }
        let mut failing = Writes {
            base: &mut interrupted,
            writes: 0,
            fail_after: Some(fail_after),
        };
        assert!(job::freeze(&admission, &mut failing, TreeLimits::default()).is_err());
        let again = job::freeze(&admission, &mut interrupted, TreeLimits::default()).unwrap();
        assert_eq!(again.selection(), expected.selection());
        assert!(
            Tree::from_root(context.selection().root.clone())
                .entries(&mut interrupted, TreeLimits::default())
                .is_ok()
        );
    }
}
#[test]
fn thousand_primary_inputs_still_use_a_small_linked_job_envelope() {
    let f = Fixture::new();
    let mut deps = Vec::new();
    for i in 0..1000 {
        let id = format!("inputs/row-{i}");
        f.record(
            &format!("row-{i}"),
            &id,
            true,
            json!([]),
            Some(format!("row payload {i}").as_bytes()),
        );
        deps.push(json!({"id":id}));
    }
    f.record(
        "rows",
        "inputs/rows",
        true,
        json!(deps),
        Some(b"root payload"),
    );
    let repository = reader(f.root());
    let mut source = capture(&repository, commit(f.root()));
    let mut q = query(&source, "inputs/rows");
    q.limits = graph::Limits {
        nodes: 2000,
        edges: 2000,
        depth: 10,
        bytes: 16 * 1024 * 1024,
    };
    let graph = graph::resolve(&mut source, &q, &f.profiles).unwrap();
    let plan = recipe::plan(
        &graph,
        &mut source,
        &f.profiles,
        &policies(&f),
        &operation(),
        recipe::Limits {
            nodes: 2000,
            edges: 2000,
            bytes: 16 * 1024 * 1024,
        },
    )
    .unwrap();
    let mut nodes = MemoryNodes::default();
    let context = source
        .freeze_production_context(&q, &f.profiles, b"{}", &mut nodes, TreeLimits::default())
        .unwrap();
    assert_eq!(context.requirements().primary().len(), 1001);
    let runtime = names();
    let admission = Admission {
        context: &context,
        plan: &plan,
        runtime: &runtime,
        output: None,
    };
    let mut counted = Writes {
        base: &mut nodes,
        writes: 0,
        fail_after: None,
    };
    let job = job::freeze(&admission, &mut counted, TreeLimits::default()).unwrap();
    assert!(counted.writes < 64, "job copied expanded source metadata");
    let (entries, _) = Tree::from_root(job.selection().root.clone())
        .entries(&mut counted, TreeLimits::default())
        .unwrap();
    assert_eq!(entries.len(), 5);
    assert_eq!(job.of(Role::Primary).len(), 1001);
}
