use super::*;
use crate::ledger::{Checkpoint, Response};
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tempfile::TempDir;
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
struct Fixture {
    temp: TempDir,
    store: LocalStore,
    ledger: CapacityLedger,
    witness: Checkpoint,
    planned: RetentionPlan,
    job: JournalRef,
}
impl Fixture {
    fn new() -> Self {
        Self::bytes(b"AAA")
    }
    fn bytes(bytes: &[u8]) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut store = LocalStore::create(&temp.path().join("store"), key("place")).unwrap();
        let publication = Request {
            operation: key("base"),
            owner: key("owner"),
            object: tig_core::Object::from_bytes(bytes),
            required: AckLevel::ProcessRestart,
        };
        publish(&mut store, &publication, &mut Cursor::new(bytes)).unwrap();
        let mut ledger =
            CapacityLedger::create(&temp.path().join("ledger"), key("volume"), key("epoch"), 0)
                .unwrap();
        let witness = ledger.checkpoint();
        let planned = plan(
            &store,
            &RetentionRequest {
                operation: key("hold"),
                owner: key("owner"),
                publication,
                minimum_ack: AckLevel::ProcessRestart,
            },
        )
        .unwrap();
        let p = ledger.begin_probe(key("probe")).unwrap();
        ledger
            .observe(key("sample"), p.reference(), 1024 * 1024 * 1024)
            .unwrap();
        let job = ledger
            .reserve(key("job"), key("owner"), planned.limit * 20)
            .unwrap()
            .reference()
            .clone();
        Self {
            temp,
            store,
            ledger,
            witness,
            planned,
            job,
        }
    }
    fn retain(&mut self) -> RetentionReceipt {
        retain(&mut self.ledger, &self.store, &self.job, &self.planned).unwrap()
    }
    fn object(&self) -> &tig_core::Object {
        &self.planned.request.publication.object
    }
    fn dir(&self) -> PathBuf {
        self.temp
            .path()
            .join("store/retentions")
            .join(self.object().sha256.as_str())
    }
    fn pin(&self) -> PathBuf {
        self.dir().join(format!(
            "{}.json",
            self.planned.request.pin_id().unwrap().as_str()
        ))
    }
    fn scope(&self) -> PathBuf {
        let (op, _) = keys(&self.planned).unwrap();
        self.temp.path().join("store/staging").join(format!(
            "attempt-{}",
            Digest::blob(op.as_str().as_bytes()).as_str()
        ))
    }
    fn reopen(&mut self) {
        self.store = LocalStore::open(&self.temp.path().join("store")).unwrap();
        self.ledger =
            CapacityLedger::open(&self.temp.path().join("ledger"), &self.witness).unwrap();
    }
}
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(root: &Path, p: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for e in fs::read_dir(p).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out)
            } else {
                out.push((
                    p.strip_prefix(root).unwrap().to_owned(),
                    fs::read(p).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}
#[test]
fn settled_retention_survives_reopen_and_session_exit() {
    let mut f = Fixture::new();
    let r = f.retain();
    let live = verify(&mut f.ledger, &f.store, &r.grant).unwrap();
    drop(live);
    f.ledger.finish(key("finish"), &f.job).unwrap();
    f.reopen();
    assert!(matches!(
        f.store.try_exclusive_object(f.object()),
        Err(Error::Retained)
    ));
    assert!(verify(&mut f.ledger, &f.store, &r.grant).is_ok());
    assert!(
        matches!(r.settlement.response(),Response::RetentionSettled(n) if *n==r.charged_footprint)
    );
}
#[test]
fn exact_settled_retry_is_readonly_after_job_close() {
    let mut f = Fixture::new();
    let one = f.retain();
    f.ledger.finish(key("finish"), &f.job).unwrap();
    let before = snapshot(f.temp.path());
    let two = f.retain();
    assert_eq!(one.grant.to_json().unwrap(), two.grant.to_json().unwrap());
    assert!(two.settlement.was_replayed());
    assert_eq!(snapshot(f.temp.path()), before);
    assert_eq!(
        f.ledger.accounting().unwrap().allocated_after_watermark,
        one.charged_footprint
    );
}
#[test]
fn invalid_budget_or_owner_rejects_before_provider_mutation() {
    let mut f = Fixture::new();
    let p = f.ledger.begin_probe(key("small-probe")).unwrap();
    f.ledger
        .observe(key("small-sample"), p.reference(), 1024 * 1024 * 1024)
        .unwrap();
    let small = f.ledger.reserve(key("small"), key("owner"), 1).unwrap();
    let before = snapshot(&f.temp.path().join("store"));
    assert!(retain(&mut f.ledger, &f.store, small.reference(), &f.planned).is_err());
    let mut wrong = f.planned.request.clone();
    wrong.owner = key("other");
    let planned = plan(&f.store, &wrong).unwrap();
    assert!(retain(&mut f.ledger, &f.store, &f.job, &planned).is_err());
    assert_eq!(snapshot(&f.temp.path().join("store")), before);
}
#[test]
fn corrupt_or_incomplete_target_never_receives_a_retention_claim() {
    for missing in [false, true] {
        let mut f = Fixture::new();
        if missing {
            let name = format!("{}.done", Digest::blob(b"base").as_str());
            fs::remove_file(f.temp.path().join("store/operations").join(name)).unwrap();
        } else {
            fs::write(
                f.temp
                    .path()
                    .join("store/objects")
                    .join(f.object().sha256.as_str()),
                b"bad",
            )
            .unwrap();
        }
        assert!(retain(&mut f.ledger, &f.store, &f.job, &f.planned).is_err());
        assert!(!f.pin().exists());
        assert!(f.ledger.finish(key("finish"), &f.job).is_err());
    }
}
#[test]
fn unsupported_or_forged_contracts_and_other_store_reject() {
    let mut f = Fixture::new();
    let before = snapshot(&f.temp.path().join("store"));
    let mut req = f.planned.request.clone();
    req.minimum_ack = AckLevel::ProviderDurable;
    assert!(plan(&f.store, &req).is_err());
    let mut fake = f.planned.clone();
    fake.limit -= 1;
    assert!(matches!(
        retain(&mut f.ledger, &f.store, &f.job, &fake),
        Err(RetentionError::InvalidPlan)
    ));
    let other = LocalStore::create(&f.temp.path().join("other"), key("place")).unwrap();
    assert!(retain(&mut f.ledger, &other, &f.job, &f.planned).is_err());
    assert_eq!(snapshot(&f.temp.path().join("store")), before);
}
#[test]
fn operation_rebinding_cannot_replace_an_existing_claim() {
    let mut f = Fixture::new();
    f.retain();
    let before = fs::read(f.pin()).unwrap();
    let mut req = f.planned.request.clone();
    req.publication.operation = key("different");
    let planned = plan(&f.store, &req).unwrap();
    assert!(matches!(
        retain(&mut f.ledger, &f.store, &f.job, &planned),
        Err(RetentionError::Ledger(LedgerError::Conflict))
    ));
    assert_eq!(fs::read(f.pin()).unwrap(), before);
}
#[test]
fn each_protocol_boundary_retries_without_an_unprotected_ack_or_double_charge() {
    for cut in [
        Stage::Admitted,
        Stage::Bound,
        Stage::Installed,
        Stage::Settled,
    ] {
        let mut f = Fixture::new();
        assert!(
            retain_hook(&mut f.ledger, &f.store, &f.job, &f.planned, |s| {
                if s == cut {
                    Err(RetentionError::Injected)
                } else {
                    Ok(())
                }
            })
            .is_err()
        );
        f.reopen();
        if matches!(cut, Stage::Installed | Stage::Settled) {
            assert!(matches!(
                f.store.try_exclusive_object(f.object()),
                Err(Error::Retained)
            ));
        }
        let r = f.retain();
        f.ledger.finish(key("finish"), &f.job).unwrap();
        assert_eq!(
            f.ledger.accounting().unwrap().allocated_after_watermark,
            r.charged_footprint
        );
        assert!(verify(&mut f.ledger, &f.store, &r.grant).is_ok());
    }
}
#[test]
fn malformed_or_foreign_object_records_fail_closed_without_leaking_a_guard() {
    for unknown in [false, true] {
        let mut f = Fixture::new();
        f.retain();
        let path = if unknown {
            f.dir().join("foreign")
        } else {
            f.pin()
        };
        let old = fs::read(f.pin()).unwrap();
        fs::write(&path, b"{}").unwrap();
        assert!(f.store.try_exclusive_object(f.object()).is_err());
        if unknown {
            fs::remove_file(path).unwrap();
        } else {
            fs::write(path, old).unwrap();
        }
        assert!(matches!(
            f.store.try_exclusive_object(f.object()),
            Err(Error::Retained)
        ));
    }
}
#[test]
fn per_object_retainer_limit_is_atomic_and_bounded() {
    let mut f = Fixture::new();
    for i in 0..MAX_RETAINERS {
        let mut req = f.planned.request.clone();
        req.operation = key(&format!("hold-{i}"));
        let p = plan(&f.store, &req).unwrap();
        retain(&mut f.ledger, &f.store, &f.job, &p).unwrap();
    }
    let mut req = f.planned.request.clone();
    req.operation = key("over-limit");
    let p = plan(&f.store, &req).unwrap();
    assert!(matches!(
        retain(&mut f.ledger, &f.store, &f.job, &p),
        Err(RetentionError::Store(Error::Budget))
    ));
    assert_eq!(fs::read_dir(f.dir()).unwrap().count(), MAX_RETAINERS);
}
#[test]
fn concurrent_retainers_cannot_overrun_the_last_object_slot() {
    let mut f = Fixture::new();
    for i in 0..MAX_RETAINERS - 1 {
        let mut req = f.planned.request.clone();
        req.operation = key(&format!("seed-{i}"));
        retain(
            &mut f.ledger,
            &f.store,
            &f.job,
            &plan(&f.store, &req).unwrap(),
        )
        .unwrap();
    }
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut workers = Vec::new();
    let mut planned = Vec::new();
    for i in 0..2 {
        let mut req = f.planned.request.clone();
        req.operation = key(&format!("racer-{i}"));
        let p = plan(&f.store, &req).unwrap();
        planned.push(p.clone());
        let mut ledger = CapacityLedger::open(&f.temp.path().join("ledger"), &f.witness).unwrap();
        let store = LocalStore::open(&f.temp.path().join("store")).unwrap();
        let job = f.job.clone();
        let gate = barrier.clone();
        workers.push(std::thread::spawn(move || {
            gate.wait();
            retain(&mut ledger, &store, &job, &p)
        }));
    }
    for worker in workers {
        let result = worker.join().unwrap();
        assert!(matches!(
            result,
            Ok(_)
                | Err(RetentionError::Ledger(LedgerError::Busy))
                | Err(RetentionError::Store(Error::Busy | Error::Budget))
        ));
    }
    assert!(fs::read_dir(f.dir()).unwrap().count() <= MAX_RETAINERS);
    // Nonblocking admission may reject both racers. Serialized exact retries
    // must fill exactly one remaining slot and refuse the other.
    let mut successes = 0;
    for p in planned {
        f.reopen();
        match retain(&mut f.ledger, &f.store, &f.job, &p) {
            Ok(_) => successes += 1,
            Err(RetentionError::Store(Error::Budget)) => (),
            Err(e) => panic!("unexpected serial retry: {e}"),
        }
    }
    assert_eq!(successes, 1);
    assert_eq!(fs::read_dir(f.dir()).unwrap().count(), MAX_RETAINERS);
}
#[test]
fn exclusive_actor_blocks_new_retention_only_for_its_object() {
    let mut f = Fixture::new();
    let exclusive = f.store.try_exclusive_object(f.object()).unwrap();
    assert!(retain(&mut f.ledger, &f.store, &f.job, &f.planned).is_err());
    assert!(!f.pin().exists());
    let other = Request {
        operation: key("other"),
        owner: key("owner"),
        object: tig_core::Object::from_bytes(b"BBB"),
        required: AckLevel::ProcessRestart,
    };
    publish(&mut f.store, &other, &mut Cursor::new(b"BBB")).unwrap();
    assert!(f.store.try_exclusive_object(&other.object).is_ok());
    drop(exclusive);
    f.retain();
}
#[test]
fn portable_grant_requires_matching_record_ledger_and_bytes() {
    let mut f = Fixture::new();
    let r = f.retain();
    let encoded = r.grant.to_json().unwrap();
    let grant = RetentionGrant::from_json(&encoded).unwrap();
    assert!(verify(&mut f.ledger, &f.store, &grant).is_ok());
    let mut other = CapacityLedger::create(
        &f.temp.path().join("other-ledger"),
        key("volume"),
        key("epoch"),
        0,
    )
    .unwrap();
    assert!(verify(&mut other, &f.store, &grant).is_err());
    let mut fake = grant.clone();
    fake.sha256 = Digest::blob(b"false");
    assert!(RetentionGrant::from_json(&fake.to_json().unwrap()).is_err());
    assert!(verify(&mut f.ledger, &f.store, &fake).is_err());
}
#[test]
fn missing_or_corrupt_settled_record_is_not_recreated_under_spent_budget() {
    for corrupt in [false, true] {
        let mut f = Fixture::new();
        let grant = f.retain().grant;
        f.ledger.finish(key("finish"), &f.job).unwrap();
        if corrupt {
            fs::write(f.pin(), b"{}").unwrap();
        } else {
            fs::remove_file(f.pin()).unwrap();
        }
        let before = snapshot(f.temp.path());
        assert!(retain(&mut f.ledger, &f.store, &f.job, &f.planned).is_err());
        assert!(verify(&mut f.ledger, &f.store, &grant).is_err());
        assert_eq!(snapshot(f.temp.path()), before);
    }
}
#[test]
fn retained_revalidation_failure_keeps_protection_but_invalidates_coverage() {
    let mut f = Fixture::new();
    let r = f.retain();
    let mut retained = verify(&mut f.ledger, &f.store, &r.grant).unwrap();
    let source = Fixture::new();
    let live = replica::verify(&source.store, &source.planned.request.publication).unwrap();
    let enrollment = vec![
        replica::ReplicaEnrollment {
            store: source.store.identity().clone(),
            failure_domain: key("source"),
        },
        replica::ReplicaEnrollment {
            store: f.store.identity().clone(),
            failure_domain: key("retained"),
        },
    ];
    assert!(
        coverage(
            &live,
            &[&retained],
            &enrollment,
            1,
            AckLevel::ProcessRestart
        )
        .unwrap()
        .retention_established()
    );
    let original = fs::read(f.pin()).unwrap();
    fs::write(f.pin(), b"{}").unwrap();
    assert!(retained.revalidate(&mut f.ledger, &f.store).is_err());
    assert!(matches!(
        coverage(
            &live,
            &[&retained],
            &enrollment,
            1,
            AckLevel::ProcessRestart
        ),
        Err(RetentionError::Invalidated)
    ));
    assert!(f.store.try_exclusive_object(f.object()).is_err());
    fs::write(f.pin(), original).unwrap();
    retained.revalidate(&mut f.ledger, &f.store).unwrap();
}
#[test]
fn retained_coverage_is_explicit_and_still_not_final_eviction_authority() {
    let source = Fixture::new();
    let live = replica::verify(&source.store, &source.planned.request.publication).unwrap();
    let mut f = Fixture::new();
    let r = f.retain();
    let retained = verify(&mut f.ledger, &f.store, &r.grant).unwrap();
    let enrollment = vec![
        replica::ReplicaEnrollment {
            store: source.store.identity().clone(),
            failure_domain: key("source"),
        },
        replica::ReplicaEnrollment {
            store: f.store.identity().clone(),
            failure_domain: key("retained"),
        },
    ];
    let c = coverage(
        &live,
        &[&retained],
        &enrollment,
        1,
        AckLevel::ProcessRestart,
    )
    .unwrap();
    assert!(c.retention_established());
    assert!(!c.authorizes_eviction());
    assert!(
        coverage(
            &live,
            &[&retained, &retained],
            &enrollment,
            1,
            AckLevel::ProcessRestart
        )
        .is_err()
    );
}
#[test]
fn typed_retention_cannot_be_settled_by_publication_or_generic_completion() {
    let mut f = Fixture::new();
    let r = f.retain();
    let binding = Digest::blob(&encode(&(&f.planned, &f.job)).unwrap());
    let allocation = &r.grant.record.allocation;
    assert!(matches!(
        f.ledger.complete_allocation(key("generic"), allocation, 0),
        Err(LedgerError::InvalidReference)
    ));
    assert!(matches!(
        f.ledger
            .settle_footprint(key("publication"), allocation, binding, 0),
        Err(LedgerError::InvalidReference)
    ));
}
#[test]
fn already_present_large_payload_is_not_charged_again_as_metadata() {
    let mut f = Fixture::bytes(&vec![0x5a; 2 * 1024 * 1024]);
    assert!(f.planned.limit < f.object().bytes);
    let r = f.retain();
    assert!(r.charged_footprint < f.object().bytes);
    assert!(r.charged_footprint <= f.planned.limit);
}
#[test]
fn older_manifest_rejects_without_migrating_a_root() {
    let f = Fixture::new();
    let path = f.temp.path().join("store/store.json");
    let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    v["schema"] = "tig.local-store.design-3".into();
    fs::write(path, serde_json::to_vec(&v).unwrap()).unwrap();
    let before = snapshot(f.temp.path());
    assert!(LocalStore::open(&f.temp.path().join("store")).is_err());
    assert_eq!(snapshot(f.temp.path()), before);
}
#[test]
fn unknown_partial_without_final_claim_keeps_capacity_and_blocks_new_staging() {
    let mut f = Fixture::new();
    assert!(
        retain_hook(&mut f.ledger, &f.store, &f.job, &f.planned, |s| {
            if s == Stage::Bound {
                Err(RetentionError::Injected)
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    fs::write(f.scope().join("123-0.part"), b"uncertain").unwrap();
    let before = snapshot(&f.temp.path().join("store"));
    assert!(retain(&mut f.ledger, &f.store, &f.job, &f.planned).is_err());
    assert_eq!(snapshot(&f.temp.path().join("store")), before);
    assert!(f.ledger.finish(key("finish"), &f.job).is_err());
}
#[test]
fn retention_child() {
    let Ok(root) = std::env::var("TIG_RETENTION_TEST_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let cut = std::env::var("TIG_RETENTION_TEST_CUT").unwrap();
    let witness: Checkpoint =
        serde_json::from_slice(&fs::read(root.join("witness.json")).unwrap()).unwrap();
    let planned: RetentionPlan =
        serde_json::from_slice(&fs::read(root.join("plan.json")).unwrap()).unwrap();
    let job: JournalRef =
        serde_json::from_slice(&fs::read(root.join("job.json")).unwrap()).unwrap();
    let store = LocalStore::open(&root.join("store")).unwrap();
    let mut ledger = CapacityLedger::open(&root.join("ledger"), &witness).unwrap();
    retain_hook(&mut ledger, &store, &job, &planned, |s| {
        if matches!(
            (s, cut.as_str()),
            (Stage::Bound, "bound") | (Stage::Installed, "installed") | (Stage::Settled, "settled")
        ) {
            std::process::exit(77);
        }
        Ok(())
    })
    .unwrap();
    panic!("child must exit at boundary");
}
#[test]
fn actual_process_exit_preserves_installed_claims_and_exact_recovery() {
    for cut in ["bound", "installed", "settled"] {
        let mut f = Fixture::new();
        for (name, bytes) in [
            ("witness.json", serde_json::to_vec(&f.witness).unwrap()),
            ("plan.json", serde_json::to_vec(&f.planned).unwrap()),
            ("job.json", serde_json::to_vec(&f.job).unwrap()),
        ] {
            fs::write(f.temp.path().join(name), bytes).unwrap();
        }
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "retention::tests::retention_child",
                "--nocapture",
            ])
            .env("TIG_RETENTION_TEST_ROOT", f.temp.path())
            .env("TIG_RETENTION_TEST_CUT", cut)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(77));
        f.reopen();
        if cut != "bound" {
            assert!(matches!(
                f.store.try_exclusive_object(f.object()),
                Err(Error::Retained)
            ));
        }
        let r = f.retain();
        f.ledger.finish(key("finish"), &f.job).unwrap();
        f.reopen();
        assert!(verify(&mut f.ledger, &f.store, &r.grant).is_ok());
        assert!(matches!(
            f.store.try_exclusive_object(f.object()),
            Err(Error::Retained)
        ));
    }
}
