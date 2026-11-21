use super::super::super::tests::{Fixture, control, fixture, minimum};
use super::super::tests::{create, refresh};
use super::*;
use history::tests::assert_receipt;

mod rotations;
mod seal;
mod sources;

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn reference(l: &CapacityLedger, s: &str) -> JournalRef {
    JournalRef {
        genesis: l.identity().clone(),
        operation: key(s),
    }
}
fn activate(f: &mut Fixture) -> ActiveControl {
    install_fixture(f, true)
}
fn install_fixture(f: &mut Fixture, seal: bool) -> ActiveControl {
    let c = control(f);
    drop(create(f, &c));
    let root = f.ledger.root.clone();
    fs::create_dir(root.join("segments")).unwrap();
    fs::create_dir(root.join("controls")).unwrap();
    fs::rename(
        f._temp.path().join("tail.sqlite"),
        root.join(c.preparation.tail.relative_name().unwrap()),
    )
    .unwrap();
    if seal {
        activation::seal_fixture(&mut f.ledger, &c.seal.intent.to_json().unwrap()).unwrap();
    }
    fs::write(root.join(c.relative_name().unwrap()), c.to_json().unwrap()).unwrap();
    fs::write(root.join("domain.json"), c.to_json().unwrap()).unwrap();
    c
}

#[test]
fn a_wire_receipt_without_actual_source_guards_never_admits_a_writer() {
    let mut f = fixture();
    let c = install_fixture(&mut f, false);
    assert!(matches!(
        open(
            &f.ledger.root,
            &c,
            &minimum(&c),
            f.nodes.finished_reader().unwrap()
        ),
        Err(LedgerError::NeedsRecovery)
    ));
    assert_eq!(
        head(
            &connect(
                &f.ledger
                    .root
                    .join(c.preparation.tail.relative_name().unwrap())
            )
            .unwrap(),
            &c.preparation.tail
        )
        .unwrap()
        .0,
        c.preparation.tail.base
    );
}

