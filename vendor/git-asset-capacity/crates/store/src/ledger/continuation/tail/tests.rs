use super::super::tests::{Fixture, control, fixture, minimum, read};
use super::*;
pub(super) mod joined;
use git_asset_core::{capacity::CapacityError, tree::TreeResult};
use history::index::build_limits;
use std::cell::{Cell, RefCell};

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
pub(super) fn create(f: &Fixture, c: &ActiveControl) -> Connection {
    let path = f._temp.path().join("tail.sqlite");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    let mut conn = connect(&path).unwrap();
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    for sql in [META, EVENTS, HEADER] {
        tx.execute_batch(sql).unwrap();
    }
    let identity = &c.preparation.tail;
    tx.execute(
        "INSERT INTO meta VALUES(1,?1,?2,?3,0)",
        params![
            identity.authority.as_str(),
            sql_number(identity.base.sequence).unwrap(),
            identity.base.hash.as_str()
        ],
    )
    .unwrap();
    let raw = bytes(identity).unwrap();
    tx.execute(
        "INSERT INTO git_asset_tail VALUES(1,?1,?2)",
        params![raw, Digest::blob(&raw).as_str()],
    )
    .unwrap();
    for (kind, sql) in definitions().values() {
        if kind == "trigger" {
            tx.execute_batch(sql).unwrap();
        }
    }
    tx.commit().unwrap();
    conn
}
fn append(
    conn: &mut Connection,
    identity: &TailIdentity,
    operation: &str,
    command: Command,
) -> Checkpoint {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let (previous, total) = head(&tx, identity).unwrap();
    let request = Request {
        operation: key(operation),
        command,
    };
    let raw = encode(&request).unwrap();
    let sequence = previous.sequence + 1;
    let hash = event_hash(sequence, &previous.hash, &request).unwrap();
    tx.execute(
        "INSERT INTO events VALUES(?1,?2,?3,?4,?5)",
        params![
            sql_number(sequence).unwrap(),
            operation,
            raw,
            previous.hash.as_str(),
            hash.as_str()
        ],
    )
    .unwrap();
    tx.execute(
        "UPDATE meta SET sequence=?1,hash=?2,bytes=?3 WHERE id=1",
        params![
            sql_number(sequence).unwrap(),
            hash.as_str(),
            sql_number(total + raw.len() as u64).unwrap()
        ],
    )
    .unwrap();
    tx.commit().unwrap();
    Checkpoint {
        genesis: identity.authority.clone(),
        sequence,
        hash,
    }
}
pub(super) fn refresh(f: &mut Fixture) {
    let export = f
        .ledger
        .export_history(&mut f.nodes, ReadLimits::default())
        .unwrap();
    let (index, _) = Index::from_legacy(export.snapshot(), &mut f.nodes, build_limits()).unwrap();
    let (operations, sequences) = index.roots();
    f.data.source = f.ledger.checkpoint();
    f.data.operation_count = f.data.source.sequence;
    f.data.working_state = export.snapshot().working_state().clone();
    f.data.operations = operations.clone();
    f.data.sequences = sequences.clone();
    f.data.clock_high = export.snapshot().clock_high_water();
}
fn run(f: &Fixture, c: &ActiveControl, m: &Minimum, conn: &mut Connection) -> Result<Replayed> {
    let base = read(f, c, m, ReadLimits::default())?;
    read_tail(
        base,
        c,
        conn,
        &mut f.nodes.finished_reader().unwrap(),
        build_limits(),
    )
}

