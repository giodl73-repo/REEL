use git_asset_core::capacity::*;
use std::sync::{Arc, Barrier, Mutex, mpsc};
use std::time::Duration;

fn domain(free: u64, floor: u64) -> CapacityDomain {
    let mut d = CapacityDomain::new("volume-a", "epoch-1", floor).unwrap();
    refresh(&mut d, free);
    d
}
fn refresh(d: &mut CapacityDomain, free: u64) {
    let ticket = d.begin_observation().unwrap();
    d.observe(ticket, free).unwrap();
}

#[test]
fn retired_closed_history_preserves_debits_and_fences_old_generations() {
    let mut d = domain(100, 0);
    let old = d.reserve("job", "owner", 30).unwrap();
    let ticket = d.begin_allocation(&old, "bytes", 30).unwrap();
    d.complete_allocation(&ticket, 20).unwrap();
    d.finish(&old).unwrap();
    let before = d.summary().unwrap();
    let history = d.retire_closed(&old).unwrap();
    assert_eq!(d.summary().unwrap(), before);
    assert_eq!(history.allocation_status(&ticket).unwrap(), Some(20));
    assert_eq!(d.finish(&old), Err(CapacityError::Stale));
    let new = d.reserve("job", "owner", 30).unwrap();
    assert_ne!(new, old);
    assert_eq!(
        d.begin_allocation(&old, "late", 1).unwrap_err(),
        CapacityError::Stale
    );
    let new_ticket = d.begin_allocation(&new, "bytes", 30).unwrap();
    assert_eq!(
        history.allocation_status(&new_ticket),
        Err(CapacityError::Stale)
    );
    refresh(&mut d, 80);
    assert_eq!(d.summary().unwrap().allocated_after_watermark, 0);
    assert_eq!(d.summary().unwrap().reserved_remaining, 30);
}

#[test]
fn retirement_refuses_open_and_inflight_records_and_rolls_back_when_staged() {
    let mut d = domain(100, 0);
    let token = d.reserve("job", "owner", 30).unwrap();
    assert_eq!(
        d.retire_closed(&token).unwrap_err(),
        CapacityError::InFlight
    );
    let ticket = d.begin_allocation(&token, "bytes", 30).unwrap();
    assert_eq!(
        d.retire_closed(&token).unwrap_err(),
        CapacityError::InFlight
    );
    d.complete_allocation(&ticket, 10).unwrap();
    assert_eq!(
        d.retire_closed(&token).unwrap_err(),
        CapacityError::InFlight
    );
    d.finish(&token).unwrap();
    let before = d.summary().unwrap();
    let result: Result<()> = d.try_transition(|staged| {
        staged.retire_closed(&token)?;
        Err(CapacityError::Invalid)
    });
    assert_eq!(result, Err(CapacityError::Invalid));
    assert!(d.is_closed(&token).unwrap());
    assert_eq!(d.summary().unwrap(), before);
    d.retire_closed(&token).unwrap();
}

#[test]
fn retirement_reuses_bounded_working_slots_without_losing_debit_bounds() {
    let mut d = domain(100_000, 0);
    for i in 0..MAX_EVENTS {
        let token = d.reserve(&format!("job-{i}"), "owner", 1).unwrap();
        let ticket = d.begin_allocation(&token, "bytes", 1).unwrap();
        d.complete_allocation(&ticket, 1).unwrap();
        d.finish(&token).unwrap();
        d.retire_closed(&token).unwrap();
    }
    assert_eq!(
        d.summary().unwrap().allocated_after_watermark,
        MAX_EVENTS as u64
    );
    let token = d.reserve("next", "owner", 1).unwrap();
    let ticket = d.begin_allocation(&token, "bytes", 1).unwrap();
    assert_eq!(
        d.complete_allocation(&ticket, 1),
        Err(CapacityError::Budget)
    );
    refresh(&mut d, 90_000);
    d.complete_allocation(&ticket, 1).unwrap();
    assert_eq!(d.summary().unwrap().allocated_after_watermark, 1);
}