#[test]
fn every_new_command_family_uses_existing_methods_and_retains_tail_receipts_after_reopen() {
    let mut f = fixture();
    let (ledger, _, _, _, _) = history::tests::fixture(&f._temp.path().join("new-kinds"), true);
    f.ledger = ledger;
    f.data.config = f.ledger.config.clone();
    refresh(&mut f);
    let c = activate(&mut f);
    let mut l = reopened(&f, &c, &minimum(&c));
    let job = reference(&l, "live-job");
    let old = reference(&l, "live-allocation");
    let binding = Digest::blob(b"new-binding");
    let reviewed = Digest::blob(b"new-review");
    let mut receipts = vec![
        l.observe_clock(key("tail-clock"), 2001, true).unwrap(),
        l.complete_allocation(key("tail-old-complete"), &old, 50)
            .unwrap(),
    ];
    let a = l
        .begin_allocation(key("tail-generic"), &job, key("tail-event"), 100)
        .unwrap();
    receipts.push(a.clone());
    receipts.push(
        l.complete_allocation(key("tail-generic-complete"), a.reference(), 0)
            .unwrap(),
    );
    let p = l
        .begin_publication(
            key("tail-publication"),
            &job,
            key("live-owner"),
            binding.clone(),
            100,
        )
        .unwrap();
    receipts.push(p.clone());
    receipts.push(
        l.settle_footprint(key("tail-published"), p.reference(), binding.clone(), 25)
            .unwrap(),
    );
    let r = l
        .begin_retention(
            key("tail-retention"),
            &job,
            key("live-owner"),
            binding.clone(),
            100,
        )
        .unwrap();
    receipts.push(r.clone());
    receipts.push(
        l.settle_retention(key("tail-retained"), r.reference(), binding.clone(), 30)
            .unwrap(),
    );
    let h = l
        .begin_handoff(
            key("tail-handoff"),
            &job,
            key("live-owner"),
            binding.clone(),
            100,
        )
        .unwrap();
    receipts.push(h.clone());
    receipts.push(
        l.settle_handoff(key("tail-handed"), h.reference(), binding.clone(), 35)
            .unwrap(),
    );
    let clean = l
        .begin_publication(
            key("tail-cleanup"),
            &job,
            key("live-owner"),
            binding.clone(),
            100,
        )
        .unwrap();
    receipts.push(clean.clone());
    receipts.push(
        l.begin_cleanup(clean.reference(), binding.clone(), reviewed.clone())
            .unwrap(),
    );
    assert!(matches!(
        l.settle_footprint(key("cancelled"), clean.reference(), binding.clone(), 0),
        Err(LedgerError::Cancelled)
    ));
    receipts.push(
        l.complete_cleanup(clean.reference(), binding.clone(), reviewed, 0)
            .unwrap(),
    );
    receipts.push(l.finish(key("tail-finish"), &job).unwrap());
    receipts.push(l.retire_closed(&job).unwrap());
    let probe = l.begin_fresh_probe(key("tail-probe")).unwrap();
    receipts.push(probe.clone());
    receipts.push(
        l.observe(key("tail-observe"), probe.reference(), 100000)
            .unwrap(),
    );
    receipts.push(l.reserve(key("tail-job"), key("owner"), 100).unwrap());
    receipts.push(l.invalidate(key("tail-invalidate")).unwrap());
    assert_eq!(
        l.publication_status(p.reference(), &binding).unwrap(),
        Some(25)
    );
    assert_eq!(
        l.retention_status(r.reference(), &binding).unwrap(),
        Some(30)
    );
    assert_eq!(l.handoff_status(h.reference(), &binding).unwrap(), Some(35));
    let requests: Vec<Request> = {
        let mut s = l
            .conn
            .prepare("SELECT body FROM events ORDER BY sequence")
            .unwrap();
        s.query_map([], |r| r.get::<_, Vec<u8>>(0))
            .unwrap()
            .map(|raw| decode(&raw.unwrap()).unwrap())
            .collect()
    };
    assert_eq!(requests.len(), receipts.len());
    let m = Minimum {
        checkpoint: l.checkpoint(),
        ..minimum(&c)
    };
    drop(l);
    let mut l = reopened(&f, &c, &m);
    for (request, receipt) in requests.into_iter().zip(&receipts) {
        assert_receipt(&issue(&mut l, request).unwrap(), receipt);
    }
    assert_eq!(l.clock_high_water().unwrap(), Some(2001));
    assert_eq!(
        l.publication_status(p.reference(), &binding).unwrap(),
        Some(25)
    );
    assert_eq!(
        l.retention_status(r.reference(), &binding).unwrap(),
        Some(30)
    );
    assert_eq!(l.handoff_status(h.reference(), &binding).unwrap(), Some(35));
    assert_eq!(
        l.cleanup_status(clean.reference(), &binding)
            .unwrap()
            .unwrap()
            .1,
        Some(0)
    );
    assert!(matches!(
        l.accounting(),
        Err(LedgerError::Capacity(
            capacity::CapacityError::UnknownObservation
        ))
    ));
}
fn reopened(f: &Fixture, c: &ActiveControl, m: &Minimum) -> CapacityLedger {
    open(&f.ledger.root, c, m, f.nodes.finished_reader().unwrap()).unwrap()
}
fn issue(l: &mut CapacityLedger, request: Request) -> LR<Receipt> {
    let genesis = l.identity().clone();
    let reference = |key: Key| JournalRef {
        genesis: genesis.clone(),
        operation: key,
    };
    let op = request.operation;
    match request.command {
        Command::ObserveClock { millis, strict } => l.observe_clock(op, millis, strict),
        Command::RetireClosed { reservation } => l.retire_closed(&reference(reservation)),
        Command::BeginHandoff {
            reservation,
            owner,
            binding,
            limit,
        } => l.begin_handoff(op, &reference(reservation), owner, binding, limit),
        Command::SettleHandoff {
            allocation,
            binding,
            footprint,
        } => l.settle_handoff(op, &reference(allocation), binding, footprint),
        Command::BeginRetention {
            reservation,
            owner,
            binding,
            limit,
        } => l.begin_retention(op, &reference(reservation), owner, binding, limit),
        Command::SettleRetention {
            allocation,
            binding,
            footprint,
        } => l.settle_retention(op, &reference(allocation), binding, footprint),
        Command::BeginPublication {
            reservation,
            owner,
            binding,
            limit,
        } => l.begin_publication(op, &reference(reservation), owner, binding, limit),
        Command::SettleFootprint {
            allocation,
            binding,
            footprint,
        } => l.settle_footprint(op, &reference(allocation), binding, footprint),
        Command::BeginCleanup {
            allocation,
            binding,
            reviewed,
        } => l.begin_cleanup(&reference(allocation), binding, reviewed),
        Command::CompleteCleanup {
            allocation,
            binding,
            reviewed,
            footprint,
        } => l.complete_cleanup(&reference(allocation), binding, reviewed, footprint),
        Command::Reserve { owner, peak } => l.reserve(op, owner, peak),
        Command::BeginAllocation {
            reservation,
            event,
            limit,
        } => l.begin_allocation(op, &reference(reservation), event, limit),
        Command::CompleteAllocation { allocation, actual } => {
            l.complete_allocation(op, &reference(allocation), actual)
        }
        Command::BeginProbe => l.begin_probe(op),
        Command::Observe { probe, free } => l.observe(op, &reference(probe), free),
        Command::Finish { reservation } => l.finish(op, &reference(reservation)),
        Command::Invalidate => l.invalidate(op),
    }
}
fn refresh_probe(l: &mut CapacityLedger, s: &str) {
    let p = l.begin_fresh_probe(key(&format!("probe-{s}"))).unwrap();
    l.observe(key(&format!("observe-{s}")), p.reference(), 100000)
        .unwrap();
}

