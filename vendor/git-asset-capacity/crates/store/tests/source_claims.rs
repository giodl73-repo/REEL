use git_asset_core::{Digest, Object};
use git_asset_store::{
    Key,
    source_claims::{
        self, Checkpoint, Command, Error, PreviewDisposition, Reference, Role, Selection,
        SourceJournal, Stage,
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command as Process,
    sync::{Arc, Barrier},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn selection(s: &str) -> Selection {
    Selection {
        root: Object::from_bytes(s.as_bytes()),
        context: Digest::blob(format!("context-{s}").as_bytes()),
    }
}
fn prepare(source: &str, expected: u64, s: &str) -> Command {
    Command::Prepare {
        source: key(source),
        owner: key("owner"),
        expected_revision: expected,
        selection: selection(s),
    }
}
fn adopt(source: &str, expected: u64, reference: Reference) -> Command {
    Command::Adopt {
        source: key(source),
        owner: key("owner"),
        expected_revision: expected,
        prepared: reference,
    }
}
fn fixture() -> (TempDir, PathBuf, SourceJournal) {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("journal");
    let j = SourceJournal::create(&root, key("project"), key("epoch")).unwrap();
    (t, root, j)
}

#[test]
fn batch_observation_preserves_full_states_absence_and_readonly_authority() {
    let (_temp, root, mut journal) = fixture();
    let minimum = journal.checkpoint();
    selected(&mut journal, "main", 0, "A");
    selected(&mut journal, "main", 2, "B");
    journal.apply(key("next"), prepare("main", 4, "C")).unwrap();
    journal
        .apply(key("draft"), prepare("draft", 0, "D"))
        .unwrap();
    let expected_main = journal.source(&key("main")).unwrap();
    let expected_draft = journal.source(&key("draft")).unwrap();
    let latest = journal.checkpoint();
    drop(journal);
    let database = root.join("claims.sqlite");
    let before = fs::read(&database).unwrap();
    let permissions = fs::metadata(&database).unwrap().permissions();
    let mut readonly = permissions.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&database, readonly).unwrap();
    let result =
        SourceJournal::observe_many(&root, &minimum, &[key("main"), key("absent"), key("draft")]);
    fs::set_permissions(&database, permissions).unwrap();
    let observed = result.unwrap();
    assert_eq!(observed.checkpoint, latest);
    assert_eq!(observed.sources.len(), 3);
    assert_eq!(observed.sources["main"], expected_main);
    assert_eq!(observed.sources["draft"], expected_draft);
    assert_eq!(observed.sources["absent"], None);
    assert_eq!(fs::read(database).unwrap(), before);
    assert_eq!(fs::read_dir(root).unwrap().count(), 2);
}

#[test]
fn batch_observation_refuses_invalid_selection_and_untrusted_authority() {
    let (_temp, root, mut journal) = fixture();
    let initial = journal.checkpoint();
    let backup = fs::read(root.join("claims.sqlite")).unwrap();
    journal
        .apply(key("first"), prepare("main", 0, "A"))
        .unwrap();
    let latest = journal.checkpoint();
    drop(journal);
    let missing = root.join("missing");
    for sources in [vec![], (0..65).map(|n| key(&format!("s-{n}"))).collect()] {
        assert_eq!(
            SourceJournal::observe_many(&missing, &initial, &sources),
            Err(Error::Budget)
        );
    }
    assert_eq!(
        SourceJournal::observe_many(&missing, &initial, &[key("a"), key("a")]),
        Err(Error::InvalidReference)
    );
    assert!(!missing.exists());
    assert!(SourceJournal::observe_many(&missing, &initial, &[key("a")]).is_err());
    assert_eq!(
        SourceJournal::observe_many(
            &root,
            &initial,
            &(0..64).map(|n| key(&format!("s-{n}"))).collect::<Vec<_>>()
        )
        .unwrap()
        .sources
        .len(),
        64
    );
    let (_foreign_temp, foreign_root, foreign) = fixture();
    drop(foreign);
    assert_eq!(
        SourceJournal::observe_many(&foreign_root, &latest, &[key("main")]),
        Err(Error::InvalidReference)
    );
    fs::write(root.join("claims.sqlite"), backup).unwrap();
    assert_eq!(
        SourceJournal::observe_many(&root, &latest, &[key("main")]),
        Err(Error::Rollback)
    );
    fs::write(root.join("claims.sqlite"), b"corrupt database").unwrap();
    assert!(SourceJournal::observe_many(&root, &initial, &[key("main")]).is_err());
}

#[test]
fn batch_observation_matches_one_checkpoint_while_two_sources_advance() {
    let (_temp, root, journal) = fixture();
    let minimum = journal.checkpoint();
    drop(journal);
    let writer_root = root.clone();
    let writer_minimum = minimum.clone();
    let gate = Arc::new(Barrier::new(2));
    let writer_gate = gate.clone();
    let writer = thread::spawn(move || {
        let mut journal = SourceJournal::open(&writer_root, &writer_minimum).unwrap();
        writer_gate.wait();
        for n in 0..16 {
            for id in ["a", "b"] {
                let prepared = retry(
                    &mut journal,
                    key(&format!("p-{id}-{n}")),
                    prepare(id, n * 2, &format!("root-{id}-{n}")),
                )
                .unwrap();
                retry(
                    &mut journal,
                    key(&format!("a-{id}-{n}")),
                    adopt(id, n * 2 + 1, prepared.reference),
                )
                .unwrap();
            }
        }
    });
    gate.wait();
    let selection = [key("a"), key("b"), key("absent")];
    let mut successes = 0;
    for _ in 0..50 {
        match SourceJournal::observe_many(&root, &minimum, &selection) {
            Ok(observed) => {
                successes += 1;
                let revisions: u64 = observed
                    .sources
                    .values()
                    .flatten()
                    .map(|s| s.revision)
                    .sum();
                assert_eq!(revisions, observed.checkpoint.sequence());
                assert!(observed.sources["absent"].is_none());
            }
            // A readonly hot-journal observation cannot recover on behalf of
            // the writer. Retry the observation; never downgrade it to success.
            Err(Error::Busy | Error::NeedsRecovery) => (),
            Err(error) => panic!("unexpected batch observation error: {error}"),
        }
        thread::sleep(Duration::from_millis(1));
    }
    writer.join().unwrap();
    assert!(successes > 0);
    let final_state = SourceJournal::observe_many(&root, &minimum, &selection).unwrap();
    assert_eq!(final_state.checkpoint.sequence(), 64);
    assert_eq!(final_state.sources["a"].as_ref().unwrap().revision, 32);
    assert_eq!(final_state.sources["b"].as_ref().unwrap().revision, 32);
}

#[test]
fn readonly_rollback_is_recovery_required_not_generic_sql_or_success() {
    use rusqlite::ffi;
    for (code, expected) in [
        (ffi::SQLITE_READONLY_ROLLBACK, Error::NeedsRecovery),
        (ffi::SQLITE_READONLY, Error::Sql),
        (ffi::SQLITE_READONLY_DBMOVED, Error::Sql),
        (ffi::SQLITE_READONLY_RECOVERY, Error::Sql),
        (ffi::SQLITE_BUSY, Error::Busy),
        (ffi::SQLITE_LOCKED, Error::Busy),
        (ffi::SQLITE_CORRUPT, Error::Sql),
        (ffi::SQLITE_NOTADB, Error::Sql),
    ] {
        assert_eq!(
            Error::from(rusqlite::Error::SqliteFailure(ffi::Error::new(code), None)),
            expected
        );
    }
}

#[test]
fn source_observation_hot_journal_child() {
    let Some(root) = std::env::var_os("GIT_ASSET_SOURCE_OBSERVATION_HOT_ROOT") else {
        return;
    };
    let database = Path::new(&root).join("claims.sqlite");
    let connection = rusqlite::Connection::open(database).unwrap();
    connection
        .execute_batch(
            "PRAGMA cache_size=1; BEGIN IMMEDIATE;
        CREATE TABLE interrupted_observation_test (payload BLOB);
        INSERT INTO interrupted_observation_test VALUES (zeroblob(65536));",
        )
        .unwrap();
    std::process::exit(94);
}

#[test]
fn hot_journal_observation_preserves_bytes_until_explicit_owner_recovery() {
    let (_temp, root, mut journal) = fixture();
    selected(&mut journal, "main", 0, "A");
    let witness = journal.checkpoint();
    drop(journal);
    let mut child = Process::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "source_observation_hot_journal_child",
            "--nocapture",
        ])
        .env("GIT_ASSET_SOURCE_OBSERVATION_HOT_ROOT", &root)
        .spawn()
        .unwrap();
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > Duration::from_secs(10) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("hot-journal child deadline");
        }
        thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(status.code(), Some(94));
    let database = root.join("claims.sqlite");
    let rollback = root.join("claims.sqlite-journal");
    let before_database = fs::read(&database).unwrap();
    let before_rollback = fs::read(&rollback).unwrap();
    assert!(before_rollback.len() > 512 && before_rollback[..8] != [0; 8]);
    assert_eq!(
        SourceJournal::observe(&root, &witness, &key("main")),
        Err(Error::NeedsRecovery)
    );
    assert_eq!(
        SourceJournal::observe_many(&root, &witness, &[key("main")]),
        Err(Error::NeedsRecovery)
    );
    assert_eq!(fs::read(&database).unwrap(), before_database);
    assert_eq!(fs::read(&rollback).unwrap(), before_rollback);
    let mut recovered = SourceJournal::open(&root, &witness).unwrap();
    assert_eq!(recovered.checkpoint(), witness);
    assert_eq!(
        recovered
            .source(&key("main"))
            .unwrap()
            .unwrap()
            .current
            .unwrap()
            .selection,
        selection("A")
    );
    assert!(!rollback.exists());
}

