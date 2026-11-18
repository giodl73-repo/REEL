use super::*;
use crate::{
    local::{
        LocalStore,
        removal::{self, tests::Finite},
    },
    metadata::{Checkpoint as MetadataCheckpoint, MetadataStore},
    retention::release::tests::Fixture,
    source_claims::Checkpoint,
};
use std::{cell::Cell, fs};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn d(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn fixture() -> Fixture {
    let mut f = Fixture::new_removable();
    let n = f.start();
    let r = f.run(n).unwrap();
    f.finish(n, r.evidence);
    f
}
fn object(f: &Fixture) -> Object {
    f.grant.request().publication.object.clone()
}
fn start_work(f: &mut Fixture) -> Result<Admission> {
    let object = object(f);
    let store = f.store.identity().clone();
    let ctx = &f.ctx;
    let native = &f.store;
    let job = f.job.clone();
    start(
        &mut f.journal,
        &mut f.nodes,
        ctx,
        &object,
        &store,
        |attempt| {
            Ok(LocalWork::new(
                removal::plan(native, ctx, &object, attempt).map_err(|_| Error::Invalid)?,
                job,
            ))
        },
    )
}
fn work(f: &mut Fixture) -> LocalWork {
    let object = object(f);
    current(&mut f.journal, &f.nodes, &f.ctx, &object)
        .unwrap()
        .unwrap()
}
fn execute(f: &mut Fixture, w: &LocalWork) -> removal::Result<removal::RemovalReceipt> {
    removal::apply(
        removal::Execution {
            store: &f.store,
            ledger: &mut f.ledger,
            job: w.job(),
            journal: &mut f.journal,
            nodes: &f.nodes,
            intent: &f.ctx,
            protection: &mut Finite::default(),
        },
        w.plan(),
    )
}
fn report(f: &mut Fixture, w: &LocalWork, r: &removal::RemovalReceipt) {
    let object = object(f);
    finish(
        &mut f.journal,
        &mut f.nodes,
        &f.ctx,
        &object,
        w.plan.scope().attempt,
        Action::Remove {
            store: w.plan.scope().store.clone(),
        },
        Outcome::Removed {
            evidence: r.evidence.clone(),
            allocation: Allocation {
                before: Some(r.file_data_before),
                reclaimed: None,
            },
        },
    )
    .unwrap();
}
#[test]
fn acknowledged_work_reopens_without_plan_roots_or_repeating_factory() {
    let mut f = fixture();
    let a = start_work(&mut f).unwrap();
    assert_eq!(a.kind, AdmissionKind::New);
    let expected = work(&mut f);
    assert_eq!(
        at_attempt(
            &mut f.journal,
            &f.nodes,
            &f.ctx,
            &expected.plan().scope().object,
            expected.plan().scope().attempt
        )
        .unwrap(),
        Some(expected.clone())
    );
    let cp = f.nodes.checkpoint();
    f.reopen();
    let object = object(&f);
    let store = f.store.identity().clone();
    let replay = start(
        &mut f.journal,
        &mut f.nodes,
        &f.ctx,
        &object,
        &store,
        |_| panic!("existing work must not reserve or rebuild"),
    )
    .unwrap();
    assert_eq!(replay.kind, AdmissionKind::Existing);
    assert_eq!(cp.nodes(), f.nodes.checkpoint().nodes());
    let restored = work(&mut f);
    assert_eq!(restored, expected);
    let r = execute(&mut f, &restored).unwrap();
    report(&mut f, &restored, &r);
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
            &store,
            |_| panic!("completed work must not rebuild")
        )
        .unwrap()
        .kind,
        AdmissionKind::Complete
    );
    assert_eq!(execute(&mut f, &restored).unwrap().evidence, r.evidence);
    assert_eq!(
        at_attempt(
            &mut f.journal,
            &f.nodes,
            &f.ctx,
            &object,
            restored.plan().scope().attempt
        )
        .unwrap(),
        Some(restored)
    );
    assert!(
        at_attempt(&mut f.journal, &f.nodes, &f.ctx, &object, 999)
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        at_attempt(&mut f.journal, &f.nodes, &f.ctx, &object, 0),
        Err(Error::Invalid)
    ));
}

