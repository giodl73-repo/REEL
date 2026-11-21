use super::*;
use crate::retention::release::{self, ReleaseError, tests::Fixture};
fn object(f: &Fixture) -> Object {
    f.grant.request().publication.object.clone()
}
fn start_work(f: &mut Fixture) -> Result<Admission> {
    let object = object(f);
    let work = ReleaseWork::new(f.plan.clone(), f.job.clone());
    start(
        &mut f.journal,
        &mut f.nodes,
        &f.ctx,
        &object,
        f.grant.claim_id(),
        |_| Ok(work),
    )
}
fn work(f: &mut Fixture) -> ReleaseWork {
    let object = object(f);
    current(&mut f.journal, &f.nodes, &f.ctx, &object)
        .unwrap()
        .unwrap()
}
fn execute(
    f: &mut Fixture,
    w: &ReleaseWork,
    attempt: u64,
) -> release::Result<release::ReleaseReceipt> {
    release::apply(
        &mut f.ledger,
        &f.store,
        w.job(),
        w.plan(),
        &mut f.journal,
        &f.nodes,
        &f.ctx,
        attempt,
    )
}
#[test]
fn release_reopens_discovers_exact_work_and_reconciles_unreported_effect() {
    let mut f = Fixture::new_removable();
    let a = start_work(&mut f).unwrap();
    let expected = work(&mut f);
    let before = f.nodes.checkpoint();
    f.reopen();
    let object = object(&f);
    assert_eq!(
        start(
            &mut f.journal,
            &mut f.nodes,
            &f.ctx,
            &object,
            f.grant.claim_id(),
            |_| panic!("must not rebuild or reserve")
        )
        .unwrap()
        .kind,
        AdmissionKind::Existing
    );
    assert_eq!(before.nodes(), f.nodes.checkpoint().nodes());
    let restored = work(&mut f);
    assert_eq!(restored, expected);
    // Simulate effect succeeded but acknowledgement/outcome was lost.
    let evidence = execute(&mut f, &restored, a.attempt).unwrap().evidence;
    f.reopen();
    let restored = work(&mut f);
    assert_eq!(
        execute(&mut f, &restored, a.attempt).unwrap().evidence,
        evidence
    );
    f.finish(a.attempt, evidence);
    f.reopen();
    assert!(
        current(&mut f.journal, &f.nodes, &f.ctx, &object)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        start(
            &mut f.journal,
            &mut f.nodes,
            &f.ctx,
            &object,
            f.grant.claim_id(),
            |_| panic!("complete must not rebuild")
        )
        .unwrap()
        .kind,
        AdmissionKind::Complete
    );
    assert!(f.store.read_object(&object).is_ok());
}