#[test]
fn batch_observation_excludes_an_uncommitted_source_without_blocking_the_writer() {
    let (_temp, root, mut journal) = fixture();
    let minimum = journal.checkpoint();
    selected(&mut journal, "a", 0, "A");
    let before = journal.checkpoint();
    drop(journal);
    let writer_root = root.clone();
    let writer_minimum = minimum.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let writer = thread::spawn(move || {
        let mut journal = SourceJournal::open(&writer_root, &writer_minimum).unwrap();
        journal.apply_hook(key("pending-b"), prepare("b", 0, "B"), |stage| {
            if stage == Stage::BeforeCommit {
                ready_tx.send(()).map_err(|_| Error::Injected)?;
                release_rx
                    .recv_timeout(Duration::from_secs(10))
                    .map_err(|_| Error::Injected)?;
            }
            Ok(())
        })
    });
    ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let selection = [key("a"), key("b")];
    let observed = SourceJournal::observe_many(&root, &minimum, &selection);
    release_tx.send(()).unwrap();
    writer.join().unwrap().unwrap();
    let observed = observed.unwrap();
    assert_eq!(observed.checkpoint, before);
    assert_eq!(observed.sources["a"].as_ref().unwrap().revision, 2);
    assert!(observed.sources["b"].is_none());
    let committed = SourceJournal::observe_many(&root, &minimum, &selection).unwrap();
    assert_eq!(committed.checkpoint.sequence(), 3);
    assert_eq!(committed.sources["a"], observed.sources["a"]);
    assert_eq!(committed.sources["b"].as_ref().unwrap().revision, 1);
}