#[test]
fn physical_tail_replays_actual_original_commands_with_old_typed_dependencies() {
    let mut f = fixture();
    let c = control(&mut f);
    let mut conn = create(&f, &c);
    let allocation = JournalRef {
        genesis: f.data.source.genesis.clone(),
        operation: key("allocation"),
    };
    let reservation = JournalRef {
        genesis: f.data.source.genesis.clone(),
        operation: key("job"),
    };
    let completed = f
        .ledger
        .complete_allocation(key("complete"), &allocation, 50)
        .unwrap();
    let finished = f.ledger.finish(key("finish"), &reservation).unwrap();
    // Exact original request bodies generate the same global event chain.
    let end = append(
        &mut conn,
        &c.preparation.tail,
        "complete",
        Command::CompleteAllocation {
            allocation: key("allocation"),
            actual: 50,
        },
    );
    assert_eq!(&end, completed.checkpoint());
    let end = append(
        &mut conn,
        &c.preparation.tail,
        "finish",
        Command::Finish {
            reservation: key("job"),
        },
    );
    assert_eq!(&end, finished.checkpoint());
    let changes = conn.total_changes();
    let result = run(&f, &c, &minimum(&c), &mut conn).unwrap();
    assert_eq!(result.checkpoint, f.ledger.checkpoint());
    assert_eq!(result.outputs.len(), 2);
    assert!(result.bytes > 0 && result.metrics.reads > 0 && result.metrics.writes == 0);
    assert_eq!(result.clock.high, None);
    assert_eq!(conn.total_changes(), changes);
    assert!(conn.is_autocommit());
    assert_eq!(
        result.model.summary(),
        Err(CapacityError::UnknownObservation)
    );
    let Output::Reservation(token) = f.ledger.outputs.get("job").unwrap() else {
        panic!()
    };
    let Output::Allocation(ticket) = f.ledger.outputs.get("allocation").unwrap() else {
        panic!()
    };
    assert!(result.model.is_closed(token).unwrap());
    assert_eq!(result.model.allocation_status(ticket).unwrap(), Some(50));
}

#[test]
fn deferred_minimum_requires_exact_tail_prefix_including_empty_tail_refusal() {
    let mut f = fixture();
    let c = control(&mut f);
    let mut conn = create(&f, &c);
    let end = append(
        &mut conn,
        &c.preparation.tail,
        "tail-1",
        Command::Invalidate,
    );
    let mut m = minimum(&c);
    m.checkpoint = end.clone();
    assert_eq!(run(&f, &c, &m, &mut conn).unwrap().checkpoint, end);
    m.checkpoint.hash = Digest::blob(b"wrong acknowledged hash");
    assert!(matches!(
        run(&f, &c, &m, &mut conn),
        Err(Error::Journal(LedgerError::Rollback))
    ));
    conn.execute("DELETE FROM events", []).unwrap();
    conn.execute(
        "UPDATE meta SET sequence=?1,hash=?2,bytes=0",
        params![
            sql_number(c.preparation.tail.base.sequence).unwrap(),
            c.preparation.tail.base.hash.as_str()
        ],
    )
    .unwrap();
    m.checkpoint = end;
    assert!(matches!(
        run(&f, &c, &m, &mut conn),
        Err(Error::Journal(LedgerError::Rollback))
    ));
}

#[test]
fn immutable_tail_header_and_exact_schema_reject_empty_generation_rebinding() {
    let mut f = fixture();
    let c = control(&mut f);
    let mut conn = create(&f, &c);
    for sql in [
        "INSERT INTO git_asset_tail SELECT * FROM git_asset_tail",
        "UPDATE git_asset_tail SET body=body",
        "DELETE FROM git_asset_tail",
    ] {
        assert!(conn.execute(sql, []).is_err());
    }
    let mut other = c.preparation.tail.clone();
    other.operation = key("another-operation");
    assert!(collect(&mut conn, &other).is_err());
    other = c.preparation.tail.clone();
    other.generation += 1;
    assert!(collect(&mut conn, &other).is_err());
    conn.execute_batch("DROP TRIGGER git_asset_tail_UPDATE")
        .unwrap();
    assert!(collect(&mut conn, &c.preparation.tail).is_err());
}

#[test]
fn checked_base_cannot_be_rebound_to_another_preparation_with_the_same_head() {
    let mut f = fixture();
    let first = control(&mut f);
    let base = read(&f, &first, &minimum(&first), ReadLimits::default()).unwrap();
    f.data.operations = Tree::empty(&mut f.nodes).unwrap().root().clone();
    let other = control(&mut f);
    assert_eq!(first.preparation.tail, other.preparation.tail);
    assert_ne!(first.preparation.data, other.preparation.data);
    let mut conn = create(&f, &other);
    let mut reader = f.nodes.finished_reader().unwrap();
    assert!(matches!(
        read_tail(base, &other, &mut conn, &mut reader, build_limits()),
        Err(Error::Invalid)
    ));
    assert_eq!(reader.metrics().node_gets, 0);
}

