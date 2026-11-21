use super::super::Selection;
use super::*;
use git_asset_core::{
    LogicalPath,
    tree::{MemoryNodes, TreeResult},
};
use std::{cell::Cell, path::PathBuf, process::Command as Process};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn limits() -> Limits {
    Limits {
        millis: 60_000,
        roots: 256,
        bindings: 10000,
        associations: 100000,
        reads: 100000,
        read_bytes: 32 * 1024 * 1024,
    }
}
struct ConditionRace<'a> {
    nodes: &'a MemoryNodes,
    path: PathBuf,
    minimum: Checkpoint,
    revision: u64,
    change_witness: bool,
    fired: Cell<bool>,
}
impl NodeStore for ConditionRace<'_> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        if !self.fired.replace(true) {
            let mut peer = Namespace::open(&self.path, &self.minimum).unwrap();
            let command = if self.change_witness {
                Command::Withdraw {
                    source: key("main"),
                    owner: key("owner"),
                    expected_revision: self.revision,
                }
            } else {
                Command::Enroll {
                    source: key("unrelated"),
                    owner: key("owner"),
                    class: Kind::Job,
                }
            };
            peer.apply(key("racing-condition"), command, self.nodes, &limits())
                .unwrap();
        }
        self.nodes.get(object)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        panic!("condition assessment is read-only")
    }
}
#[test]
fn source_conditions_recheck_after_tree_io_and_allow_unrelated_progress() {
    for change_witness in [false, true] {
        let mut f = Fixture::new(true);
        f.enroll("main", Kind::Branch);
        f.selected("main", f.old.clone());
        f.enroll("draft", Kind::Draft);
        f.selected("draft", f.old.clone());
        let main = f.namespace.source(&key("main")).unwrap().unwrap();
        let draft = f.namespace.source(&key("draft")).unwrap().unwrap();
        let race = ConditionRace {
            nodes: &f.nodes,
            path: f.root(),
            minimum: f.namespace.checkpoint(),
            revision: main.revision,
            change_witness,
            fired: Cell::new(false),
        };
        let command = Command::Withdraw {
            source: key("draft"),
            owner: key("owner"),
            expected_revision: draft.revision,
        };
        let result = f.namespace.apply_if_sources(
            key("settle-draft"),
            command.clone(),
            &[SourceCondition {
                source: key("main"),
                revision: main.revision,
            }],
            &race,
            &limits(),
        );
        assert!(race.fired.get());
        if change_witness {
            assert_eq!(result, Err(Error::Journal(super::super::Error::Stale)));
            assert!(
                f.namespace
                    .command_status(key("settle-draft"), command)
                    .unwrap()
                    .is_none()
            );
        } else {
            assert!(result.is_ok());
            assert!(f.namespace.source(&key("unrelated")).unwrap().is_some());
        }
        assert_eq!(
            f.namespace
                .source(&key("draft"))
                .unwrap()
                .unwrap()
                .current
                .is_some(),
            change_witness
        );
    }
}
#[test]
fn conditional_withdrawal_replays_historically_without_regranting_changed_conditions() {
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Branch);
    f.selected("main", f.old.clone());
    f.enroll("draft", Kind::Draft);
    f.selected("draft", f.old.clone());
    let conditions = [SourceCondition {
        source: key("main"),
        revision: f.namespace.source(&key("main")).unwrap().unwrap().revision,
    }];
    let command = Command::Withdraw {
        source: key("draft"),
        owner: key("owner"),
        expected_revision: f.namespace.source(&key("draft")).unwrap().unwrap().revision,
    };
    let first = f
        .namespace
        .apply_if_sources(
            key("conditional"),
            command.clone(),
            &conditions,
            &f.nodes,
            &limits(),
        )
        .unwrap();
    f.selected("main", f.new.clone());
    f.selected("draft", f.new.clone());
    let before = f.namespace.source(&key("draft")).unwrap().unwrap();
    let checkpoint = f.namespace.checkpoint();
    let replay = f
        .namespace
        .apply_if_sources(
            key("conditional"),
            command.clone(),
            &conditions,
            &MemoryNodes::default(),
            &limits(),
        )
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.reference, first.reference);
    assert_eq!(f.namespace.source(&key("draft")).unwrap().unwrap(), before);
    assert_eq!(f.namespace.checkpoint(), checkpoint);
    assert!(
        f.namespace
            .command_status(key("conditional"), command)
            .unwrap()
            .unwrap()
            .replayed
    );
}
#[test]
fn malformed_or_missing_source_conditions_refuse_without_mutation() {
    let mut f = Fixture::new(true);
    f.enroll("draft", Kind::Draft);
    f.selected("draft", f.old.clone());
    let command = Command::Withdraw {
        source: key("draft"),
        owner: key("owner"),
        expected_revision: f.namespace.source(&key("draft")).unwrap().unwrap().revision,
    };
    let before = f.namespace.checkpoint();
    for conditions in [
        vec![SourceCondition {
            source: key("missing"),
            revision: 1,
        }],
        vec![SourceCondition {
            source: key("draft"),
            revision: 0,
        }],
        vec![
            SourceCondition {
                source: key("draft"),
                revision: 1
            };
            2
        ],
        vec![
            SourceCondition {
                source: key("draft"),
                revision: 1
            };
            9
        ],
    ] {
        assert!(
            f.namespace
                .apply_if_sources(
                    key("invalid-condition"),
                    command.clone(),
                    &conditions,
                    &f.nodes,
                    &limits()
                )
                .is_err()
        );
        assert_eq!(f.namespace.checkpoint(), before);
    }
}
fn policy(prefer_active: bool) -> Policy {
    let places = Places {
        current: vec![key("working"), key("backup")],
        pending: vec![key("working"), key("backup")],
        predecessor: vec![key("history"), key("backup")],
    };
    let retained = Places {
        current: vec![key("backup")],
        pending: vec![key("backup")],
        predecessor: vec![key("backup")],
    };
    Policy {
        prefer_active,
        classes: [
            (Kind::Branch, places.clone()),
            (Kind::Draft, places.clone()),
            (Kind::Job, places),
            (Kind::Retention, retained),
        ]
        .into_iter()
        .collect(),
    }
}
fn tree(nodes: &mut impl NodeStore, entries: &[(&str, Object)]) -> Selection {
    let entries = entries
        .iter()
        .map(|(name, object)| (LogicalPath::parse(name).unwrap(), object.clone()))
        .collect();
    let (tree, _) = Tree::build(&entries, nodes, TreeLimits::default()).unwrap();
    Selection {
        root: tree.root().clone(),
        context: Digest::blob(b"context"),
    }
}
struct Fixture {
    temp: tempfile::TempDir,
    namespace: Namespace,
    nodes: MemoryNodes,
    a: Object,
    b: Object,
    common: Object,
    old: Selection,
    new: Selection,
}
impl Fixture {
    fn new(prefer_active: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let namespace = Namespace::create(
            &temp.path().join("scope"),
            key("namespace"),
            key("epoch"),
            policy(prefer_active),
        )
        .unwrap();
        let mut nodes = MemoryNodes::default();
        let a = Object::from_bytes(b"old");
        let b = Object::from_bytes(b"new");
        let common = Object::from_bytes(b"common");
        let old = tree(
            &mut nodes,
            &[("changed", a.clone()), ("unchanged", common.clone())],
        );
        let new = tree(
            &mut nodes,
            &[("changed", b.clone()), ("unchanged", common.clone())],
        );
        Self {
            temp,
            namespace,
            nodes,
            a,
            b,
            common,
            old,
            new,
        }
    }
    fn root(&self) -> PathBuf {
        self.temp.path().join("scope")
    }
    fn enroll(&mut self, source: &str, class: Kind) {
        self.namespace
            .apply(
                key(&format!("enroll-{source}")),
                Command::Enroll {
                    source: key(source),
                    owner: key("owner"),
                    class,
                },
                &self.nodes,
                &limits(),
            )
            .unwrap();
    }
    fn selected(&mut self, source: &str, selection: Selection) {
        let revision = self
            .namespace
            .source(&key(source))
            .unwrap()
            .unwrap()
            .revision;
        let receipt = self
            .namespace
            .apply(
                key(&format!("prepare-{source}-{revision}")),
                Command::Prepare {
                    source: key(source),
                    owner: key("owner"),
                    expected_revision: revision,
                    selection,
                },
                &self.nodes,
                &limits(),
            )
            .unwrap();
        self.namespace
            .apply(
                key(&format!("adopt-{source}-{revision}")),
                Command::Adopt {
                    source: key(source),
                    owner: key("owner"),
                    expected_revision: revision + 1,
                    prepared: receipt.reference,
                },
                &self.nodes,
                &limits(),
            )
            .unwrap();
    }
    fn absent(&mut self, object: &Object, place: &str) -> bool {
        let fence = self.namespace.fence(object).unwrap();
        self.namespace
            .absence(&fence, &key(place), &self.nodes, &limits())
            .unwrap()
            .is_some()
    }
}
#[test]
fn guarded_marker_and_legacy_encoding_are_distinct_and_direct_bypass_refuses() {
    let mut f = Fixture::new(true);
    let minimum = f.namespace.checkpoint();
    let marker = fs::read(f.root().join("claims.json")).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&marker).unwrap();
    assert_eq!(value["schema"], "tig.source-claims.design-2");
    assert!(value.get("admission").is_some());
    assert_eq!(
        fs::read_dir(f.root().join("admission-fences"))
            .unwrap()
            .count(),
        256
    );
    let command = Command::Enroll {
        source: key("main"),
        owner: key("owner"),
        class: Kind::Branch,
    };
    let mut direct = SourceJournal::open(&f.root(), &minimum).unwrap();
    assert_eq!(
        direct.apply(key("direct"), command.clone()),
        Err(super::super::Error::AdmissionRequired)
    );
    assert_eq!(
        direct.apply_hook(key("hook"), command.clone(), |_| Ok(())),
        Err(super::super::Error::AdmissionRequired)
    );
    assert_eq!(
        SourceJournal::preview(&f.root(), &minimum, key("preview"), command.clone()),
        Err(super::super::Error::AdmissionRequired)
    );
    assert_eq!(direct.checkpoint(), minimum);
    let receipt = f
        .namespace
        .apply(key("enrolled"), command.clone(), &f.nodes, &limits())
        .unwrap();
    let recovered =
        SourceJournal::acknowledge(&f.root(), &minimum, key("enrolled"), command).unwrap();
    assert!(recovered.replayed);
    assert_eq!(receipt.checkpoint, recovered.checkpoint);
    let legacy_root = f.temp.path().join("legacy");
    let legacy = SourceJournal::create(&legacy_root, key("project"), key("epoch")).unwrap();
    let raw = fs::read(legacy_root.join("claims.json")).unwrap();
    let legacy_value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(legacy_value["schema"], "tig.source-claims.design-1");
    assert!(legacy_value.get("admission").is_none());
    assert_eq!(fs::read_dir(&legacy_root).unwrap().count(), 2);
    assert!(Namespace::open(&legacy_root, &legacy.checkpoint()).is_err());
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct OldConfig {
        schema: String,
        project: Key,
        epoch: Key,
        instance: Digest,
    }
    assert!(serde_json::from_slice::<OldConfig>(&marker).is_err());
    let old: OldConfig = serde_json::from_slice(&raw).unwrap();
    assert_eq!(old.schema, "tig.source-claims.design-1");
    let _ = (old.project, old.epoch, old.instance);
}