#[test]
fn prepared_released_outcome_resumes_only_retirement_and_preserves_payload() {
    let mut f = Fixture::new_removable();
    let a = start_work(&mut f).unwrap();
    let w = work(&mut f);
    let r = execute(&mut f, &w, a.attempt).unwrap();
    let object = object(&f);
    let loaded = load(&mut f.journal, &f.nodes, &f.ctx, &object).unwrap();
    let previous = loaded.frames.last().unwrap();
    let frame = build(
        &f.ctx,
        &object,
        Some(previous),
        Action::Release {
            claim: f.grant.claim_id().clone(),
        },
        Some(Outcome::Released {
            evidence: r.evidence.clone(),
        }),
        previous.1.execution.clone(),
        previous.1.budget.clone(),
    )
    .unwrap();
    assert!(
        publish_frame_hook(
            &mut f.journal,
            &mut f.nodes,
            &f.ctx,
            &object,
            &loaded,
            frame,
            |_| Err(Error::Pending)
        )
        .is_err()
    );
    f.reopen();
    let before = f.journal.checkpoint();
    assert!(matches!(
        at_attempt(&mut f.journal, &f.nodes, &f.ctx, &object, a.attempt),
        Err(Error::Pending)
    ));
    assert!(matches!(
        resume_released_outcome(
            &mut f.journal,
            &mut f.nodes,
            &f.ctx,
            &object,
            a.attempt,
            f.grant.claim_id(),
            ClaimProtocol::Handoff
        ),
        Err(Error::Occupied)
    ));
    assert_eq!(before, f.journal.checkpoint());
    assert!(
        resume_released_outcome(
            &mut f.journal,
            &mut f.nodes,
            &f.ctx,
            &object,
            a.attempt,
            f.grant.claim_id(),
            ClaimProtocol::Retirement
        )
        .unwrap()
    );
    let (restored, outcome) = at_attempt(&mut f.journal, &f.nodes, &f.ctx, &object, a.attempt)
        .unwrap()
        .unwrap();
    assert_eq!(restored, w);
    assert_eq!(
        outcome,
        Some(Outcome::Released {
            evidence: r.evidence
        })
    );
    assert!(
        handoff_work::at_attempt(&mut f.journal, &f.nodes, &f.ctx, &object, a.attempt)
            .unwrap()
            .is_none()
    );
    let complete = f.journal.checkpoint();
    assert!(
        !resume_released_outcome(
            &mut f.journal,
            &mut f.nodes,
            &f.ctx,
            &object,
            a.attempt,
            f.grant.claim_id(),
            ClaimProtocol::Retirement
        )
        .unwrap()
    );
    assert_eq!(complete, f.journal.checkpoint());
    assert!(f.store.read_object(&object).is_ok());
}
#[test]
fn changed_capacity_job_refuses_before_ledger_mutation() {
    let mut f = Fixture::new_removable();
    let a = start_work(&mut f).unwrap();
    let w = work(&mut f);
    let probe = f
        .ledger
        .begin_probe(Key::parse("extra-probe").unwrap())
        .unwrap();
    f.ledger
        .observe(
            Key::parse("extra-sample").unwrap(),
            probe.reference(),
            1024 * 1024 * 1024,
        )
        .unwrap();
    let other = f
        .ledger
        .reserve(
            Key::parse("other-job").unwrap(),
            f.ctx.owner().clone(),
            w.plan.required_peak(),
        )
        .unwrap()
        .reference()
        .clone();
    let before = f.ledger.checkpoint();
    assert!(matches!(
        execute(&mut f, &ReleaseWork::new(w.plan.clone(), other), a.attempt),
        Err(ReleaseError::Progress(Error::Occupied))
    ));
    assert_eq!(f.ledger.checkpoint(), before);
    assert!(crate::retention::verify(&mut f.ledger, &f.store, &f.grant).is_ok());
}
#[test]
fn pending_work_refuses_effect_and_resumes_without_factory() {
    let mut f = Fixture::new_removable();
    let object = object(&f);
    let w = ReleaseWork::new(f.plan.clone(), f.job.clone());
    assert!(matches!(
        start_hook(
            &mut f.journal,
            &mut f.nodes,
            &f.ctx,
            &object,
            f.grant.claim_id(),
            |_| Ok(w.clone()),
            |_| Err(Error::Invalid)
        ),
        Err(Error::Invalid)
    ));
    assert!(matches!(
        current(&mut f.journal, &f.nodes, &f.ctx, &object),
        Err(Error::Pending)
    ));
    assert!(matches!(
        execute(&mut f, &w, 1),
        Err(ReleaseError::NotAdmitted)
    ));
    f.reopen();
    let a = start(
        &mut f.journal,
        &mut f.nodes,
        &f.ctx,
        &object,
        f.grant.claim_id(),
        |_| panic!("pending work must not reserve twice"),
    )
    .unwrap();
    let w = work(&mut f);
    let r = execute(&mut f, &w, a.attempt).unwrap();
    f.finish(a.attempt, r.evidence);
}
#[test]
fn legacy_unbound_release_cannot_silently_attach_a_plan() {
    let mut f = Fixture::new_removable();
    f.start();
    let object = object(&f);
    assert!(matches!(
        start(
            &mut f.journal,
            &mut f.nodes,
            &f.ctx,
            &object,
            f.grant.claim_id(),
            |_| panic!("must refuse before factory")
        ),
        Err(Error::Occupied)
    ));
    assert!(
        current(&mut f.journal, &f.nodes, &f.ctx, &object)
            .unwrap()
            .is_none()
    );
    f.run(1).unwrap(); // Original external-plan protocol remains valid.
}
#[test]
fn invalid_scope_never_enters_the_journal_and_missing_work_never_downgrades() {
    let mut f = Fixture::new_removable();
    let object = object(&f);
    let before = f.journal.checkpoint();
    let mut value = serde_json::to_value(&f.plan).unwrap();
    value["operation"] = serde_json::to_value(Digest::blob(b"different-operation")).unwrap();
    let bad = serde_json::from_value(value).unwrap();
    assert!(
        start(
            &mut f.journal,
            &mut f.nodes,
            &f.ctx,
            &object,
            f.grant.claim_id(),
            |_| Ok(ReleaseWork::new(bad, f.job.clone()))
        )
        .is_err()
    );
    assert_eq!(before, f.journal.checkpoint());
    start_work(&mut f).unwrap();
    let loaded = load(&mut f.journal, &f.nodes, &f.ctx, &object).unwrap();
    let root = loaded.frames.last().unwrap().1.execution.clone().unwrap();
    struct Hidden<'a>(&'a MetadataStore, Object);
    impl NodeStore for Hidden<'_> {
        fn get(&self, o: &Object) -> tig_core::tree::TreeResult<Option<Vec<u8>>> {
            if *o == self.1 {
                Ok(None)
            } else {
                self.0.get(o)
            }
        }
        fn put(&mut self, _: &[u8]) -> tig_core::tree::TreeResult<Object> {
            panic!("read only")
        }
    }
    let hidden = Hidden(&f.nodes, root);
    assert!(current(&mut f.journal, &hidden, &f.ctx, &object).is_err());
    assert!(
        release::apply(
            &mut f.ledger,
            &f.store,
            &f.job,
            &f.plan,
            &mut f.journal,
            &hidden,
            &f.ctx,
            1
        )
        .is_err()
    );
}

