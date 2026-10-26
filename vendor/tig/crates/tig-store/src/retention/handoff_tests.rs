use super::*;
use crate::ledger::Checkpoint;
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
struct Endpoint {
    temp: TempDir,
    store: LocalStore,
    ledger: CapacityLedger,
    witness: Checkpoint,
    job: JournalRef,
    grant: RetentionGrant,
}
impl Endpoint {
    fn new() -> Self {
        Self::with(b"AAA", AckLevel::ProcessRestart)
    }
    fn with(bytes: &[u8], level: AckLevel) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut store = LocalStore::create(&temp.path().join("store"), key("place")).unwrap();
        let publication = Request {
            operation: key("base"),
            owner: key("owner"),
            object: tig_core::Object::from_bytes(bytes),
            required: level,
        };
        crate::publish(&mut store, &publication, &mut Cursor::new(bytes)).unwrap();
        let mut ledger =
            CapacityLedger::create(&temp.path().join("ledger"), key("volume"), key("epoch"), 0)
                .unwrap();
        let witness = ledger.checkpoint();
        let p = super::super::plan(
            &store,
            &RetentionRequest {
                operation: key("pin"),
                owner: key("owner"),
                publication,
                minimum_ack: level,
            },
        )
        .unwrap();
        let t = ledger.begin_probe(key("probe")).unwrap();
        ledger
            .observe(key("sample"), t.reference(), 1024 * 1024 * 1024)
            .unwrap();
        let job = ledger
            .reserve(key("job"), key("owner"), p.required_peak() * 24)
            .unwrap()
            .reference()
            .clone();
        let grant = super::super::retain(&mut ledger, &store, &job, &p)
            .unwrap()
            .grant;
        Self {
            temp,
            store,
            ledger,
            witness,
            job,
            grant,
        }
    }
    fn object(&self) -> &tig_core::Object {
        &self.grant.request().publication.object
    }
    fn pin(&self) -> PathBuf {
        self.temp
            .path()
            .join("store/retentions")
            .join(self.object().sha256.as_str())
            .join(format!(
                "{}.json",
                self.grant.request().pin_id().unwrap().as_str()
            ))
    }
    fn marker(&self) -> PathBuf {
        self.temp
            .path()
            .join("store/releases")
            .join(self.object().sha256.as_str())
            .join(format!(
                "{}.json",
                self.grant.request().pin_id().unwrap().as_str()
            ))
    }
    fn payload(&self) -> PathBuf {
        self.temp
            .path()
            .join("store/objects")
            .join(self.object().sha256.as_str())
    }
    fn reopen(&mut self) {
        self.store = LocalStore::open(&self.temp.path().join("store")).unwrap();
        self.ledger =
            CapacityLedger::open(&self.temp.path().join("ledger"), &self.witness).unwrap();
    }
}
fn planned(a: &Endpoint, b: &Endpoint) -> HandoffPlan {
    plan(
        &a.store,
        &HandoffRequest {
            operation: key("move"),
            owner: key("owner"),
            previous: a.grant.clone(),
            successor: b.grant.clone(),
            source_domain: key("source"),
            successor_domain: key("successor"),
        },
    )
    .unwrap()
}
fn run(
    a: &mut Endpoint,
    b: &mut Endpoint,
    p: &HandoffPlan,
    auth: &HandoffAuthorization,
) -> Result<HandoffReceipt> {
    apply(
        &mut a.ledger,
        &a.store,
        &mut b.ledger,
        &b.store,
        &a.job,
        p,
        auth,
    )
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
fn handoff_retires_only_claim_metadata_and_keeps_both_payloads() {
    let mut a = Endpoint::new();
    let mut b = Endpoint::new();
    let p = planned(&a, &b);
    let auth = p.authorize_handoff().unwrap();
    let r = run(&mut a, &mut b, &p, &auth).unwrap();
    assert!(!a.pin().exists());
    assert!(a.marker().exists());
    assert_eq!(fs::read(a.payload()).unwrap(), b"AAA");
    assert_eq!(fs::read(b.payload()).unwrap(), b"AAA");
    assert!(a.store.try_exclusive_object(a.object()).is_ok());
    assert!(matches!(
        b.store.try_exclusive_object(b.object()),
        Err(Error::Retained)
    ));
    assert!(matches!(
        super::super::verify(&mut a.ledger, &a.store, &a.grant),
        Err(RetentionError::Released)
    ));
    assert!(super::super::verify(&mut b.ledger, &b.store, &b.grant).is_ok());
    assert!(r.charged_footprint <= p.limit);
}
#[test]
fn exact_owner_authorization_and_budget_are_required_before_mutation() {
    let mut a = Endpoint::new();
    let mut b = Endpoint::new();
    let p = planned(&a, &b);
    let mut other = p.clone();
    other.request.operation = key("other");
    let wrong = other.authorize_handoff().unwrap();
    let before = snapshot(a.temp.path());
    assert!(matches!(
        run(&mut a, &mut b, &p, &wrong),
        Err(HandoffError::WrongAuthorization)
    ));
    assert_eq!(snapshot(a.temp.path()), before);
    let t = a.ledger.begin_probe(key("small-probe")).unwrap();
    a.ledger
        .observe(key("small-sample"), t.reference(), 1024 * 1024 * 1024)
        .unwrap();
    let tiny = a.ledger.reserve(key("tiny"), key("owner"), 1).unwrap();
    let before = snapshot(&a.temp.path().join("store"));
    assert!(
        apply(
            &mut a.ledger,
            &a.store,
            &mut b.ledger,
            &b.store,
            tiny.reference(),
            &p,
            &p.authorize_handoff().unwrap()
        )
        .is_err()
    );
    assert_eq!(snapshot(&a.temp.path().join("store")), before);
    assert!(a.pin().exists());
}
#[test]
fn wrong_owner_object_domain_store_or_acknowledgement_cannot_form_a_plan() {
    let a = Endpoint::new();
    let b = Endpoint::new();
    let p = planned(&a, &b);
    for kind in 0..3 {
        let mut r = p.request.clone();
        match kind {
            0 => r.owner = key("other"),
            1 => r.source_domain = r.successor_domain.clone(),
            _ => r.successor = r.previous.clone(),
        };
        assert!(plan(&a.store, &r).is_err());
    }
    for other in [
        Endpoint::with(b"BBB", AckLevel::ProcessRestart),
        Endpoint::with(b"AAA", AckLevel::VerifiedLocal),
    ] {
        let mut r = p.request.clone();
        r.successor = other.grant;
        assert!(plan(&a.store, &r).is_err());
    }
}
#[test]
fn damaged_or_missing_successor_never_releases_the_old_claim() {
    for missing in [false, true] {
        let mut a = Endpoint::new();
        let mut b = Endpoint::new();
        let p = planned(&a, &b);
        if missing {
            fs::remove_file(b.pin()).unwrap();
        } else {
            fs::write(b.payload(), b"bad").unwrap();
        }
        assert!(run(&mut a, &mut b, &p, &p.authorize_handoff().unwrap()).is_err());
        assert!(a.pin().exists());
        assert!(!a.marker().exists());
        assert!(a.ledger.finish(key("finish"), &a.job).is_err());
    }
}
#[test]
fn missing_old_claim_without_a_marker_is_not_recovery_permission() {
    let mut a = Endpoint::new();
    let mut b = Endpoint::new();
    let p = planned(&a, &b);
    fs::remove_file(a.pin()).unwrap();
    assert!(run(&mut a, &mut b, &p, &p.authorize_handoff().unwrap()).is_err());
    assert!(!a.marker().exists());
    assert_eq!(fs::read(a.payload()).unwrap(), b"AAA");
}
#[test]
fn old_readers_block_handoff_without_blocking_unrelated_objects() {
    let mut a = Endpoint::new();
    let mut b = Endpoint::new();
    let p = planned(&a, &b);
    let held = super::super::verify(&mut a.ledger, &a.store, &a.grant).unwrap();
    assert!(run(&mut a, &mut b, &p, &p.authorize_handoff().unwrap()).is_err());
    assert!(a.pin().exists());
    let req = Request {
        operation: key("other"),
        owner: key("owner"),
        object: tig_core::Object::from_bytes(b"BBB"),
        required: AckLevel::ProcessRestart,
    };
    crate::publish(&mut a.store, &req, &mut Cursor::new(b"BBB")).unwrap();
    drop(held);
    run(&mut a, &mut b, &p, &p.authorize_handoff().unwrap()).unwrap();
}
#[test]
fn every_boundary_recovers_without_repeating_the_release_decision() {
    for cut in [
        Stage::Admitted,
        Stage::Bound,
        Stage::SuccessorVerified,
        Stage::Marked,
        Stage::Retired,
        Stage::Settled,
    ] {
        let mut a = Endpoint::new();
        let mut b = Endpoint::new();
        let p = planned(&a, &b);
        let auth = p.authorize_handoff().unwrap();
        assert!(
            apply_hook(
                &mut a.ledger,
                &a.store,
                &mut b.ledger,
                &b.store,
                &a.job,
                &p,
                &auth,
                |s| if s == cut {
                    Err(HandoffError::Injected)
                } else {
                    Ok(())
                }
            )
            .is_err()
        );
        a.reopen();
        b.reopen();
        if matches!(cut, Stage::Marked | Stage::Retired | Stage::Settled) {
            assert!(matches!(
                super::super::verify(&mut a.ledger, &a.store, &a.grant),
                Err(RetentionError::Released)
            ));
        }
        run(&mut a, &mut b, &p, &auth).unwrap();
        assert!(!a.pin().exists());
        assert!(b.pin().exists());
        assert_eq!(fs::read(a.payload()).unwrap(), b"AAA");
    }
}
#[test]
fn settled_replay_is_readonly_and_historical_after_custody_moves_on() {
    let mut a = Endpoint::new();
    let mut b = Endpoint::new();
    let mut c = Endpoint::new();
    let p = planned(&a, &b);
    let auth = p.authorize_handoff().unwrap();
    run(&mut a, &mut b, &p, &auth).unwrap();
    a.ledger.finish(key("finish"), &a.job).unwrap();
    let next = planned(&b, &c);
    run(&mut b, &mut c, &next, &next.authorize_handoff().unwrap()).unwrap();
    let before = snapshot(a.temp.path());
    let r = run(&mut a, &mut b, &p, &auth).unwrap();
    assert!(r.settlement.was_replayed());
    assert_eq!(snapshot(a.temp.path()), before);
    assert!(c.pin().exists());
}
#[test]
fn marked_but_unsettled_handoff_recovers_after_successor_moves_on() {
    let mut a = Endpoint::new();
    let mut b = Endpoint::new();
    let mut c = Endpoint::new();
    let p = planned(&a, &b);
    let auth = p.authorize_handoff().unwrap();
    assert!(
        apply_hook(
            &mut a.ledger,
            &a.store,
            &mut b.ledger,
            &b.store,
            &a.job,
            &p,
            &auth,
            |s| if s == Stage::Marked {
                Err(HandoffError::Injected)
            } else {
                Ok(())
            }
        )
        .is_err()
    );
    assert!(a.pin().exists());
    let next = planned(&b, &c);
    run(&mut b, &mut c, &next, &next.authorize_handoff().unwrap()).unwrap();
    run(&mut a, &mut b, &p, &auth).unwrap();
    assert!(!a.pin().exists());
    assert!(!b.pin().exists());
    assert!(c.pin().exists());
}
#[test]
fn released_operations_cannot_resurrect_and_other_claims_still_protect() {
    let mut a = Endpoint::new();
    let mut b = Endpoint::new();
    let mut r = a.grant.request().clone();
    r.operation = key("another");
    let extra = super::super::plan(&a.store, &r).unwrap();
    super::super::retain(&mut a.ledger, &a.store, &a.job, &extra).unwrap();
    let p = planned(&a, &b);
    run(&mut a, &mut b, &p, &p.authorize_handoff().unwrap()).unwrap();
    assert!(matches!(
        a.store.try_exclusive_object(a.object()),
        Err(Error::Retained)
    ));
    let old = a.grant.record.plan.clone();
    assert!(matches!(
        super::super::retain(&mut a.ledger, &a.store, &a.job, &old),
        Err(RetentionError::Released)
    ));
    assert!(!a.pin().exists());
}
#[test]
fn missing_or_damaged_settled_marker_is_never_rebuilt_under_spent_budget() {
    for corrupt in [false, true] {
        let mut a = Endpoint::new();
        let mut b = Endpoint::new();
        let p = planned(&a, &b);
        let auth = p.authorize_handoff().unwrap();
        run(&mut a, &mut b, &p, &auth).unwrap();
        a.ledger.finish(key("finish"), &a.job).unwrap();
        if corrupt {
            fs::write(a.marker(), b"{}").unwrap();
        } else {
            fs::remove_file(a.marker()).unwrap();
        }
        let before = snapshot(a.temp.path());
        assert!(run(&mut a, &mut b, &p, &auth).is_err());
        assert_eq!(snapshot(a.temp.path()), before);
    }
}
#[test]
fn changed_successor_requires_new_authorization_and_cannot_rewrite_history() {
    let mut a = Endpoint::new();
    let mut b = Endpoint::new();
    let mut c = Endpoint::new();
    let p = planned(&a, &b);
    let auth = p.authorize_handoff().unwrap();
    let mut next = planned(&a, &c);
    assert!(matches!(
        run(&mut a, &mut c, &next, &auth),
        Err(HandoffError::WrongAuthorization)
    ));
    run(&mut a, &mut b, &p, &auth).unwrap();
    next.request.operation = key("new-move");
    let before = fs::read(a.marker()).unwrap();
    assert!(matches!(
        run(&mut a, &mut c, &next, &next.authorize_handoff().unwrap()),
        Err(HandoffError::AlreadyReleased)
    ));
    assert_eq!(fs::read(a.marker()).unwrap(), before);
}
#[test]
fn reciprocal_handoffs_cannot_retire_both_custodians() {
    let mut a = Endpoint::new();
    let mut b = Endpoint::new();
    let pa = planned(&a, &b);
    let pb = planned(&b, &a);
    let prepare = |a: &mut Endpoint, b: &mut Endpoint, p: &HandoffPlan| {
        assert!(
            apply_hook(
                &mut a.ledger,
                &a.store,
                &mut b.ledger,
                &b.store,
                &a.job,
                p,
                &p.authorize_handoff().unwrap(),
                |s| if s == Stage::Bound {
                    Err(HandoffError::Injected)
                } else {
                    Ok(())
                }
            )
            .is_err()
        );
    };
    prepare(&mut a, &mut b, &pa);
    prepare(&mut b, &mut a, &pb);
    let (ready, wait) = std::sync::mpsc::channel();
    let mut releases = Vec::new();
    let mut workers = Vec::new();
    for (source, target, p) in [(&a, &b, pa.clone()), (&b, &a, pb)] {
        let mut sl =
            CapacityLedger::open(&source.temp.path().join("ledger"), &source.witness).unwrap();
        let ss = LocalStore::open(&source.temp.path().join("store")).unwrap();
        let mut tl =
            CapacityLedger::open(&target.temp.path().join("ledger"), &target.witness).unwrap();
        let ts = LocalStore::open(&target.temp.path().join("store")).unwrap();
        let job = source.job.clone();
        let signal = ready.clone();
        let (release, gate) = std::sync::mpsc::channel();
        releases.push(release);
        workers.push(std::thread::spawn(move || {
            apply_hook(
                &mut sl,
                &ss,
                &mut tl,
                &ts,
                &job,
                &p,
                &p.authorize_handoff().unwrap(),
                |s| {
                    if s == Stage::SuccessorVerified {
                        signal.send(()).map_err(|_| HandoffError::Injected)?;
                        gate.recv_timeout(std::time::Duration::from_secs(10))
                            .map_err(|_| HandoffError::Injected)?;
                    }
                    Ok(())
                },
            )
        }));
    }
    let overlap = wait.recv_timeout(std::time::Duration::from_secs(5)).is_ok()
        && wait.recv_timeout(std::time::Duration::from_secs(5)).is_ok();
    for release in releases {
        let _ = release.send(());
    }
    let mut wins = 0;
    let mut outcomes = Vec::new();
    for worker in workers {
        outcomes.push(worker.join().unwrap());
    }
    assert!(
        overlap,
        "both guarded successors must overlap before source acquisition"
    );
    for result in outcomes {
        match result {
            Ok(_) => wins += 1,
            Err(HandoffError::Retention(RetentionError::Store(Error::Busy))) => (),
            Err(e) => panic!("unexpected race outcome: {e}"),
        }
    }
    assert!(wins <= 1);
    assert!(a.pin().exists() || b.pin().exists());
    if wins == 0 {
        a.reopen();
        b.reopen();
        run(&mut a, &mut b, &pa, &pa.authorize_handoff().unwrap()).unwrap();
    }
    assert!(a.pin().exists() || b.pin().exists());
}
#[test]
fn bounded_plan_roundtrip_and_type_separation_preserve_the_contract() {
    let mut a = Endpoint::new();
    let mut b = Endpoint::new();
    let p = planned(&a, &b);
    let bytes = p.to_json().unwrap();
    let p = HandoffPlan::from_json(&bytes).unwrap();
    assert_eq!(p.to_json().unwrap(), bytes);
    assert!(HandoffPlan::from_json(&vec![b' '; 65_537]).is_err());
    let auth = p.authorize_handoff().unwrap();
    run(&mut a, &mut b, &p, &auth).unwrap();
    let (op, _) = keys(&p).unwrap();
    assert!(a.ledger.publication_reference(op).is_err());
}
#[test]
fn handoff_child() {
    let Ok(root) = std::env::var("TIG_HANDOFF_SOURCE") else {
        return;
    };
    let root = PathBuf::from(root);
    let target = PathBuf::from(std::env::var("TIG_HANDOFF_TARGET").unwrap());
    let cut = std::env::var("TIG_HANDOFF_CUT").unwrap();
    let witness: Checkpoint =
        serde_json::from_slice(&fs::read(root.join("witness.json")).unwrap()).unwrap();
    let tw: Checkpoint =
        serde_json::from_slice(&fs::read(target.join("witness.json")).unwrap()).unwrap();
    let job: JournalRef =
        serde_json::from_slice(&fs::read(root.join("job.json")).unwrap()).unwrap();
    let p = HandoffPlan::from_json(&fs::read(root.join("handoff.json")).unwrap()).unwrap();
    let mut sl = CapacityLedger::open(&root.join("ledger"), &witness).unwrap();
    let ss = LocalStore::open(&root.join("store")).unwrap();
    let mut tl = CapacityLedger::open(&target.join("ledger"), &tw).unwrap();
    let ts = LocalStore::open(&target.join("store")).unwrap();
    apply_hook(
        &mut sl,
        &ss,
        &mut tl,
        &ts,
        &job,
        &p,
        &p.authorize_handoff().unwrap(),
        |s| {
            if matches!(
                (s, cut.as_str()),
                (Stage::Marked, "marked")
                    | (Stage::Retired, "retired")
                    | (Stage::Settled, "settled")
            ) {
                std::process::exit(88);
            }
            Ok(())
        },
    )
    .unwrap();
    panic!("child must exit");
}
#[test]
fn actual_process_exit_keeps_successor_protected_and_recovers_exactly() {
    for cut in ["marked", "retired", "settled"] {
        let mut a = Endpoint::new();
        let mut b = Endpoint::new();
        let p = planned(&a, &b);
        for e in [&a, &b] {
            fs::write(
                e.temp.path().join("witness.json"),
                serde_json::to_vec(&e.witness).unwrap(),
            )
            .unwrap();
        }
        fs::write(
            a.temp.path().join("job.json"),
            serde_json::to_vec(&a.job).unwrap(),
        )
        .unwrap();
        fs::write(a.temp.path().join("handoff.json"), p.to_json().unwrap()).unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "retention::handoff::tests::handoff_child",
                "--nocapture",
            ])
            .env("TIG_HANDOFF_SOURCE", a.temp.path())
            .env("TIG_HANDOFF_TARGET", b.temp.path())
            .env("TIG_HANDOFF_CUT", cut)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(88));
        a.reopen();
        b.reopen();
        assert!(matches!(
            super::super::verify(&mut a.ledger, &a.store, &a.grant),
            Err(RetentionError::Released)
        ));
        run(&mut a, &mut b, &p, &p.authorize_handoff().unwrap()).unwrap();
        assert!(!a.pin().exists());
        assert!(matches!(
            b.store.try_exclusive_object(b.object()),
            Err(Error::Retained)
        ));
        assert_eq!(fs::read(a.payload()).unwrap(), b"AAA");
    }
}