#[test]
fn all_original_public_method_retries_statuses_and_paged_zero_survive_actual_sealed_open() {
    let mut f = fixture();
    let (ledger, receipts, _, zero, _) =
        history::tests::fixture(&f._temp.path().join("all-kinds"), true);
    f.ledger = ledger;
    f.data.config = f.ledger.config.clone();
    refresh(&mut f);
    let requests: Vec<Request> = {
        let mut s = f
            .ledger
            .conn
            .prepare("SELECT body FROM events ORDER BY sequence")
            .unwrap();
        s.query_map([], |r| r.get::<_, Vec<u8>>(0))
            .unwrap()
            .map(|raw| decode(&raw.unwrap()).unwrap())
            .collect()
    };
    let c = activate(&mut f);
    let mut l = reopened(&f, &c, &minimum(&c));
    let before = l.checkpoint();
    for (request, receipt) in requests.into_iter().zip(&receipts) {
        assert_receipt(&issue(&mut l, request).unwrap(), receipt);
    }
    assert_eq!(l.checkpoint(), before);
    assert!(l.outputs.is_empty());
    assert_eq!(l.clock_high_water().unwrap(), Some(2000));
    assert!(matches!(
        l.accounting(),
        Err(LedgerError::Capacity(
            capacity::CapacityError::UnknownObservation
        ))
    ));
    let binding = Digest::blob(b"binding");
    for (name, value) in [("publication", 25), ("retention", 30), ("handoff", 35)] {
        let r = reference(&l, name);
        let actual = match name {
            "publication" => l.publication_status(&r, &binding),
            "retention" => l.retention_status(&r, &binding),
            _ => l.handoff_status(&r, &binding),
        };
        assert_eq!(actual.unwrap(), Some(value));
    }
    assert_eq!(l.allocation_state(&zero).unwrap(), Some(0));
    let cleanup = reference(&l, "cleanup-publication");
    assert!(matches!(
        l.publication_status(&cleanup, &binding),
        Err(LedgerError::Cancelled)
    ));
    assert_eq!(
        l.cleanup_status(&cleanup, &binding).unwrap(),
        Some((Digest::blob(b"reviewed"), Some(0)))
    );
    assert!(
        l.allocation_reference(&key("publication"), false)
            .unwrap()
            .is_some()
    );
    assert!(
        l.allocation_reference(&key("retention"), true)
            .unwrap()
            .is_some()
    );
    assert!(l.handoff_reference(&key("handoff")).unwrap().is_some());
    assert!(matches!(
        l.allocation_reference(&key("probe-1"), false),
        Err(LedgerError::Conflict)
    ));
    assert!(l.handoff_reference(&key("missing")).unwrap().is_none());
    assert!(matches!(
        l.begin_fresh_probe(key("probe-1")),
        Err(LedgerError::NeedsProbe)
    ));
    let probe = reference(&l, "pending-probe");
    assert!(matches!(
        l.observe(key("old-probe-reuse"), &probe, 10000),
        Err(LedgerError::NeedsProbe)
    ));
    assert!(matches!(
        l.observe_clock(key("backward"), 1999, false),
        Err(LedgerError::Clock)
    ));
    assert!(CapacityLedger::open(&f.ledger.root, &before).is_err());
}