#[test]
fn placement_requirements_project_precedence_without_journal_mutation() {
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Branch);
    f.selected("main", f.old.clone());
    f.selected("main", f.new.clone());
    let before = f.namespace.checkpoint();
    let requirements = f.namespace.requirements(&f.nodes, &limits()).unwrap();
    assert_eq!(requirements.checkpoint, before);
    assert_eq!(f.namespace.checkpoint(), before);
    let get = |o: &Object| {
        requirements
            .entries
            .iter()
            .find(|e| &e.object == o)
            .unwrap()
            .places
            .iter()
            .map(Key::as_str)
            .collect::<Vec<_>>()
    };
    assert_eq!(get(&f.a), vec!["backup", "history"]);
    assert_eq!(get(&f.b), vec!["backup", "working"]);
    assert_eq!(get(&f.common), vec!["backup", "working"]);
    assert!(matches!(
        f.namespace.requirements(&MemoryNodes::default(), &limits()),
        Err(Error::IncompleteClosure)
    ));
    assert_eq!(f.namespace.checkpoint(), before);
}

#[test]
fn placement_requirements_share_budget_across_namespaces() {
    let mut a = Fixture::new(true);
    let mut b = Fixture::new(true);
    a.enroll("main", Kind::Branch);
    a.selected("main", a.old.clone());
    b.enroll("main", Kind::Branch);
    b.selected("main", b.old.clone());
    let mut small = limits();
    small.roots = 1;
    let mut work = Work::new(&a.nodes, &small, Instant::now()).unwrap();
    a.namespace.requirements_work(&mut work).unwrap();
    assert!(matches!(
        b.namespace.requirements_work(&mut work),
        Err(Error::Budget)
    ));
    // The same second namespace succeeds with its own separately admitted work.
    b.namespace.requirements(&b.nodes, &small).unwrap();
}
#[test]
fn source_enrollment_is_mandatory_and_class_owner_and_operation_are_immutable() {
    let mut f = Fixture::new(true);
    let before = f.namespace.checkpoint();
    let request = Command::Prepare {
        source: key("main"),
        owner: key("owner"),
        expected_revision: 0,
        selection: f.old.clone(),
    };
    assert!(matches!(
        f.namespace
            .apply(key("unregistered"), request, &f.nodes, &limits()),
        Err(Error::Journal(super::super::Error::AdmissionRequired))
    ));
    assert_eq!(f.namespace.checkpoint(), before);
    f.enroll("main", Kind::Branch);
    assert_eq!(
        f.namespace.source(&key("main")).unwrap().unwrap().kind,
        Some(Kind::Branch)
    );
    assert!(
        f.namespace
            .apply(
                key("steal"),
                Command::Enroll {
                    source: key("main"),
                    owner: key("foreign"),
                    class: Kind::Job
                },
                &f.nodes,
                &limits()
            )
            .is_err()
    );
    assert!(
        f.namespace
            .apply(
                key("reclassify"),
                Command::Enroll {
                    source: key("main"),
                    owner: key("owner"),
                    class: Kind::Job
                },
                &f.nodes,
                &limits()
            )
            .is_err()
    );
    assert!(
        f.namespace
            .apply(
                key("enroll-main"),
                Command::Enroll {
                    source: key("other"),
                    owner: key("owner"),
                    class: Kind::Branch
                },
                &f.nodes,
                &limits()
            )
            .is_err()
    );
    let receipt = f
        .namespace
        .apply(
            key("enroll-main"),
            Command::Enroll {
                source: key("main"),
                owner: key("owner"),
                class: Kind::Branch,
            },
            &f.nodes,
            &limits(),
        )
        .unwrap();
    assert!(receipt.replayed);
}
#[test]
fn exclusive_subject_blocks_late_claim_and_allows_it_only_after_release() {
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Branch);
    let before = f.namespace.checkpoint();
    let fence = f.namespace.fence(&f.a).unwrap();
    let request = Command::Prepare {
        source: key("main"),
        owner: key("owner"),
        expected_revision: 1,
        selection: f.old.clone(),
    };
    assert_eq!(
        f.namespace
            .apply(key("late"), request.clone(), &f.nodes, &limits()),
        Err(Error::Busy)
    );
    assert_eq!(f.namespace.checkpoint(), before);
    let fact = f
        .namespace
        .absence(&fence, &key("working"), &f.nodes, &limits())
        .unwrap()
        .unwrap();
    assert!(!fact.authorizes_removal());
    assert_eq!(fact.object(), &f.a);
    assert_eq!(fact.authority(), f.namespace.identity());
    drop(fact);
    drop(fence);
    f.namespace
        .apply(key("late"), request, &f.nodes, &limits())
        .unwrap();
    let fence = f.namespace.fence(&f.a).unwrap();
    assert!(
        f.namespace
            .absence(&fence, &key("working"), &f.nodes, &limits())
            .unwrap()
            .is_none()
    );
}
#[test]
fn unknown_source_can_enroll_but_cannot_add_conflicting_payload_under_subject_fence() {
    let mut f = Fixture::new(true);
    let fence = f.namespace.fence(&f.a).unwrap();
    f.enroll("new-source", Kind::Job);
    let before = f.namespace.checkpoint();
    assert_eq!(
        f.namespace.apply(
            key("new-payload"),
            Command::Prepare {
                source: key("new-source"),
                owner: key("owner"),
                expected_revision: 1,
                selection: f.old.clone()
            },
            &f.nodes,
            &limits()
        ),
        Err(Error::Busy)
    );
    assert_eq!(f.namespace.checkpoint(), before);
    assert!(
        f.namespace
            .absence(&fence, &key("working"), &f.nodes, &limits())
            .unwrap()
            .is_some()
    );
}
#[test]
fn active_history_precedence_does_not_duplicate_unchanged_or_job_used_bytes() {
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Branch);
    f.selected("main", f.old.clone());
    f.selected("main", f.new.clone());
    let a = f.a.clone();
    let b = f.b.clone();
    let common = f.common.clone();
    assert!(f.absent(&a, "working"));
    assert!(!f.absent(&a, "history"));
    assert!(!f.absent(&a, "backup"));
    assert!(!f.absent(&b, "working"));
    assert!(f.absent(&common, "history"));
    f.enroll("job", Kind::Job);
    f.selected("job", f.old.clone());
    assert!(!f.absent(&a, "working"));
    assert!(f.absent(&a, "history"));
    let revision = f.namespace.source(&key("job")).unwrap().unwrap().revision;
    f.namespace
        .apply(
            key("withdraw-job"),
            Command::Withdraw {
                source: key("job"),
                owner: key("owner"),
                expected_revision: revision,
            },
            &f.nodes,
            &limits(),
        )
        .unwrap();
    assert!(f.absent(&a, "working"));
    assert!(!f.absent(&a, "history"));
}
#[test]
fn retained_class_is_additive_and_precedence_can_be_explicitly_disabled() {
    let mut p = policy(true);
    let history = Places {
        current: vec![key("history")],
        pending: vec![key("history")],
        predecessor: vec![key("history")],
    };
    p.classes.insert(Kind::Retention, history);
    let mut f = Fixture::new(true);
    let path = f.temp.path().join("hard-retention");
    f.namespace = Namespace::create(&path, key("project"), key("epoch"), p).unwrap();
    f.enroll("main", Kind::Branch);
    f.selected("main", f.old.clone());
    f.enroll("archive", Kind::Retention);
    f.selected("archive", f.old.clone());
    let a = f.a.clone();
    assert!(!f.absent(&a, "history"));
    assert!(!f.absent(&a, "working"));
    let mut f = Fixture::new(false);
    f.enroll("main", Kind::Branch);
    f.selected("main", f.old.clone());
    f.selected("main", f.new.clone());
    let common = f.common.clone();
    assert!(!f.absent(&common, "history"));
}
#[test]
fn role_changes_are_fenced_even_when_the_object_identity_is_unchanged() {
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Branch);
    f.selected("main", f.old.clone());
    let before = f.namespace.source(&key("main")).unwrap().unwrap();
    let prepared = f
        .namespace
        .apply(
            key("same-prepare"),
            Command::Prepare {
                source: key("main"),
                owner: key("owner"),
                expected_revision: before.revision,
                selection: f.old.clone(),
            },
            &f.nodes,
            &limits(),
        )
        .unwrap();
    let fence = f.namespace.fence(&f.a).unwrap();
    let before = f.namespace.checkpoint();
    assert_eq!(
        f.namespace.apply(
            key("same-adopt"),
            Command::Adopt {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 4,
                prepared: prepared.reference
            },
            &f.nodes,
            &limits()
        ),
        Err(Error::Busy)
    );
    assert_eq!(f.namespace.checkpoint(), before);
    drop(fence);
}
fn unrelated(target: &Object) -> Object {
    (0..1024)
        .map(|n| Object::from_bytes(format!("unrelated-{n}").as_bytes()))
        .find(|o| partition(&o.sha256).unwrap() != partition(&target.sha256).unwrap())
        .unwrap()
}
#[test]
fn unrelated_partition_admission_proceeds_and_same_partition_contention_is_honest() {
    let mut f = Fixture::new(true);
    f.enroll("other", Kind::Draft);
    let other = unrelated(&f.a);
    let selection = tree(&mut f.nodes, &[("other", other)]);
    let fence = f.namespace.fence(&f.a).unwrap();
    f.namespace
        .apply(
            key("other-prepare"),
            Command::Prepare {
                source: key("other"),
                owner: key("owner"),
                expected_revision: 1,
                selection,
            },
            &f.nodes,
            &limits(),
        )
        .unwrap();
    assert!(
        f.namespace
            .absence(&fence, &key("working"), &f.nodes, &limits())
            .unwrap()
            .is_some()
    );
    assert!(matches!(f.namespace.fence(&f.a), Err(Error::Busy)));
}
struct Mutating<'a> {
    inner: &'a MemoryNodes,
    path: PathBuf,
    witness: Checkpoint,
    selection: Selection,
    fired: Cell<bool>,
}
impl NodeStore for Mutating<'_> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        if !self.fired.replace(true) {
            let mut peer = Namespace::open(&self.path, &self.witness).unwrap();
            peer.apply(
                key("late-enroll"),
                Command::Enroll {
                    source: key("late"),
                    owner: key("owner"),
                    class: Kind::Job,
                },
                self.inner,
                &limits(),
            )
            .unwrap();
            peer.apply(
                key("late-prepare"),
                Command::Prepare {
                    source: key("late"),
                    owner: key("owner"),
                    expected_revision: 1,
                    selection: self.selection.clone(),
                },
                self.inner,
                &limits(),
            )
            .unwrap();
        }
        self.inner.get(o)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        panic!("assessment is read-only")
    }
}
#[test]
fn subject_predicate_remains_valid_while_unrelated_heads_advance_during_io() {
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Branch);
    f.selected("main", f.old.clone());
    let target = Object::from_bytes(b"unclaimed");
    let other = unrelated(&target);
    let selection = tree(&mut f.nodes, &[("other", other)]);
    let fence = f.namespace.fence(&target).unwrap();
    let witness = f.namespace.checkpoint();
    let path = f.root();
    let nodes = Mutating {
        inner: &f.nodes,
        path,
        witness: witness.clone(),
        selection,
        fired: Cell::new(false),
    };
    let fact = f
        .namespace
        .absence(&fence, &key("working"), &nodes, &limits())
        .unwrap()
        .unwrap();
    assert!(nodes.fired.get());
    assert_eq!(fact.checkpoint(), &witness);
    drop(fact);
    let current = SourceJournal::observe_all(&f.root(), &witness, 1024).unwrap();
    assert!(current.checkpoint.sequence() > witness.sequence());
}
struct Missing;
impl NodeStore for Missing {
    fn get(&self, _: &Object) -> TreeResult<Option<Vec<u8>>> {
        Ok(None)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        panic!("no writes")
    }
}
#[test]
fn missing_closure_and_budgets_refuse_before_mutation_or_absence() {
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Branch);
    let before = f.namespace.checkpoint();
    let command = Command::Prepare {
        source: key("main"),
        owner: key("owner"),
        expected_revision: 1,
        selection: f.old.clone(),
    };
    assert_eq!(
        f.namespace
            .apply(key("missing"), command.clone(), &Missing, &limits()),
        Err(Error::IncompleteClosure)
    );
    assert_eq!(f.namespace.checkpoint(), before);
    let mut tiny = limits();
    tiny.reads = 1;
    assert_eq!(
        f.namespace
            .apply(key("tiny"), command.clone(), &f.nodes, &tiny),
        Err(Error::Budget)
    );
    assert_eq!(f.namespace.checkpoint(), before);
    f.namespace
        .apply(key("good"), command, &f.nodes, &limits())
        .unwrap();
    let fence = f.namespace.fence(&f.a).unwrap();
    assert!(matches!(
        f.namespace
            .absence(&fence, &key("working"), &Missing, &limits()),
        Err(Error::IncompleteClosure)
    ));
    assert!(matches!(f.namespace.fence(&f.a), Err(Error::Busy)));
}
#[test]
fn foreign_fence_size_mismatch_and_unknown_place_cannot_mint_absence() {
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Branch);
    f.selected("main", f.old.clone());
    let other = Fixture::new(true);
    let fence = other.namespace.fence(&f.a).unwrap();
    assert!(matches!(
        f.namespace
            .absence(&fence, &key("working"), &f.nodes, &limits()),
        Err(Error::WrongAuthority)
    ));
    drop(fence);
    let mut wrong = f.a.clone();
    wrong.bytes += 1;
    let fence = f.namespace.fence(&wrong).unwrap();
    assert!(matches!(
        f.namespace
            .absence(&fence, &key("working"), &f.nodes, &limits()),
        Err(Error::ConflictingSize)
    ));
    assert!(matches!(
        f.namespace
            .absence(&fence, &key("unknown"), &f.nodes, &limits()),
        Err(Error::InvalidPolicy)
    ));
}
#[test]
fn exact_replay_does_not_need_old_nodes_and_changed_operation_refuses() {
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Branch);
    let command = Command::Prepare {
        source: key("main"),
        owner: key("owner"),
        expected_revision: 1,
        selection: f.old.clone(),
    };
    let one = f
        .namespace
        .apply(key("prepare"), command.clone(), &f.nodes, &limits())
        .unwrap();
    let before = f.namespace.checkpoint();
    let two = f
        .namespace
        .apply(key("prepare"), command.clone(), &Missing, &limits())
        .unwrap();
    assert!(two.replayed);
    assert_eq!(one.checkpoint, two.checkpoint);
    assert_eq!(before, f.namespace.checkpoint());
    let changed = Command::Prepare {
        source: key("main"),
        owner: key("owner"),
        expected_revision: 1,
        selection: f.new.clone(),
    };
    assert!(matches!(
        f.namespace
            .apply(key("prepare"), changed, &Missing, &limits()),
        Err(Error::Journal(super::super::Error::Conflict))
    ));
}
#[test]
fn source_change_during_compilation_refuses_stale_permit_without_overwriting_peer() {
    struct Racing<'a> {
        nodes: &'a MemoryNodes,
        root: PathBuf,
        minimum: Checkpoint,
        command: Command,
        fired: Cell<bool>,
    }
    impl NodeStore for Racing<'_> {
        fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
            if !self.fired.replace(true) {
                Namespace::open(&self.root, &self.minimum)
                    .unwrap()
                    .apply(key("peer"), self.command.clone(), self.nodes, &limits())
                    .unwrap();
            }
            self.nodes.get(object)
        }
        fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
            panic!("compilation is read-only")
        }
    }
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Branch);
    let command = |selection| Command::Prepare {
        source: key("main"),
        owner: key("owner"),
        expected_revision: 1,
        selection,
    };
    let nodes = Racing {
        nodes: &f.nodes,
        root: f.root(),
        minimum: f.namespace.checkpoint(),
        command: command(f.new.clone()),
        fired: Cell::new(false),
    };
    assert_eq!(
        f.namespace
            .apply(key("stale"), command(f.old.clone()), &nodes, &limits()),
        Err(Error::Journal(super::super::Error::Stale))
    );
    assert!(nodes.fired.get());
    let state = f.namespace.source(&key("main")).unwrap().unwrap();
    assert_eq!(state.pending.unwrap().selection, f.new);
    assert_eq!(state.revision, 2);
    drop(f.namespace.fence(&f.a).unwrap());
}
#[test]
fn duplicate_handles_do_not_keep_a_dropped_subject_fence_live() {
    let f = Fixture::new(true);
    let fence = f.namespace.fence(&f.a).unwrap();
    let inherited = fence._locks.0[0].try_clone().unwrap();
    drop(fence);
    let _again = f.namespace.fence(&f.a).unwrap();
    drop(inherited);
}
#[test]
fn protection_projection_cannot_reclassify_a_guarded_source() {
    use crate::protection::{self, Format, Gap, Journal, Location, Profile};
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Branch);
    f.selected("main", f.old.clone());
    let mut profile = Profile {
        authority: Digest::blob(b"profile-authority"),
        generation: 1,
        kinds: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
            .into_iter()
            .collect(),
        unmanaged_boundary: Some(Digest::blob(b"admitted-boundary")),
        journals: vec![Journal {
            identity: f.namespace.identity().clone(),
            sources: vec![protection::Source {
                key: key("main"),
                owner: key("owner"),
                kind: Kind::Job,
                format: Format::FlatBindings,
                places: policy(true).classes[&Kind::Branch].clone(),
            }],
        }],
        limits: limits(),
    };
    let locations = [Location {
        root: f.root(),
        minimum: f.namespace.checkpoint(),
    }];
    let bad = protection::capture(&profile, &locations, &f.nodes).unwrap();
    assert!(
        bad.summary()
            .gap_samples
            .iter()
            .any(|gap| matches!(gap, Gap::ConflictingKind { .. }))
    );
    assert!(!bad.summary().observed_scope_resolved);
    profile.journals[0].sources[0].kind = Kind::Branch;
    let good = protection::capture(&profile, &locations, &f.nodes).unwrap();
    assert!(good.summary().observed_scope_resolved);
}
#[test]
fn replaced_marker_and_missing_anchor_refuse_without_repair() {
    let mut f = Fixture::new(true);
    let marker = f.root().join("claims.json");
    let raw = fs::read(&marker).unwrap();
    let mut changed: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    changed["epoch"] = "changed".into();
    fs::write(
        &marker,
        git_asset_core::identity::canonical_json(&changed).unwrap(),
    )
    .unwrap();
    assert!(f.namespace.fence(&f.a).is_err());
    assert!(f.namespace.hold(&f.a).is_err());
    assert!(
        f.namespace
            .apply(
                key("none"),
                Command::Enroll {
                    source: key("main"),
                    owner: key("owner"),
                    class: Kind::Branch
                },
                &f.nodes,
                &limits()
            )
            .is_err()
    );
    fs::write(&marker, raw).unwrap();
    let anchor = f
        .root()
        .join("admission-fences")
        .join(format!("{:02x}", partition(&f.a.sha256).unwrap()));
    fs::remove_file(&anchor).unwrap();
    assert!(f.namespace.fence(&f.a).is_err());
    assert!(!anchor.exists());
    assert!(f.namespace.hold(&f.a).is_err());
    assert!(!anchor.exists());
}
#[test]
fn invalid_policy_never_initializes_and_epoch_identity_is_distinct() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("must-not-create");
    let mut bad = policy(true);
    bad.classes.remove(&Kind::Job);
    assert!(Namespace::create(&path, key("project"), key("epoch"), bad).is_err());
    assert!(!path.exists());
    let f = Fixture::new(true);
    let other = Fixture::new(true);
    assert_ne!(f.namespace.identity(), other.namespace.identity());
}

