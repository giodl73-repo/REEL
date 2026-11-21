use super::*;
use crate::final_retirement::provider::{Clock, TimeBounds, Window};
use std::time::Duration;
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn d(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn policy() -> Policy {
    Policy {
        authority: d("authority"),
        epoch: d("epoch"),
        boundary: d("native-boundary"),
        max_sample_millis: 20,
        max_drift_millis: 5,
        max_session_millis: 1000,
    }
}
fn ledger(root: &std::path::Path, policy: Policy) -> CapacityLedger {
    CapacityLedger::create_clock(root, key("clock"), key("epoch"), 0, policy).unwrap()
}
fn port() -> (tempfile::TempDir, LocalClock, Enrollment) {
    let root = tempfile::tempdir().unwrap();
    let ledger = ledger(&root.path().join("clock"), policy());
    let enrollment = ledger.clock_enrollment().unwrap();
    let port = LocalClock::new(ledger, enrollment.clone()).unwrap();
    (root, port, enrollment)
}
fn at(base: Instant, wall: u64, millis: u64) -> Sample {
    Sample {
        wall,
        before: base + Duration::from_millis(millis),
        after: base + Duration::from_millis(millis),
    }
}
fn scripted(port: &LocalClock, id: &Digest, samples: &[Sample]) -> Result<TimeBounds> {
    let mut samples = samples.iter().copied();
    let result = port.sample_with(id, || Ok(samples.next().expect("unexpected extra sample")));
    if result.is_ok() {
        assert!(samples.next().is_none());
    }
    result
}
#[test]
fn enrollment_is_explicit_immutable_and_status_is_not_a_clock_observation() {
    let (root, port, e) = port();
    assert_eq!(port.status().unwrap().checkpoint.sequence(), 0);
    assert!(!port.status().unwrap().current_time_authority);
    assert!(matches!(
        port.sample(&d("foreign")),
        Err(Error::WrongEnrollment)
    ));
    assert_eq!(port.status().unwrap().checkpoint.sequence(), 0);
    let legacy =
        CapacityLedger::create(&root.path().join("legacy"), key("legacy"), key("epoch"), 0)
            .unwrap();
    assert!(LocalClock::new(legacy, e.clone()).is_err());
    let other = ledger(&root.path().join("other"), policy());
    assert_ne!(
        other.clock_enrollment().unwrap().id().unwrap(),
        e.id().unwrap()
    );
    assert!(matches!(
        LocalClock::new(other, e),
        Err(Error::WrongEnrollment)
    ));
    let mut invalid = policy();
    invalid.max_sample_millis = 0;
    assert!(
        CapacityLedger::create_clock(
            &root.path().join("invalid"),
            key("bad"),
            key("epoch"),
            0,
            invalid
        )
        .is_err()
    );
    assert!(!root.path().join("invalid").exists());
}
#[test]
fn accepted_floor_survives_reopen_and_equal_old_time_cannot_start_a_new_invocation() {
    let (root, port, e) = port();
    let id = e.id().unwrap();
    let base = Instant::now();
    let bounds = scripted(&port, &id, &[at(base, 100, 0), at(base, 101, 1)]).unwrap();
    assert_eq!(
        bounds,
        TimeBounds {
            earliest: 100,
            latest: 125
        }
    );
    let status = port.status().unwrap();
    assert_eq!(status.accepted_lower_bound, Some(100));
    assert_eq!(status.checkpoint.sequence(), 1);
    drop(port);
    let reopened = LocalClock::new(
        CapacityLedger::open(&root.path().join("clock"), &status.checkpoint).unwrap(),
        e,
    )
    .unwrap();
    assert!(matches!(
        scripted(&reopened, &id, &[at(base, 100, 2)]),
        Err(Error::Ledger(LedgerError::Clock))
    ));
    assert_eq!(reopened.status().unwrap().checkpoint.sequence(), 1);
    assert!(scripted(&reopened, &id, &[at(base, 101, 3), at(base, 102, 4)]).is_ok());
    assert_eq!(reopened.status().unwrap().accepted_lower_bound, Some(101));
}
#[test]
fn backward_wall_monotonic_delay_and_drift_grant_no_time_even_after_commit() {
    let base = Instant::now();
    let bad_mono = Sample {
        wall: 100,
        before: base + Duration::from_millis(1),
        after: base,
    };
    for (samples, floor) in [
        (vec![bad_mono], None),
        (vec![at(base, 100, 0), at(base, 99, 1)], Some(100)),
        (vec![at(base, 100, 0), at(base, 121, 21)], Some(100)),
        (vec![at(base, 100, 0), at(base, 130, 1)], Some(100)),
        (vec![at(base, 100, 1), at(base, 101, 0)], Some(100)),
        (
            vec![
                Sample {
                    wall: 100,
                    before: base,
                    after: base + Duration::from_millis(10),
                },
                at(base, 101, 5),
            ],
            Some(100),
        ),
        (vec![at(base, u64::MAX, 0), at(base, u64::MAX, 1)], None),
    ] {
        let (_root, port, e) = port();
        let result = scripted(&port, &e.id().unwrap(), &samples);
        assert!(
            matches!(result, Err(Error::Time)),
            "expected floor {floor:?}: {result:?}"
        );
        assert_eq!(port.status().unwrap().accepted_lower_bound, floor);
        assert!(matches!(
            port.sample_with(&e.id().unwrap(), || panic!("failed session sampled again")),
            Err(Error::Time)
        ));
    }
}
#[test]
fn invocation_drift_and_duration_are_checked_against_the_original_anchor() {
    for (wall, mark, expired) in [(101, 10, false), (1101, 1001, true)] {
        let (_root, port, e) = port();
        let base = Instant::now();
        let id = e.id().unwrap();
        scripted(&port, &id, &[at(base, 100, 0), at(base, 101, 1)]).unwrap();
        let result = scripted(&port, &id, &[at(base, wall, mark)]);
        assert!(if expired {
            matches!(result, Err(Error::Expired))
        } else {
            matches!(result, Err(Error::Time))
        });
        assert_eq!(port.status().unwrap().checkpoint.sequence(), 1);
    }
}
#[test]
fn busy_clock_scope_does_not_block_another_authority_and_retry_uses_a_new_sample() {
    let (root, port, e) = port();
    let base = Instant::now();
    let id = e.id().unwrap();
    let conn = rusqlite::Connection::open(root.path().join("clock/ledger.sqlite")).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(matches!(
        scripted(&port, &id, &[at(base, 100, 0)]),
        Err(Error::Ledger(LedgerError::Busy))
    ));
    let other = ledger(&root.path().join("other"), policy());
    let other_e = other.clock_enrollment().unwrap();
    let other = LocalClock::new(other, other_e.clone()).unwrap();
    scripted(
        &other,
        &other_e.id().unwrap(),
        &[at(base, 100, 0), at(base, 101, 1)],
    )
    .unwrap();
    conn.execute_batch("ROLLBACK").unwrap();
    scripted(&port, &id, &[at(base, 102, 2), at(base, 103, 3)]).unwrap();
    assert_eq!(port.status().unwrap().accepted_lower_bound, Some(102));
}
#[test]
fn whole_clock_interval_must_fit_the_window_and_minimum() {
    struct B(TimeBounds);
    impl Clock for B {
        fn now(&self, _: &Digest) -> provider::Result<u64> {
            panic!("point clock bypassed interval")
        }
        fn bounds(&self, _: &Digest) -> provider::Result<TimeBounds> {
            Ok(self.0)
        }
    }
    let w = Window {
        not_before: 100,
        expires: 200,
    };
    for bounds in [
        TimeBounds {
            earliest: 99,
            latest: 101,
        },
        TimeBounds {
            earliest: 100,
            latest: 200,
        },
        TimeBounds {
            earliest: 150,
            latest: 149,
        },
    ] {
        assert!(w.check_clock(&B(bounds), &d("clock"), 0).is_err());
    }
    let b = B(TimeBounds {
        earliest: 100,
        latest: 199,
    });
    assert_eq!(w.check_clock(&b, &d("clock"), 100).unwrap(), 100);
    assert!(w.check_clock(&b, &d("clock"), 101).is_err());
}
#[test]
fn actual_native_os_samples_persist_and_supply_a_bounded_window() {
    let root = tempfile::tempdir().unwrap();
    let mut p = policy();
    p.max_sample_millis = 5000;
    p.max_drift_millis = 1000;
    p.max_session_millis = 60000;
    let ledger = ledger(&root.path().join("clock"), p);
    let e = ledger.clock_enrollment().unwrap();
    let port = LocalClock::new(ledger, e.clone()).unwrap();
    let id = e.id().unwrap();
    let first = port.sample(&id).unwrap();
    assert!(first.earliest > 0);
    assert!(first.latest > first.earliest);
    let w = Window {
        not_before: first.earliest.saturating_sub(6000),
        expires: first.latest + 30000,
    };
    assert!(w.check_clock(&port, &id, first.earliest).unwrap() >= first.earliest);
    let status = port.status().unwrap();
    assert_eq!(status.checkpoint.sequence(), 2);
    assert!(!status.current_time_authority);
}
