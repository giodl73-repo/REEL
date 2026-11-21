use super::*;
use crate::{
    Request,
    retention::{self, RetentionGrant, RetentionRequest, release::tests::Fixture},
};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn object(f: &Fixture) -> Object {
    f.grant.request().publication.object.clone()
}
struct FixturePair {
    f: Fixture,
    successor: LocalStore,
    grant: RetentionGrant,
}
impl FixturePair {
    fn new() -> Self {
        let mut f = Fixture::new_removable();
        let mut successor = LocalStore::create(&f.temp.path().join("successor"), key("M")).unwrap();
        let publication = Request {
            operation: key("backup"),
            owner: key("owner"),
            object: object(&f),
            required: crate::AckLevel::ProcessRestart,
        };
        crate::publish(
            &mut successor,
            &publication,
            &mut f.store.read_object(&publication.object).unwrap(),
        )
        .unwrap();
        let plan = retention::plan(
            &successor,
            &RetentionRequest {
                operation: key("backup-hold"),
                owner: key("owner"),
                publication,
                minimum_ack: crate::AckLevel::ProcessRestart,
            },
        )
        .unwrap();
        let grant = retention::retain(&mut f.ledger, &successor, &f.job, &plan)
            .unwrap()
            .grant;
        Self {
            f,
            successor,
            grant,
        }
    }
    fn prepare(&self, attempt: u64) -> HandoffWork {
        let p = handoff::plan(
            &self.f.store,
            &handoff::HandoffRequest {
                operation: operation(&self.f.ctx, &object(&self.f), attempt).unwrap(),
                owner: key("owner"),
                previous: self.f.grant.clone(),
                successor: self.grant.clone(),
                source_domain: key("working"),
                successor_domain: key("backup"),
            },
        )
        .unwrap();
        let auth = p.authorize_handoff().unwrap();
        HandoffWork::new(p, auth, self.f.job.clone())
    }
    fn start(&mut self) -> Result<Admission> {
        let w = self.prepare(1);
        let object = object(&self.f);
        start(
            &mut self.f.journal,
            &mut self.f.nodes,
            &self.f.ctx,
            &object,
            self.f.grant.claim_id(),
            |_| Ok(w),
        )
    }
    fn work(&mut self) -> HandoffWork {
        let object = object(&self.f);
        current(&mut self.f.journal, &self.f.nodes, &self.f.ctx, &object)
            .unwrap()
            .unwrap()
    }
    fn run(&mut self, attempt: u64) -> handoff::Result<HandoffReceipt> {
        let mut peer = CapacityLedger::open(
            &self.f.temp.path().join("ledger"),
            &self.f.ledger.checkpoint(),
        )
        .unwrap();
        let object = object(&self.f);
        apply(
            Execution {
                source: &self.f.store,
                source_ledger: &mut self.f.ledger,
                successor: &self.successor,
                successor_ledger: &mut peer,
                journal: &mut self.f.journal,
                nodes: &self.f.nodes,
                intent: &self.f.ctx,
            },
            &object,
            attempt,
        )
    }
    fn reopen(&mut self) {
        self.f.reopen();
        self.successor = LocalStore::open(&self.f.temp.path().join("successor")).unwrap();
    }
}
#[test]
fn admitted_handoff_reopens_discovers_and_reconciles_effect_without_plan_handles() {
    let mut f = FixturePair::new();
    let a = f.start().unwrap();
    let expected = f.work();
    f.reopen();
    let object = object(&f.f);
    assert_eq!(
        start(
            &mut f.f.journal,
            &mut f.f.nodes,
            &f.f.ctx,
            &object,
            f.f.grant.claim_id(),
            |_| panic!("existing must not plan or reserve")
        )
        .unwrap()
        .kind,
        AdmissionKind::Existing
    );
    assert_eq!(expected, f.work());
    assert!(
        release_work::current(&mut f.f.journal, &f.f.nodes, &f.f.ctx, &object)
            .unwrap()
            .is_none()
    );
    assert!(
        local_work::current(&mut f.f.journal, &f.f.nodes, &f.f.ctx, &object)
            .unwrap()
            .is_none()
    );
    let receipt = f.run(a.attempt).unwrap();
    f.reopen();
    assert_eq!(f.run(a.attempt).unwrap().marker, receipt.marker);
    f.f.finish(a.attempt, receipt.marker);
    assert!(f.run(a.attempt).is_err());
    assert!(f.f.store.read_object(&object).is_ok());
    assert!(retention::verify(&mut f.f.ledger, &f.successor, &f.grant).is_ok());
    assert!(retention::verify(&mut f.f.ledger, &f.f.store, &f.f.grant).is_err());
}
#[test]
fn pending_work_and_wrong_attempt_refuse_before_native_mutation() {
    let mut f = FixturePair::new();
    let w = f.prepare(1);
    let object = object(&f.f);
    assert!(
        start_hook(
            &mut f.f.journal,
            &mut f.f.nodes,
            &f.f.ctx,
            &object,
            f.f.grant.claim_id(),
            |_| Ok(w),
            |_| Err(Error::Invalid)
        )
        .is_err()
    );
    let before = f.f.ledger.checkpoint();
    assert!(f.run(1).is_err());
    assert_eq!(f.f.ledger.checkpoint(), before);
    f.reopen();
    let a = start(
        &mut f.f.journal,
        &mut f.f.nodes,
        &f.f.ctx,
        &object,
        f.f.grant.claim_id(),
        |_| panic!("pending must not rebuild"),
    )
    .unwrap();
    let before = f.f.ledger.checkpoint();
    assert!(f.run(a.attempt + 1).is_err());
    assert_eq!(f.f.ledger.checkpoint(), before);
    f.run(a.attempt).unwrap();
}