#[test]
fn preview_is_readonly_and_distinguishes_proposed_state_from_historical_receipts() {
    let (_temp, root, mut journal) = fixture();
    let minimum = journal.checkpoint();
    let command = prepare("scene", 0, "A");
    let before = fs::read(root.join("claims.sqlite")).unwrap();
    let preview = SourceJournal::preview(&root, &minimum, key("op"), command.clone()).unwrap();
    assert_eq!(preview.checkpoint, minimum);
    let PreviewDisposition::Proposed {
        before: absent,
        after,
    } = preview.disposition
    else {
        panic!("must propose")
    };
    assert!(absent.is_none());
    assert_eq!(after.revision, 1);
    assert_eq!(after.pending.unwrap().selection, selection("A"));
    assert_eq!(fs::read(root.join("claims.sqlite")).unwrap(), before);
    assert!(journal.source(&key("scene")).unwrap().is_none());
    let receipt = journal.apply(key("op"), command.clone()).unwrap();
    journal
        .apply(key("adopt"), adopt("scene", 1, receipt.reference.clone()))
        .unwrap();
    let latest = journal.checkpoint();
    let historical = SourceJournal::preview(&root, &minimum, key("op"), command).unwrap();
    assert_eq!(historical.checkpoint, latest);
    let PreviewDisposition::Recorded { receipt: old } = historical.disposition else {
        panic!("must replay")
    };
    assert!(old.replayed);
    assert_eq!(old.checkpoint, receipt.checkpoint);
    assert_eq!(old.source_revision, 1);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
}

#[test]
fn preview_enforces_transition_fences_and_proposals_do_not_reserve_a_revision() {
    let (_temp, root, mut journal) = fixture();
    let minimum = journal.checkpoint();
    let original = prepare("scene", 0, "A");
    SourceJournal::preview(&root, &minimum, key("proposal"), original.clone()).unwrap();
    journal
        .apply(key("winner"), prepare("scene", 0, "B"))
        .unwrap();
    assert_eq!(
        SourceJournal::preview(&root, &minimum, key("proposal"), original.clone()),
        Err(Error::Stale)
    );
    assert_eq!(journal.apply(key("proposal"), original), Err(Error::Stale));
    assert_eq!(
        SourceJournal::preview(
            &root,
            &minimum,
            key("winner"),
            prepare("scene", 0, "changed")
        ),
        Err(Error::Conflict)
    );
    let mut wrong_owner = prepare("scene", 1, "A");
    if let Command::Prepare { owner, .. } = &mut wrong_owner {
        *owner = key("foreign");
    }
    assert_eq!(
        SourceJournal::preview(&root, &minimum, key("foreign"), wrong_owner),
        Err(Error::Owner)
    );
    assert_eq!(
        SourceJournal::preview(&root, &minimum, key("pending"), prepare("scene", 1, "A")),
        Err(Error::Pending)
    );
    assert_eq!(journal.source(&key("scene")).unwrap().unwrap().revision, 1);
}

#[test]
fn acknowledgment_recovers_historical_receipt_without_executing_unknown_or_changed_requests() {
    let (_temp, root, mut journal) = fixture();
    let minimum = journal.checkpoint();
    let command = prepare("scene", 0, "A");
    let original = journal.apply(key("prepare-A"), command.clone()).unwrap();
    journal
        .apply(
            key("adopt-A"),
            adopt("scene", 1, original.reference.clone()),
        )
        .unwrap();
    let latest = journal.checkpoint();
    drop(journal);
    let before = fs::read(root.join("claims.sqlite")).unwrap();
    let acknowledged =
        SourceJournal::acknowledge(&root, &latest, key("prepare-A"), command.clone()).unwrap();
    assert!(acknowledged.replayed);
    assert_eq!(acknowledged.reference, original.reference);
    assert_eq!(acknowledged.checkpoint, original.checkpoint);
    assert_eq!(acknowledged.source_revision, 1);
    assert_eq!(
        SourceJournal::acknowledge(&root, &minimum, key("typo"), prepare("new-scene", 0, "B")),
        Err(Error::Missing)
    );
    assert_eq!(
        SourceJournal::acknowledge(
            &root,
            &minimum,
            key("prepare-A"),
            prepare("scene", 0, "changed")
        ),
        Err(Error::Conflict)
    );
    assert_eq!(
        SourceJournal::observe(&root, &latest, &key("scene"))
            .unwrap()
            .source
            .unwrap()
            .revision,
        2
    );
    assert!(
        SourceJournal::observe(&root, &latest, &key("new-scene"))
            .unwrap()
            .source
            .is_none()
    );
    assert_eq!(fs::read(root.join("claims.sqlite")).unwrap(), before);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
}

#[test]
fn acknowledgment_requires_existing_matching_authority_and_trusted_floor() {
    let (_temp, root, mut journal) = fixture();
    let floor = journal.checkpoint();
    let backup = fs::read(root.join("claims.sqlite")).unwrap();
    let request = prepare("scene", 0, "A");
    journal.apply(key("op"), request.clone()).unwrap();
    let later = journal.checkpoint();
    drop(journal);
    let (_foreign, foreign_root, foreign) = fixture();
    drop(foreign);
    assert_eq!(
        SourceJournal::acknowledge(&foreign_root, &floor, key("op"), request.clone()),
        Err(Error::InvalidReference)
    );
    let missing = root.join("missing");
    assert!(SourceJournal::acknowledge(&missing, &floor, key("op"), request.clone()).is_err());
    assert!(!missing.exists());
    fs::write(root.join("claims.sqlite"), backup).unwrap();
    assert_eq!(
        SourceJournal::acknowledge(&root, &later, key("op"), request),
        Err(Error::Rollback)
    );
}

#[test]
fn authority_identity_survives_reopen_and_distinguishes_same_named_journals() {
    let (_temp, path, mut journal) = fixture();
    let identity = journal.identity().clone();
    journal
        .apply(key("prepare-identity"), prepare("job", 0, "root"))
        .unwrap();
    assert_eq!(journal.identity(), &identity);
    let checkpoint = journal.checkpoint();
    drop(journal);
    let journal = SourceJournal::open(&path, &checkpoint).unwrap();
    assert_eq!(journal.identity(), &identity);
    let (_other_temp, _other_path, other) = fixture();
    assert_ne!(journal.identity(), other.identity());
}