#[test]
fn unknown_capacity_cannot_admit_work() {
    let mut d = CapacityDomain::new("volume-a", "epoch-1", 10).unwrap();
    assert_eq!(
        d.reserve("job", "owner", 1),
        Err(CapacityError::UnknownObservation)
    );
    assert!(CapacityDomain::new("", "epoch", 0).is_err());
}
#[test]
fn known_headroom_subtracts_floor_and_only_remaining_reservations() {
    let mut d = domain(1000, 400);
    d.reserve("preview", "alpha", 80).unwrap();
    d.reserve("export", "alpha", 105).unwrap();
    assert_eq!(d.summary().unwrap().available, 415);
    d.reserve("batch", "alpha", 240).unwrap();
    assert_eq!(d.summary().unwrap().available, 175);
    assert_eq!(
        d.reserve("too-large", "beta", 176),
        Err(CapacityError::Insufficient)
    );
}
#[test]
fn reservation_replay_is_idempotent_and_conflicting_reuse_is_rejected() {
    let mut d = domain(100, 0);
    let token = d.reserve("job", "owner", 80).unwrap();
    assert_eq!(d.reserve("job", "owner", 80).unwrap(), token);
    assert_eq!(d.reserve("job", "other", 80), Err(CapacityError::Conflict));
    assert_eq!(d.reserve("job", "owner", 81), Err(CapacityError::Conflict));
    assert_eq!(d.summary().unwrap().available, 20);
}
#[test]
fn idle_worktrees_do_not_reserve_all_possible_job_peaks() {
    let mut d = domain(100, 10);
    assert_eq!(d.reserve("idle", "owner", 0), Err(CapacityError::Invalid));
    assert_eq!(d.summary().unwrap().available, 90);
}
#[test]
fn in_flight_allocation_keeps_its_whole_reservation_and_blocks_close() {
    let mut d = domain(1000, 100);
    let r = d.reserve("job", "owner", 200).unwrap();
    d.begin_allocation(&r, "write", 150).unwrap();
    assert_eq!(d.summary().unwrap().reserved_remaining, 200);
    assert_eq!(d.summary().unwrap().available, 700);
    assert_eq!(d.finish(&r), Err(CapacityError::InFlight));
    assert_eq!(
        d.begin_allocation(&r, "another", 51).unwrap_err(),
        CapacityError::Insufficient
    );
}
#[test]
fn completed_allocation_transfers_charge_without_a_gap_or_permanent_double_count() {
    let mut d = domain(1000, 100);
    let r = d.reserve("job", "owner", 200).unwrap();
    let a = d.begin_allocation(&r, "write", 100).unwrap();
    d.complete_allocation(&a, 100).unwrap();
    let s = d.summary().unwrap();
    assert_eq!(
        (
            s.reserved_remaining,
            s.allocated_after_watermark,
            s.available
        ),
        (100, 100, 700)
    );
    refresh(&mut d, 900);
    let s = d.summary().unwrap();
    assert_eq!(
        (
            s.reserved_remaining,
            s.allocated_after_watermark,
            s.available
        ),
        (100, 0, 700)
    );
}
#[test]
fn allocation_racing_a_probe_is_conservatively_charged_until_next_observation() {
    let mut d = domain(1000, 100);
    let r = d.reserve("job", "owner", 200).unwrap();
    let a = d.begin_allocation(&r, "write", 100).unwrap();
    let before = d.begin_observation().unwrap();
    d.complete_allocation(&a, 100).unwrap();
    // This sample already saw the write, but the earlier watermark did not.
    d.observe(before, 900).unwrap();
    assert_eq!(d.summary().unwrap().available, 600);
    refresh(&mut d, 900);
    assert_eq!(d.summary().unwrap().available, 700);
}
#[test]
fn stale_duplicate_and_invalidated_probe_tickets_cannot_restore_admission() {
    let mut d = domain(1000, 100);
    let old = d.begin_observation().unwrap();
    let new = d.begin_observation().unwrap();
    assert_eq!(d.observe(old, 1000), Err(CapacityError::Stale));
    d.observe(new.clone(), 800).unwrap();
    assert_eq!(d.observe(new, 1000), Err(CapacityError::Stale));
    let old = d.begin_observation().unwrap();
    d.invalidate_observation();
    assert_eq!(d.observe(old, 1000), Err(CapacityError::Stale));
    assert_eq!(
        d.reserve("job", "owner", 1),
        Err(CapacityError::UnknownObservation)
    );
    refresh(&mut d, 50);
    assert_eq!(d.summary().unwrap().available, 0);
}
#[test]
fn tokens_cannot_cross_domains_or_restart_epochs() {
    let mut a = domain(100, 0);
    let r = a.reserve("job", "owner", 10).unwrap();
    let mut b = CapacityDomain::new("volume-b", "epoch-1", 0).unwrap();
    let mut restarted = CapacityDomain::new("volume-a", "epoch-2", 0).unwrap();
    assert_eq!(b.finish(&r), Err(CapacityError::Stale));
    assert_eq!(restarted.finish(&r), Err(CapacityError::Stale));
    let ticket = a.begin_observation().unwrap();
    assert_eq!(restarted.observe(ticket, 100), Err(CapacityError::Stale));
}
#[test]
fn allocation_events_retry_once_and_reject_changed_limits_or_results() {
    let mut d = domain(100, 0);
    let r = d.reserve("job", "owner", 80).unwrap();
    let a = d.begin_allocation(&r, "write", 40).unwrap();
    assert_eq!(
        d.begin_allocation(&r, "write", 41).unwrap_err(),
        CapacityError::Conflict
    );
    let seq = d.complete_allocation(&a, 30).unwrap();
    assert_eq!(d.complete_allocation(&a, 30).unwrap(), seq);
    assert_eq!(d.complete_allocation(&a, 31), Err(CapacityError::Conflict));
    assert_eq!(d.summary().unwrap().available, 20);
}
#[test]
fn closing_a_job_releases_unused_headroom_without_refunding_stored_bytes() {
    let mut d = domain(1000, 100);
    let r = d.reserve("job", "owner", 200).unwrap();
    let a = d.begin_allocation(&r, "write", 100).unwrap();
    d.complete_allocation(&a, 80).unwrap();
    d.finish(&r).unwrap();
    d.finish(&r).unwrap();
    assert_eq!(d.summary().unwrap().available, 820);
    assert_eq!(d.reserve("job", "owner", 200), Err(CapacityError::Closed));
    assert_eq!(
        d.begin_allocation(&r, "new-write", 1).unwrap_err(),
        CapacityError::Closed
    );
    assert_eq!(d.complete_allocation(&a, 80).unwrap(), 1);
    refresh(&mut d, 920);
    assert_eq!(d.summary().unwrap().available, 820);
}
#[test]
fn uncertain_partial_write_never_expires_or_becomes_spendable() {
    let mut d = domain(100, 0);
    let r = d.reserve("job", "owner", 100).unwrap();
    d.begin_allocation(&r, "uncertain", 100).unwrap();
    refresh(&mut d, 75);
    assert_eq!(d.finish(&r), Err(CapacityError::InFlight));
    assert_eq!(
        d.reserve("other", "owner", 1),
        Err(CapacityError::Insufficient)
    );
}
#[test]
fn unsigned_boundary_accounting_cannot_wrap_to_free_space() {
    let mut d = domain(u64::MAX, 0);
    let r = d.reserve("job", "owner", u64::MAX).unwrap();
    let a = d.begin_allocation(&r, "write", u64::MAX).unwrap();
    d.complete_allocation(&a, u64::MAX).unwrap();
    assert_eq!(d.summary().unwrap().available, 0);
    assert_eq!(
        d.reserve("extra", "owner", 1),
        Err(CapacityError::Insufficient)
    );
    refresh(&mut d, 0);
    d.finish(&r).unwrap();
    assert_eq!(d.summary().unwrap().available, 0);
}
#[test]
fn retained_tombstones_and_event_history_have_explicit_bounds() {
    let mut d = domain(10_000, 0);
    for n in 0..MAX_RESERVATIONS {
        let r = d.reserve(&format!("job-{n}"), "owner", 1).unwrap();
        d.finish(&r).unwrap();
    }
    assert_eq!(d.reserve("extra", "owner", 1), Err(CapacityError::Budget));
    let mut d = domain(100, 0);
    let r = d.reserve("job", "owner", 1).unwrap();
    for n in 0..MAX_EVENTS {
        let a = d.begin_allocation(&r, &format!("write-{n}"), 1).unwrap();
        d.complete_allocation(&a, 0).unwrap();
    }
    assert_eq!(
        d.begin_allocation(&r, "extra", 1).unwrap_err(),
        CapacityError::Budget
    );
}
#[test]
fn aliases_share_one_atomic_admission_state() {
    let d = Arc::new(Mutex::new(domain(100, 0)));
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|n| {
            let alias = d.clone();
            let start = barrier.clone();
            std::thread::spawn(move || {
                start.wait();
                alias
                    .lock()
                    .unwrap()
                    .reserve(&format!("job-{n}"), "owner", 80)
            })
        })
        .collect();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(d.lock().unwrap().summary().unwrap().available, 20);
}
#[test]
fn busy_domain_does_not_hold_an_independent_domain() {
    let a = Arc::new(Mutex::new(domain(100, 0)));
    let b = Arc::new(Mutex::new(
        CapacityDomain::new("volume-b", "epoch-1", 0).unwrap(),
    ));
    refresh(&mut b.lock().unwrap(), 100);
    let held = a.lock().unwrap();
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let admitted = b.lock().unwrap().reserve("job", "owner", 80).is_ok();
        tx.send(admitted).unwrap();
    });
    assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap());
    drop(held);
    worker.join().unwrap();
}
#[test]
fn oversize_completion_is_atomic_and_zero_disposition_can_close() {
    let mut d = domain(100, 0);
    let r = d.reserve("job", "owner", 80).unwrap();
    let a = d.begin_allocation(&r, "write", 40).unwrap();
    assert_eq!(d.complete_allocation(&a, 41), Err(CapacityError::Conflict));
    assert_eq!(d.finish(&r), Err(CapacityError::InFlight));
    d.complete_allocation(&a, 0).unwrap();
    d.finish(&r).unwrap();
    assert_eq!(d.summary().unwrap().available, 100);
}

#[test]
fn staged_batch_failure_rolls_back_earlier_changes_and_success_adopts_once() {
    let mut d = domain(100, 0);
    let failed = d.try_transition(|staged| {
        staged.reserve("one", "owner", 80)?;
        staged.reserve("two", "owner", 80)
    });
    assert_eq!(failed, Err(CapacityError::Insufficient));
    assert_eq!(d.summary().unwrap().reserved_remaining, 0);
    let token = d
        .try_transition(|staged| staged.reserve("one", "owner", 80))
        .unwrap();
    assert_eq!(d.summary().unwrap().reserved_remaining, 80);
    d.finish(&token).unwrap();
    assert_eq!(d.summary().unwrap().available, 100);
}