#[derive(Serialize, Deserialize)]
struct ChildInput {
    minimum: Checkpoint,
    command: Command,
    object: Object,
}
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn spawn(root: &Path, mode: &str) -> Child {
    Child(
        Process::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "source_claims::admission::tests::admission_child",
                "--nocapture",
            ])
            .env("GIT_ASSET_ADMISSION_CHILD_ROOT", root)
            .env("GIT_ASSET_ADMISSION_CHILD_MODE", mode)
            .spawn()
            .unwrap(),
    )
}
fn child_input(f: &Fixture) -> Command {
    let command = Command::Prepare {
        source: key("main"),
        owner: key("owner"),
        expected_revision: 1,
        selection: f.old.clone(),
    };
    fs::write(
        f.root().join("child-input.json"),
        serde_json::to_vec(&ChildInput {
            minimum: f.namespace.checkpoint(),
            command: command.clone(),
            object: f.a.clone(),
        })
        .unwrap(),
    )
    .unwrap();
    command
}
#[test]
fn admission_child() {
    let Some(root) = std::env::var_os("GIT_ASSET_ADMISSION_CHILD_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let input: ChildInput =
        serde_json::from_slice(&fs::read(root.join("child-input.json")).unwrap()).unwrap();
    let mut namespace = Namespace::open(&root, &input.minimum).unwrap();
    let mode = std::env::var("GIT_ASSET_ADMISSION_CHILD_MODE").unwrap();
    if mode == "hold" || mode == "shared" {
        let _fence = if mode == "hold" {
            Some(namespace.fence(&input.object).unwrap())
        } else {
            None
        };
        let _usage = if mode == "shared" {
            Some(namespace.hold(&input.object).unwrap())
        } else {
            None
        };
        fs::write(root.join("child-ready"), b"ready").unwrap();
        // Only the owned test parent terminates this helper. Deadline bounds a
        // lost parent's lifetime; expiry itself grants no mutation authority.
        let start = Instant::now();
        while start.elapsed().as_secs() < 20 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("test parent did not terminate helper");
    }
    let mut nodes = MemoryNodes::default();
    tree(
        &mut nodes,
        &[
            ("changed", Object::from_bytes(b"old")),
            ("unchanged", Object::from_bytes(b"common")),
        ],
    );
    if mode.starts_with("pair-") || mode.starts_with("history-") {
        let Command::Prepare {
            source,
            owner,
            expected_revision,
            selection,
        } = input.command
        else {
            panic!("prepare input required")
        };
        let select = Select {
            prepare: key("child-prepare"),
            adopt: key("child-adopt"),
            source,
            owner,
            expected_revision,
            selection,
        };
        let mut appends = 0;
        let hook = |stage| {
            if stage == Stage::BeforeAppend {
                appends += 1;
            }
            if (mode == "pair-between" && stage == Stage::BeforeAppend && appends == 2)
                || (mode == "history-between" && stage == Stage::BeforeAppend && appends == 3)
                || (mode.ends_with("-before") && stage == Stage::BeforeCommit)
                || (mode.ends_with("-after") && stage == Stage::AfterCommit)
            {
                std::process::exit(75);
            }
            Ok(())
        };
        if mode.starts_with("history-") {
            namespace
                .select_predecessor_hook(&select, key("child-withdraw"), &nodes, &limits(), hook)
                .unwrap();
        } else {
            namespace
                .select_hook(&select, &nodes, &limits(), hook)
                .unwrap();
        }
        panic!("pair child missed boundary");
    }
    let stop = match mode.as_str() {
        "before" => Stage::BeforeCommit,
        "after" => Stage::AfterCommit,
        _ => panic!("unknown child mode"),
    };
    namespace
        .apply_hook(
            key("child-prepare"),
            input.command,
            &nodes,
            &limits(),
            |stage| {
                if stage == stop {
                    std::process::exit(75);
                }
                Ok(())
            },
        )
        .unwrap();
    panic!("child did not reach injected boundary");
}
#[test]
fn process_exit_before_and_after_commit_recovers_exact_receipt_and_releases_locks() {
    for mode in ["before", "after"] {
        let mut f = Fixture::new(true);
        f.enroll("main", Kind::Branch);
        let command = child_input(&f);
        let minimum = f.namespace.checkpoint();
        let mut child = spawn(&f.root(), mode);
        assert_eq!(child.0.wait().unwrap().code(), Some(75));
        let mut reopened = Namespace::open(&f.root(), &minimum).unwrap();
        let state = reopened.source(&key("main")).unwrap().unwrap();
        assert_eq!(state.pending.is_some(), mode == "after");
        // OS process exit releases admission locks even without Rust Drop.
        drop(reopened.fence(&f.a).unwrap());
        let receipt = reopened
            .apply(key("child-prepare"), command.clone(), &f.nodes, &limits())
            .unwrap();
        assert_eq!(receipt.replayed, mode == "after");
        assert_eq!(reopened.source(&key("main")).unwrap().unwrap().revision, 2);
        let saved = reopened.checkpoint();
        drop(reopened);
        let mut restarted = Namespace::open(&f.root(), &saved).unwrap();
        let replay = restarted
            .apply(key("child-prepare"), command, &Missing, &limits())
            .unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.checkpoint, receipt.checkpoint);
        assert_eq!(restarted.checkpoint(), saved);
    }
}
#[test]
fn another_process_blocks_overlapping_admission_until_actual_exit() {
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Branch);
    let command = child_input(&f);
    let mut child = spawn(&f.root(), "hold");
    let start = Instant::now();
    while !f.root().join("child-ready").exists() {
        assert!(child.0.try_wait().unwrap().is_none(), "helper exited early");
        assert!(
            start.elapsed().as_secs() < 10,
            "helper failed to acquire fence"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let before = f.namespace.checkpoint();
    assert_eq!(
        f.namespace
            .apply(key("overlap"), command.clone(), &f.nodes, &limits()),
        Err(Error::Busy)
    );
    assert_eq!(f.namespace.checkpoint(), before);
    // Registry-only enrollment takes no object partition and still progresses.
    f.enroll("unrelated", Kind::Draft);
    assert!(child.0.try_wait().unwrap().is_none());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    f.namespace
        .apply(key("overlap"), command, &f.nodes, &limits())
        .unwrap();
    assert!(
        f.namespace
            .source(&key("main"))
            .unwrap()
            .unwrap()
            .pending
            .is_some()
    );
}

#[test]
fn shared_use_allows_adoption_blocks_fences_and_outlives_namespace_handle() {
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Job);
    let usage = f.namespace.hold(&f.a).unwrap();
    assert_eq!(usage.object(), &f.a);
    assert_eq!(usage.authority(), f.namespace.identity());
    let peer = Namespace::open(&f.root(), &f.namespace.checkpoint()).unwrap();
    let second = peer.hold(&f.a).unwrap();
    assert!(matches!(peer.fence(&f.a), Err(Error::Busy)));
    let alias = Object {
        sha256: f.a.sha256.clone(),
        bytes: f.a.bytes + 1,
    };
    assert!(matches!(peer.fence(&alias), Err(Error::Busy)));
    assert_ne!(
        partition(&f.a.sha256).unwrap(),
        partition(&f.b.sha256).unwrap()
    );
    drop(peer.fence(&f.b).unwrap());
    f.selected("main", f.old.clone());
    let state = f.namespace.source(&key("main")).unwrap().unwrap();
    let root = f.root();
    let checkpoint = f.namespace.checkpoint();
    drop(second);
    drop(peer);
    drop(f.namespace);
    let mut reopened = Namespace::open(&root, &checkpoint).unwrap();
    assert!(matches!(reopened.fence(&f.a), Err(Error::Busy)));
    drop(usage);
    let fence = reopened.fence(&f.a).unwrap();
    assert!(matches!(reopened.hold(&f.a), Err(Error::Busy)));
    drop(fence);
    assert_eq!(reopened.source(&key("main")).unwrap().unwrap(), state);
}

