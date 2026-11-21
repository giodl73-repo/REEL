use super::*;
use crate::{PublicationBackend, ledger::Checkpoint, local_capacity::LocalCapacityProbe, publish};
use std::{fs, io::Cursor, path::Path, process::Command};
use tempfile::TempDir;

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn digest(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn basis() -> Basis {
    Basis {
        source: digest("source"),
        policy: digest("policy"),
        claims: digest("claims"),
        authority: digest("authority"),
        protection: ProtectionScope::Unknown {
            gaps: vec![key("jobs")],
        },
    }
}
struct Fixture {
    root: TempDir,
    plan: Plan,
    job: JournalRef,
    witness: Checkpoint,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut source = LocalStore::create(&root.path().join("source"), key("source")).unwrap();
        let destination =
            LocalStore::create(&root.path().join("destination"), key("destination")).unwrap();
        let mut ledger = CapacityLedger::create(
            &root.path().join("ledger"),
            key("capacity"),
            key("epoch"),
            0,
        )
        .unwrap();
        let witness = ledger.checkpoint();
        let mut copies = Vec::new();
        for (i, bytes) in [
            b"unchanged-one".as_slice(),
            b"unchanged-two",
            b"replacement",
        ]
        .iter()
        .enumerate()
        {
            let request = Request {
                operation: key(&format!("source-{i}")),
                owner: key("source-owner"),
                object: git_asset_core::Object::from_bytes(bytes),
                required: AckLevel::ProcessRestart,
            };
            publish(&mut source, &request, &mut Cursor::new(bytes)).unwrap();
            copies.push(Copy {
                source: request,
                operation: key(&format!("copy-{i}")),
            });
        }
        let plan = Plan::new(PlanRequest {
            source: Place::local(&source, 1).unwrap(),
            destination: Place::local(&destination, 1).unwrap(),
            capacity: ledger.identity().clone(),
            owner: key("owner"),
            basis: basis(),
            budget: Budget {
                max_objects: MAX_ACTIONS,
                max_payload_bytes: 1024,
                max_peak_bytes: 64 * 1024 * 1024,
            },
            copies,
        })
        .unwrap();
        let peak = plan.local_peak(&destination).unwrap();
        LocalCapacityProbe::bind(&destination, &mut ledger)
            .unwrap()
            .observe(&destination, &mut ledger, key("probe"), key("sample"))
            .unwrap();
        let job = ledger
            .reserve(key("job"), key("owner"), peak)
            .unwrap()
            .reference()
            .clone();
        Self {
            root,
            plan,
            job,
            witness,
        }
    }
    fn open(&self) -> (LocalStore, LocalStore, CapacityLedger) {
        open(self.root.path(), &self.witness)
    }
    fn execution<'a>(
        &'a self,
        s: &'a LocalStore,
        d: &'a LocalStore,
        l: &'a mut CapacityLedger,
    ) -> LocalExecution<'a> {
        LocalExecution {
            source: s,
            destination: d,
            ledger: l,
            admitted_source: &self.plan.request.source,
            admitted_destination: &self.plan.request.destination,
            admitted_basis: &self.plan.request.basis,
        }
    }
}
fn open(root: &Path, witness: &Checkpoint) -> (LocalStore, LocalStore, CapacityLedger) {
    (
        LocalStore::open(&root.join("source")).unwrap(),
        LocalStore::open(&root.join("destination")).unwrap(),
        CapacityLedger::open(&root.join("ledger"), witness).unwrap(),
    )
}
#[test]
fn exact_plan_roundtrip_and_permutation_have_stable_identity() {
    let f = Fixture::new();
    let bytes = f.plan.to_json().unwrap();
    assert_eq!(Plan::from_json(&bytes).unwrap().id(), f.plan.id());
    let mut request = f.plan.request.clone();
    request.copies.reverse();
    assert_eq!(Plan::new(request).unwrap().to_json().unwrap(), bytes);
    assert!(
        std::str::from_utf8(&bytes)
            .unwrap()
            .contains("tig.storage-plan.design-1")
    );
}
#[test]
fn every_admission_and_selection_change_changes_identity() {
    let f = Fixture::new();
    let mut variants = Vec::new();
    let mut r = f.plan.request.clone();
    r.basis.source = digest("changed");
    variants.push(r);
    let mut r = f.plan.request.clone();
    r.basis.policy = digest("changed");
    variants.push(r);
    let mut r = f.plan.request.clone();
    r.basis.claims = digest("changed");
    variants.push(r);
    let mut r = f.plan.request.clone();
    r.basis.authority = digest("changed");
    variants.push(r);
    let mut r = f.plan.request.clone();
    r.source.generation += 1;
    variants.push(r);
    let mut r = f.plan.request.clone();
    r.destination.generation += 1;
    variants.push(r);
    let mut r = f.plan.request.clone();
    r.capacity = digest("changed");
    variants.push(r);
    let mut r = f.plan.request.clone();
    r.copies.pop();
    variants.push(r);
    for r in variants {
        assert_ne!(Plan::new(r).unwrap().id(), f.plan.id());
    }
}
#[test]
fn malformed_conflicting_duplicate_and_oversized_plans_refuse() {
    let f = Fixture::new();
    let mut r = f.plan.request.clone();
    r.copies[1].operation = r.copies[0].operation.clone();
    assert!(Plan::new(r).is_err());
    let mut r = f.plan.request.clone();
    r.copies[1].source.object.sha256 = r.copies[0].source.object.sha256.clone();
    assert!(Plan::new(r).is_err());
    let mut r = f.plan.request.clone();
    r.source = r.destination.clone();
    assert!(Plan::new(r).is_err());
    let mut r = f.plan.request.clone();
    r.budget.max_objects = 0;
    assert!(Plan::new(r).is_err());
    let mut r = f.plan.request.clone();
    r.budget.max_payload_bytes = 1;
    assert!(Plan::new(r).is_err());
    let mut r = f.plan.request.clone();
    r.source.contract = digest("unsupported");
    assert!(Plan::new(r).is_err());
    let mut r = f.plan.request.clone();
    r.copies[0].source.required = AckLevel::ProviderDurable;
    assert!(Plan::new(r).is_err());
    let bytes = f.plan.to_json().unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(Plan::from_json(text.replacen("design-1", "design-2", 1).as_bytes()).is_err());
    assert!(Plan::from_json(format!(" {text}").as_bytes()).is_err());
    assert!(Plan::from_json(&vec![b' '; 65537]).is_err());
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["extra"] = true.into();
    assert!(Plan::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
    value.as_object_mut().unwrap().remove("extra");
    value["id"] = digest("wrong").as_str().into();
    assert!(Plan::from_json(&identity::canonical_json(&value).unwrap()).is_err());
}
#[test]
fn complete_assertion_never_grants_removal_and_unknown_scope_permits_copy() {
    let f = Fixture::new();
    assert_eq!(
        f.plan.holds(),
        vec![
            Hold::RemovalExecutorUnavailable,
            Hold::IncompleteProtectionScope
        ]
    );
    let mut r = f.plan.request.clone();
    r.basis.protection = ProtectionScope::DeclaredComplete {
        registry: digest("registry"),
        generation: 1,
    };
    assert_eq!(
        Plan::new(r).unwrap().holds(),
        vec![Hold::RemovalExecutorUnavailable]
    );
    let (s, d, mut l) = f.open();
    assert!(
        f.execution(&s, &d, &mut l)
            .apply(&f.plan, 0, &f.job)
            .is_ok()
    );
}
#[test]
fn multi_object_copy_reopens_retention_and_preserves_every_source() {
    let f = Fixture::new();
    let (s, d, mut l) = f.open();
    for index in 0..3 {
        let before = l.checkpoint();
        let status = f
            .execution(&s, &d, &mut l)
            .status(&f.plan, index, &f.job)
            .unwrap();
        assert_eq!(status.phase, Phase::NotStarted);
        assert_eq!(l.checkpoint(), before);
        let receipt = f
            .execution(&s, &d, &mut l)
            .apply(&f.plan, index, &f.job)
            .unwrap();
        assert_eq!(receipt.plan, *f.plan.id());
        let req = &f.plan.request.copies[index].source;
        assert!(s.read_object(&req.object).is_ok());
        assert!(
            receipt.publication_footprint + receipt.retention_footprint
                <= f.plan.local_peak(&d).unwrap()
        );
        assert!(d.read_object(&req.object).is_ok());
    }
    drop((s, d, l));
    let (s, d, mut l) = f.open();
    for index in 0..3 {
        let before = l.checkpoint();
        let status = f
            .execution(&s, &d, &mut l)
            .status(&f.plan, index, &f.job)
            .unwrap();
        assert_eq!(status.phase, Phase::Retained);
        assert!(status.retained.is_some());
        assert_eq!(l.checkpoint(), before);
        let receipt = f
            .execution(&s, &d, &mut l)
            .apply(&f.plan, index, &f.job)
            .unwrap();
        assert_eq!(receipt.operation, f.plan.request.copies[index].operation);
        assert_eq!(l.checkpoint(), before);
    }
    l.finish(key("finish"), &f.job).unwrap();
    assert_eq!(l.accounting().unwrap().reserved_remaining, 0);
}
#[test]
fn changed_basis_same_operation_conflicts_after_restart_before_new_bytes() {
    let f = Fixture::new();
    let (s, d, mut l) = f.open();
    drop(
        f.execution(&s, &d, &mut l)
            .apply(&f.plan, 0, &f.job)
            .unwrap(),
    );
    drop((s, d, l));
    let mut changed = f.plan.request.clone();
    changed.basis.policy = digest("changed-policy");
    let changed = Plan::new(changed).unwrap();
    let (s, d, mut l) = f.open();
    let before = l.checkpoint();
    let mut exec = f.execution(&s, &d, &mut l);
    exec.admitted_basis = &changed.request.basis;
    assert!(exec.apply(&changed, 0, &f.job).is_err());
    assert_eq!(l.checkpoint(), before);
    assert!(
        s.read_object(&f.plan.request.copies[0].source.object)
            .is_ok()
    );
}
#[test]
fn stale_admission_wrong_store_ledger_and_replaced_root_refuse_before_writes() {
    let f = Fixture::new();
    let other = Fixture::new();
    let (s, d, mut l) = f.open();
    let (_, wrong, mut wrong_ledger) = other.open();
    let mut changed = f.plan.request.basis.clone();
    changed.policy = digest("new");
    let before = l.checkpoint();
    let mut exec = f.execution(&s, &d, &mut l);
    exec.admitted_basis = &changed;
    assert!(matches!(
        exec.apply(&f.plan, 0, &f.job),
        Err(Error::StaleAdmission)
    ));
    assert!(
        f.execution(&s, &wrong, &mut l)
            .apply(&f.plan, 0, &f.job)
            .is_err()
    );
    assert!(
        f.execution(&s, &d, &mut wrong_ledger)
            .apply(&f.plan, 0, &f.job)
            .is_err()
    );
    fs::rename(
        f.root.path().join("destination"),
        f.root.path().join("old-destination"),
    )
    .unwrap();
    LocalStore::create(&f.root.path().join("destination"), key("destination")).unwrap();
    assert!(
        f.execution(&s, &d, &mut l)
            .apply(&f.plan, 0, &f.job)
            .is_err()
    );
    assert_eq!(l.checkpoint(), before);
}
#[test]
fn insufficient_peak_or_reservation_cannot_publish() {
    let f = Fixture::new();
    let (s, d, mut l) = f.open();
    let mut r = f.plan.request.clone();
    r.budget.max_peak_bytes = 1;
    let p = Plan::new(r).unwrap();
    let before = l.checkpoint();
    assert!(f.execution(&s, &d, &mut l).apply(&p, 0, &f.job).is_err());
    assert_eq!(l.checkpoint(), before);
    LocalCapacityProbe::bind(&d, &mut l)
        .unwrap()
        .observe(&d, &mut l, key("tiny-probe"), key("tiny-sample"))
        .unwrap();
    let tiny = l
        .reserve(key("tiny"), key("owner"), 1)
        .unwrap()
        .reference()
        .clone();
    assert!(
        f.execution(&s, &d, &mut l)
            .apply(&f.plan, 0, &tiny)
            .is_err()
    );
    assert_eq!(
        fs::read_dir(f.root.path().join("destination/objects"))
            .unwrap()
            .count(),
        0
    );
}
#[test]
fn publication_reply_loss_resumes_retention_without_rereading_source() {
    let f = Fixture::new();
    let (s, d, mut l) = f.open();
    let mut exec = f.execution(&s, &d, &mut l);
    assert!(
        exec.apply_hook(&f.plan, 0, &f.job, |phase| {
            if phase == Phase::Partial {
                assert!(matches!(
                    s.try_exclusive_object(&f.plan.request.copies[0].source.object),
                    Err(StoreError::Busy)
                ));
                assert!(
                    s.try_exclusive_object(&f.plan.request.copies[1].source.object)
                        .is_ok()
                );
                Err(Error::InvalidPlan)
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    assert_eq!(
        exec.status(&f.plan, 0, &f.job).unwrap().phase,
        Phase::Partial
    );
    // External source loss is not caused by the orchestrator. Completed exact
    // destination publication is sufficient to recover this interrupted action.
    fs::remove_file(
        f.root
            .path()
            .join("source/objects")
            .join(f.plan.request.copies[0].source.object.sha256.as_str()),
    )
    .unwrap();
    drop((s, d, l));
    let (s, d, mut l) = f.open();
    assert!(
        f.execution(&s, &d, &mut l)
            .apply(&f.plan, 0, &f.job)
            .is_ok()
    );
}
#[test]
fn corrupt_source_cannot_become_completed_destination() {
    let f = Fixture::new();
    let (s, d, mut l) = f.open();
    let object = &f.plan.request.copies[0].source.object;
    fs::write(
        f.root
            .path()
            .join("source/objects")
            .join(object.sha256.as_str()),
        b"bad",
    )
    .unwrap();
    assert!(
        f.execution(&s, &d, &mut l)
            .apply(&f.plan, 0, &f.job)
            .is_err()
    );
    assert!(
        !f.root
            .path()
            .join("destination/objects")
            .join(object.sha256.as_str())
            .exists()
    );
    assert_ne!(
        f.execution(&s, &d, &mut l)
            .status(&f.plan, 0, &f.job)
            .unwrap()
            .phase,
        Phase::Retained
    );
}
#[test]
fn missing_or_corrupt_acknowledged_destination_is_error_never_not_started() {
    for bytes in [None, Some(b"bad".as_slice())] {
        let f = Fixture::new();
        let (s, d, mut l) = f.open();
        drop(
            f.execution(&s, &d, &mut l)
                .apply(&f.plan, 0, &f.job)
                .unwrap(),
        );
        let path = f
            .root
            .path()
            .join("destination/objects")
            .join(f.plan.request.copies[0].source.object.sha256.as_str());
        if let Some(bytes) = bytes {
            fs::write(path, bytes).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
        assert!(
            f.execution(&s, &d, &mut l)
                .status(&f.plan, 0, &f.job)
                .is_err()
        );
        assert!(
            f.execution(&s, &d, &mut l)
                .apply(&f.plan, 0, &f.job)
                .is_err()
        );
        assert!(
            s.read_object(&f.plan.request.copies[0].source.object)
                .is_ok()
        );
    }
}
#[test]
fn process_exit_helper() {
    let Ok(root) = std::env::var("GIT_ASSET_ORCHESTRATION_CHILD_ROOT") else {
        return;
    };
    let root = Path::new(&root);
    let plan = Plan::from_json(&fs::read(root.join("plan.json")).unwrap()).unwrap();
    let witness: Checkpoint =
        serde_json::from_slice(&fs::read(root.join("witness.json")).unwrap()).unwrap();
    let job: JournalRef =
        serde_json::from_slice(&fs::read(root.join("job.json")).unwrap()).unwrap();
    let (s, d, mut l) = open(root, &witness);
    let mut exec = LocalExecution {
        source: &s,
        destination: &d,
        ledger: &mut l,
        admitted_source: &plan.request.source,
        admitted_destination: &plan.request.destination,
        admitted_basis: &plan.request.basis,
    };
    let target = std::env::var("GIT_ASSET_ORCHESTRATION_CHILD_PHASE").unwrap();
    exec.apply_hook(&plan, 0, &job, |phase| {
        if format!("{phase:?}") == target {
            std::process::exit(75);
        }
        Ok(())
    })
    .unwrap();
    panic!("expected abrupt child exit");
}
#[test]
fn actual_child_exit_after_publication_or_retention_reopens_and_resumes_exactly() {
    for phase in ["Partial", "Retained"] {
        let f = Fixture::new();
        fs::write(f.root.path().join("plan.json"), f.plan.to_json().unwrap()).unwrap();
        fs::write(
            f.root.path().join("witness.json"),
            serde_json::to_vec(&f.witness).unwrap(),
        )
        .unwrap();
        fs::write(
            f.root.path().join("job.json"),
            serde_json::to_vec(&f.job).unwrap(),
        )
        .unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "orchestration::tests::process_exit_helper",
                "--nocapture",
            ])
            .env("GIT_ASSET_ORCHESTRATION_CHILD_ROOT", f.root.path())
            .env("GIT_ASSET_ORCHESTRATION_CHILD_PHASE", phase)
            .status()
            .unwrap();
        assert_eq!(child.code(), Some(75));
        let (s, d, mut l) = f.open();
        let status = f
            .execution(&s, &d, &mut l)
            .status(&f.plan, 0, &f.job)
            .unwrap();
        assert_eq!(
            status.phase,
            if phase == "Partial" {
                Phase::Partial
            } else {
                Phase::Retained
            }
        );
        drop(status);
        assert!(
            f.execution(&s, &d, &mut l)
                .apply(&f.plan, 0, &f.job)
                .is_ok()
        );
        assert!(
            s.read_object(&f.plan.request.copies[0].source.object)
                .is_ok()
        );
        assert_eq!(
            f.execution(&s, &d, &mut l)
                .status(&f.plan, 1, &f.job)
                .unwrap()
                .phase,
            Phase::NotStarted
        );
    }
}
#[test]
fn legacy_accounted_plan_encoding_remains_context_free() {
    let f = Fixture::new();
    let (_, d, _) = f.open();
    let request = f.plan.publication(0).unwrap();
    let legacy = accounted::plan(&d, &request).unwrap();
    let json = serde_json::to_value(&legacy).unwrap();
    assert!(json.get("context").is_none());
    let bound = accounted::plan_bound(&d, &request, f.plan.id.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(bound).unwrap()["context"],
        f.plan.id.as_str()
    );
    assert!(d.capabilities().max_object_bytes <= 8 * 1024 * 1024);
}

#[test]
fn context_bound_private_cleanup_cancels_retry_without_removing_final_objects() {
    let f = Fixture::new();
    let (s, d, mut l) = f.open();
    let damaged = &f.plan.request.copies[0].source.object;
    fs::write(
        f.root
            .path()
            .join("source/objects")
            .join(damaged.sha256.as_str()),
        b"bad",
    )
    .unwrap();
    assert!(
        f.execution(&s, &d, &mut l)
            .apply(&f.plan, 0, &f.job)
            .is_err()
    );
    let (publication, _) = f.plan.plans(&d, 0).unwrap();
    let reviewed = accounted::cleanup::inspect(&mut l, &d, &f.job, &publication).unwrap();
    let authorization = reviewed.authorize_discard().unwrap();
    accounted::cleanup::apply(&mut l, &d, &reviewed, &authorization).unwrap();
    let before = l.checkpoint();
    assert!(
        f.execution(&s, &d, &mut l)
            .apply(&f.plan, 0, &f.job)
            .is_err()
    );
    assert!(
        f.execution(&s, &d, &mut l)
            .status(&f.plan, 0, &f.job)
            .is_err()
    );
    assert_eq!(l.checkpoint(), before);
    assert!(
        s.read_object(&f.plan.request.copies[1].source.object)
            .is_ok()
    );
    assert_eq!(
        fs::read_dir(f.root.path().join("destination/objects"))
            .unwrap()
            .count(),
        0
    );
}