#[test]
fn sql_field_limits_measure_encoded_bytes_before_copying_text_values() {
    let mut f = fixture();
    let c = control(&mut f);
    let mut conn = create(&f, &c);
    append(
        &mut conn,
        &c.preparation.tail,
        "initial",
        Command::Invalidate,
    );
    // SQLite BLOB affinity still admits TEXT. Its character length is below
    // 64KiB but the encoded body is above it; the byte guard must run first.
    conn.execute("UPDATE events SET body=?1", ["界".repeat(33000)])
        .unwrap();
    assert!(matches!(
        collect(&mut conn, &c.preparation.tail),
        Err(Error::Budget)
    ));
}

#[test]
fn sqlite_internal_name_filter_cannot_hide_similarly_named_user_schema() {
    let mut f = fixture();
    let c = control(&mut f);
    let mut conn = create(&f, &c);
    conn.execute_batch("CREATE TABLE sqlitex_intruder(id INTEGER)")
        .unwrap();
    assert!(matches!(
        collect(&mut conn, &c.preparation.tail),
        Err(Error::Invalid)
    ));
}

#[test]
fn old_probe_cannot_be_rehydrated_and_old_operation_or_wrong_type_cannot_be_reused() {
    for (operation, command) in [
        (
            "observe-old",
            Command::Observe {
                probe: key("probe"),
                free: 10000,
            },
        ),
        ("job", Command::Invalidate),
        (
            "wrong-type",
            Command::CompleteAllocation {
                allocation: key("job"),
                actual: 0,
            },
        ),
    ] {
        let mut f = fixture();
        let c = control(&mut f);
        let mut conn = create(&f, &c);
        append(&mut conn, &c.preparation.tail, operation, command);
        assert!(run(&f, &c, &minimum(&c), &mut conn).is_err());
        assert_eq!(f.ledger.checkpoint(), f.data.source);
    }
}

#[test]
fn old_cleanup_still_cancels_settlement_and_its_exact_review_allows_completion() {
    let mut f = fixture();
    let job = JournalRef {
        genesis: f.data.source.genesis.clone(),
        operation: key("job"),
    };
    let binding = Digest::blob(b"binding");
    let reviewed = Digest::blob(b"reviewed");
    let publication = f
        .ledger
        .begin_publication(key("publication"), &job, key("owner"), binding.clone(), 100)
        .unwrap();
    f.ledger
        .begin_cleanup(publication.reference(), binding.clone(), reviewed.clone())
        .unwrap();
    refresh(&mut f);
    let c = control(&mut f);
    let mut conn = create(&f, &c);
    append(
        &mut conn,
        &c.preparation.tail,
        "settle",
        Command::SettleFootprint {
            allocation: key("publication"),
            binding: binding.clone(),
            footprint: 1,
        },
    );
    assert!(matches!(
        run(&f, &c, &minimum(&c), &mut conn),
        Err(Error::Journal(LedgerError::Cancelled))
    ));
    conn.execute("DELETE FROM events", []).unwrap();
    conn.execute(
        "UPDATE meta SET sequence=?1,hash=?2,bytes=0",
        params![
            sql_number(c.preparation.tail.base.sequence).unwrap(),
            c.preparation.tail.base.hash.as_str()
        ],
    )
    .unwrap();
    append(
        &mut conn,
        &c.preparation.tail,
        "cleanup-complete",
        Command::CompleteCleanup {
            allocation: key("publication"),
            binding,
            reviewed,
            footprint: 0,
        },
    );
    let result = run(&f, &c, &minimum(&c), &mut conn).unwrap();
    assert!(matches!(
        result.outputs.get("cleanup-complete"),
        Some(Output::Cleaned(0))
    ));
}