#[test]
fn actual_methods_write_complete_finish_retire_and_reopen_original_tail_receipts() {
    let mut f = fixture();
    let c = activate(&mut f);
    let mut l = reopened(&f, &c, &minimum(&c));
    let allocation = reference(&l, "allocation");
    let job = reference(&l, "job");
    let completed = l
        .complete_allocation(key("continued-complete"), &allocation, 50)
        .unwrap();
    let finished = l.finish(key("continued-finish"), &job).unwrap();
    assert_eq!(l.retirable_reservations(256).unwrap(), vec![job.clone()]);
    let retired = l.retire_closed(&job).unwrap();
    assert!(l.retirable_reservations(256).unwrap().is_empty());
    let m = Minimum {
        checkpoint: l.checkpoint(),
        ..minimum(&c)
    };
    drop(l);
    let mut l = reopened(&f, &c, &m);
    assert_receipt(
        &l.complete_allocation(key("continued-complete"), &allocation, 50)
            .unwrap(),
        &completed,
    );
    assert_receipt(&l.finish(key("continued-finish"), &job).unwrap(), &finished);
    assert_receipt(&l.retire_closed(&job).unwrap(), &retired);
    assert!(matches!(
        l.complete_allocation(key("continued-complete"), &allocation, 49),
        Err(LedgerError::Conflict)
    ));
    assert!(matches!(
        l.reserve(key("new-job"), key("owner"), 1),
        Err(LedgerError::NeedsProbe)
    ));
    refresh_probe(&mut l, "new");
    l.reserve(key("new-job"), key("owner"), 100).unwrap();
}

#[test]
fn reopened_peer_catches_up_valid_reserve_without_gaining_the_other_handles_probe() {
    let mut f = fixture();
    let c = activate(&mut f);
    let mut first = reopened(&f, &c, &minimum(&c));
    refresh_probe(&mut first, "first");
    let mut second = reopened(&f, &c, &minimum(&c));
    first.reserve(key("first-job"), key("owner"), 100).unwrap();
    second.check_history().unwrap();
    assert_eq!(second.last_replay_count(), 1);
    assert!(matches!(
        second.reserve(key("second-job"), key("owner"), 100),
        Err(LedgerError::NeedsProbe)
    ));
    let stale = reference(&second, "probe-first");
    assert!(matches!(
        second.observe(key("reused"), &stale, 10000),
        Err(LedgerError::NeedsProbe)
    ));
    refresh_probe(&mut second, "second");
    second
        .reserve(key("second-job"), key("owner"), 100)
        .unwrap();
    first.check_history().unwrap();
    assert_eq!(first.last_replay_count(), 3);
    assert_eq!(first.checkpoint(), second.checkpoint());
}

#[test]
fn reopened_peer_replays_later_observe_without_admitting_the_recorded_pending_ticket() {
    let mut f = fixture();
    let c = activate(&mut f);
    let mut first = reopened(&f, &c, &minimum(&c));
    let probe = first.begin_fresh_probe(key("pending-on-reopen")).unwrap();
    let mut second = reopened(&f, &c, &minimum(&c));
    first
        .observe(key("observed-after-reopen"), probe.reference(), 100000)
        .unwrap();
    second.check_history().unwrap();
    assert_eq!(second.last_replay_count(), 1);
    assert!(matches!(
        second.accounting(),
        Err(LedgerError::Capacity(
            capacity::CapacityError::UnknownObservation
        ))
    ));
    assert!(matches!(
        second.observe(key("stolen-observe"), probe.reference(), 100000),
        Err(LedgerError::NeedsProbe)
    ));
    assert!(matches!(
        second.reserve(key("stolen-reserve"), key("owner"), 100),
        Err(LedgerError::NeedsProbe)
    ));
    first
        .reserve(key("valid-peer-reserve"), key("owner"), 100)
        .unwrap();
    second.check_history().unwrap();
    assert_eq!(first.checkpoint(), second.checkpoint());
}

struct NoNodes;
impl NodeStore for NoNodes {
    fn get(&self, _: &Object) -> git_asset_core::tree::TreeResult<Option<Vec<u8>>> {
        panic!("legacy history must refuse before node IO")
    }
    fn put(&mut self, _: &[u8]) -> git_asset_core::tree::TreeResult<Object> {
        panic!("legacy history must refuse before node IO")
    }
}
#[test]
fn working_checkpoint_export_binds_current_global_state_but_legacy_history_export_refuses() {
    let mut f = fixture();
    let c = activate(&mut f);
    let mut l = reopened(&f, &c, &minimum(&c));
    let allocation = reference(&l, "allocation");
    l.complete_allocation(key("exported-complete"), &allocation, 50)
        .unwrap();
    let mut nodes = git_asset_core::tree::MemoryNodes::default();
    let exported = l
        .export_checkpoint(&mut nodes, state::Limits::default())
        .unwrap();
    assert_eq!(exported.descriptor().journal(), &l.checkpoint());
    let model = exported
        .descriptor()
        .state()
        .read(&mut nodes, state::Limits::default())
        .unwrap()
        .into_domain_for_context(
            l.config.domain.as_str(),
            l.config.instance.as_str(),
            l.config.floor,
        )
        .unwrap();
    let Output::Allocation(ticket) = f.ledger.outputs.get("allocation").unwrap() else {
        panic!()
    };
    assert_eq!(model.allocation_status(ticket).unwrap(), Some(50));
    assert_eq!(
        model.summary(),
        Err(capacity::CapacityError::UnknownObservation)
    );
    assert!(matches!(
        l.export_history(&mut NoNodes, ReadLimits::default()),
        Err(Error::Invalid)
    ));
}