#[test]
fn observations_read_immutable_files_and_preserve_pending_current_and_predecessors() {
    let (_temp, root, mut journal) = fixture();
    let minimum = journal.checkpoint();
    let empty = SourceJournal::observe(&root, &minimum, &key("main")).unwrap();
    assert!(empty.source.is_none());
    assert_eq!(empty.checkpoint, minimum);
    selected(&mut journal, "main", 0, "A");
    selected(&mut journal, "main", 2, "B");
    journal
        .apply(key("pending-C"), prepare("main", 4, "C"))
        .unwrap();
    let expected = journal.source(&key("main")).unwrap();
    let latest = journal.checkpoint();
    drop(journal);
    let database = root.join("claims.sqlite");
    let original = fs::read(&database).unwrap();
    let marker = fs::read(root.join("claims.json")).unwrap();
    let permissions = fs::metadata(&database).unwrap().permissions();
    let mut read_only = permissions.clone();
    read_only.set_readonly(true);
    fs::set_permissions(&database, read_only).unwrap();
    let result = SourceJournal::observe(&root, &minimum, &key("main"));
    fs::set_permissions(&database, permissions).unwrap();
    let observed = result.unwrap();
    assert_eq!(observed.checkpoint, latest);
    assert_eq!(observed.source, expected);
    let state = observed.source.unwrap();
    assert_eq!(state.revision, 5);
    assert_eq!(state.current.unwrap().selection, selection("B"));
    assert_eq!(state.pending.unwrap().selection, selection("C"));
    assert_eq!(state.predecessors.len(), 1);
    assert_eq!(fs::read(database).unwrap(), original);
    assert_eq!(fs::read(root.join("claims.json")).unwrap(), marker);
    assert_eq!(fs::read_dir(root).unwrap().count(), 2);
}

#[test]
fn observations_refuse_foreign_rollback_missing_and_corrupt_authorities() {
    let (temp, root, mut journal) = fixture();
    let initial = journal.checkpoint();
    let old = fs::read(root.join("claims.sqlite")).unwrap();
    selected(&mut journal, "main", 0, "A");
    let latest = journal.checkpoint();
    drop(journal);
    let (_other, foreign, _journal) = fixture();
    assert_eq!(
        SourceJournal::observe(&foreign, &latest, &key("main")),
        Err(Error::InvalidReference)
    );
    let missing = temp.path().join("missing");
    assert!(SourceJournal::observe(&missing, &initial, &key("main")).is_err());
    assert!(!missing.exists());
    fs::write(root.join("claims.sqlite"), old).unwrap();
    assert_eq!(
        SourceJournal::observe(&root, &latest, &key("main")),
        Err(Error::Rollback)
    );
    fs::write(root.join("claims.sqlite"), b"invalid database").unwrap();
    assert!(SourceJournal::observe(&root, &initial, &key("main")).is_err());
    assert_eq!(
        fs::read(root.join("claims.sqlite")).unwrap(),
        b"invalid database"
    );
}

#[test]
fn observed_source_and_checkpoint_are_coherent_while_a_writer_progresses() {
    let (_temp, root, journal) = fixture();
    let minimum = journal.checkpoint();
    let writer_root = root.clone();
    let writer_minimum = minimum.clone();
    drop(journal);
    let gate = Arc::new(Barrier::new(2));
    let writer_gate = gate.clone();
    let writer = thread::spawn(move || {
        let mut journal = SourceJournal::open(&writer_root, &writer_minimum).unwrap();
        writer_gate.wait();
        for n in 0..16 {
            let prepared = retry(
                &mut journal,
                key(&format!("p-{n}")),
                prepare("main", n * 2, &format!("root-{n}")),
            )
            .unwrap();
            retry(
                &mut journal,
                key(&format!("a-{n}")),
                adopt("main", n * 2 + 1, prepared.reference),
            )
            .unwrap();
        }
    });
    gate.wait();
    let mut observations = 0;
    for _ in 0..50 {
        match SourceJournal::observe(&root, &minimum, &key("main")) {
            Ok(observed) => {
                observations += 1;
                assert_eq!(
                    observed.source.as_ref().map_or(0, |source| source.revision),
                    observed.checkpoint.sequence()
                );
                if let Some(source) = observed.source {
                    assert!(
                        source
                            .current
                            .as_ref()
                            .is_none_or(|current| current.revision <= source.revision)
                    );
                }
            }
            Err(Error::Busy | Error::NeedsRecovery) => (),
            Err(error) => panic!("unexpected observation error: {error}"),
        }
        thread::sleep(Duration::from_millis(1));
    }
    writer.join().unwrap();
    assert!(observations > 0);
    let final_state = SourceJournal::observe(&root, &minimum, &key("main")).unwrap();
    assert_eq!(final_state.checkpoint.sequence(), 32);
    let source = final_state.source.unwrap();
    assert_eq!(source.revision, 32);
    assert!(source.pending.is_none());
    assert_eq!(source.predecessors.len(), 15);
}

#[test]
fn observation_sees_committed_truth_while_an_append_waits_before_commit() {
    let (_temp, root, journal) = fixture();
    let minimum = journal.checkpoint();
    drop(journal);
    let writer_root = root.clone();
    let writer_minimum = minimum.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let writer = thread::spawn(move || {
        let mut journal = SourceJournal::open(&writer_root, &writer_minimum).unwrap();
        journal.apply_hook(key("in-flight"), prepare("main", 0, "A"), |stage| {
            if stage == Stage::BeforeCommit {
                ready_tx.send(()).map_err(|_| Error::Injected)?;
                release_rx
                    .recv_timeout(Duration::from_secs(10))
                    .map_err(|_| Error::Injected)?;
            }
            Ok(())
        })
    });
    ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let observed = SourceJournal::observe(&root, &minimum, &key("main"));
    release_tx.send(()).unwrap();
    writer.join().unwrap().unwrap();
    let observed = observed.unwrap();
    assert_eq!(observed.checkpoint, minimum);
    assert!(observed.source.is_none());
    let committed = SourceJournal::observe(&root, &minimum, &key("main")).unwrap();
    assert_eq!(committed.checkpoint.sequence(), 1);
    assert_eq!(committed.source.unwrap().revision, 1);
}
fn selected(j: &mut SourceJournal, source: &str, expected: u64, s: &str) -> u64 {
    let p = j
        .apply(
            key(&format!("p-{source}-{expected}")),
            prepare(source, expected, s),
        )
        .unwrap();
    j.apply(
        key(&format!("a-{source}-{expected}")),
        adopt(source, p.source_revision, p.reference),
    )
    .unwrap()
    .source_revision
}