#[test]
fn prepared_removed_outcome_after_effect_recovers_only_its_exact_adopted_attempt() {
    let mut f = fixture();
    start_work(&mut f).unwrap();
    let w = work(&mut f);
    let r = execute(&mut f, &w).unwrap();
    let object = object(&f);
    let loaded = load(&mut f.journal, &f.nodes, &f.ctx, &object).unwrap();
    let previous = loaded.frames.last().unwrap();
    let frame = build(
        &f.ctx,
        &object,
        Some(previous),
        Action::Remove {
            store: w.plan().scope().store.clone(),
        },
        Some(Outcome::Removed {
            evidence: r.evidence.clone(),
            allocation: Allocation {
                before: Some(r.file_data_before),
                reclaimed: None,
            },
        }),
        previous.1.execution.clone(),
        previous.1.budget.clone(),
    )
    .unwrap();
    assert!(matches!(
        publish_frame_hook(
            &mut f.journal,
            &mut f.nodes,
            &f.ctx,
            &object,
            &loaded,
            frame,
            |_| Err(Error::Pending)
        ),
        Err(Error::Pending)
    ));
    f.reopen();
    assert!(f.store.read_object(&object).is_err());
    assert!(
        inspect(&mut f.journal, &f.nodes, &f.ctx, &object)
            .unwrap()
            .pending
    );
    assert!(matches!(
        at_attempt(
            &mut f.journal,
            &f.nodes,
            &f.ctx,
            &object,
            w.plan().scope().attempt
        ),
        Err(Error::Pending)
    ));
    println!(
        "Prepared-outcome counterexample: actual native disposition exists after unlink; strict saved-attempt lookup refuses pending progress, so the original CLI cannot finish its retry."
    );
    let before = f.journal.checkpoint();
    for (attempt, store, preservation) in [
        (
            w.plan().scope().attempt + 1,
            w.plan().scope().store.clone(),
            None,
        ),
        (w.plan().scope().attempt, d("foreign-store"), None),
        (
            w.plan().scope().attempt,
            w.plan().scope().store.clone(),
            Some(d("foreign-mode")),
        ),
    ] {
        assert!(
            resume_removed_outcome(
                &mut f.journal,
                &mut f.nodes,
                &f.ctx,
                &object,
                attempt,
                &store,
                preservation.as_ref()
            )
            .is_err()
        );
        assert_eq!(f.journal.checkpoint(), before);
    }
    assert!(
        resume_removed_outcome(
            &mut f.journal,
            &mut f.nodes,
            &f.ctx,
            &object,
            w.plan().scope().attempt,
            &w.plan().scope().store,
            None
        )
        .unwrap()
    );
    assert!(
        !inspect(&mut f.journal, &f.nodes, &f.ctx, &object)
            .unwrap()
            .pending
    );
    assert_eq!(
        at_attempt(
            &mut f.journal,
            &f.nodes,
            &f.ctx,
            &object,
            w.plan().scope().attempt
        )
        .unwrap(),
        Some(w.clone())
    );
    let complete = f.journal.checkpoint();
    assert!(
        !resume_removed_outcome(
            &mut f.journal,
            &mut f.nodes,
            &f.ctx,
            &object,
            w.plan().scope().attempt,
            &w.plan().scope().store,
            None
        )
        .unwrap()
    );
    assert_eq!(f.journal.checkpoint(), complete);
    let replay = execute(&mut f, &w).unwrap();
    assert_eq!(replay.evidence, r.evidence);
    report(&mut f, &w, &replay);
    assert_eq!(f.journal.checkpoint(), complete);
    println!(
        "Exact prepared Removed outcome adopted; original provider disposition verifies on replay; repeated recovery adds no events."
    );
}
#[test]
fn changed_plan_or_job_is_refused_before_resource_mutation() {
    let mut f = fixture();
    start_work(&mut f).unwrap();
    let w = work(&mut f);
    let ticket = f.ledger.begin_probe(key("extra-probe")).unwrap();
    f.ledger
        .observe(key("extra-sample"), ticket.reference(), 1024 * 1024 * 1024)
        .unwrap();
    let job = f
        .ledger
        .reserve(key("other-job"), key("owner"), w.plan.required_peak())
        .unwrap()
        .reference()
        .clone();
    let before = f.ledger.checkpoint();
    assert!(matches!(
        execute(&mut f, &LocalWork::new(w.plan.clone(), job)),
        Err(removal::RemovalError::Progress(Error::Occupied))
    ));
    let mut value = serde_json::to_value(&w.plan).unwrap();
    value["scope"]["incarnation"] = serde_json::to_value(d("another-incarnation")).unwrap();
    let changed: Plan = serde_json::from_value(value).unwrap();
    assert!(matches!(
        execute(&mut f, &LocalWork::new(changed, w.job.clone())),
        Err(removal::RemovalError::Progress(Error::Occupied))
    ));
    assert_eq!(before, f.ledger.checkpoint());
    assert!(f.store.read_object(&object(&f)).is_ok());
}
#[test]
fn prepared_work_is_not_executable_and_resumes_exactly_without_factory() {
    let mut f = fixture();
    let object = object(&f);
    let store = f.store.identity().clone();
    let native = &f.store;
    let ctx = &f.ctx;
    let job = f.job.clone();
    assert_eq!(
        start_hook(
            &mut f.journal,
            &mut f.nodes,
            ctx,
            &object,
            &store,
            |attempt| Ok(LocalWork::new(
                removal::plan(native, ctx, &object, attempt).unwrap(),
                job
            )),
            |_| Err(Error::Stale)
        ),
        Err(Error::Stale)
    );
    assert!(matches!(
        current(&mut f.journal, &f.nodes, &f.ctx, &object),
        Err(Error::Pending)
    ));
    assert!(matches!(
        at_attempt(&mut f.journal, &f.nodes, &f.ctx, &object, 1),
        Err(Error::Pending)
    ));
    assert!(matches!(
        resume_removed_outcome(
            &mut f.journal,
            &mut f.nodes,
            &f.ctx,
            &object,
            1,
            &store,
            None
        ),
        Err(Error::Pending)
    ));
    let cp = f.nodes.checkpoint();
    f.reopen();
    start(
        &mut f.journal,
        &mut f.nodes,
        &f.ctx,
        &object,
        &store,
        |_| panic!("must resume prepared work"),
    )
    .unwrap();
    assert_eq!(cp.nodes(), f.nodes.checkpoint().nodes());
    assert!(work(&mut f).plan.scope().attempt > 0);
}
#[test]
fn cancellation_excludes_new_admission_but_keeps_admitted_work_discoverable() {
    let mut f = fixture();
    let object = object(&f);
    let store = f.store.identity().clone();
    let native = &f.store;
    let ctx = &f.ctx;
    let job = f.job.clone();
    let result = start_hook(
        &mut f.journal,
        &mut f.nodes,
        ctx,
        &object,
        &store,
        |attempt| {
            Ok(LocalWork::new(
                removal::plan(native, ctx, &object, attempt).unwrap(),
                job,
            ))
        },
        |j| {
            crate::retirement_intent::cancel(
                j,
                &key("owner"),
                &d("retire"),
                2,
                key("cancel-before-adopt"),
            )?;
            Ok(())
        },
    );
    assert_eq!(result, Err(Error::Cancelled));
    assert!(f.store.read_object(&object).is_ok());
    assert!(matches!(
        current(&mut f.journal, &f.nodes, &f.ctx, &object),
        Err(Error::Pending)
    ));
    let mut f = fixture();
    start_work(&mut f).unwrap();
    let object = self::object(&f);
    crate::retirement_intent::cancel(
        &mut f.journal,
        &key("owner"),
        &d("retire"),
        2,
        key("cancel-after-adopt"),
    )
    .unwrap();
    f.reopen();
    let w = work(&mut f);
    let r = execute(&mut f, &w).unwrap();
    report(&mut f, &w, &r);
    assert!(
        current(&mut f.journal, &f.nodes, &f.ctx, &object)
            .unwrap()
            .is_none()
    );
}
#[test]
fn legacy_unbound_begin_cannot_silently_acquire_a_replacement_plan() {
    let mut f = fixture();
    let object = object(&f);
    let store = f.store.identity().clone();
    let admission = super::super::start(
        &mut f.journal,
        &mut f.nodes,
        &f.ctx,
        &object,
        Action::Remove {
            store: store.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        start(
            &mut f.journal,
            &mut f.nodes,
            &f.ctx,
            &object,
            &store,
            |_| panic!("unbound admission must not rebuild")
        ),
        Err(Error::Occupied)
    );
    assert!(
        current(&mut f.journal, &f.nodes, &f.ctx, &object)
            .unwrap()
            .is_none()
    );
    assert!(
        at_attempt(&mut f.journal, &f.nodes, &f.ctx, &object, admission.attempt)
            .unwrap()
            .is_none()
    );
}
#[test]
fn no_effect_retry_keeps_distinct_immutable_work_for_each_attempt() {
    let mut f = fixture();
    start_work(&mut f).unwrap();
    let old = work(&mut f);
    let object = object(&f);
    // No worker was dispatched in this serial fixture; the native object fence
    // is held while reporting this admitted synthetic no-effect assertion.
    let fence = f.store.try_exclusive_object(&object).unwrap();
    finish(
        &mut f.journal,
        &mut f.nodes,
        &f.ctx,
        &object,
        old.plan.scope().attempt,
        Action::Remove {
            store: old.plan.scope().store.clone(),
        },
        Outcome::NoEffect {
            evidence: d("fenced-undispatched-work"),
        },
    )
    .unwrap();
    drop(fence);
    start_work(&mut f).unwrap();
    let new = work(&mut f);
    assert_eq!(new.plan.scope().attempt, old.plan.scope().attempt + 1);
    assert!(execute(&mut f, &old).is_err());
    let r = execute(&mut f, &new).unwrap();
    report(&mut f, &new, &r);
}
struct Hidden<'a> {
    nodes: &'a MetadataStore,
    hide: Object,
}
impl NodeStore for Hidden<'_> {
    fn get(&self, o: &Object) -> tig_core::tree::TreeResult<Option<Vec<u8>>> {
        if *o == self.hide {
            Ok(None)
        } else {
            self.nodes.get(o)
        }
    }
    fn put(&mut self, _: &[u8]) -> tig_core::tree::TreeResult<Object> {
        panic!("read-only missing-work test")
    }
}
#[test]
fn missing_work_or_wrong_scope_never_downgrades_to_unbound_admission() {
    let mut f = fixture();
    let object = object(&f);
    let store = f.store.identity().clone();
    let calls = Cell::new(0);
    let native = &f.store;
    let ctx = &f.ctx;
    let job = f.job.clone();
    let before = f.journal.checkpoint();
    assert!(
        start(
            &mut f.journal,
            &mut f.nodes,
            ctx,
            &object,
            &store,
            |attempt| {
                calls.set(calls.get() + 1);
                let mut value =
                    serde_json::to_value(removal::plan(native, ctx, &object, attempt).unwrap())
                        .unwrap();
                value["scope"]["attempt"] = serde_json::json!(attempt + 1);
                Ok(LocalWork::new(serde_json::from_value(value).unwrap(), job))
            }
        )
        .is_err()
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(before, f.journal.checkpoint());
    start_work(&mut f).unwrap();
    let loaded = load(&mut f.journal, &f.nodes, &f.ctx, &object).unwrap();
    let root = loaded.frames.last().unwrap().1.execution.clone().unwrap();
    assert!(
        current(
            &mut f.journal,
            &Hidden {
                nodes: &f.nodes,
                hide: root
            },
            &f.ctx,
            &object
        )
        .is_err()
    );
}
#[derive(Serialize, Deserialize)]
struct Controls {
    journal: Checkpoint,
    nodes: MetadataCheckpoint,
    ledger: crate::ledger::Checkpoint,
    job: JournalRef,
}
#[test]
fn local_work_child() {
    let Some(root) = std::env::var_os("TIG_BOUND_WORK_TEST_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let c: Controls =
        serde_json::from_slice(&fs::read(root.join("work-controls.json")).unwrap()).unwrap();
    let store = LocalStore::open(&root.join("store")).unwrap();
    let mut ledger = crate::ledger::CapacityLedger::open(&root.join("ledger"), &c.ledger).unwrap();
    let mut j = SourceJournal::open(&root.join("journal"), &c.journal).unwrap();
    let mut n = MetadataStore::open(&root.join("nodes"), &c.nodes).unwrap();
    let ctx = crate::retirement_intent::observe(
        &mut j,
        &n,
        &Enrollment {
            owner: key("owner"),
            subject: d("subject"),
        },
        &d("retire"),
    )
    .unwrap();
    let object = ctx.review().objects()[0].object().clone();
    let phase = std::env::var("TIG_BOUND_WORK_TEST_PHASE").unwrap();
    start_hook(
        &mut j,
        &mut n,
        &ctx,
        &object,
        store.identity(),
        |attempt| {
            Ok(LocalWork::new(
                removal::plan(&store, &ctx, &object, attempt).unwrap(),
                c.job,
            ))
        },
        |_| {
            if phase == "prepared" {
                std::process::exit(76)
            };
            Ok(())
        },
    )
    .unwrap();
    if phase == "effected" {
        let w = current(&mut j, &n, &ctx, &object).unwrap().unwrap();
        removal::apply(
            removal::Execution {
                store: &store,
                ledger: &mut ledger,
                job: w.job(),
                journal: &mut j,
                nodes: &n,
                intent: &ctx,
                protection: &mut Finite::default(),
            },
            w.plan(),
        )
        .unwrap();
    }
    std::process::exit(76);
}
#[test]
fn child_exits_restore_prepared_admitted_and_effected_work_without_plan_handles() {
    for phase in ["prepared", "admitted", "effected"] {
        let mut f = fixture();
        fs::write(
            f.temp.path().join("work-controls.json"),
            serde_json::to_vec(&Controls {
                journal: f.journal.checkpoint(),
                nodes: f.nodes.checkpoint(),
                ledger: f.ledger.checkpoint(),
                job: f.job.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "retirement_intent::progress::local_work::tests::local_work_child",
                "--nocapture",
            ])
            .env("TIG_BOUND_WORK_TEST_ROOT", f.temp.path())
            .env("TIG_BOUND_WORK_TEST_PHASE", phase)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(76));
        f.reopen();
        let object = object(&f);
        let store = f.store.identity().clone();
        if phase == "prepared" {
            start(
                &mut f.journal,
                &mut f.nodes,
                &f.ctx,
                &object,
                &store,
                |_| panic!("lost caller root must not cause rebuild"),
            )
            .unwrap();
        }
        let w = work(&mut f);
        let r = execute(&mut f, &w).unwrap();
        report(&mut f, &w, &r);
        assert!(f.store.read_object(&object).is_err());
    }
}
