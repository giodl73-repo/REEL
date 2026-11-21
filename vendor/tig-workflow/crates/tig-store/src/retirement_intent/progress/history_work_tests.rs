use super::*;
use crate::retirement_intent::Error;
use tig_core::{
    LogicalPath,
    retirement::{
        review::{decision::*, *},
        *,
    },
};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn d(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn path(s: &str) -> LogicalPath {
    LogicalPath::parse(s).unwrap()
}
fn object(id: &str) -> Object {
    let sample: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../examples/teaching/catalogue.json"
    ))
    .unwrap();
    let a = sample["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)
        .unwrap();
    Object::from_bytes(a["content"].as_str().unwrap().as_bytes())
}
fn selected(j: &mut SourceJournal, selection: Selection, revision: u64) {
    let p = j
        .apply(
            key(&format!("source-prepare-{revision}")),
            Command::Prepare {
                source: key("main"),
                owner: key("source-owner"),
                expected_revision: revision,
                selection,
            },
        )
        .unwrap();
    j.apply(
        key(&format!("source-adopt-{revision}")),
        Command::Adopt {
            source: key("main"),
            owner: key("source-owner"),
            expected_revision: revision + 1,
            prepared: p.reference,
        },
    )
    .unwrap();
}
fn enrollment() -> Enrollment {
    Enrollment {
        owner: key("controller"),
        subject: d("subject"),
    }
}
fn source_enrollment() -> Enrollment {
    Enrollment {
        owner: key("source-owner"),
        subject: d("subject"),
    }
}
pub(crate) struct Fixture {
    pub(crate) temp: tempfile::TempDir,
    pub(crate) j: SourceJournal,
    pub(crate) n: MetadataStore,
    pub(crate) ctx: ObservedIntent,
    pub(crate) claim: Digest,
    pub(crate) old: BTreeMap<LogicalPath, Object>,
}
impl Fixture {
    pub(crate) fn new() -> Self {
        Self::with_only_old(false)
    }
    fn with_only_old(only: bool) -> Self {
        Self::configured(only, 1)
    }
    pub(crate) fn new_block() -> Self {
        Self::configured(false, 2)
    }
    pub(crate) fn new_members(count: usize) -> Self {
        Self::configured(false, count)
    }
    fn configured(only: bool, count: usize) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut j =
            SourceJournal::create(&temp.path().join("journal"), key("runtime"), key("epoch"))
                .unwrap();
        let mut n = MetadataStore::create(
            &temp.path().join("nodes"),
            crate::metadata::Limits {
                nodes: 2000,
                bytes: 4 * 1024 * 1024,
                database_bytes: 8 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let mut old = BTreeMap::from([
            (path("cues/old.tig"), object("old")),
            (path("aliases/old.tig"), object("old")),
        ]);
        if !only {
            old.insert(path("cels/shared.tig"), object("shared"));
            old.insert(
                path("documents/invoice.tig"),
                Object::from_bytes(b"public synthetic invoice"),
            );
        }
        for i in 1..count {
            old.insert(
                path(&format!("documents/retired-{i}.tig")),
                Object::from_bytes(format!("public synthetic retired report {i}").as_bytes()),
            );
        }
        let root = Tree::build(&old, &mut n, TreeLimits::default())
            .unwrap()
            .0
            .root()
            .clone();
        n.finish().unwrap();
        selected(
            &mut j,
            Selection {
                root,
                context: d("initial"),
            },
            0,
        );
        let current = Tree::build(
            &BTreeMap::from([
                (path("cues/fixed.tig"), object("fixed")),
                (path("cels/shared.tig"), object("shared")),
            ]),
            &mut n,
            TreeLimits::default(),
        )
        .unwrap()
        .0;
        n.finish().unwrap();
        selected(
            &mut j,
            Selection {
                root: current.root().clone(),
                context: d("fixed"),
            },
            2,
        );
        let state = j.source(&key("main")).unwrap().unwrap();
        let claim = claim_id(
            j.identity(),
            &key("main"),
            &state.owner,
            &state.predecessors[0],
            &object("old"),
        )
        .unwrap();
        let mut selected = BTreeMap::from([(path("cues/old.tig"), object("old"))]);
        for i in 1..count {
            selected.insert(
                path(&format!("documents/retired-{i}.tig")),
                Object::from_bytes(format!("public synthetic retired report {i}").as_bytes()),
            );
        }
        let releases = selected
            .values()
            .map(|o| Release {
                claim: claim_id(
                    j.identity(),
                    &key("main"),
                    &state.owner,
                    &state.predecessors[0],
                    o,
                )
                .unwrap(),
                object: o.clone(),
            })
            .collect::<BTreeSet<_>>();
        let preview = Preview::create(
            &Catalogue {
                snapshot: d("catalogue"),
                bindings: selected
                    .iter()
                    .map(|(p, o)| {
                        (
                            p.clone(),
                            Binding {
                                object: o.clone(),
                                activity: Activity::Inactive(InactiveInterval {
                                    clock: d("clock"),
                                    identity: d("interval"),
                                    started: 1,
                                    through: 100,
                                }),
                            },
                        )
                    })
                    .collect(),
            },
            selected.keys().cloned().collect(),
            Policy {
                identity: d("policy"),
                minimum_inactive_ticks: 10,
                places: ["D".into()].into(),
            },
            ClockPoint {
                era: d("clock"),
                tick: 100,
            },
        )
        .unwrap();
        let review = Review::create(
            preview,
            d("subject"),
            [("D".into(), d("D"))].into(),
            Protection {
                checkpoint: d("protection"),
                complete: true,
                obligations: releases
                    .iter()
                    .map(|r| Obligation {
                        claim: r.claim.clone(),
                        object: r.object.clone(),
                        owner: d("subject"),
                        kind: Kind::Ordinary,
                    })
                    .collect(),
            },
            Inventory {
                checkpoint: d("inventory"),
                complete: true,
                stores: [d("D")].into(),
                copies: selected
                    .values()
                    .map(|o| Copy {
                        object: o.clone(),
                        store: d("D"),
                        state: CopyState::Verified { evidence: d("D") },
                    })
                    .collect(),
            },
            releases.clone(),
        )
        .unwrap();
        let decision = Decision::authorize(
            &review,
            Approval {
                namespace: j.identity().clone(),
                operation: d("retire"),
                owner: d("subject"),
                evidence: d("explicit"),
            },
            Consent {
                release_owned_claims: releases,
                remove_copies: selected
                    .values()
                    .map(|o| Target {
                        object: o.clone(),
                        store: d("D"),
                    })
                    .collect(),
                accept_intentional_loss: selected.values().cloned().collect(),
            },
        )
        .unwrap();
        let candidate =
            crate::retirement_intent::stage(&mut j, &mut n, &enrollment(), &review, &decision)
                .unwrap();
        let ready = crate::retirement_intent::finish(&mut j, &mut n, candidate).unwrap();
        let prepared =
            crate::retirement_intent::prepare(&mut j, &n, ready, key("intent-prepare")).unwrap();
        crate::retirement_intent::adopt(&mut j, &n, &prepared, key("intent-adopt")).unwrap();
        let ctx =
            crate::retirement_intent::observe(&mut j, &n, &enrollment(), &d("retire")).unwrap();
        Self {
            temp,
            j,
            n,
            ctx,
            claim,
            old,
        }
    }
    fn start(&mut self) -> Result<Admission> {
        let state = self.j.source(&key("main")).unwrap().unwrap();
        let journal = self.j.identity().clone();
        start(
            &mut self.j,
            &mut self.n,
            &self.ctx,
            &object("old"),
            &self.claim,
            |nodes, _| {
                plan(
                    &journal,
                    &key("main"),
                    &state,
                    2,
                    &object("old"),
                    &source_enrollment(),
                    nodes,
                )
            },
        )
    }
    fn run(&mut self, attempt: u64) -> Result<Digest> {
        apply(&mut self.j, &self.n, &self.ctx, &object("old"), attempt)
    }
    fn report(&mut self, attempt: u64, evidence: Digest) {
        finish(
            &mut self.j,
            &mut self.n,
            &self.ctx,
            &object("old"),
            attempt,
            Action::Release {
                claim: self.claim.clone(),
            },
            Outcome::Released { evidence },
        )
        .unwrap();
    }
    pub(crate) fn reopen(&mut self) {
        self.j =
            SourceJournal::open(&self.temp.path().join("journal"), &self.j.checkpoint()).unwrap();
        self.n =
            MetadataStore::open(&self.temp.path().join("nodes"), &self.n.checkpoint()).unwrap();
    }
    pub(crate) fn retained(&mut self) -> BTreeMap<LogicalPath, Object> {
        let s = self.j.source(&key("main")).unwrap().unwrap();
        assert_eq!(s.predecessors.len(), 1);
        Tree::from_root(s.predecessors[0].selection.root.clone())
            .entries(&mut self.n, TreeLimits::default())
            .unwrap()
            .0
    }
}
#[test]
fn projection_preserves_unapproved_shared_and_history_only_nonmedia_bindings() {
    let mut f = Fixture::new();
    let a = f.start().unwrap();
    f.reopen();
    assert_eq!(
        start(
            &mut f.j,
            &mut f.n,
            &f.ctx,
            &object("old"),
            &f.claim,
            |_, _| panic!("no caller-held plan or rebuild")
        )
        .unwrap()
        .kind,
        AdmissionKind::Existing
    );
    let evidence = f.run(a.attempt).unwrap();
    f.reopen();
    assert_eq!(evidence, f.run(a.attempt).unwrap());
    let expected = f
        .old
        .iter()
        .filter(|(_, o)| **o != object("old"))
        .map(|(p, o)| (p.clone(), o.clone()))
        .collect();
    assert_eq!(f.retained(), expected);
    assert!(f.retained().contains_key(&path("documents/invoice.tig")));
    f.report(a.attempt, evidence);
    assert!(
        current(&mut f.j, &f.n, &f.ctx, &object("old"))
            .unwrap()
            .is_none()
    );
    assert!(f.run(a.attempt).is_err());
    assert!(!f.j.obligations(32).unwrap().complete_claim_coverage());
}
#[test]
fn whole_predecessor_is_removed_only_when_every_binding_is_for_approved_object() {
    let mut f = Fixture::with_only_old(true);
    let a = f.start().unwrap();
    assert!(
        current(&mut f.j, &f.n, &f.ctx, &object("old"))
            .unwrap()
            .unwrap()
            .replacement()
            .is_none()
    );
    let e = f.run(a.attempt).unwrap();
    f.report(a.attempt, e);
    assert!(
        f.j.source(&key("main"))
            .unwrap()
            .unwrap()
            .predecessors
            .is_empty()
    );
}
#[test]
fn no_effect_progress_advance_after_validation_is_atomically_fenced() {
    let mut f = Fixture::new();
    let a = f.start().unwrap();
    let mut n = MetadataStore::open(&f.temp.path().join("nodes"), &f.n.checkpoint()).unwrap();
    let before = f.j.source(&key("main")).unwrap().unwrap();
    assert_eq!(
        apply_hook(&mut f.j, &f.n, &f.ctx, &object("old"), a.attempt, |j| {
            finish(
                j,
                &mut n,
                &f.ctx,
                &object("old"),
                a.attempt,
                Action::Release {
                    claim: f.claim.clone(),
                },
                Outcome::NoEffect {
                    evidence: d("undispatched-before-command"),
                },
            )?;
            Ok(())
        }),
        Err(Error::Journal(source_claims::Error::Stale))
    );
    assert_eq!(before, f.j.source(&key("main")).unwrap().unwrap());
    f.reopen();
    let b = f.start().unwrap();
    assert!(f.run(a.attempt).is_err());
    f.run(b.attempt).unwrap();
}

#[test]
fn pending_and_cancelled_work_do_not_gain_new_authority() {
    let mut f = Fixture::new();
    let state = f.j.source(&key("main")).unwrap().unwrap();
    let journal = f.j.identity().clone();
    assert!(
        start_hook(
            &mut f.j,
            &mut f.n,
            &f.ctx,
            &object("old"),
            &f.claim,
            |nodes, _| plan(
                &journal,
                &key("main"),
                &state,
                2,
                &object("old"),
                &source_enrollment(),
                nodes
            ),
            |_| Err(Error::Invalid)
        )
        .is_err()
    );
    assert!(f.run(1).is_err());
    f.reopen();
    let a = start(
        &mut f.j,
        &mut f.n,
        &f.ctx,
        &object("old"),
        &f.claim,
        |_, _| panic!("prepared work must not rebuild"),
    )
    .unwrap();
    crate::retirement_intent::cancel(
        &mut f.j,
        &enrollment().owner,
        &d("retire"),
        2,
        key("cancel"),
    )
    .unwrap();
    f.reopen();
    let e = f.run(a.attempt).unwrap();
    f.report(a.attempt, e);
    assert!(!f.retained().is_empty());
    let mut f = Fixture::new();
    let state = f.j.source(&key("main")).unwrap().unwrap();
    let journal = f.j.identity().clone();
    assert_eq!(
        start_hook(
            &mut f.j,
            &mut f.n,
            &f.ctx,
            &object("old"),
            &f.claim,
            |nodes, _| plan(
                &journal,
                &key("main"),
                &state,
                2,
                &object("old"),
                &source_enrollment(),
                nodes
            ),
            |j| {
                crate::retirement_intent::cancel(
                    j,
                    &enrollment().owner,
                    &d("retire"),
                    2,
                    key("cancel-before"),
                )?;
                Ok(())
            }
        ),
        Err(Error::Cancelled)
    );
    assert!(f.run(1).is_err());
    assert_eq!(f.j.source(&key("main")).unwrap().unwrap(), state);
}
#[test]
fn an_incorrect_projection_cannot_drop_unapproved_history_only_assets() {
    let mut f = Fixture::new();
    let state = f.j.source(&key("main")).unwrap().unwrap();
    let mut w = plan(
        f.j.identity(),
        &key("main"),
        &state,
        2,
        &object("old"),
        &source_enrollment(),
        &mut f.n,
    )
    .unwrap();
    let incomplete = Tree::build(
        &BTreeMap::from([(path("cels/shared.tig"), object("shared"))]),
        &mut f.n,
        TreeLimits::default(),
    )
    .unwrap()
    .0;
    w.replacement.as_mut().unwrap().root = incomplete.root().clone();
    let a = start(
        &mut f.j,
        &mut f.n,
        &f.ctx,
        &object("old"),
        &f.claim,
        |_, _| Ok(w),
    )
    .unwrap();
    let before = f.j.checkpoint();
    assert_eq!(f.run(a.attempt), Err(Error::Integrity));
    assert_eq!(before, f.j.checkpoint());
    assert_eq!(f.retained(), f.old);
}
#[test]
fn a_source_advance_after_validation_refuses_the_exact_command() {
    let mut f = Fixture::new();
    let a = f.start().unwrap();
    let source = f.j.source(&key("main")).unwrap().unwrap();
    let selection = source.current.unwrap().selection;
    assert_eq!(
        apply_hook(&mut f.j, &f.n, &f.ctx, &object("old"), a.attempt, |j| {
            j.apply(
                key("racing-prepare"),
                Command::Prepare {
                    source: key("main"),
                    owner: key("source-owner"),
                    expected_revision: 4,
                    selection,
                },
            )
            .map_err(Error::Journal)?;
            Ok(())
        }),
        Err(Error::Journal(source_claims::Error::Stale))
    );
    assert_eq!(f.retained(), f.old);
}
#[test]
fn foreign_journal_owner_and_missing_projection_metadata_refuse() {
    let mut f = Fixture::new();
    let state = f.j.source(&key("main")).unwrap().unwrap();
    assert!(
        plan(
            f.j.identity(),
            &key("main"),
            &state,
            2,
            &object("old"),
            &enrollment(),
            &mut f.n
        )
        .is_err()
    );
    let foreign = d("foreign-journal");
    let w = plan(
        &foreign,
        &key("main"),
        &state,
        2,
        &object("old"),
        &source_enrollment(),
        &mut f.n,
    )
    .unwrap();
    assert!(
        start(
            &mut f.j,
            &mut f.n,
            &f.ctx,
            &object("old"),
            &f.claim,
            |_, _| Ok(w)
        )
        .is_err()
    );
    let a = f.start().unwrap();
    let root = current(&mut f.j, &f.n, &f.ctx, &object("old"))
        .unwrap()
        .unwrap()
        .replacement
        .unwrap()
        .root;
    struct Hidden<'a>(&'a MetadataStore, Object);
    impl NodeStore for Hidden<'_> {
        fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
            if *o == self.1 {
                Ok(None)
            } else {
                self.0.get(o)
            }
        }
        fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
            panic!("read only")
        }
    }
    let before = f.j.checkpoint();
    assert!(
        apply(
            &mut f.j,
            &Hidden(&f.n, root),
            &f.ctx,
            &object("old"),
            a.attempt
        )
        .is_err()
    );
    assert_eq!(before, f.j.checkpoint());
}
#[test]
fn shared_io_budget_counts_failed_reads_and_never_resets_between_traversals() {
    let mut nodes = tig_core::tree::MemoryNodes::default();
    let budget = BudgetNodes::new(&mut nodes);
    budget.reads.set(MAX_READS - 1);
    budget.read_bytes.set(MAX_READ_BYTES - 1);
    let missing = Object::from_bytes(b"x");
    assert_eq!(budget.get(&missing).unwrap(), None);
    assert_eq!(budget.reads.get(), MAX_READS);
    assert_eq!(budget.read_bytes.get(), MAX_READ_BYTES);
    assert_eq!(budget.get(&missing), Err(TreeError::Budget));
}
#[derive(Serialize, Deserialize)]
struct Controls {
    journal: source_claims::Checkpoint,
    nodes: crate::metadata::Checkpoint,
}
#[test]
fn history_work_child() {
    let Some(root) = std::env::var_os("TIG_HISTORY_WORK_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let controls: Controls =
        serde_json::from_slice(&std::fs::read(root.join("history-controls.json")).unwrap())
            .unwrap();
    let mut j = SourceJournal::open(&root.join("journal"), &controls.journal).unwrap();
    let nodes = MetadataStore::open(&root.join("nodes"), &controls.nodes).unwrap();
    let ctx =
        crate::retirement_intent::observe(&mut j, &nodes, &enrollment(), &d("retire")).unwrap();
    let (attempt, _) = inspect(&mut j, &nodes, &ctx, &object("old"))
        .unwrap()
        .in_flight
        .unwrap();
    apply(&mut j, &nodes, &ctx, &object("old"), attempt).unwrap();
    std::process::exit(79);
}
#[test]
fn child_discovers_exact_history_projection_and_exits_before_outcome() {
    let mut f = Fixture::new();
    let a = f.start().unwrap();
    std::fs::write(
        f.temp.path().join("history-controls.json"),
        serde_json::to_vec(&Controls {
            journal: f.j.checkpoint(),
            nodes: f.n.checkpoint(),
        })
        .unwrap(),
    )
    .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "retirement_intent::progress::history_work::tests::history_work_child",
            "--nocapture",
        ])
        .env("TIG_HISTORY_WORK_ROOT", f.temp.path())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(79));
    f.reopen();
    let evidence = f.run(a.attempt).unwrap();
    f.report(a.attempt, evidence);
    assert!(f.retained().contains_key(&path("documents/invoice.tig")));
    assert!(!f.retained().values().any(|o| *o == object("old")));
}