#[test]
fn shared_use_process_death_releases_only_lock_and_preserves_job_history() {
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Job);
    f.selected("main", f.old.clone());
    child_input(&f);
    let mut child = spawn(&f.root(), "shared");
    let start = Instant::now();
    while !f.root().join("child-ready").exists() {
        assert!(child.0.try_wait().unwrap().is_none());
        assert!(start.elapsed().as_secs() < 10);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(matches!(f.namespace.fence(&f.a), Err(Error::Busy)));
    f.selected("main", f.new.clone());
    assert!(child.0.try_wait().unwrap().is_none());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    drop(f.namespace.fence(&f.a).unwrap());
    assert!(!f.absent(&f.a.clone(), "backup"));
    let state = f.namespace.source(&key("main")).unwrap().unwrap();
    assert!(state.predecessors.iter().any(|p| p.selection == f.old));
    assert_eq!(state.current.unwrap().selection, f.new);
}

fn select(f: &mut Fixture, label: &str, selection: Selection) -> Select {
    Select {
        prepare: key(&format!("pair-prepare-{label}")),
        adopt: key(&format!("pair-adopt-{label}")),
        source: key("main"),
        owner: key("owner"),
        expected_revision: f.namespace.source(&key("main")).unwrap().unwrap().revision,
        selection,
    }
}
#[test]
fn atomic_selection_is_visible_as_old_or_fully_adopted_and_replays_existing_commands() {
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Branch);
    f.selected("main", f.old.clone());
    let old = f.namespace.source(&key("main")).unwrap().unwrap();
    let before = f.namespace.checkpoint();
    let selection = f.new.clone();
    let request = select(&mut f, "new", selection);
    assert!(f.namespace.selection_status(&request).unwrap().is_none());
    let root = f.root();
    let mut callbacks = 0;
    let receipt = f
        .namespace
        .select_hook(&request, &f.nodes, &limits(), |stage| {
            if stage == Stage::BeforeAppend || stage == Stage::BeforeCommit {
                let observed = SourceJournal::observe(&root, &before, &key("main")).unwrap();
                assert_eq!(observed.source.as_ref(), Some(&old));
                assert_eq!(observed.checkpoint, before);
                callbacks += 1;
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(callbacks, 3);
    let state = f.namespace.source(&key("main")).unwrap().unwrap();
    assert!(state.pending.is_none());
    assert_eq!(state.current.unwrap().selection, f.new);
    assert_eq!(state.predecessors[0].selection, f.old);
    assert_eq!(receipt.checkpoint.sequence(), before.sequence() + 2);
    let commands = request.requests(f.namespace.identity()).unwrap();
    for command in commands {
        let recovered =
            SourceJournal::acknowledge(&root, &before, command.operation, command.command).unwrap();
        assert!(recovered.replayed);
    }
    let saved = f.namespace.checkpoint();
    let replay = f.namespace.select(&request, &Missing, &limits()).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.checkpoint, receipt.checkpoint);
    assert_eq!(f.namespace.checkpoint(), saved);
}
#[test]
fn atomic_selection_respects_subject_fences_and_stale_source_revisions() {
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Branch);
    f.selected("main", f.old.clone());
    let selection = f.new.clone();
    let request = select(&mut f, "winner", selection);
    let mut loser = request.clone();
    loser.prepare = key("loser-prepare");
    loser.adopt = key("loser-adopt");
    let fence = f.namespace.fence(&f.b).unwrap();
    let before = f.namespace.checkpoint();
    assert_eq!(
        f.namespace.select(&request, &f.nodes, &limits()),
        Err(Error::Busy)
    );
    assert_eq!(f.namespace.checkpoint(), before);
    f.enroll("unrelated", Kind::Job);
    drop(fence);
    f.namespace.select(&request, &f.nodes, &limits()).unwrap();
    let saved = f.namespace.checkpoint();
    assert_eq!(
        f.namespace.select(&loser, &f.nodes, &limits()),
        Err(Error::Journal(super::super::Error::Stale))
    );
    assert_eq!(f.namespace.checkpoint(), saved);
    assert!(
        f.namespace
            .source(&key("main"))
            .unwrap()
            .unwrap()
            .pending
            .is_none()
    );
}
#[test]
fn partial_pair_or_conflicting_operation_refuses_without_finishing_it() {
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Branch);
    let selection = f.old.clone();
    let request = select(&mut f, "partial", selection);
    let first = request.requests(f.namespace.identity()).unwrap()[0].clone();
    f.namespace
        .apply(first.operation, first.command, &f.nodes, &limits())
        .unwrap();
    let saved = f.namespace.checkpoint();
    assert_eq!(
        f.namespace.select(&request, &f.nodes, &limits()),
        Err(Error::Journal(super::super::Error::NeedsRecovery))
    );
    assert_eq!(f.namespace.checkpoint(), saved);
    let mut changed = request.clone();
    changed.selection = f.new.clone();
    assert_eq!(
        f.namespace.selection_status(&changed),
        Err(Error::Journal(super::super::Error::Conflict))
    );
    assert!(
        f.namespace
            .source(&key("main"))
            .unwrap()
            .unwrap()
            .current
            .is_none()
    );
}
#[test]
fn atomic_selection_faults_before_commit_roll_back_both_and_after_commit_recover_both() {
    for stop in [Stage::BeforeAppend, Stage::BeforeCommit, Stage::AfterCommit] {
        let mut f = Fixture::new(true);
        f.enroll("main", Kind::Branch);
        let selection = f.old.clone();
        let request = select(&mut f, "fault", selection);
        let before = f.namespace.checkpoint();
        let mut appends = 0;
        assert!(
            f.namespace
                .select_hook(&request, &f.nodes, &limits(), |stage| {
                    if stage == Stage::BeforeAppend {
                        appends += 1;
                    }
                    if stage == stop && (stop != Stage::BeforeAppend || appends == 2) {
                        return Err(super::super::Error::Injected);
                    }
                    Ok(())
                })
                .is_err()
        );
        let mut reopened = Namespace::open(&f.root(), &before).unwrap();
        let state = reopened.source(&key("main")).unwrap().unwrap();
        assert!(state.pending.is_none());
        assert_eq!(state.current.is_some(), stop == Stage::AfterCommit);
        let receipt = reopened.select(&request, &f.nodes, &limits()).unwrap();
        assert_eq!(receipt.replayed, stop == Stage::AfterCommit);
        assert_eq!(reopened.checkpoint().sequence(), before.sequence() + 2);
    }
}
#[test]
fn actual_exit_between_or_around_atomic_commands_never_leaves_a_pending_half() {
    for mode in ["pair-between", "pair-before", "pair-after"] {
        let mut f = Fixture::new(true);
        f.enroll("main", Kind::Branch);
        child_input(&f);
        let before = f.namespace.checkpoint();
        let mut child = spawn(&f.root(), mode);
        assert_eq!(child.0.wait().unwrap().code(), Some(75));
        let mut reopened = Namespace::open(&f.root(), &before).unwrap();
        let state = reopened.source(&key("main")).unwrap().unwrap();
        assert!(state.pending.is_none());
        assert_eq!(state.current.is_some(), mode == "pair-after");
        drop(reopened.fence(&f.a).unwrap());
        let request = Select {
            prepare: key("child-prepare"),
            adopt: key("child-adopt"),
            source: key("main"),
            owner: key("owner"),
            expected_revision: 1,
            selection: f.old.clone(),
        };
        let receipt = reopened.select(&request, &f.nodes, &limits()).unwrap();
        assert_eq!(receipt.replayed, mode == "pair-after");
        assert_eq!(reopened.checkpoint().sequence(), before.sequence() + 2);
    }
}

