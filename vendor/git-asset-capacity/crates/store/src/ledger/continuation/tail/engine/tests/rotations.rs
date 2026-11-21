//! Full physical scale with manual synthetic publication, not controller recovery.
use super::*;
use crate::ledger::continuation::tests::fixture_with_limits;
use crate::metadata::Limits as MetadataLimits;

fn fill(l: &mut CapacityLedger, prefix: &str) {
    for n in 1..MAX_COMMANDS {
        l.invalidate(key(&format!("{prefix}-{n}"))).unwrap();
    }
    assert_eq!(l.outputs.len(), MAX_COMMANDS as usize);
    assert!(matches!(
        l.invalidate(key(&format!("{prefix}-overfull"))),
        Err(LedgerError::Budget)
    ));
}

#[test]
fn full_actual_three_generations_reach_49153_with_original_receipts_and_inflight_state() {
    let mut f = fixture_with_limits(MetadataLimits {
        nodes: 1048576,
        bytes: 512 * 1024 * 1024,
        database_bytes: 768 * 1024 * 1024,
        min_free_bytes: 65536,
    });
    while f.ledger.checkpoint().sequence < MAX_COMMANDS {
        f.ledger
            .invalidate(key(&format!(
                "full-original-{}",
                f.ledger.checkpoint().sequence
            )))
            .unwrap();
    }
    let original = f.ledger.reserve(key("job"), key("owner"), 1000).unwrap();
    refresh(&mut f);
    eprintln!("full rotations original16384 exported");
    let c1 = activate(&mut f);
    let mut l1 = reopened(&f, &c1, &minimum(&c1));
    let old_allocation = reference(&l1, "allocation");
    let first = l1
        .complete_allocation(key("full-first-completion"), &old_allocation, 50)
        .unwrap();
    fill(&mut l1, "full-first");
    assert_eq!(l1.checkpoint().sequence, 32768);
    let m1 = Minimum {
        checkpoint: l1.checkpoint(),
        ..minimum(&c1)
    };
    eprintln!(
        "full rotations generation1 local16384/global32768 bytes={}",
        l1.bytes
    );
    let c2 = sources::next(&mut f, &c1, &mut l1);
    assert!(matches!(
        l1.invalidate(key("full-stale-one")),
        Err(LedgerError::NeedsRecovery)
    ));
    let mut l2 = reopened(&f, &c2, &m1);
    assert_eq!(l2.outputs.len(), 0);
    assert_receipt(
        &l2.complete_allocation(key("full-first-completion"), &old_allocation, 50)
            .unwrap(),
        &first,
    );
    let job = reference(&l2, "job");
    let pending = l2
        .begin_allocation(
            key("full-second-pending"),
            &job,
            key("full-second-event"),
            75,
        )
        .unwrap();
    let Output::Allocation(ticket) = l2.outputs.get("full-second-pending").unwrap() else {
        panic!()
    };
    let ticket = ticket.clone();
    fill(&mut l2, "full-second");
    assert_eq!(l2.checkpoint().sequence, 49152);
    let m2 = Minimum {
        checkpoint: l2.checkpoint(),
        ..minimum(&c2)
    };
    eprintln!(
        "full rotations generation2 local16384/global49152 bytes={}",
        l2.bytes
    );
    let c3 = sources::next(&mut f, &c2, &mut l2);
    assert!(matches!(
        l2.invalidate(key("full-stale-two")),
        Err(LedgerError::NeedsRecovery)
    ));
    let mut l3 = reopened(&f, &c3, &m2);
    assert_eq!(l3.outputs.len(), 0);
    assert_eq!(l3.config, f.ledger.config);
    assert_eq!(l3.domain.allocation_status(&ticket).unwrap(), None);
    assert_receipt(
        &l3.reserve(key("job"), key("owner"), 1000).unwrap(),
        &original,
    );
    assert_receipt(
        &l3.complete_allocation(key("full-first-completion"), &old_allocation, 50)
            .unwrap(),
        &first,
    );
    assert_receipt(
        &l3.begin_allocation(
            key("full-second-pending"),
            &job,
            key("full-second-event"),
            75,
        )
        .unwrap(),
        &pending,
    );
    let last = l3
        .complete_allocation(key("full-third-completion"), pending.reference(), 0)
        .unwrap();
    assert_eq!(l3.checkpoint().sequence, 49153);
    assert_eq!(l3.domain.allocation_status(&ticket).unwrap(), Some(0));
    let m3 = Minimum {
        checkpoint: l3.checkpoint(),
        ..minimum(&c3)
    };
    drop(l3);
    let mut l3 = reopened(&f, &c3, &m3);
    assert_eq!(l3.outputs.len(), 1);
    assert_eq!(l3.domain.allocation_status(&ticket).unwrap(), Some(0));
    assert_receipt(
        &l3.reserve(key("job"), key("owner"), 1000).unwrap(),
        &original,
    );
    assert_receipt(
        &l3.complete_allocation(key("full-first-completion"), &old_allocation, 50)
            .unwrap(),
        &first,
    );
    assert_receipt(
        &l3.complete_allocation(key("full-third-completion"), pending.reference(), 0)
            .unwrap(),
        &last,
    );
    assert!(matches!(
        l3.reserve(key("full-no-renewal"), key("owner"), 1),
        Err(LedgerError::NeedsProbe)
    ));
    let ancestor = reopened(&f, &c3, &minimum(&c1));
    assert_eq!(ancestor.checkpoint(), l3.checkpoint());
    eprintln!(
        "full rotations finished actual49153, original/intermediate/latest exact replies, pending-to-zero and local output1"
    );
}