#[test]
fn prepared_released_outcome_resumes_only_handoff_without_renewing_execution() {
    let mut f = FixturePair::new();
    let a = f.start().unwrap();
    let w = f.work();
    let r = f.run(a.attempt).unwrap();
    let object = object(&f.f);
    let loaded = load(&mut f.f.journal, &f.f.nodes, &f.f.ctx, &object).unwrap();
    let previous = loaded.frames.last().unwrap();
    let frame = build(
        &f.f.ctx,
        &object,
        Some(previous),
        Action::Release {
            claim: f.f.grant.claim_id().clone(),
        },
        Some(Outcome::Released {
            evidence: r.marker.clone(),
        }),
        previous.1.execution.clone(),
        previous.1.budget.clone(),
    )
    .unwrap();
    assert!(
        publish_frame_hook(
            &mut f.f.journal,
            &mut f.f.nodes,
            &f.f.ctx,
            &object,
            &loaded,
            frame,
            |_| Err(Error::Pending)
        )
        .is_err()
    );
    f.reopen();
    let before = f.f.journal.checkpoint();
    assert!(matches!(
        resume_released_outcome(
            &mut f.f.journal,
            &mut f.f.nodes,
            &f.f.ctx,
            &object,
            a.attempt,
            f.f.grant.claim_id(),
            ClaimProtocol::Retirement
        ),
        Err(Error::Occupied)
    ));
    assert_eq!(before, f.f.journal.checkpoint());
    assert!(
        resume_released_outcome(
            &mut f.f.journal,
            &mut f.f.nodes,
            &f.f.ctx,
            &object,
            a.attempt,
            f.f.grant.claim_id(),
            ClaimProtocol::Handoff
        )
        .unwrap()
    );
    let (restored, outcome) =
        at_attempt(&mut f.f.journal, &f.f.nodes, &f.f.ctx, &object, a.attempt)
            .unwrap()
            .unwrap();
    assert_eq!(restored, w);
    assert_eq!(outcome, Some(Outcome::Released { evidence: r.marker }));
    assert!(
        release_work::at_attempt(&mut f.f.journal, &f.f.nodes, &f.f.ctx, &object, a.attempt)
            .unwrap()
            .is_none()
    );
    assert!(f.run(a.attempt).is_err());
    assert!(f.f.store.read_object(&object).is_ok());
}
#[test]
fn native_guard_rechecks_progress_under_the_source_fence() {
    let mut f = FixturePair::new();
    let a = f.start().unwrap();
    let object = object(&f.f);
    let mut peer =
        CapacityLedger::open(&f.f.temp.path().join("ledger"), &f.f.ledger.checkpoint()).unwrap();
    let mut nodes = crate::metadata::MetadataStore::open(
        &f.f.temp.path().join("nodes"),
        &f.f.nodes.checkpoint(),
    )
    .unwrap();
    let mut checks = 0;
    assert!(matches!(
        apply_hook(
            Execution {
                source: &f.f.store,
                source_ledger: &mut f.f.ledger,
                successor: &f.successor,
                successor_ledger: &mut peer,
                journal: &mut f.f.journal,
                nodes: &f.f.nodes,
                intent: &f.f.ctx
            },
            &object,
            a.attempt,
            |j| {
                checks += 1;
                if checks == 2 {
                    // No marker/effect has run; the native source fence is held now.
                    finish(
                        j,
                        &mut nodes,
                        &f.f.ctx,
                        &object,
                        a.attempt,
                        Action::Release {
                            claim: f.f.grant.claim_id().clone(),
                        },
                        Outcome::NoEffect {
                            evidence: Digest::blob(b"fenced-before-marker"),
                        },
                    )?;
                }
                Ok(())
            }
        ),
        Err(HandoffError::Progress(Error::Stale))
    ));
    assert_eq!(checks, 2);
    assert!(retention::verify(&mut f.f.ledger, &f.f.store, &f.f.grant).is_ok());
    f.reopen();
    let next = f.prepare(a.attempt + 1);
    let previous_op = f.prepare(a.attempt).plan.request().operation.clone();
    assert_ne!(next.plan.request().operation, previous_op);
    let b = start(
        &mut f.f.journal,
        &mut f.f.nodes,
        &f.f.ctx,
        &object,
        f.f.grant.claim_id(),
        |_| Ok(next),
    )
    .unwrap();
    assert!(f.run(a.attempt).is_err());
    f.run(b.attempt).unwrap();
}
#[test]
fn final_release_and_handoff_cannot_rebind_each_others_pending_or_admitted_work() {
    for pending in [false, true] {
        let mut f = FixturePair::new();
        let object = object(&f.f);
        let w = release_work::ReleaseWork::new(f.f.plan.clone(), f.f.job.clone());
        if pending {
            // Prepare legacy native final-release work without adopting it.
            let bytes = encode(&serde_json::json!({
                "schema": release_work::SCHEMA, "attempt": 1,
                "plan": f.f.plan, "job": f.f.job,
            }))
            .unwrap();
            start_bound(
                &mut f.f.journal,
                &mut f.f.nodes,
                &f.f.ctx,
                &object,
                Action::Release {
                    claim: f.f.grant.claim_id().clone(),
                },
                release_work::SCHEMA,
                |n, _| put(n, &bytes),
                |_| Err(Error::Invalid),
            )
            .unwrap_err();
        } else {
            release_work::start(
                &mut f.f.journal,
                &mut f.f.nodes,
                &f.f.ctx,
                &object,
                f.f.grant.claim_id(),
                |_| Ok(w),
            )
            .unwrap();
        }
        let before = f.f.journal.checkpoint();
        assert!(matches!(
            start(
                &mut f.f.journal,
                &mut f.f.nodes,
                &f.f.ctx,
                &object,
                f.f.grant.claim_id(),
                |_| panic!("must refuse before factory")
            ),
            Err(Error::Occupied)
        ));
        assert_eq!(before, f.f.journal.checkpoint());
        let mut f = FixturePair::new();
        let object = self::object(&f.f);
        let w = f.prepare(1);
        if pending {
            start_hook(
                &mut f.f.journal,
                &mut f.f.nodes,
                &f.f.ctx,
                &object,
                f.f.grant.claim_id(),
                |_| Ok(w),
                |_| Err(Error::Invalid),
            )
            .unwrap_err();
        } else {
            f.start().unwrap();
        }
        let before = f.f.journal.checkpoint();
        assert!(matches!(
            release_work::start(
                &mut f.f.journal,
                &mut f.f.nodes,
                &f.f.ctx,
                &object,
                f.f.grant.claim_id(),
                |_| panic!("wrong protocol must refuse")
            ),
            Err(Error::Occupied)
        ));
        assert_eq!(before, f.f.journal.checkpoint());
    }
}