#[test]
fn uncertain_commit_poison_keeps_original_retry_after_explicit_reopen() {
    for committed in [false, true] {
        let mut f = fixture();
        let c = activate(&mut f);
        let mut l = reopened(&f, &c, &minimum(&c));
        let before = l.checkpoint();
        let allocation = reference(&l, "allocation");
        let result = l.apply_hook(
            key("injected"),
            Command::CompleteAllocation {
                allocation: key("allocation"),
                actual: 50,
            },
            |stage| {
                if stage
                    == if committed {
                        Stage::Committed
                    } else {
                        Stage::Written
                    }
                {
                    Err(LedgerError::Injected)
                } else {
                    Ok(())
                }
            },
        );
        assert!(matches!(result, Err(LedgerError::Injected)));
        assert_eq!(l.checkpoint(), before);
        assert!(matches!(l.check_history(), Err(LedgerError::NeedsRecovery)));
        drop(l);
        let mut l = reopened(&f, &c, &minimum(&c));
        let replay = l
            .complete_allocation(key("injected"), &allocation, 50)
            .unwrap();
        assert_eq!(replay.was_replayed(), committed);
        assert_eq!(l.checkpoint().sequence, before.sequence + 1);
    }
}

#[test]
fn missing_archive_or_tail_or_unsealed_source_never_initializes_a_replacement() {
    for missing in ["archive", "tail", "seal"] {
        let mut f = fixture();
        let c = activate(&mut f);
        let path = match missing {
            "archive" => f.ledger.root.join(c.relative_name().unwrap()),
            "tail" => f
                .ledger
                .root
                .join(c.preparation.tail.relative_name().unwrap()),
            _ => f.ledger.root.join("ledger.sqlite"),
        };
        // Only an explicitly fresh synthetic fixture file is removed.
        let root = f.ledger.root.clone();
        if missing == "seal" {
            drop(f.ledger);
        }
        fs::remove_file(&path).unwrap();
        assert!(open(&root, &c, &minimum(&c), f.nodes.finished_reader().unwrap()).is_err());
        assert!(!path.exists());
    }
}

#[test]
fn peer_advance_before_catchup_adoption_keeps_all_cached_state_unchanged() {
    let mut f = fixture();
    let c = activate(&mut f);
    let mut first = reopened(&f, &c, &minimum(&c));
    let mut peer = reopened(&f, &c, &minimum(&c));
    let before = first.checkpoint();
    let old = reference(&peer, "allocation");
    peer.complete_allocation(key("peer-complete"), &old, 50)
        .unwrap();
    let result = sync_hook(&mut first, || {
        peer.invalidate(key("peer-after-snapshot")).map(|_| ())
    });
    assert!(matches!(result, Err(LedgerError::Conflict)));
    assert_eq!(first.checkpoint(), before);
    assert!(first.outputs.is_empty());
    assert!(!first.poisoned);
    let Output::Allocation(ticket) = f.ledger.outputs.get("allocation").unwrap() else {
        panic!()
    };
    assert_eq!(first.domain.allocation_status(ticket).unwrap(), None);
    first.check_history().unwrap();
    assert_eq!(first.checkpoint(), peer.checkpoint());
    assert_eq!(first.last_replay_count(), 2);
    assert_eq!(first.domain.allocation_status(ticket).unwrap(), Some(50));
}

