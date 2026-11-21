//! Same-workload preparation measurement, not an activation/controller API.
use super::*;
use git_asset_core::tree::MemoryNodes;

pub(in crate::ledger::continuation::tail) fn prepare(
    f: &mut Fixture,
    c: &ActiveControl,
    m: &Minimum,
    conn: &mut Connection,
) -> Result<(PreparedData, Object, TreeMetrics)> {
    let mut reader = f
        .nodes
        .finished_reader()
        .map_err(|e| Error::Store(e.to_string()))?;
    let (data, object, metrics, body_bytes) =
        super::super::preparation::build(c, m, conn, &mut reader, &mut f.nodes, build_limits())?;
    super::super::preparation::unchanged(conn, &c.preparation.tail, &data.source, body_bytes)?;
    eprintln!("joined completed preparation: {metrics:?}");
    Ok((data, object, metrics))
}

#[test]
fn nested_work_charges_hidden_cost_and_refuses_underreporting_or_stale_totals() {
    let mut memory = MemoryNodes::default();
    let object = memory.put(b"a").unwrap();
    let port = BudgetPort::new(
        &mut memory,
        TreeLimits {
            max_reads: 4,
            max_read_bytes: 4,
            ..build_limits()
        },
    )
    .unwrap();
    let before = port.used();
    port.get(&object).unwrap();
    port.account_nested(
        &before,
        &TreeMetrics {
            reads: 2,
            read_bytes: 2,
            ..TreeMetrics::default()
        },
    )
    .unwrap();
    assert_eq!(port.used().reads, 2);
    assert_eq!(port.remaining().max_reads, 2);
    let observed = port.used();
    assert!(
        port.account_nested(&before, &TreeMetrics::default())
            .is_err()
    );
    assert_eq!(port.used(), observed);
    assert!(matches!(
        port.account_nested(
            &observed,
            &TreeMetrics {
                reads: 3,
                read_bytes: 3,
                ..TreeMetrics::default()
            }
        ),
        Err(Error::Budget)
    ));
    assert_eq!(port.used(), observed);
    let future = TreeMetrics {
        reads: 3,
        read_bytes: 3,
        ..TreeMetrics::default()
    };
    assert!(
        port.account_nested(&future, &TreeMetrics::default())
            .is_err()
    );
    assert_eq!(port.used(), observed);
}

#[test]
fn deferred_name_check_cannot_release_state_for_an_original_operation() {
    let mut f = fixture();
    let c = control(&mut f);
    let mut conn = create(&f, &c);
    append(&mut conn, &c.preparation.tail, "job", Command::Invalidate);
    let result = prepare(&mut f, &c, &minimum(&c), &mut conn);
    assert!(matches!(result, Err(Error::Journal(LedgerError::Conflict))));
    assert_eq!(f.ledger.checkpoint(), c.seal.intent.source);
    // Normal read-only replay still proves names itself and also refuses.
    f.nodes.finish().unwrap();
    assert!(matches!(
        run(&f, &c, &minimum(&c), &mut conn),
        Err(Error::Journal(LedgerError::Conflict))
    ));
}

#[test]
fn staged_replay_refuses_rebound_frame_or_changed_rows_before_releasing_state() {
    for case in 0..4 {
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
        let base = read(&f, &c, &minimum(&c), ReadLimits::default()).unwrap();
        let mut snapshot = collect(&mut conn, &c.preparation.tail).unwrap();
        let mut port = BudgetPort::new(&mut f.nodes, build_limits()).unwrap();
        let staged = stage_for_index(base, &snapshot, &mut port, build_limits()).unwrap();
        match case {
            0 => snapshot.identity.operation = key("rebound"),
            1 => snapshot.head.hash = Digest::blob(b"other end"),
            2 => {
                snapshot.rows[0].body = encode(&Request {
                    operation: key("complete"),
                    command: Command::CompleteAllocation {
                        allocation: key("allocation"),
                        actual: 49,
                    },
                })
                .unwrap()
            }
            _ => snapshot.rows[0].operation = key("rebound-column"),
        }
        let before = port.used();
        assert!(staged.finish(&snapshot, &mut port).is_err());
        assert_eq!(port.used(), before);
        assert_eq!(f.ledger.checkpoint(), c.seal.intent.source);
    }
}

