use super::super::tests::{fixture, fixture_with_limits};
use super::*;
use crate::metadata::Limits as MetadataLimits;

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn budget(error: Error) -> bool {
    matches!(
        error,
        Error::Budget | Error::State(state::Error::Budget) | Error::Journal(LedgerError::Budget)
    )
}
fn intent(ledger: &CapacityLedger) -> SealIntent {
    SealIntent {
        schema: "tig.capacity-activation-intent.design-1".into(),
        operation: key("another-operation"),
        authority: ledger.identity().clone(),
        generation: 1,
        source: ledger.checkpoint(),
        prepared: Object::from_bytes(b"explicit synthetic existing preparation"),
        predecessor: None,
    }
}

#[test]
fn actual_nonempty_and_clocked_preparation_preserves_context_replies_and_pending_state() {
    for clock in [false, true] {
        let mut f = fixture();
        if clock {
            f.ledger = history::tests::fixture(&f._temp.path().join("clocked"), true).0;
        }
        f.nodes.finish().unwrap();
        f.nodes.reset_metrics();
        let before = f.ledger.checkpoint();
        let high = f.ledger.clock.high;
        let ticket = f
            .ledger
            .outputs
            .values()
            .find_map(|o| match o {
                Output::Allocation(t)
                    if matches!(f.ledger.domain.allocation_status(t), Ok(None)) =>
                {
                    Some(t.clone())
                }
                _ => None,
            })
            .unwrap();
        let ready = legacy(
            &mut f.ledger,
            key("prepare"),
            &mut f.nodes,
            history::index::build_limits(),
        )
        .unwrap();
        assert_eq!(ready.preparation.metadata, f.nodes.checkpoint());
        assert_eq!(ready.preparation.tail.base, before);
        assert_eq!(ready.data.source, before);
        assert_eq!(ready.data.config, f.ledger.config);
        assert_eq!(ready.data.clock_high, high);
        assert_eq!(ready.data.generation, 1);
        assert_eq!(ready.data.predecessor, None);
        assert!(ready.metrics.reads > f.nodes.metrics().node_gets as usize);
        assert!(ready.metrics.writes > f.nodes.metrics().puts as usize);
        let mut reader = f.nodes.finished_reader().unwrap();
        let index = Index::from_roots(
            ready.data.config.clone(),
            before.clone(),
            ready.data.operations.clone(),
            ready.data.sequences.clone(),
        )
        .unwrap();
        assert!(matches!(
            index
                .inspect(&key("job"), &mut reader, ReadLimits::default())
                .unwrap()
                .unwrap()
                .receipt()
                .response(),
            Response::Reserved
        ));
        let mut model = ready
            .data
            .working_state
            .read(&mut reader, state::Limits::default())
            .unwrap()
            .into_domain_for_context(
                f.ledger.config.domain.as_str(),
                f.ledger.config.instance.as_str(),
                f.ledger.config.floor,
            )
            .unwrap();
        assert_eq!(model.allocation_status(&ticket).unwrap(), None);
        model.complete_allocation(&ticket, 0).unwrap();
        assert_eq!(model.allocation_status(&ticket).unwrap(), Some(0));
        assert!(matches!(
            model.reserve("new", "owner", 1),
            Err(capacity::CapacityError::UnknownObservation)
        ));
        assert_eq!(f.ledger.checkpoint(), before);
        assert!(!f.ledger.root.join("segments").exists());
        assert!(!f.ledger.root.join("controls").exists());
        unsealed(&f.ledger).unwrap();
        f.ledger.invalidate(key("ordinary-progress")).unwrap();
    }
}

#[test]
fn preflight_budget_refuses_before_any_metadata_io() {
    let mut f = fixture();
    let before = f.nodes.finish().unwrap();
    f.nodes.reset_metrics();
    let result = legacy(
        &mut f.ledger,
        key("no-room"),
        &mut f.nodes,
        TreeLimits {
            max_reads: 0,
            ..history::index::build_limits()
        },
    );
    assert!(matches!(result, Err(Error::Budget)));
    assert_eq!(f.nodes.metrics().node_gets, 0);
    assert_eq!(f.nodes.metrics().puts, 0);
    assert_eq!(f.nodes.checkpoint(), before);
    unsealed(&f.ledger).unwrap();
}

#[test]
fn continued_preparation_refuses_a_legacy_handle_before_metadata_io() {
    let mut f = fixture();
    f.nodes.reset_metrics();
    let m = Minimum {
        schema: "tig.capacity-successor-minimum.design-1".into(),
        authority: f.ledger.identity().clone(),
        activation: ActivationIdentity {
            generation: 1,
            digest: Digest::blob(b"not-admitted"),
        },
        checkpoint: f.ledger.checkpoint(),
    };
    assert!(matches!(
        continued(
            &mut f.ledger,
            key("wrong-mode"),
            &m,
            &mut f.nodes,
            history::index::build_limits()
        ),
        Err(Error::Invalid)
    ));
    assert_eq!(f.nodes.metrics().node_gets, 0);
    assert_eq!(f.nodes.metrics().puts, 0);
}