#[test]
fn prepare_adopt_and_restart_preserve_current_pending_and_predecessor_roots() {
    let (_t, root, mut j) = fixture();
    selected(&mut j, "main", 0, "A");
    let p = j.apply(key("next"), prepare("main", 2, "B")).unwrap();
    let witness = j.checkpoint();
    drop(j);
    let mut j = SourceJournal::open(&root, &witness).unwrap();
    let roots = j.obligations(10).unwrap();
    assert_eq!(roots.len(), 2);
    assert!(
        roots
            .iter()
            .any(|r| r.role == Role::Current && r.selection == selection("A"))
    );
    assert!(
        roots
            .iter()
            .any(|r| r.role == Role::Pending && r.selection == selection("B"))
    );
    j.apply(key("accept"), adopt("main", 3, p.reference))
        .unwrap();
    let witness = j.checkpoint();
    drop(j);
    let mut j = SourceJournal::open(&root, &witness).unwrap();
    let roots = j.obligations(10).unwrap();
    assert!(
        roots
            .iter()
            .any(|r| r.role == Role::Current && r.selection == selection("B"))
    );
    assert!(
        roots
            .iter()
            .any(|r| r.role == Role::Predecessor && r.selection == selection("A"))
    );
}

#[test]
fn historical_retries_are_exact_and_never_reapply_spent_transitions() {
    let (_t, _root, mut j) = fixture();
    let cmd = prepare("main", 0, "A");
    let p = j.apply(key("prepare"), cmd.clone()).unwrap();
    let accepted = adopt("main", 1, p.reference.clone());
    let a = j.apply(key("adopt"), accepted.clone()).unwrap();
    let r = j.apply(key("prepare"), cmd).unwrap();
    assert!(r.replayed);
    assert_eq!(r.source_revision, 1);
    assert_eq!(r.checkpoint, p.checkpoint);
    let r = j.apply(key("adopt"), accepted).unwrap();
    assert!(r.replayed);
    assert_eq!(r.checkpoint, a.checkpoint);
    assert_eq!(j.source(&key("main")).unwrap().unwrap().revision, 2);
    assert_eq!(
        j.apply(key("prepare"), prepare("main", 0, "different"))
            .unwrap_err(),
        Error::Conflict
    );
}

#[test]
fn owners_stale_revisions_and_wrong_pending_references_cannot_change_protection() {
    let (_t, _root, mut j) = fixture();
    let p = j.apply(key("prepare"), prepare("main", 0, "A")).unwrap();
    let before = j.obligations(10).unwrap();
    let other = j
        .apply(key("other-source"), prepare("other", 0, "B"))
        .unwrap();
    assert_eq!(
        j.apply(key("wrong-ref"), adopt("main", 1, other.reference))
            .unwrap_err(),
        Error::InvalidReference
    );
    assert_eq!(
        j.apply(key("stale"), adopt("main", 0, p.reference.clone()))
            .unwrap_err(),
        Error::Stale
    );
    let mut wrong = adopt("main", 1, p.reference);
    if let Command::Adopt { owner, .. } = &mut wrong {
        *owner = key("intruder");
    }
    assert_eq!(
        j.apply(key("wrong-owner"), wrong).unwrap_err(),
        Error::Owner
    );
    assert_eq!(j.source(&key("main")).unwrap().unwrap().revision, 1);
    assert!(j.obligations(10).unwrap().contains(&before[0]));
    assert_eq!(
        j.apply(key("duplicate-pending"), prepare("main", 1, "C"))
            .unwrap_err(),
        Error::Pending
    );
}

#[test]
fn independent_sources_share_roots_without_retiring_each_others_obligations() {
    let (_t, _root, mut j) = fixture();
    selected(&mut j, "main", 0, "A");
    selected(&mut j, "branch", 0, "A");
    selected(&mut j, "main", 2, "B");
    j.apply(
        key("retire-old"),
        Command::Retire {
            source: key("main"),
            owner: key("owner"),
            expected_revision: 4,
            predecessor_revision: 2,
        },
    )
    .unwrap();
    let roots = j.obligations(10).unwrap();
    assert_eq!(roots.len(), 2);
    assert!(
        roots
            .iter()
            .any(|r| r.source == key("branch") && r.selection == selection("A"))
    );
    assert!(
        roots
            .iter()
            .any(|r| r.source == key("main") && r.selection == selection("B"))
    );
    assert!(matches!(j.obligations(1), Err(Error::Budget)));
}

#[test]
fn explicit_abort_withdraws_only_the_pending_owner_obligation() {
    let (_t, _root, mut j) = fixture();
    selected(&mut j, "main", 0, "A");
    let p = j.apply(key("draft"), prepare("main", 2, "B")).unwrap();
    let cmd = Command::Abort {
        source: key("main"),
        owner: key("owner"),
        expected_revision: 3,
        prepared: p.reference,
    };
    j.apply(key("abort"), cmd.clone()).unwrap();
    assert!(j.apply(key("abort"), cmd).unwrap().replayed);
    let roots = j.obligations(10).unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].selection, selection("A"));
    assert_eq!(
        j.apply(key("stale-after-abort"), prepare("main", 2, "C"))
            .unwrap_err(),
        Error::Stale
    );
}

#[test]
fn injected_prepare_and_adopt_failures_reopen_and_retry_without_losing_roots() {
    for stage in [Stage::BeforeAppend, Stage::BeforeCommit, Stage::AfterCommit] {
        for is_adopt in [false, true] {
            let (_t, root, mut j) = fixture();
            selected(&mut j, "main", 0, "A");
            let witness = j.checkpoint();
            let (op, cmd) = if is_adopt {
                let p = j.apply(key("draft"), prepare("main", 2, "B")).unwrap();
                ("accept", adopt("main", 3, p.reference))
            } else {
                ("draft", prepare("main", 2, "B"))
            };
            assert_eq!(
                j.apply_hook(key(op), cmd.clone(), |s| if s == stage {
                    Err(Error::Injected)
                } else {
                    Ok(())
                })
                .unwrap_err(),
                Error::Injected
            );
            assert_eq!(j.obligations(10).unwrap_err(), Error::NeedsRecovery);
            drop(j);
            let mut j = SourceJournal::open(&root, &witness).unwrap();
            let result = j.apply(key(op), cmd).unwrap();
            assert_eq!(result.replayed, stage == Stage::AfterCommit);
            let roots = j.obligations(10).unwrap();
            assert_eq!(roots.len(), 2);
            assert!(roots.iter().any(|r| r.selection == selection("A")));
            assert!(roots.iter().any(|r| r.selection == selection("B")));
        }
    }
}

