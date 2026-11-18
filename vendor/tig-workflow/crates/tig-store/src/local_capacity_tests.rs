use super::*;
use crate::{
    AckLevel, Request,
    accounted::{plan, publish_accounted},
    ledger::{Checkpoint, Response},
};
use std::{fs, io::Cursor, path::PathBuf, time::Duration};
use tempfile::TempDir;

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
struct Fixture {
    _temp: TempDir,
    store: LocalStore,
    ledger: CapacityLedger,
    ledger_root: PathBuf,
    witness: Checkpoint,
}
impl Fixture {
    fn new(floor: u64) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let store = LocalStore::create(&temp.path().join("store"), key("place")).unwrap();
        let ledger_root = temp.path().join("ledger");
        let ledger =
            CapacityLedger::create(&ledger_root, key("volume"), key("epoch"), floor).unwrap();
        let witness = ledger.checkpoint();
        Self {
            _temp: temp,
            store,
            ledger,
            ledger_root,
            witness,
        }
    }
    fn binding(&mut self) -> LocalCapacityProbe {
        LocalCapacityProbe::bind(&self.store, &mut self.ledger).unwrap()
    }
}
fn observe(f: &mut Fixture, b: &LocalCapacityProbe, suffix: &str) -> CapacityObservation {
    b.observe(
        &f.store,
        &mut f.ledger,
        key(&format!("probe-{suffix}")),
        key(&format!("sample-{suffix}")),
    )
    .unwrap()
}

#[test]
fn actual_user_available_space_can_admit_and_publish_without_a_fixed_free_value() {
    let mut f = Fixture::new(0);
    let b = f.binding();
    let o = observe(&mut f, &b, "one");
    assert_eq!(o.store(), f.store.identity());
    assert_eq!(o.ledger(), f.ledger.identity());
    assert!(o.sample().available <= o.sample().total);
    assert!(o.sample().allocation_granularity > 0);
    assert!(!o.receipt().was_replayed());
    assert!(
        matches!(o.receipt().response(), Response::Observed(s) if s.available == o.sample().available)
    );
    let request = Request {
        operation: key("publish"),
        owner: key("owner"),
        object: tig_core::Object::from_bytes(b"synthetic"),
        required: AckLevel::ProcessRestart,
    };
    let p = plan(&f.store, &request).unwrap();
    let j = f
        .ledger
        .reserve(key("job"), key("owner"), p.required_peak())
        .unwrap();
    let r = publish_accounted(
        &mut f.ledger,
        &f.store,
        j.reference(),
        &p,
        &mut Cursor::new(b"synthetic"),
    )
    .unwrap();
    assert!(r.charged_footprint() <= p.required_peak());
    f.ledger.finish(key("finish"), j.reference()).unwrap();
}

#[test]
fn measured_capacity_respects_an_unaffordable_safety_floor() {
    let mut f = Fixture::new(u64::MAX / 4096);
    let b = f.binding();
    observe(&mut f, &b, "one");
    assert!(matches!(
        f.ledger.reserve(key("job"), key("owner"), 1),
        Err(LedgerError::Capacity(
            tig_core::capacity::CapacityError::Insufficient
        ))
    ));
}

#[test]
fn replayed_probe_never_runs_the_sampler_or_creates_fresh_permission() {
    let mut f = Fixture::new(0);
    let b = f.binding();
    observe(&mut f, &b, "one");
    let before = f.ledger.checkpoint().sequence();
    assert!(matches!(
        b.observe_with(
            &f.store,
            &mut f.ledger,
            key("probe-one"),
            key("sample-other"),
            || panic!("historical probe must not sample")
        ),
        Err(ProbeError::Ledger(LedgerError::NeedsProbe))
    ));
    assert_eq!(f.ledger.checkpoint().sequence(), before);
    assert!(matches!(
        f.ledger.reserve(key("job"), key("owner"), 1),
        Err(LedgerError::NeedsProbe)
    ));
}

#[test]
fn same_labels_do_not_allow_a_different_store_instance() {
    let mut f = Fixture::new(0);
    let b = f.binding();
    let other = LocalStore::create(&f._temp.path().join("other"), key("place")).unwrap();
    let before = f.ledger.checkpoint().sequence();
    assert!(matches!(
        b.observe(&other, &mut f.ledger, key("probe"), key("sample")),
        Err(ProbeError::WrongEnrollment)
    ));
    assert_eq!(f.ledger.checkpoint().sequence(), before);
}

#[test]
fn same_domain_labels_do_not_allow_a_different_ledger_instance() {
    let mut f = Fixture::new(0);
    let b = f.binding();
    let mut other = CapacityLedger::create(
        &f._temp.path().join("other"),
        key("volume"),
        key("epoch"),
        0,
    )
    .unwrap();
    assert!(matches!(
        b.observe(&f.store, &mut other, key("probe"), key("sample")),
        Err(ProbeError::WrongEnrollment)
    ));
    assert_eq!(other.checkpoint().sequence(), 0);
}

#[test]
fn reopen_retains_enrollment_but_requires_a_new_probe_operation() {
    let mut f = Fixture::new(0);
    let b = f.binding();
    observe(&mut f, &b, "one");
    f.ledger = CapacityLedger::open(&f.ledger_root, &f.witness).unwrap();
    assert!(matches!(
        b.observe(&f.store, &mut f.ledger, key("probe-one"), key("sample-one")),
        Err(ProbeError::Ledger(LedgerError::NeedsProbe))
    ));
    observe(&mut f, &b, "two");
    f.ledger.reserve(key("job"), key("owner"), 1).unwrap();
}