#[test]
fn peer_advance_after_file_guard_and_dependency_proof_refuses_before_new_transition() {
    let mut f = fixture();
    let c = activate(&mut f);
    let mut first = reopened(&f, &c, &minimum(&c));
    let mut peer = reopened(&f, &c, &minimum(&c));
    let before = first.checkpoint();
    let result = first.apply_hook(
        key("ours"),
        Command::CompleteAllocation {
            allocation: key("allocation"),
            actual: 50,
        },
        |stage| {
            if stage == Stage::Guarded {
                peer.invalidate(key("peer-first"))?;
            }
            Ok(())
        },
    );
    assert!(matches!(result, Err(LedgerError::Conflict)));
    assert_eq!(first.checkpoint(), before);
    assert!(first.outputs.is_empty());
    assert!(!first.poisoned);
    let allocation = reference(&first, "allocation");
    let receipt = first
        .complete_allocation(key("ours"), &allocation, 50)
        .unwrap();
    assert!(!receipt.was_replayed());
    assert_eq!(receipt.checkpoint().sequence, before.sequence + 2);
}

#[test]
fn malformed_cached_prefix_or_header_holds_without_appending_or_empty_recovery() {
    for case in 0..2 {
        let mut f = fixture();
        let c = activate(&mut f);
        let mut l = reopened(&f, &c, &minimum(&c));
        l.invalidate(key("known")).unwrap();
        let before = l.checkpoint();
        if case == 0 {
            l.conn
                .execute(
                    "UPDATE events SET hash=?1",
                    [Digest::blob(b"wrong prefix").as_str()],
                )
                .unwrap();
        } else {
            l.conn
                .execute_batch("DROP TRIGGER git_asset_tail_UPDATE")
                .unwrap();
        }
        assert!(l.check_history().is_err());
        assert!(l.poisoned);
        assert_eq!(l.checkpoint(), before);
        assert!(matches!(
            l.invalidate(key("must-not-append")),
            Err(LedgerError::NeedsRecovery)
        ));
    }
}

#[test]
fn existing_methods_fill_a_real_continued_tail_after_full_original_and_reopen_old_receipts() {
    let mut f = fixture();
    while f.ledger.checkpoint().sequence < MAX_COMMANDS {
        f.ledger
            .invalidate(key(&format!("legacy-{}", f.ledger.checkpoint().sequence)))
            .unwrap();
    }
    refresh(&mut f);
    let c = activate(&mut f);
    let mut l = reopened(&f, &c, &minimum(&c));
    let allocation = reference(&l, "allocation");
    let first = l
        .complete_allocation(key("continued-complete"), &allocation, 50)
        .unwrap();
    for n in 1..MAX_COMMANDS {
        l.invalidate(key(&format!("method-{n}"))).unwrap();
    }
    assert_eq!(l.checkpoint().sequence, 32768);
    assert_eq!(l.outputs.len(), 16384);
    assert!(matches!(
        l.invalidate(key("beyond-local-cap")),
        Err(LedgerError::Budget)
    ));
    assert_receipt(
        &l.complete_allocation(key("continued-complete"), &allocation, 50)
            .unwrap(),
        &first,
    );
    let original = l.reserve(key("job"), key("owner"), 1000).unwrap();
    assert!(original.was_replayed());
    let m = Minimum {
        checkpoint: l.checkpoint(),
        ..minimum(&c)
    };
    eprintln!(
        "existing CapacityLedger methods committed actual global32768/local16384; bytes={}",
        l.bytes
    );
    drop(l);
    let mut l = reopened(&f, &c, &m);
    assert_eq!(l.checkpoint().sequence, 32768);
    assert_eq!(l.outputs.len(), 16384);
    assert_receipt(
        &l.complete_allocation(key("continued-complete"), &allocation, 50)
            .unwrap(),
        &first,
    );
    assert_receipt(
        &l.reserve(key("job"), key("owner"), 1000).unwrap(),
        &original,
    );
    let Output::Allocation(ticket) = f.ledger.outputs.get("allocation").unwrap() else {
        panic!()
    };
    assert_eq!(l.domain.allocation_status(ticket).unwrap(), Some(50));
    assert!(matches!(
        l.reserve(key("not-renewed"), key("owner"), 1),
        Err(LedgerError::NeedsProbe)
    ));
    assert!(matches!(
        l.begin_probe(key("no-spare-slot")),
        Err(LedgerError::Budget)
    ));
    let next = seal::intent(&l, &c);
    assert_eq!(next.source.sequence, 32768);
    activation::seal(&mut l, &next, |_| Ok(())).unwrap();
    activation::verify_seal(
        &l.conn,
        &next.to_json().unwrap(),
        &Digest::blob(&next.to_json().unwrap()),
    )
    .unwrap();
    assert_eq!(
        head(&l.conn, &c.preparation.tail).unwrap().0.sequence,
        32768
    );
    assert_eq!(l.outputs.len(), 16384);
}