fn retry(
    j: &mut SourceJournal,
    op: Key,
    cmd: Command,
) -> source_claims::Result<source_claims::Receipt> {
    let start = Instant::now();
    loop {
        let result = j.apply(op.clone(), cmd.clone());
        if result != Err(Error::Busy) || start.elapsed() > Duration::from_secs(3) {
            return result;
        }
        thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn same_source_writers_have_one_winner_while_independent_sources_both_progress() {
    for same in [false, true] {
        let (_t, root, j) = fixture();
        let witness = j.checkpoint();
        drop(j);
        let mut a = SourceJournal::open(&root, &witness).unwrap();
        let mut b = SourceJournal::open(&root, &witness).unwrap();
        let gate = Arc::new(Barrier::new(2));
        let other = gate.clone();
        let one = thread::spawn(move || {
            gate.wait();
            retry(&mut a, key("one"), prepare("main", 0, "A"))
        });
        let two = thread::spawn(move || {
            other.wait();
            retry(
                &mut b,
                key("two"),
                prepare(if same { "main" } else { "other" }, 0, "B"),
            )
        });
        let results = [one.join().unwrap(), two.join().unwrap()];
        assert_eq!(
            results.iter().filter(|r| r.is_ok()).count(),
            if same { 1 } else { 2 }
        );
        if same {
            assert!(results.contains(&Err(Error::Stale)));
        }
        let mut j = SourceJournal::open(&root, &witness).unwrap();
        assert_eq!(j.obligations(10).unwrap().len(), if same { 1 } else { 2 });
    }
}

#[test]
fn known_rollback_and_foreign_namespace_reject_without_claiming_latest_global_state() {
    let (t, root, mut j) = fixture();
    let minimum = j.checkpoint();
    let backup = t.path().join("backup.sqlite");
    j.apply(key("prepare"), prepare("main", 0, "A")).unwrap();
    let newer = j.checkpoint();
    drop(j);
    fs::copy(root.join("claims.sqlite"), &backup).unwrap();
    let mut j = SourceJournal::open(&root, &newer).unwrap();
    let p = j
        .source(&key("main"))
        .unwrap()
        .unwrap()
        .pending
        .unwrap()
        .reference;
    j.apply(key("adopt"), adopt("main", 1, p)).unwrap();
    let latest = j.checkpoint();
    drop(j);
    fs::copy(&backup, root.join("claims.sqlite")).unwrap();
    assert!(matches!(
        SourceJournal::open(&root, &latest),
        Err(Error::Rollback)
    ));
    assert!(SourceJournal::open(&root, &minimum).is_ok());
    let (_other_t, other_root, _other) = fixture();
    assert!(matches!(
        SourceJournal::open(&other_root, &minimum),
        Err(Error::InvalidReference)
    ));
}

#[test]
fn corrupted_bodies_gaps_and_extra_zero_sequence_rows_are_not_empty_success() {
    for mode in 0..3 {
        let (_t, root, mut j) = fixture();
        let witness = j.checkpoint();
        selected(&mut j, "main", 0, "A");
        drop(j);
        let c = rusqlite::Connection::open(root.join("claims.sqlite")).unwrap();
        match mode {
            0 => {
                c.execute("UPDATE events SET body=x'7b7d' WHERE sequence=1", [])
                    .unwrap();
            }
            1 => {
                c.execute("DELETE FROM events WHERE sequence=1", [])
                    .unwrap();
            }
            _ => {
                c.execute("INSERT INTO events SELECT 0,'zero',body,previous,hash FROM events WHERE sequence=1",[]).unwrap();
            }
        }
        drop(c);
        assert!(SourceJournal::open(&root, &witness).is_err());
    }
}

#[test]
fn predecessor_limit_refuses_growth_until_exact_owner_retirement() {
    let (_t, root, mut j) = fixture();
    let mut rev = selected(&mut j, "main", 0, "initial");
    for i in 0..source_claims::MAX_PREDECESSORS {
        rev = selected(&mut j, "main", rev, &format!("v-{i}"));
    }
    let before = j.checkpoint();
    assert_eq!(
        SourceJournal::preview(
            &root,
            &before,
            key("overflow"),
            prepare("main", rev, "overflow")
        ),
        Err(Error::Budget)
    );
    assert_eq!(
        j.apply(key("overflow"), prepare("main", rev, "overflow"))
            .unwrap_err(),
        Error::Budget
    );
    assert_eq!(j.checkpoint(), before);
    j.apply(
        key("retire-first"),
        Command::Retire {
            source: key("main"),
            owner: key("owner"),
            expected_revision: rev,
            predecessor_revision: 2,
        },
    )
    .unwrap();
    j.apply(key("fits"), prepare("main", rev + 1, "fits"))
        .unwrap();
}

#[test]
fn source_limit_and_missing_predecessors_do_not_drop_or_reuse_owner_tombstones() {
    let (_t, root, mut j) = fixture();
    for i in 0..source_claims::MAX_SOURCES {
        j.apply(
            key(&format!("op-{i}")),
            prepare(&format!("source-{i}"), 0, "A"),
        )
        .unwrap();
    }
    assert_eq!(
        SourceJournal::preview(
            &root,
            &j.checkpoint(),
            key("extra"),
            prepare("extra", 0, "B")
        ),
        Err(Error::Budget)
    );
    assert_eq!(
        j.apply(key("extra"), prepare("extra", 0, "B")).unwrap_err(),
        Error::Budget
    );
    assert_eq!(
        j.obligations(source_claims::MAX_SOURCES).unwrap().len(),
        source_claims::MAX_SOURCES
    );
    assert_eq!(
        j.apply(
            key("bad-retire"),
            Command::Retire {
                source: key("source-0"),
                owner: key("owner"),
                expected_revision: 1,
                predecessor_revision: 1
            }
        )
        .unwrap_err(),
        Error::Missing
    );
}

#[test]
fn busy_metadata_transaction_does_not_leave_a_render_or_transfer_lock() {
    let (_t, root, mut j) = fixture();
    let c = rusqlite::Connection::open(root.join("claims.sqlite")).unwrap();
    c.execute_batch("BEGIN IMMEDIATE").unwrap();
    let start = Instant::now();
    assert_eq!(
        j.apply(key("blocked"), prepare("main", 0, "A"))
            .unwrap_err(),
        Error::Busy
    );
    assert!(start.elapsed() < Duration::from_secs(1));
    c.execute_batch("ROLLBACK").unwrap();
    j.apply(key("blocked"), prepare("main", 0, "A")).unwrap();
}

#[test]
fn manifest_damage_and_existing_roots_are_never_implicitly_reinitialized() {
    let (_t, root, j) = fixture();
    let witness = j.checkpoint();
    drop(j);
    assert!(SourceJournal::create(&root, key("other"), key("epoch")).is_err());
    fs::write(root.join("claims.json"), b"{}").unwrap();
    assert!(SourceJournal::open(&root, &witness).is_err());
}

#[test]
fn withdrawing_a_source_reclassifies_current_and_never_resets_its_owner_revision() {
    let (_t, _root, mut j) = fixture();
    selected(&mut j, "branch", 0, "A");
    let p = j.apply(key("pending"), prepare("branch", 2, "B")).unwrap();
    assert_eq!(
        j.apply(
            key("too-early"),
            Command::Withdraw {
                source: key("branch"),
                owner: key("owner"),
                expected_revision: 3
            }
        )
        .unwrap_err(),
        Error::Pending
    );
    j.apply(
        key("abort"),
        Command::Abort {
            source: key("branch"),
            owner: key("owner"),
            expected_revision: 3,
            prepared: p.reference,
        },
    )
    .unwrap();
    j.apply(
        key("withdraw"),
        Command::Withdraw {
            source: key("branch"),
            owner: key("owner"),
            expected_revision: 4,
        },
    )
    .unwrap();
    let roots = j.obligations(10).unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].role, Role::Predecessor);
    assert_eq!(roots[0].selection, selection("A"));
    assert_eq!(
        j.apply(key("old-reuse"), prepare("branch", 0, "C"))
            .unwrap_err(),
        Error::Stale
    );
    j.apply(
        key("retire"),
        Command::Retire {
            source: key("branch"),
            owner: key("owner"),
            expected_revision: 5,
            predecessor_revision: 2,
        },
    )
    .unwrap();
    assert!(j.obligations(10).unwrap().is_empty());
    assert!(!j.obligations(10).unwrap().complete_claim_coverage());
    let mut wrong = prepare("branch", 6, "C");
    if let Command::Prepare { owner, .. } = &mut wrong {
        *owner = key("other-owner");
    }
    assert_eq!(
        j.apply(key("steal-identity"), wrong).unwrap_err(),
        Error::Owner
    );
    j.apply(key("resume-same-source"), prepare("branch", 6, "C"))
        .unwrap();
}