#[test]
fn cancellation_blocks_admission_but_admitted_release_remains_discoverable() {
    let mut f = Fixture::new_removable();
    let object = object(&f);
    let w = ReleaseWork::new(f.plan.clone(), f.job.clone());
    let result = start_hook(
        &mut f.journal,
        &mut f.nodes,
        &f.ctx,
        &object,
        f.grant.claim_id(),
        |_| Ok(w.clone()),
        |j| {
            crate::retirement_intent::cancel(
                j,
                &Key::parse("owner").unwrap(),
                &Digest::blob(b"retire"),
                2,
                Key::parse("cancel-before").unwrap(),
            )?;
            Ok(())
        },
    );
    assert_eq!(result, Err(Error::Cancelled));
    assert!(execute(&mut f, &w, 1).is_err());
    assert!(crate::retention::verify(&mut f.ledger, &f.store, &f.grant).is_ok());
    let mut f = Fixture::new_removable();
    let a = start_work(&mut f).unwrap();
    crate::retirement_intent::cancel(
        &mut f.journal,
        &Key::parse("owner").unwrap(),
        &Digest::blob(b"retire"),
        2,
        Key::parse("cancel-after").unwrap(),
    )
    .unwrap();
    f.reopen();
    let w = work(&mut f);
    let r = execute(&mut f, &w, a.attempt).unwrap();
    f.finish(a.attempt, r.evidence);
    assert!(f.store.read_object(&self::object(&f)).is_ok());
}

#[derive(Serialize, Deserialize)]
struct Controls {
    journal: crate::source_claims::Checkpoint,
    nodes: crate::metadata::Checkpoint,
    ledger: crate::ledger::Checkpoint,
}
#[test]
fn release_work_child() {
    let Some(root) = std::env::var_os("TIG_BOUND_RELEASE_TEST_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let c: Controls =
        serde_json::from_slice(&std::fs::read(root.join("release-controls.json")).unwrap())
            .unwrap();
    let store = crate::local::LocalStore::open(&root.join("store")).unwrap();
    let mut ledger = crate::ledger::CapacityLedger::open(&root.join("ledger"), &c.ledger).unwrap();
    let mut j = SourceJournal::open(&root.join("journal"), &c.journal).unwrap();
    let nodes = MetadataStore::open(&root.join("nodes"), &c.nodes).unwrap();
    let ctx = crate::retirement_intent::observe(
        &mut j,
        &nodes,
        &crate::retirement_intent::Enrollment {
            owner: Key::parse("owner").unwrap(),
            subject: Digest::blob(b"subject"),
        },
        &Digest::blob(b"retire"),
    )
    .unwrap();
    let object = ctx.review().objects()[0].object().clone();
    let (attempt, _) = inspect(&mut j, &nodes, &ctx, &object)
        .unwrap()
        .in_flight
        .unwrap();
    let w = current(&mut j, &nodes, &ctx, &object).unwrap().unwrap();
    release::apply(
        &mut ledger,
        &store,
        w.job(),
        w.plan(),
        &mut j,
        &nodes,
        &ctx,
        attempt,
    )
    .unwrap();
    // No plan/grant/job was passed to this process. Exit without reporting outcome.
    std::process::exit(77);
}
#[test]
fn child_effect_without_caller_plan_or_job_resumes_once_after_lost_outcome() {
    let mut f = Fixture::new_removable();
    let a = start_work(&mut f).unwrap();
    std::fs::write(
        f.temp.path().join("release-controls.json"),
        serde_json::to_vec(&Controls {
            journal: f.journal.checkpoint(),
            nodes: f.nodes.checkpoint(),
            ledger: f.ledger.checkpoint(),
        })
        .unwrap(),
    )
    .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "retirement_intent::progress::release_work::tests::release_work_child",
            "--nocapture",
        ])
        .env("TIG_BOUND_RELEASE_TEST_ROOT", f.temp.path())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(77));
    f.reopen();
    let w = work(&mut f);
    let evidence = execute(&mut f, &w, a.attempt).unwrap().evidence;
    f.finish(a.attempt, evidence);
    assert!(f.store.read_object(&object(&f)).is_ok());
    assert!(crate::retention::verify(&mut f.ledger, &f.store, &f.grant).is_err());
}