#[test]
fn actual_exit_inside_atomic_history_never_exposes_a_current_selection() {
    for mode in ["history-between", "history-before", "history-after"] {
        let mut f = Fixture::new(true);
        f.enroll("main", Kind::Branch);
        child_input(&f);
        let before = f.namespace.checkpoint();
        let mut child = spawn(&f.root(), mode);
        assert_eq!(child.0.wait().unwrap().code(), Some(75));
        let mut reopened = Namespace::open(&f.root(), &before).unwrap();
        let state = reopened.source(&key("main")).unwrap().unwrap();
        assert!(state.current.is_none() && state.pending.is_none());
        assert_eq!(
            state.predecessors.len(),
            usize::from(mode == "history-after")
        );
        let request = Select {
            prepare: key("child-prepare"),
            adopt: key("child-adopt"),
            source: key("main"),
            owner: key("owner"),
            expected_revision: 1,
            selection: f.old.clone(),
        };
        let receipt = reopened
            .select_predecessor(&request, key("child-withdraw"), &f.nodes, &limits())
            .unwrap();
        assert_eq!(receipt.replayed, mode == "history-after");
        assert_eq!(reopened.checkpoint().sequence(), before.sequence() + 3);
    }
}
#[test]
fn atomic_pair_needs_two_remaining_slots_and_cannot_consume_the_last_single_slot() {
    use rusqlite::params;
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Branch);
    let minimum = f.namespace.checkpoint();
    let mut connection = rusqlite::Connection::open(f.root().join("claims.sqlite")).unwrap();
    let tx = connection.transaction().unwrap();
    let (mut previous, mut bytes): (String, i64) = tx
        .query_row("SELECT hash,bytes FROM meta WHERE id=1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    // Materialize an independently encoded valid history, then exercise the
    // actual public compiler/writer at its bounded edge.
    for n in 2..super::super::MAX_COMMANDS {
        let operation = key(&format!("fill-{n}"));
        let command = if n % 2 == 0 {
            Command::Prepare {
                source: key("main"),
                owner: key("owner"),
                expected_revision: n - 1,
                selection: f.old.clone(),
            }
        } else {
            Command::Abort {
                source: key("main"),
                owner: key("owner"),
                expected_revision: n - 1,
                prepared: super::super::Reference {
                    journal: f.namespace.identity().clone(),
                    operation: key(&format!("fill-{}", n - 1)),
                },
            }
        };
        let request = serde_json::json!({"operation":operation,"command":command});
        let body = git_asset_core::identity::canonical_json(&request).unwrap();
        let hash=Digest::blob(&git_asset_core::identity::canonical_json(&serde_json::json!({"schema":"tig.source-event.design-1","sequence":n,"previous":previous,"request":request})).unwrap());
        tx.execute(
            "INSERT INTO events VALUES(?1,?2,?3,?4,?5)",
            params![n as i64, operation.as_str(), body, previous, hash.as_str()],
        )
        .unwrap();
        bytes += body.len() as i64;
        previous = hash.as_str().to_owned();
    }
    tx.execute(
        "UPDATE meta SET sequence=?1,hash=?2,bytes=?3 WHERE id=1",
        params![(super::super::MAX_COMMANDS - 1) as i64, previous, bytes],
    )
    .unwrap();
    tx.commit().unwrap();
    drop(connection);
    f.namespace = Namespace::open(&f.root(), &minimum).unwrap();
    let saved = f.namespace.checkpoint();
    let selection = f.old.clone();
    let request = select(&mut f, "last-slot", selection);
    assert_eq!(
        f.namespace
            .select_predecessor(&request, key("last-withdraw"), &f.nodes, &limits()),
        Err(Error::Journal(super::super::Error::Budget))
    );
    assert_eq!(
        f.namespace.select(&request, &f.nodes, &limits()),
        Err(Error::Journal(super::super::Error::Budget))
    );
    assert_eq!(f.namespace.checkpoint(), saved);
    assert!(
        f.namespace
            .source(&key("main"))
            .unwrap()
            .unwrap()
            .pending
            .is_none()
    );
    let first = request.requests(f.namespace.identity()).unwrap()[0].clone();
    f.namespace
        .apply(first.operation, first.command, &f.nodes, &limits())
        .unwrap();
    assert_eq!(
        f.namespace.checkpoint().sequence(),
        super::super::MAX_COMMANDS
    );
    assert!(
        f.namespace
            .source(&key("main"))
            .unwrap()
            .unwrap()
            .pending
            .is_some()
    );
}