#[test]
fn joined_preparation_preserves_original_prefixes_and_actual_working_state_after_finish_reopen() {
    let mut f = fixture();
    let c = control(&mut f);
    let mut conn = create(&f, &c);
    let first = append(
        &mut conn,
        &c.preparation.tail,
        "complete",
        Command::CompleteAllocation {
            allocation: key("allocation"),
            actual: 50,
        },
    );
    append(
        &mut conn,
        &c.preparation.tail,
        "finish",
        Command::Finish {
            reservation: key("job"),
        },
    );
    let m = Minimum {
        checkpoint: first.clone(),
        ..minimum(&c)
    };
    let (data, object, metrics) = prepare(&mut f, &c, &m, &mut conn).unwrap();
    let minimum = f.nodes.finish().unwrap();
    let mut reader = MetadataReader::open(&f._temp.path().join("metadata"), &minimum).unwrap();
    assert_eq!(
        read_record::<PreparedData>(&mut reader, &object).unwrap(),
        data
    );
    let index = Index::from_roots(
        data.config.clone(),
        data.source.clone(),
        data.operations.clone(),
        data.sequences.clone(),
    )
    .unwrap();
    assert_eq!(
        index
            .prefix(
                c.seal.intent.source.sequence,
                &mut reader,
                ReadLimits::default()
            )
            .unwrap()
            .0,
        c.seal.intent.source
    );
    assert_eq!(
        index
            .prefix(first.sequence, &mut reader, ReadLimits::default())
            .unwrap()
            .0,
        first
    );
    let old = index
        .inspect(&key("job"), &mut reader, ReadLimits::default())
        .unwrap()
        .unwrap();
    assert!(matches!(old.receipt().response(), Response::Reserved));
    let model = data
        .working_state
        .read(&mut reader, state::Limits::default())
        .unwrap()
        .into_domain_for_context(
            data.config.domain.as_str(),
            data.config.instance.as_str(),
            data.config.floor,
        )
        .unwrap();
    let Output::Allocation(ticket) = f.ledger.outputs.get("allocation").unwrap() else {
        panic!()
    };
    let Output::Reservation(token) = f.ledger.outputs.get("job").unwrap() else {
        panic!()
    };
    assert_eq!(model.allocation_status(ticket).unwrap(), Some(50));
    assert!(model.is_closed(token).unwrap());
    verify_ancestor(
        &Tree::from_root(data.activations),
        &c.identity().unwrap(),
        &data.source.genesis,
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    assert!(metrics.reads > 16 && metrics.writes > 0);
    assert_eq!(f.ledger.checkpoint(), c.seal.intent.source);
}

#[test]
fn full_actual_joined_preparation_fits_one_shared_budget_without_lifetime_cap_waiver() {
    let mut f = fixture();
    while f.ledger.checkpoint().sequence < MAX_COMMANDS {
        f.ledger
            .invalidate(key(&format!("legacy-{}", f.ledger.checkpoint().sequence)))
            .unwrap();
    }
    refresh(&mut f);
    let c = control(&mut f);
    let mut conn = create(&f, &c);
    eprintln!("joined original physical journal/index16384 ready");
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
    let end = head(&conn, &c.preparation.tail).unwrap().0;
    let m = Minimum {
        checkpoint: end.clone(),
        ..minimum(&c)
    };
    let (data, object, metrics) = prepare(&mut f, &c, &m, &mut conn).unwrap();
    assert_eq!(data.source.sequence, 32768);
    assert!(
        metrics.reads <= 2_000_000
            && metrics.read_bytes <= 512 * 1024 * 1024
            && metrics.writes <= 1_000_000
            && metrics.write_bytes <= 512 * 1024 * 1024
    );
    let minimum = f.nodes.finish().unwrap();
    let mut reader = MetadataReader::open(&f._temp.path().join("metadata"), &minimum).unwrap();
    assert_eq!(
        read_record::<PreparedData>(&mut reader, &object).unwrap(),
        data
    );
    let index = Index::from_roots(
        data.config.clone(),
        end.clone(),
        data.operations,
        data.sequences,
    )
    .unwrap();
    for sequence in [1, 16384, 16385, 32768] {
        assert_eq!(
            index
                .prefix(sequence, &mut reader, ReadLimits::default())
                .unwrap()
                .0
                .sequence,
            sequence
        );
    }
    assert_eq!(
        index
            .prefix(16384, &mut reader, ReadLimits::default())
            .unwrap()
            .0,
        c.seal.intent.source
    );
    let model = data
        .working_state
        .read(&mut reader, state::Limits::default())
        .unwrap()
        .into_domain_for_context(
            data.config.domain.as_str(),
            data.config.instance.as_str(),
            data.config.floor,
        )
        .unwrap();
    let Output::Allocation(ticket) = f.ledger.outputs.get("allocation").unwrap() else {
        panic!()
    };
    assert_eq!(model.allocation_status(ticket).unwrap(), Some(50));
    assert_eq!(f.ledger.checkpoint().sequence, 16384);
}