struct Advancing<'a> {
    nodes: &'a MetadataReader,
    peer: RefCell<Connection>,
    identity: TailIdentity,
    advanced: Cell<bool>,
}
impl NodeStore for Advancing<'_> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        if !self.advanced.replace(true) {
            // A real competing commit would be Busy if the reader retained a
            // SQL snapshot while resolving this metadata dependency.
            append(
                &mut self.peer.borrow_mut(),
                &self.identity,
                "peer-advance",
                Command::Invalidate,
            );
        }
        self.nodes.get(object)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        panic!("read-only replay")
    }
}
#[test]
fn actual_peer_commit_during_node_read_proves_transaction_release_and_changed_head_refuses() {
    let mut f = fixture();
    let c = control(&mut f);
    let mut conn = create(&f, &c);
    append(
        &mut conn,
        &c.preparation.tail,
        "initial",
        Command::Invalidate,
    );
    let base = read(&f, &c, &minimum(&c), ReadLimits::default()).unwrap();
    let reader = f.nodes.finished_reader().unwrap();
    let peer = connect(&f._temp.path().join("tail.sqlite")).unwrap();
    let mut advancing = Advancing {
        nodes: &reader,
        peer: RefCell::new(peer),
        identity: c.preparation.tail.clone(),
        advanced: Cell::new(false),
    };
    assert!(matches!(
        read_tail(base, &c, &mut conn, &mut advancing, build_limits()),
        Err(Error::Journal(LedgerError::Conflict))
    ));
    assert!(advancing.advanced.get());
    assert!(conn.is_autocommit());
    assert_eq!(
        head(&conn, &c.preparation.tail).unwrap().0.sequence,
        c.preparation.tail.base.sequence + 2
    );
}

#[test]
fn malformed_rows_and_shared_dependency_budget_fail_without_source_changes() {
    for case in 0..5 {
        let mut f = fixture();
        let c = control(&mut f);
        let mut conn = create(&f, &c);
        append(
            &mut conn,
            &c.preparation.tail,
            "complete",
            Command::CompleteAllocation {
                allocation: key("allocation"),
                actual: 50,
            },
        );
        match case {
            0 => {
                conn.execute("UPDATE meta SET bytes=bytes+1", []).unwrap();
            }
            1 => {
                conn.execute(
                    "UPDATE events SET previous=?1",
                    [Digest::blob(b"wrong").as_str()],
                )
                .unwrap();
            }
            2 => {
                conn.execute("UPDATE events SET body=?1", [vec![b' '; 65537]])
                    .unwrap();
            }
            3 => {
                conn.execute("UPDATE events SET sequence=sequence+1", [])
                    .unwrap();
            }
            _ => (),
        }
        let base = read(&f, &c, &minimum(&c), ReadLimits::default()).unwrap();
        let mut reader = f.nodes.finished_reader().unwrap();
        let limits = if case == 4 {
            TreeLimits {
                max_reads: 1,
                ..build_limits()
            }
        } else {
            build_limits()
        };
        assert!(read_tail(base, &c, &mut conn, &mut reader, limits).is_err());
        if case == 4 {
            assert_eq!(reader.metrics().node_gets, 1);
        }
        assert_eq!(f.ledger.checkpoint(), f.data.source);
        assert!(conn.is_autocommit());
    }
}

#[test]
fn tail_probe_and_reserve_replay_retains_results_but_invalidates_sample_and_pending_ticket() {
    let mut f = fixture();
    let c = control(&mut f);
    let mut conn = create(&f, &c);
    append(
        &mut conn,
        &c.preparation.tail,
        "new-probe",
        Command::BeginProbe,
    );
    append(
        &mut conn,
        &c.preparation.tail,
        "new-observe",
        Command::Observe {
            probe: key("new-probe"),
            free: 10000,
        },
    );
    append(
        &mut conn,
        &c.preparation.tail,
        "new-job",
        Command::Reserve {
            owner: key("owner"),
            peak: 10,
        },
    );
    append(
        &mut conn,
        &c.preparation.tail,
        "unfinished-probe",
        Command::BeginProbe,
    );
    let mut result = run(&f, &c, &minimum(&c), &mut conn).unwrap();
    assert!(matches!(
        result.outputs.get("new-job"),
        Some(Output::Reservation(_))
    ));
    assert_eq!(
        result.model.summary(),
        Err(CapacityError::UnknownObservation)
    );
    let Some(Output::Probe(ticket)) = result.outputs.get("unfinished-probe") else {
        panic!()
    };
    assert_eq!(
        result.model.observe(ticket.clone(), 10000),
        Err(CapacityError::Stale)
    );
}