#[test]
fn combined_budget_includes_export_nested_proof_readback_and_control_work() {
    let mut f = fixture();
    f.nodes.finish().unwrap();
    let exact = legacy(
        &mut f.ledger,
        key("prepare"),
        &mut f.nodes,
        history::index::build_limits(),
    )
    .unwrap()
    .metrics;
    for field in 0..4 {
        let mut f = fixture();
        f.nodes.finish().unwrap();
        let before = f.ledger.checkpoint();
        let mut limits = history::index::build_limits();
        match field {
            0 => limits.max_reads = exact.reads - 1,
            1 => limits.max_writes = exact.writes - 1,
            2 => limits.max_read_bytes = exact.read_bytes - 1,
            _ => limits.max_write_bytes = exact.write_bytes - 1,
        }
        let error = legacy(&mut f.ledger, key("prepare"), &mut f.nodes, limits)
            .err()
            .unwrap();
        assert!(budget(error));
        assert_eq!(f.ledger.checkpoint(), before);
        unsealed(&f.ledger).unwrap();
    }
}

#[test]
fn a_real_peer_write_during_provider_work_conflicts_without_holding_source_sql() {
    let mut f = fixture();
    let before = f.ledger.checkpoint();
    let mut peer = CapacityLedger::open(&f.ledger.root, &before).unwrap();
    let result = legacy_hook(
        &mut f.ledger,
        key("prepare"),
        &mut f.nodes,
        history::index::build_limits(),
        |stage| {
            if stage == Stage::Materialized {
                peer.invalidate(key("peer-during-metadata"))?;
            }
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::Journal(LedgerError::Conflict))));
    assert_eq!(f.ledger.checkpoint(), peer.checkpoint());
    unsealed(&f.ledger).unwrap();
}

#[test]
fn already_sealed_partial_controls_and_sealing_during_readback_refuse_without_new_preparation() {
    for case in 0..3 {
        let mut f = fixture();
        let before = f.ledger.checkpoint();
        let mut peer = CapacityLedger::open(&f.ledger.root, &before).unwrap();
        let existing = intent(&peer);
        if case == 0 {
            activation::seal(&mut peer, &existing, |_| Ok(())).unwrap();
        } else if case == 1 {
            f.ledger
                .conn
                .execute_batch("CREATE TABLE git_asset_activation_partial(value INTEGER)")
                .unwrap();
        }
        f.nodes.reset_metrics();
        let result = legacy_hook(
            &mut f.ledger,
            key("prepare"),
            &mut f.nodes,
            history::index::build_limits(),
            |stage| {
                if case == 2 && stage == Stage::Finished {
                    activation::seal(&mut peer, &existing, |_| Ok(()))?;
                }
                Ok(())
            },
        );
        assert!(matches!(
            result,
            Err(Error::Journal(LedgerError::NeedsRecovery))
        ));
        assert_eq!(f.ledger.checkpoint(), before);
        if case != 2 {
            assert_eq!(f.nodes.metrics().puts, 0);
            assert_eq!(f.nodes.metrics().node_gets, 0);
        }
    }
}

#[test]
fn a_full_original_legacy_preparation_uses_one_shared_budget_and_physical_readback() {
    let mut f = fixture_with_limits(MetadataLimits {
        nodes: 1048576,
        bytes: 512 * 1024 * 1024,
        database_bytes: 768 * 1024 * 1024,
        min_free_bytes: 65536,
    });
    while f.ledger.checkpoint().sequence < MAX_COMMANDS {
        f.ledger
            .invalidate(key(&format!(
                "prepare-original-{}",
                f.ledger.checkpoint().sequence
            )))
            .unwrap();
    }
    let before = f.ledger.checkpoint();
    let ready = legacy(
        &mut f.ledger,
        key("full-prepare"),
        &mut f.nodes,
        history::index::build_limits(),
    )
    .unwrap();
    assert_eq!(ready.data.source, before);
    assert_eq!(ready.data.operation_count, MAX_COMMANDS);
    assert_eq!(f.ledger.checkpoint(), before);
    assert!(matches!(
        f.ledger.invalidate(key("no-source-slot")),
        Err(LedgerError::Budget)
    ));
    unsealed(&f.ledger).unwrap();
    eprintln!("full legacy shared preparation: {:?}", ready.metrics);
}