#[test]
fn source_claim_child() {
    let Some(root) = std::env::var_os("GIT_ASSET_SOURCE_CLAIM_TEST_ROOT") else {
        return;
    };
    let root = Path::new(&root);
    let witness: Checkpoint =
        serde_json::from_slice(&fs::read(root.parent().unwrap().join("witness.json")).unwrap())
            .unwrap();
    let cmd: Command =
        serde_json::from_slice(&fs::read(root.parent().unwrap().join("command.json")).unwrap())
            .unwrap();
    let mut j = SourceJournal::open(root, &witness).unwrap();
    let cut = std::env::var("GIT_ASSET_SOURCE_CLAIM_TEST_STAGE").unwrap();
    j.apply_hook(key("child-op"), cmd, |s| {
        if format!("{s:?}") == cut {
            std::process::exit(91);
        }
        Ok(())
    })
    .unwrap();
    panic!("child did not exit");
}

#[test]
fn actual_child_exits_preserve_prepare_and_adopt_handoffs() {
    for stage in [Stage::BeforeAppend, Stage::BeforeCommit, Stage::AfterCommit] {
        for is_adopt in [false, true] {
            let (t, root, mut j) = fixture();
            selected(&mut j, "main", 0, "A");
            let cmd = if is_adopt {
                let p = j.apply(key("draft"), prepare("main", 2, "B")).unwrap();
                adopt("main", 3, p.reference)
            } else {
                prepare("main", 2, "B")
            };
            let witness = j.checkpoint();
            fs::write(
                t.path().join("witness.json"),
                serde_json::to_vec(&witness).unwrap(),
            )
            .unwrap();
            fs::write(
                t.path().join("command.json"),
                serde_json::to_vec(&cmd).unwrap(),
            )
            .unwrap();
            drop(j);
            let mut child = Process::new(std::env::current_exe().unwrap())
                .args(["--exact", "source_claim_child", "--nocapture"])
                .env("GIT_ASSET_SOURCE_CLAIM_TEST_ROOT", &root)
                .env("GIT_ASSET_SOURCE_CLAIM_TEST_STAGE", format!("{stage:?}"))
                .spawn()
                .unwrap();
            let start = Instant::now();
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if start.elapsed() > Duration::from_secs(10) {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("child deadline");
                }
                thread::sleep(Duration::from_millis(5));
            };
            assert_eq!(status.code(), Some(91));
            let mut j = SourceJournal::open(&root, &witness).unwrap();
            let roots = j.obligations(10).unwrap();
            assert!(roots.iter().any(|r| r.selection == selection("A")));
            if is_adopt || stage == Stage::AfterCommit {
                assert!(roots.iter().any(|r| r.selection == selection("B")));
            }
            let retried = j.apply(key("child-op"), cmd).unwrap();
            assert_eq!(retried.replayed, stage == Stage::AfterCommit);
            assert_eq!(j.obligations(10).unwrap().len(), 2);
        }
    }
}

