use super::*;

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn selection() -> Selection {
    Selection {
        root: Object::from_bytes(b"synthetic capacity fixture"),
        context: Digest::blob(b"capacity"),
    }
}
fn check(j: &mut SourceJournal) -> JournalCapacity {
    let before = j.checkpoint();
    let report = j.capacity().unwrap();
    assert_eq!(report.checkpoint, j.checkpoint());
    assert_eq!(before, j.checkpoint());
    assert!(!report.complete_claim_coverage);
    for p in [
        &report.sources,
        &report.commands,
        &report.command_body_bytes,
        &report.cancellations,
        &report.cancellation_body_bytes,
    ] {
        assert_eq!(p.limit, p.used + p.held + p.available);
    }
    let old = progress_budget::capacity(j).unwrap();
    assert_eq!(report.commands.available, old.available_events);
    assert_eq!(report.commands.held, old.held_events);
    assert_eq!(report.command_body_bytes.available, old.available_bytes);
    assert_eq!(report.command_body_bytes.held, old.held_bytes);
    report
}
#[test]
fn capacity_accounts_for_pending_cancellation_and_reopen_without_commands() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("journal");
    let mut j = SourceJournal::create(&root, key("project"), key("epoch")).unwrap();
    assert_eq!(check(&mut j).sources.available, MAX_SOURCES as u64);
    let genesis = j.checkpoint();
    let mut observer = SourceJournal::open(&root, &genesis).unwrap();
    j.apply(
        key("prepare"),
        Command::Prepare {
            source: key("asset"),
            owner: key("owner"),
            expected_revision: 0,
            selection: selection(),
        },
    )
    .unwrap();
    // Inspection catches up external writers but creates no event itself.
    let pending = observer.capacity().unwrap();
    assert_eq!(pending.checkpoint, j.checkpoint());
    assert_eq!(pending.pending_sources, 1);
    assert_eq!(pending.commands.used, 1);
    j.apply(
        key("cancel"),
        Command::Cancel {
            source: key("asset"),
            owner: key("owner"),
            expected_revision: 1,
        },
    )
    .unwrap();
    let report = check(&mut j);
    assert_eq!(report.sources.used, 1); // Cancelled identities still occupy a slot.
    assert_eq!(report.cancellations.used, 1);
    assert_eq!(report.commands.used, 1);
    assert!(report.cancellation_body_bytes.used > 0);
    let mut reopened = SourceJournal::open(&root, &report.checkpoint).unwrap();
    assert_eq!(report, check(&mut reopened));
    reopened.poisoned = true;
    assert_eq!(reopened.capacity(), Err(Error::NeedsRecovery));
}

#[test]
fn capacity_counts_both_reservation_pools_and_release_restores_unspent_room() {
    let t = tempfile::tempdir().unwrap();
    let mut j =
        SourceJournal::create(&t.path().join("journal"), key("project"), key("epoch")).unwrap();
    let p = j
        .apply(
            key("prepare"),
            Command::Prepare {
                source: key("parent"),
                owner: key("owner"),
                expected_revision: 0,
                selection: selection(),
            },
        )
        .unwrap();
    j.apply(
        key("adopt"),
        Command::Adopt {
            source: key("parent"),
            owner: key("owner"),
            expected_revision: 1,
            prepared: p.reference,
        },
    )
    .unwrap();
    let guards = vec![Guard {
        source: key("parent"),
        owner: key("owner"),
        expected_revision: 2,
    }];
    let progress = progress_budget::reserve(
        &mut j,
        key("progress"),
        key("owner"),
        key("work"),
        0,
        guards,
    )
    .unwrap();
    let planned = key("planned");
    let outcomes = outcome_budget::reserve(
        &mut j,
        key("outcomes"),
        key("owner"),
        outcome_budget::Plan {
            steps: vec![outcome_budget::Step {
                operation: planned.clone(),
                command: Box::new(Command::Prepare {
                    source: key("outcome-work"),
                    owner: key("owner"),
                    expected_revision: 0,
                    selection: selection(),
                }),
            }],
            admissions: vec![],
            endings: vec![vec![planned]],
        },
    )
    .unwrap();
    let held = check(&mut j);
    assert_eq!(held.sources.used, 1);
    assert_eq!(held.sources.held, 2);
    assert!(held.commands.held > progress_budget::EVENTS);
    progress_budget::release(&mut j, &progress).unwrap();
    assert_eq!(check(&mut j).sources.held, 1);
    outcome_budget::release(&mut j, &outcomes).unwrap();
    let released = check(&mut j);
    assert_eq!(released.sources.held, 0);
    assert_eq!(released.commands.held, 0);
    assert_eq!(released.command_body_bytes.held, 0);
}
