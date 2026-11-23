use super::*;
use std::io::Cursor;
use tig_core::Object;
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
struct Lab {
    temp: tempfile::TempDir,
    store: LocalStore,
    ledger: CapacityLedger,
    plan: PublicationPlan,
}
impl Lab {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let store = LocalStore::create(&temp.path().join("M"), key("M")).unwrap();
        let ledger =
            CapacityLedger::create(&temp.path().join("ledger"), key("volume"), key("epoch"), 0)
                .unwrap();
        let request = Request {
            operation: key("publish"),
            owner: key("owner"),
            object: Object::from_bytes(b"abcdef"),
            required: AckLevel::ProcessRestart,
        };
        let planned = plan(&store, &request).unwrap();
        Self {
            temp,
            store,
            ledger,
            plan: planned,
        }
    }
    fn sample(&mut self, id: &str, free: u64) {
        let p = self
            .ledger
            .begin_probe(key(&format!("probe-{id}")))
            .unwrap();
        self.ledger
            .observe(key(&format!("sample-{id}")), p.reference(), free)
            .unwrap();
    }
    fn reserve(&mut self) -> ledger::Receipt {
        self.sample("initial", 1 << 30);
        reserve_publication(&mut self.ledger, key("job"), &self.plan).unwrap()
    }
}
#[test]
fn fresh_admission_is_required_and_failed_capacity_does_not_create_a_bound_job() {
    let mut lab = Lab::new();
    assert!(matches!(
        reserve_publication(&mut lab.ledger, key("job"), &lab.plan),
        Err(AccountedError::Ledger(LedgerError::NeedsProbe))
    ));
    assert_eq!(lab.ledger.checkpoint().sequence(), 0);
    lab.sample("empty", 0);
    assert!(reserve_publication(&mut lab.ledger, key("job"), &lab.plan).is_err());
    assert_eq!(lab.ledger.checkpoint().sequence(), 2);
    lab.sample("available", 1 << 30);
    let receipt = reserve_publication(&mut lab.ledger, key("job"), &lab.plan).unwrap();
    assert!(!receipt.was_replayed());
    assert_eq!(lab.ledger.checkpoint().sequence(), 5);
    assert!(matches!(
        reserve_publication(&mut lab.ledger, key("another-job"), &lab.plan),
        Err(AccountedError::Ledger(LedgerError::NeedsProbe))
    ));
}
#[test]
fn bound_job_rejects_changed_same_size_plan_before_bytes_or_new_allocation() {
    let mut lab = Lab::new();
    let job = lab.reserve().reference().clone();
    let before = lab.ledger.checkpoint();
    for case in 0..4 {
        let mut request = lab.plan.request().clone();
        match case {
            0 => request.object = Object::from_bytes(b"ghijkl"),
            1 => request.operation = key("different-operation"),
            2 => request.owner = key("different-owner"),
            _ => request.required = AckLevel::VerifiedLocal,
        }
        let changed = plan(&lab.store, &request).unwrap();
        assert_eq!(changed.required_peak(), lab.plan.required_peak());
        assert!(matches!(
            reserve_publication(&mut lab.ledger, key("job"), &changed),
            Err(AccountedError::Ledger(LedgerError::Conflict))
        ));
        assert!(matches!(
            publish_accounted(
                &mut lab.ledger,
                &lab.store,
                &job,
                &changed,
                &mut Cursor::new(b"ghijkl")
            ),
            Err(AccountedError::Ledger(LedgerError::Conflict))
        ));
        assert_eq!(lab.ledger.checkpoint(), before);
    }
    publish_accounted(
        &mut lab.ledger,
        &lab.store,
        &job,
        &lab.plan,
        &mut Cursor::new(b"abcdef"),
    )
    .unwrap();
}
#[test]
fn bound_publication_budget_cannot_fund_generic_retention_or_handoff_work() {
    let mut lab = Lab::new();
    let job = lab.reserve().reference().clone();
    let before = lab.ledger.checkpoint();
    assert!(matches!(
        lab.ledger
            .begin_allocation(key("raw"), &job, key("event"), 1),
        Err(LedgerError::InvalidReference)
    ));
    assert!(matches!(
        lab.ledger.begin_retention(
            key("retain"),
            &job,
            key("owner"),
            Digest::blob(b"binding"),
            1
        ),
        Err(LedgerError::InvalidReference)
    ));
    assert!(matches!(
        lab.ledger.begin_handoff(
            key("handoff"),
            &job,
            key("owner"),
            Digest::blob(b"binding"),
            1
        ),
        Err(LedgerError::InvalidReference)
    ));
    assert_eq!(lab.ledger.checkpoint(), before);
    assert!(lab.ledger.retire_closed(&job).is_err());
    assert_eq!(lab.ledger.checkpoint(), before);
}
#[test]
fn completed_bound_work_reopens_finishes_retires_and_replays_without_renewal() {
    let mut lab = Lab::new();
    let job = lab.reserve().reference().clone();
    let original = publish_accounted(
        &mut lab.ledger,
        &lab.store,
        &job,
        &lab.plan,
        &mut Cursor::new(b"abcdef"),
    )
    .unwrap();
    lab.ledger.finish(key("finish"), &job).unwrap();
    assert_eq!(
        lab.ledger.retirable_reservations(4).unwrap(),
        vec![job.clone()]
    );
    lab.ledger.retire_closed(&job).unwrap();
    let checkpoint = lab.ledger.checkpoint();
    lab.ledger = CapacityLedger::open(&lab.temp.path().join("ledger"), &checkpoint).unwrap();
    let admitted = reserve_publication(&mut lab.ledger, key("job"), &lab.plan).unwrap();
    assert!(admitted.was_replayed());
    let replay = publish_accounted(
        &mut lab.ledger,
        &lab.store,
        &job,
        &lab.plan,
        &mut Cursor::new(b"not consumed"),
    )
    .unwrap();
    assert_eq!(original.allocation(), replay.allocation());
    assert_eq!(original.charged_footprint(), replay.charged_footprint());
    assert_eq!(lab.ledger.checkpoint(), checkpoint);
    assert!(lab.ledger.retirable_reservations(4).unwrap().is_empty());
}
#[test]
fn legacy_general_reservation_keeps_exact_wire_bytes_and_rejects_silent_upgrade() {
    let mut lab = Lab::new();
    lab.sample("legacy", 1 << 30);
    let job = lab
        .ledger
        .reserve(key("legacy-job"), key("owner"), 64)
        .unwrap()
        .reference()
        .clone();
    let conn = rusqlite::Connection::open_with_flags(
        lab.temp.path().join("ledger/ledger.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let body: Vec<u8> = conn
        .query_row(
            "SELECT body FROM events WHERE operation='legacy-job'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        body,
        br#"{"command":{"kind":"reserve","owner":"owner","peak":64},"operation":"legacy-job"}"#
    );
    let before = lab.ledger.checkpoint();
    assert!(matches!(
        reserve_publication(&mut lab.ledger, key("legacy-job"), &lab.plan),
        Err(AccountedError::Ledger(LedgerError::Conflict))
    ));
    assert_eq!(lab.ledger.checkpoint(), before);
    lab.ledger
        .begin_allocation(key("legacy-allocation"), &job, key("legacy-event"), 1)
        .unwrap();
}