#[test]
fn atomic_predecessor_never_exposes_active_or_pending_survivor() {
    let mut f = Fixture::new(true);
    f.enroll("history", Kind::Branch);
    let minimum = f.namespace.checkpoint();
    let root = f.root();
    let request = Select {
        prepare: key("history-prepare"),
        adopt: key("history-adopt"),
        source: key("history"),
        owner: key("owner"),
        expected_revision: 1,
        selection: f.old.clone(),
    };
    let mut observations = 0;
    f.namespace
        .select_predecessor_hook(
            &request,
            key("history-withdraw"),
            &f.nodes,
            &limits(),
            |stage| {
                let observed = SourceJournal::observe(&root, &minimum, &key("history")).unwrap();
                let source = observed.source.unwrap();
                assert!(source.current.is_none() && source.pending.is_none());
                assert_eq!(
                    source.predecessors.len(),
                    usize::from(stage == Stage::AfterCommit)
                );
                observations += 1;
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(observations, 5); // three appends, before commit, after commit
    let state = f.namespace.source(&key("history")).unwrap().unwrap();
    assert_eq!(state.revision, 4);
    assert_eq!(state.predecessors[0].revision, 3);
    let assignments = f.namespace.requirements(&f.nodes, &limits()).unwrap();
    assert!(
        assignments
            .entries
            .iter()
            .all(|entry| entry.places == vec![key("backup"), key("history")])
    );
}

#[test]
fn atomic_predecessor_rolls_back_every_internal_boundary_and_recovers_ack() {
    for fail_at in 1..=5 {
        let mut f = Fixture::new(true);
        f.enroll("history", Kind::Draft);
        let before = f.namespace.checkpoint();
        let request = Select {
            prepare: key("history-prepare"),
            adopt: key("history-adopt"),
            source: key("history"),
            owner: key("owner"),
            expected_revision: 1,
            selection: f.old.clone(),
        };
        let mut calls = 0;
        assert!(
            f.namespace
                .select_predecessor_hook(
                    &request,
                    key("history-withdraw"),
                    &f.nodes,
                    &limits(),
                    |_| {
                        calls += 1;
                        if calls == fail_at {
                            Err(super::super::Error::Injected)
                        } else {
                            Ok(())
                        }
                    }
                )
                .is_err()
        );
        f.namespace = Namespace::open(&f.root(), &before).unwrap();
        let state = f.namespace.source(&key("history")).unwrap().unwrap();
        assert!(state.current.is_none() && state.pending.is_none());
        assert_eq!(state.predecessors.len(), usize::from(fail_at == 5));
        let receipt = f
            .namespace
            .select_predecessor(&request, key("history-withdraw"), &f.nodes, &limits())
            .unwrap();
        assert_eq!(receipt.replayed, fail_at == 5);
        assert_eq!(f.namespace.checkpoint().sequence(), before.sequence() + 3);
    }
}

#[test]
fn atomic_predecessor_rejects_duplicate_phase_ids_and_effective_read_guard() {
    let mut f = Fixture::new(true);
    f.enroll("history", Kind::Branch);
    let before = f.namespace.checkpoint();
    let request = Select {
        prepare: key("history-prepare"),
        adopt: key("history-adopt"),
        source: key("history"),
        owner: key("owner"),
        expected_revision: 1,
        selection: f.old.clone(),
    };
    for duplicate in [request.prepare.clone(), request.adopt.clone()] {
        assert_eq!(
            f.namespace
                .select_predecessor(&request, duplicate, &f.nodes, &limits()),
            Err(Error::Journal(super::super::Error::Conflict))
        );
    }
    let guard = f.namespace.fence(&f.a).unwrap();
    assert_eq!(
        f.namespace
            .select_predecessor(&request, key("history-withdraw"), &f.nodes, &limits()),
        Err(Error::Busy)
    );
    assert_eq!(f.namespace.checkpoint(), before);
    drop(guard);
    f.namespace
        .select_predecessor(&request, key("history-withdraw"), &f.nodes, &limits())
        .unwrap();
}
#[test]
#[ignore = "requires GIT_ASSET_BASELINE_SOURCE_CLI built from pinned pre-pair source"]
fn baseline_cli_replays_atomic_pairs_without_format_migration() {
    let executable = std::env::var_os("GIT_ASSET_BASELINE_SOURCE_CLI")
        .expect("explicit baseline artifact required");
    assert!(Path::new(&executable).is_absolute());
    let mut f = Fixture::new(true);
    f.enroll("main", Kind::Branch);
    f.selected("main", f.old.clone());
    let minimum = f.namespace.checkpoint();
    let selection = f.new.clone();
    let request = select(&mut f, "compatibility", selection);
    f.namespace.select(&request, &f.nodes, &limits()).unwrap();
    f.enroll("successor", Kind::Branch);
    f.selected("successor", f.new.clone());
    let conditions = [SourceCondition {
        source: key("successor"),
        revision: f
            .namespace
            .source(&key("successor"))
            .unwrap()
            .unwrap()
            .revision,
    }];
    let withdrawal = Command::Withdraw {
        source: key("main"),
        owner: key("owner"),
        expected_revision: f.namespace.source(&key("main")).unwrap().unwrap().revision,
    };
    f.namespace
        .apply_if_sources(
            key("conditional-compatibility"),
            withdrawal.clone(),
            &conditions,
            &f.nodes,
            &limits(),
        )
        .unwrap();
    let current = f.namespace.source(&key("main")).unwrap().unwrap();
    let profile = f.temp.path().join("baseline-profile.json");
    fs::write(&profile,serde_json::to_vec(&serde_json::json!({"schema":"tig.source-observation-profile.experimental-1","journal_root":f.root(),"minimum":minimum,"source":"main"})).unwrap()).unwrap();
    let report = f.temp.path().join("baseline-report.json");
    let output = Process::new(&executable)
        .arg("inspect")
        .arg(&profile)
        .arg(&report)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    let old_reader_source: super::super::Source =
        serde_json::from_value(value["source"].clone()).unwrap();
    assert_eq!(old_reader_source, current);
    assert_eq!(
        value["checkpoint"],
        serde_json::to_value(f.namespace.checkpoint()).unwrap()
    );
    let mut commands = request.requests(f.namespace.identity()).unwrap().to_vec();
    commands.push(super::super::Request {
        operation: key("conditional-compatibility"),
        command: withdrawal,
    });
    for (index, command) in commands.into_iter().enumerate() {
        let profile = f.temp.path().join(format!("baseline-ack-{index}.json"));
        let report = f
            .temp
            .path()
            .join(format!("baseline-ack-report-{index}.json"));
        fs::write(&profile,serde_json::to_vec(&serde_json::json!({"schema":"tig.source-acknowledgment-profile.experimental-1","journal_root":f.root(),"minimum":minimum,"operation":command.operation,"command":command.command})).unwrap()).unwrap();
        let output = Process::new(&executable)
            .arg("acknowledge")
            .arg(profile)
            .arg(&report)
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        assert_eq!(value["replayed"], true);
    }
}