#[test]
fn failed_sampling_cannot_spend_this_handles_previous_freshness() {
    let mut f = Fixture::new(0);
    let b = f.binding();
    observe(&mut f, &b, "one");
    assert!(matches!(
        b.observe_with(
            &f.store,
            &mut f.ledger,
            key("probe-fail"),
            key("sample-fail"),
            || Err(ProbeError::Store(crate::Error::Unsupported))
        ),
        Err(ProbeError::Store(crate::Error::Unsupported))
    ));
    assert!(matches!(
        f.ledger.reserve(key("job"), key("owner"), 1),
        Err(LedgerError::NeedsProbe)
    ));
    observe(&mut f, &b, "retry");
    f.ledger.reserve(key("job"), key("owner"), 1).unwrap();
}

#[test]
fn invalid_statistics_never_commit_an_observation() {
    for (i, sample) in [
        SpaceSample {
            available: 2,
            total: 1,
            allocation_granularity: 4096,
        },
        SpaceSample {
            available: 1,
            total: 2,
            allocation_granularity: 0,
        },
    ]
    .into_iter()
    .enumerate()
    {
        let mut f = Fixture::new(0);
        let b = f.binding();
        assert!(matches!(
            b.observe_with(
                &f.store,
                &mut f.ledger,
                key(&format!("probe-{i}")),
                key("sample"),
                || Ok(sample)
            ),
            Err(ProbeError::InvalidSample)
        ));
        assert_eq!(f.ledger.checkpoint().sequence(), 1);
        assert!(matches!(
            f.ledger.reserve(key("job"), key("owner"), 1),
            Err(LedgerError::NeedsProbe)
        ));
    }
}

#[test]
fn slow_measurement_is_rejected_by_the_existing_probe_deadline() {
    let mut f = Fixture::new(0);
    let b = f.binding();
    assert!(matches!(
        b.observe_with(&f.store, &mut f.ledger, key("probe"), key("sample"), || {
            std::thread::sleep(Duration::from_millis(5100));
            Ok(f.store.sample_space()?)
        }),
        Err(ProbeError::Ledger(LedgerError::NeedsProbe))
    ));
    assert!(matches!(
        f.ledger.reserve(key("job"), key("owner"), 1),
        Err(LedgerError::NeedsProbe)
    ));
}

#[test]
fn store_mutation_during_sampling_rejects_before_observation() {
    let mut f = Fixture::new(0);
    let b = f.binding();
    let path = f._temp.path().join("store/store.json");
    assert!(
        b.observe_with(&f.store, &mut f.ledger, key("probe"), key("sample"), || {
            let sample = f.store.sample_space()?;
            fs::write(path, b"{}").unwrap();
            Ok(sample)
        })
        .is_err()
    );
    assert_eq!(f.ledger.checkpoint().sequence(), 1);
    assert!(matches!(
        f.ledger.reserve(key("job"), key("owner"), 1),
        Err(LedgerError::NeedsProbe)
    ));
}

#[test]
fn sampling_holds_no_db_transaction_and_racing_allocation_remains_charged() {
    let mut f = Fixture::new(0);
    let b = f.binding();
    observe(&mut f, &b, "initial");
    let request = Request {
        operation: key("publish"),
        owner: key("owner"),
        object: tig_core::Object::from_bytes(b"racing"),
        required: AckLevel::ProcessRestart,
    };
    let p = plan(&f.store, &request).unwrap();
    let job = f
        .ledger
        .reserve(key("job"), key("owner"), p.required_peak())
        .unwrap();
    let mut writer = CapacityLedger::open(&f.ledger_root, &f.witness).unwrap();
    let mut charged = 0;
    b.observe_with(
        &f.store,
        &mut f.ledger,
        key("probe-race"),
        key("sample-race"),
        || {
            charged = publish_accounted(
                &mut writer,
                &f.store,
                job.reference(),
                &p,
                &mut Cursor::new(b"racing"),
            )
            .unwrap()
            .charged_footprint();
            Ok(f.store.sample_space()?)
        },
    )
    .unwrap();
    assert!(charged > 0);
    assert_eq!(
        f.ledger.accounting().unwrap().allocated_after_watermark,
        charged
    );
    observe(&mut f, &b, "coherent");
    assert_eq!(f.ledger.accounting().unwrap().allocated_after_watermark, 0);
}

#[test]
fn different_places_enrolled_on_one_ledger_share_reserved_headroom() {
    let mut f = Fixture::new(0);
    let b = f.binding();
    observe(&mut f, &b, "one");
    f.ledger.reserve(key("first"), key("owner"), 1).unwrap();
    let other = LocalStore::create(&f._temp.path().join("other"), key("other")).unwrap();
    let another = LocalCapacityProbe::bind(&other, &mut f.ledger).unwrap();
    let o = another
        .observe(&other, &mut f.ledger, key("probe-two"), key("sample-two"))
        .unwrap();
    assert!(matches!(
        f.ledger
            .reserve(key("second"), key("owner"), o.sample().available),
        Err(LedgerError::Capacity(
            tig_core::capacity::CapacityError::Insufficient
        ))
    ));
    assert_eq!(f.ledger.accounting().unwrap().reserved_remaining, 1);
}