#[test]
fn cancellation_before_adoption_refuses_and_admitted_handoff_reconciles_after_cancel() {
    let mut f = FixturePair::new();
    let object = object(&f.f);
    let w = f.prepare(1);
    assert_eq!(
        start_hook(
            &mut f.f.journal,
            &mut f.f.nodes,
            &f.f.ctx,
            &object,
            f.f.grant.claim_id(),
            |_| Ok(w),
            |j| {
                crate::retirement_intent::cancel(
                    j,
                    &key("owner"),
                    &Digest::blob(b"retire"),
                    2,
                    key("cancel-before"),
                )?;
                Ok(())
            }
        ),
        Err(Error::Cancelled)
    );
    assert!(f.run(1).is_err());
    assert!(retention::verify(&mut f.f.ledger, &f.f.store, &f.f.grant).is_ok());
    let mut f = FixturePair::new();
    let a = f.start().unwrap();
    crate::retirement_intent::cancel(
        &mut f.f.journal,
        &key("owner"),
        &Digest::blob(b"retire"),
        2,
        key("cancel-after"),
    )
    .unwrap();
    f.reopen();
    let r = f.run(a.attempt).unwrap();
    f.f.finish(a.attempt, r.marker);
    assert!(f.f.store.read_object(&object).is_ok());
}
#[test]
fn offline_successor_keeps_source_protected_and_handoff_in_flight_until_exact_resume() {
    let mut f = FixturePair::new();
    let a = f.start().unwrap();
    let root = f.f.temp.path().join("successor");
    let offline = f.f.temp.path().join("successor.offline");
    std::fs::rename(&root, &offline).unwrap();
    assert!(f.run(a.attempt).is_err());
    let object = object(&f.f);
    let state = inspect(&mut f.f.journal, &f.f.nodes, &f.f.ctx, &object).unwrap();
    assert!(state.in_flight.is_some());
    assert!(state.released.is_empty());
    assert!(retention::verify(&mut f.f.ledger, &f.f.store, &f.f.grant).is_ok());
    std::fs::rename(&offline, &root).unwrap();
    f.reopen();
    let r = f.run(a.attempt).unwrap();
    f.f.finish(a.attempt, r.marker);
}
#[test]
fn invalid_attempt_authorization_and_missing_work_never_downgrade_to_unbound() {
    let mut f = FixturePair::new();
    let object = object(&f.f);
    for wrong_auth in [false, true] {
        let mut w = f.prepare(2);
        if wrong_auth {
            w.plan = f.prepare(1).plan;
        }
        let before = f.f.journal.checkpoint();
        assert!(
            start(
                &mut f.f.journal,
                &mut f.f.nodes,
                &f.f.ctx,
                &object,
                f.f.grant.claim_id(),
                |_| Ok(w)
            )
            .is_err()
        );
        assert_eq!(before, f.f.journal.checkpoint());
    }
    let a = f.start().unwrap();
    let loaded = load(&mut f.f.journal, &f.f.nodes, &f.f.ctx, &object).unwrap();
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
    let hidden = Hidden(&f.f.nodes, root);
    let before = f.f.ledger.checkpoint();
    let mut peer = CapacityLedger::open(&f.f.temp.path().join("ledger"), &before).unwrap();
    assert!(
        apply(
            Execution {
                source: &f.f.store,
                source_ledger: &mut f.f.ledger,
                successor: &f.successor,
                successor_ledger: &mut peer,
                journal: &mut f.f.journal,
                nodes: &hidden,
                intent: &f.f.ctx
            },
            &object,
            a.attempt
        )
        .is_err()
    );
    assert_eq!(before, f.f.ledger.checkpoint());
}
#[derive(Serialize, Deserialize)]
struct Controls {
    journal: crate::source_claims::Checkpoint,
    nodes: crate::metadata::Checkpoint,
    ledger: crate::ledger::Checkpoint,
}
#[test]
fn handoff_work_child() {
    let Some(root) = std::env::var_os("TIG_BOUND_HANDOFF_TEST_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let c: Controls =
        serde_json::from_slice(&std::fs::read(root.join("handoff-controls.json")).unwrap())
            .unwrap();
    let source = LocalStore::open(&root.join("store")).unwrap();
    let successor = LocalStore::open(&root.join("successor")).unwrap();
    let mut ledger = CapacityLedger::open(&root.join("ledger"), &c.ledger).unwrap();
    let mut peer = CapacityLedger::open(&root.join("ledger"), &c.ledger).unwrap();
    let mut j = SourceJournal::open(&root.join("journal"), &c.journal).unwrap();
    let nodes = MetadataStore::open(&root.join("nodes"), &c.nodes).unwrap();
    let ctx = crate::retirement_intent::observe(
        &mut j,
        &nodes,
        &crate::retirement_intent::Enrollment {
            owner: key("owner"),
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
    apply(
        Execution {
            source: &source,
            source_ledger: &mut ledger,
            successor: &successor,
            successor_ledger: &mut peer,
            journal: &mut j,
            nodes: &nodes,
            intent: &ctx,
        },
        &object,
        attempt,
    )
    .unwrap();
    std::process::exit(78); // No outcome; caller passed no plan/grant/auth/job.
}
#[test]
fn child_discovers_handoff_and_exits_after_effect_before_outcome_without_plan_inputs() {
    let mut f = FixturePair::new();
    let a = f.start().unwrap();
    std::fs::write(
        f.f.temp.path().join("handoff-controls.json"),
        serde_json::to_vec(&Controls {
            journal: f.f.journal.checkpoint(),
            nodes: f.f.nodes.checkpoint(),
            ledger: f.f.ledger.checkpoint(),
        })
        .unwrap(),
    )
    .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "retirement_intent::progress::handoff_work::tests::handoff_work_child",
            "--nocapture",
        ])
        .env("TIG_BOUND_HANDOFF_TEST_ROOT", f.f.temp.path())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(78));
    f.reopen();
    let r = f.run(a.attempt).unwrap();
    f.f.finish(a.attempt, r.marker);
    assert!(retention::verify(&mut f.f.ledger, &f.successor, &f.grant).is_ok());
    assert!(f.f.store.read_object(&object(&f.f)).is_ok());
}