#[test]
fn malformed_candidates_and_foreign_journal_references_leave_the_handle_usable() {
    let (_t, _root, mut j) = fixture();
    let (_other_t, _other_root, mut other) = fixture();
    let p = j.apply(key("prepare"), prepare("main", 0, "A")).unwrap();
    let foreign = other
        .apply(key("prepare"), prepare("main", 0, "A"))
        .unwrap();
    assert_eq!(
        j.apply(key("foreign"), adopt("main", 1, foreign.reference))
            .unwrap_err(),
        Error::InvalidReference
    );
    let mut bad = prepare("other", 0, "A");
    if let Command::Prepare { selection, .. } = &mut bad {
        selection.root.bytes = u64::MAX;
    }
    assert_eq!(
        j.apply(key("bad"), bad).unwrap_err(),
        Error::InvalidReference
    );
    let mut bad = prepare("other", 0, "A");
    if let Command::Prepare { selection, .. } = &mut bad {
        selection.root.bytes = 65_537;
    }
    assert_eq!(
        j.apply(key("oversize"), bad).unwrap_err(),
        Error::InvalidReference
    );
    j.apply(key("accept"), adopt("main", 1, p.reference))
        .unwrap();
}

#[test]
fn withdrawal_faults_are_atomic_and_replay_the_exact_owner_decision() {
    for stage in [Stage::BeforeAppend, Stage::BeforeCommit, Stage::AfterCommit] {
        for action in 0..3 {
            let (_t, root, mut j) = fixture();
            selected(&mut j, "main", 0, "A");
            let cmd = match action {
                0 => {
                    let p = j.apply(key("draft"), prepare("main", 2, "B")).unwrap();
                    Command::Abort {
                        source: key("main"),
                        owner: key("owner"),
                        expected_revision: 3,
                        prepared: p.reference,
                    }
                }
                1 => Command::Withdraw {
                    source: key("main"),
                    owner: key("owner"),
                    expected_revision: 2,
                },
                _ => {
                    selected(&mut j, "main", 2, "B");
                    Command::Retire {
                        source: key("main"),
                        owner: key("owner"),
                        expected_revision: 4,
                        predecessor_revision: 2,
                    }
                }
            };
            let witness = j.checkpoint();
            let before = j.obligations(10).unwrap();
            assert_eq!(
                j.apply_hook(key("decision"), cmd.clone(), |s| if s == stage {
                    Err(Error::Injected)
                } else {
                    Ok(())
                })
                .unwrap_err(),
                Error::Injected
            );
            drop(j);
            let mut j = SourceJournal::open(&root, &witness).unwrap();
            if stage != Stage::AfterCommit {
                assert_eq!(j.obligations(10).unwrap(), before);
            }
            assert_eq!(
                j.apply(key("decision"), cmd).unwrap().replayed,
                stage == Stage::AfterCommit
            );
            assert!(!j.obligations(10).unwrap().is_empty());
        }
    }
}

#[test]
fn full_command_history_reopens_and_refuses_growth_without_discarding_retries() {
    use git_asset_core::identity;
    use rusqlite::params;
    let (_t, root, j) = fixture();
    let minimum = j.checkpoint();
    let genesis = serde_json::to_value(&minimum).unwrap()["genesis"]
        .as_str()
        .unwrap()
        .to_owned();
    drop(j);
    let mut c = rusqlite::Connection::open(root.join("claims.sqlite")).unwrap();
    let tx = c.transaction().unwrap();
    let mut previous = Digest::parse(&genesis).unwrap();
    let mut bytes = 0u64;
    let mut last = None;
    for n in 1..=source_claims::MAX_COMMANDS {
        let op = key(&format!("bulk-{n}"));
        let cmd = if n % 2 == 1 {
            prepare("main", n - 1, "A")
        } else {
            let reference: Reference = serde_json::from_value(
                serde_json::json!({"journal":genesis,"operation":format!("bulk-{}",n-1)}),
            )
            .unwrap();
            Command::Abort {
                source: key("main"),
                owner: key("owner"),
                expected_revision: n - 1,
                prepared: reference,
            }
        };
        let request = serde_json::json!({"operation":op,"command":cmd});
        let body = identity::canonical_json(&request).unwrap();
        let event = serde_json::json!({"schema":"tig.source-event.design-1","sequence":n,"previous":previous,"request":request});
        let hash = Digest::blob(&identity::canonical_json(&event).unwrap());
        tx.execute(
            "INSERT INTO events VALUES(?1,?2,?3,?4,?5)",
            params![
                n as i64,
                op.as_str(),
                body,
                previous.as_str(),
                hash.as_str()
            ],
        )
        .unwrap();
        bytes += body.len() as u64;
        previous = hash;
        last = Some((op, cmd));
    }
    tx.execute(
        "UPDATE meta SET sequence=?1,hash=?2,bytes=?3 WHERE id=1",
        params![
            source_claims::MAX_COMMANDS as i64,
            previous.as_str(),
            bytes as i64
        ],
    )
    .unwrap();
    tx.commit().unwrap();
    drop(c);
    let mut j = SourceJournal::open(&root, &minimum).unwrap();
    assert_eq!(j.checkpoint().sequence(), source_claims::MAX_COMMANDS);
    assert_eq!(
        SourceJournal::preview(
            &root,
            &j.checkpoint(),
            key("overflow"),
            prepare("main", source_claims::MAX_COMMANDS, "B")
        ),
        Err(Error::Budget)
    );
    assert_eq!(
        j.apply(
            key("overflow"),
            prepare("main", source_claims::MAX_COMMANDS, "B")
        )
        .unwrap_err(),
        Error::Budget
    );
    let (op, cmd) = last.unwrap();
    assert!(j.apply(op, cmd).unwrap().replayed);
    assert!(j.obligations(10).unwrap().is_empty());
    assert_eq!(
        j.source(&key("main")).unwrap().unwrap().revision,
        source_claims::MAX_COMMANDS
    );
}