#[test]
fn original_clock_high_survives_tail_replay_and_backward_or_repeated_strict_time_refuses() {
    let mut f = fixture();
    let policy = crate::clock::Policy {
        authority: Digest::blob(b"time-authority"),
        epoch: Digest::blob(b"time-epoch"),
        boundary: Digest::blob(b"native-boundary"),
        max_sample_millis: 100,
        max_drift_millis: 100,
        max_session_millis: 1000,
    };
    f.ledger = CapacityLedger::create_clock(
        &f._temp.path().join("clock-ledger"),
        key("volume"),
        key("epoch"),
        10,
        policy,
    )
    .unwrap();
    f.ledger
        .observe_clock(key("original-time"), 100, false)
        .unwrap();
    f.data.config = f.ledger.config.clone();
    refresh(&mut f);
    let c = control(&mut f);
    let mut conn = create(&f, &c);
    append(
        &mut conn,
        &c.preparation.tail,
        "continued-time",
        Command::ObserveClock {
            millis: 150,
            strict: true,
        },
    );
    let result = run(&f, &c, &minimum(&c), &mut conn).unwrap();
    assert_eq!(result.clock.high, Some(150));
    assert!(result.clock.enabled);
    append(
        &mut conn,
        &c.preparation.tail,
        "backward-time",
        Command::ObserveClock {
            millis: 150,
            strict: true,
        },
    );
    assert!(matches!(
        run(&f, &c, &minimum(&c), &mut conn),
        Err(Error::Journal(LedgerError::Clock))
    ));
}

#[test]
fn real_full_original_and_full_sql_tail_replay_crosses_lifetime_cap_without_waiving_local_cap() {
    let mut f = fixture();
    while f.ledger.checkpoint().sequence < MAX_COMMANDS {
        f.ledger
            .invalidate(key(&format!("legacy-{}", f.ledger.checkpoint().sequence)))
            .unwrap();
    }
    assert!(matches!(
        f.ledger.invalidate(key("beyond-legacy")),
        Err(LedgerError::Budget)
    ));
    eprintln!("original physical journal reached16384 with old in-flight allocation");
    refresh(&mut f);
    eprintln!("original metadata/index exported; preparing private SQL tail");
    let c = control(&mut f);
    let mut conn = create(&f, &c);
    // Physically committed SQL commands, not immutable synthetic index items.
    for n in 0..MAX_COMMANDS {
        let command = if n == 0 {
            Command::CompleteAllocation {
                allocation: key("allocation"),
                actual: 50,
            }
        } else {
            Command::Invalidate
        };
        append(
            &mut conn,
            &c.preparation.tail,
            &format!("continued-{n}"),
            command,
        );
    }
    let m = Minimum {
        checkpoint: head(&conn, &c.preparation.tail).unwrap().0,
        ..minimum(&c)
    };
    eprintln!("private SQL tail committed16384 local rows; replaying original dependencies");
    let result = run(&f, &c, &m, &mut conn).unwrap();
    assert_eq!(result.checkpoint.sequence, 32768);
    assert_eq!(result.outputs.len(), 16384);
    assert_eq!(
        result.model.summary(),
        Err(CapacityError::UnknownObservation)
    );
    assert!(result.metrics.reads <= 2_000_000 && result.metrics.read_bytes <= 512 * 1024 * 1024);
    assert_eq!(result.metrics.writes, 0);
    let Output::Allocation(ticket) = f.ledger.outputs.get("allocation").unwrap() else {
        panic!()
    };
    assert_eq!(result.model.allocation_status(ticket).unwrap(), Some(50));
    let Output::Reservation(token) = f.ledger.outputs.get("job").unwrap() else {
        panic!()
    };
    assert!(!result.model.is_closed(token).unwrap());
    assert!(matches!(
        result.outputs.get("continued-0"),
        Some(Output::Sequence(1))
    ));
    eprintln!(
        "full actual SQL tail replay: {:?}, head={}, body_bytes={}",
        result.metrics, result.checkpoint.sequence, result.bytes
    );
    // A corrupt/overfull tail cannot be admitted through a global-sequence cap waiver.
    conn.execute("UPDATE meta SET sequence=sequence+1", [])
        .unwrap();
    assert!(collect(&mut conn, &c.preparation.tail).is_err());
    assert_eq!(f.ledger.checkpoint().sequence, 16384);
}
