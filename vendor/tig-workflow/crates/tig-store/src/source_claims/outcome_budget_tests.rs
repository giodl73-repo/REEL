use super::*;
fn k(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn selection() -> Selection {
    Selection {
        root: Object::from_bytes(b"public outcome fixture"),
        context: Digest::blob(b"context"),
    }
}
fn fixture() -> (tempfile::TempDir, SourceJournal) {
    let t = tempfile::tempdir().unwrap();
    let j = SourceJournal::create(&t.path().join("journal"), k("project"), k("epoch")).unwrap();
    (t, j)
}
fn pair(j: &SourceJournal) -> Plan {
    let p = k("planned-prepare");
    let a = k("planned-select");
    Plan {
        steps: vec![
            Step {
                operation: p.clone(),
                command: Box::new(Command::Prepare {
                    source: k("reserved"),
                    owner: k("owner"),
                    expected_revision: 0,
                    selection: selection(),
                }),
            },
            Step {
                operation: a.clone(),
                command: Box::new(Command::Adopt {
                    source: k("reserved"),
                    owner: k("owner"),
                    expected_revision: 1,
                    prepared: Reference {
                        journal: j.identity().clone(),
                        operation: p.clone(),
                    },
                }),
            },
        ],
        admissions: vec![],
        endings: vec![vec![p, a]],
    }
}
fn normalize(mut p: Plan) -> Plan {
    p.steps
        .sort_by(|a, b| a.operation.as_str().cmp(b.operation.as_str()));
    for end in &mut p.endings {
        end.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    }
    p
}
fn run(j: &mut SourceJournal, s: &Step) -> Receipt {
    j.apply(s.operation.clone(), (*s.command).clone()).unwrap()
}
/// Valid synthetic prefix construction, not thousands of separate durable effects.
fn append(tx: &rusqlite::Transaction<'_>, state: &mut State, request: Request) -> Result<()> {
    let body = encode(&request)?;
    let (bytes, cancel) = crate::source_claims::admit(state, &request, body.len() as u64)?;
    state.transition(&request)?;
    let sequence = state.head.sequence + 1;
    let hash = event_hash(sequence, &state.head.hash, &request)?;
    tx.execute(
        "INSERT INTO events VALUES(?1,?2,?3,?4,?5)",
        rusqlite::params![
            sequence as i64,
            request.operation.as_str(),
            body,
            state.head.hash.as_str(),
            hash.as_str()
        ],
    )?;
    state.head.sequence = sequence;
    state.head.hash = hash;
    state.bytes = bytes;
    state.cancel_bytes = cancel;
    Ok(())
}
fn finish(tx: rusqlite::Transaction<'_>, state: &State) {
    tx.execute(
        "UPDATE meta SET sequence=?1,hash=?2,bytes=?3 WHERE id=1",
        rusqlite::params![
            state.head.sequence as i64,
            state.head.hash.as_str(),
            state.bytes as i64
        ],
    )
    .unwrap();
    tx.commit().unwrap();
}
fn noise(state: &State) -> Request {
    let source = k("pressure-noise");
    let s = state.sources.get(source.as_str());
    let revision = s.map_or(0, |s| s.revision);
    let command = match s.and_then(|s| s.pending.as_ref()) {
        Some(p) => Command::Abort {
            source,
            owner: k("noise-owner"),
            expected_revision: revision,
            prepared: p.reference.clone(),
        },
        None => Command::Prepare {
            source,
            owner: k("noise-owner"),
            expected_revision: revision,
            selection: selection(),
        },
    };
    Request {
        operation: k(&format!("noise-{}", state.head.sequence)),
        command,
    }
}
pub(crate) fn exhaust_events(j: &mut SourceJournal) {
    let mut state = j.state.clone();
    let tx = j.conn.transaction().unwrap();
    loop {
        let request = noise(&state);
        match append(&tx, &mut state, request) {
            Ok(()) => {}
            Err(Error::Budget) => break,
            Err(e) => panic!("pressure: {e}"),
        }
    }
    finish(tx, &state);
    j.state = state;
}
pub(crate) fn exhaust_bytes(j: &mut SourceJournal) {
    let mut state = j.state.clone();
    let tx = j.conn.transaction().unwrap();
    let mut count = 0;
    loop {
        let owner = k(&"o".repeat(128));
        let source = k(&"s".repeat(128));
        let id = k(&format!("byte-allocation-{count}"));
        let mut steps = Vec::new();
        for rev in (0..8).step_by(2) {
            let p = k(&format!("p-{count}-{rev}-{}", "x".repeat(100)));
            let a = k(&format!("a-{count}-{rev}-{}", "y".repeat(100)));
            steps.push(Step {
                operation: p.clone(),
                command: Box::new(Command::Prepare {
                    source: source.clone(),
                    owner: owner.clone(),
                    expected_revision: rev,
                    selection: selection(),
                }),
            });
            steps.push(Step {
                operation: a,
                command: Box::new(Command::Abort {
                    source: source.clone(),
                    owner: owner.clone(),
                    expected_revision: rev + 1,
                    prepared: Reference {
                        journal: state.head.genesis.clone(),
                        operation: p,
                    },
                }),
            });
        }
        let endings = vec![steps.iter().map(|s| s.operation.clone()).collect()];
        let plan = normalize(Plan {
            steps,
            admissions: vec![],
            endings,
        });
        let creation = operation("obr", &state.head.genesis, &id).unwrap();
        let reference = BudgetRef {
            journal: state.head.genesis.clone(),
            id: id.clone(),
            owner: owner.clone(),
            creation: creation.clone(),
        };
        let request = Request {
            operation: creation,
            command: Command::ReserveOutcomes { id, owner, plan },
        };
        let body = encode(&request).unwrap();
        if crate::source_claims::admit(&state, &request, body.len() as u64) == Err(Error::Budget) {
            break;
        }
        append(&tx, &mut state, request).unwrap();
        let close = operation("obf", &state.head.genesis, &reference.id).unwrap();
        append(
            &tx,
            &mut state,
            Request {
                operation: close,
                command: Command::ReleaseOutcomes { budget: reference },
            },
        )
        .unwrap();
        count += 1;
    }
    assert!(count > 100);
    loop {
        let request = noise(&state);
        match append(&tx, &mut state, request) {
            Ok(()) => {}
            Err(Error::Budget) => break,
            Err(e) => panic!("byte pressure: {e}"),
        }
    }
    finish(tx, &state);
    j.state = state;
}
#[test]
fn exact_scope_reopen_and_terminal_settlement_preserve_legacy_command_bytes() {
    let (t, mut j) = fixture();
    let p = normalize(pair(&j));
    let r = reserve(&mut j, k("room"), k("owner"), p.clone()).unwrap();
    let prepare = p
        .steps
        .iter()
        .find(|s| matches!(*s.command, Command::Prepare { .. }))
        .unwrap();
    let adopt = p
        .steps
        .iter()
        .find(|s| matches!(*s.command, Command::Adopt { .. }))
        .unwrap();
    assert_eq!(
        j.apply(k("wrong-operation"), (*prepare.command).clone()),
        Err(Error::Budget)
    );
    let mut changed = (*prepare.command).clone();
    if let Command::Prepare { selection, .. } = &mut changed {
        selection.context = Digest::blob(b"changed")
    }
    assert_eq!(
        j.apply(prepare.operation.clone(), changed),
        Err(Error::Conflict)
    );
    j.apply(
        k("unrelated"),
        Command::Prepare {
            source: k("other"),
            owner: k("owner"),
            expected_revision: 0,
            selection: selection(),
        },
    )
    .unwrap();
    run(&mut j, prepare);
    assert_eq!(release(&mut j, &r), Err(Error::Pending));
    let cp = j.checkpoint();
    j = SourceJournal::open(&t.path().join("journal"), &cp).unwrap();
    let receipt = run(&mut j, adopt);
    release(&mut j, &r).unwrap();
    assert_eq!(held(&j.state).unwrap(), (0, 0));
    assert!(run(&mut j, adopt).replayed);
    assert_eq!(run(&mut j, adopt).reference, receipt.reference);
}
#[test]
fn closed_and_conflicting_allocations_cannot_rebind_unspent_phase_ids() {
    let (_t, mut j) = fixture();
    let p = normalize(pair(&j));
    let r = reserve(&mut j, k("room"), k("owner"), p.clone()).unwrap();
    assert_eq!(
        reserve(&mut j, k("other-room"), k("owner"), p.clone()),
        Err(Error::Conflict)
    );
    release(&mut j, &r).unwrap();
    assert_eq!(
        reserve(&mut j, k("room"), k("owner"), p.clone()),
        Err(Error::Conflict)
    );
    assert_eq!(
        reserve(&mut j, k("new-room"), k("owner"), p),
        Err(Error::Conflict)
    );
}
#[test]
fn admission_and_outcome_remain_covered_under_full_ordinary_event_pressure() {
    let (_t, mut j) = fixture();
    let p = normalize(pair(&j));
    run(&mut j, &p.steps[0]);
    run(&mut j, &p.steps[1]);
    let next = k("next-prepare");
    let a = k("next-select");
    let step = Step {
        operation: a.clone(),
        command: Box::new(Command::Adopt {
            source: k("reserved"),
            owner: k("owner"),
            expected_revision: 3,
            prepared: Reference {
                journal: j.identity().clone(),
                operation: next.clone(),
            },
        }),
    };
    let plan = Plan {
        steps: vec![step.clone()],
        admissions: vec![Admission {
            operation: next.clone(),
            source: k("reserved"),
            owner: k("owner"),
            revision: 2,
        }],
        endings: vec![vec![a]],
    };
    let r = reserve(&mut j, k("next-room"), k("owner"), plan).unwrap();
    exhaust_events(&mut j);
    assert_eq!(
        progress_budget::capacity(&mut j).unwrap().available_events,
        0
    );
    j.apply(
        next,
        Command::Prepare {
            source: k("reserved"),
            owner: k("owner"),
            expected_revision: 2,
            selection: selection(),
        },
    )
    .unwrap();
    assert_eq!(release(&mut j, &r), Err(Error::Pending));
    run(&mut j, &step);
    release(&mut j, &r).unwrap();
}
#[test]
fn event_pressure_cannot_spend_reserved_outcome_and_release_room() {
    let (t, mut j) = fixture();
    let p = normalize(pair(&j));
    let r = reserve(&mut j, k("room"), k("owner"), p.clone()).unwrap();
    exhaust_events(&mut j);
    assert_eq!(
        progress_budget::capacity(&mut j).unwrap().available_events,
        0
    );
    let cp = j.checkpoint();
    j = SourceJournal::open(&t.path().join("journal"), &cp).unwrap();
    for step in &p.steps {
        run(&mut j, step);
    }
    release(&mut j, &r).unwrap();
    assert_eq!(held(&j.state).unwrap(), (0, 0));
}

#[test]
fn reserved_missing_source_slot_survives_a_full_source_table() {
    let (t, mut j) = fixture();
    let p = normalize(pair(&j));
    let r = reserve(&mut j, k("room"), k("owner"), p.clone()).unwrap();
    let mut state = j.state.clone();
    let tx = j.conn.transaction().unwrap();
    for i in 0..MAX_SOURCES - 1 {
        append(
            &tx,
            &mut state,
            Request {
                operation: k(&format!("slot-{i}")),
                command: Command::Prepare {
                    source: k(&format!("occupied-{i}")),
                    owner: k("noise-owner"),
                    expected_revision: 0,
                    selection: selection(),
                },
            },
        )
        .unwrap();
    }
    finish(tx, &state);
    j.state = state;
    let cp = j.checkpoint();
    j = SourceJournal::open(&t.path().join("journal"), &cp).unwrap();
    assert_eq!(
        j.apply(
            k("steal-slot"),
            Command::Prepare {
                source: k("unreserved-source"),
                owner: k("owner"),
                expected_revision: 0,
                selection: selection()
            }
        ),
        Err(Error::Budget)
    );
    for step in &p.steps {
        run(&mut j, step);
    }
    release(&mut j, &r).unwrap();
    assert_eq!(j.state.sources.len(), MAX_SOURCES);
}
#[test]
fn progress_and_exact_outcome_allocations_cannot_overlap_the_same_source() {
    let (_t, mut j) = fixture();
    let p0 = j
        .apply(
            k("control-prepare"),
            Command::Prepare {
                source: k("control"),
                owner: k("owner"),
                expected_revision: 0,
                selection: selection(),
            },
        )
        .unwrap();
    j.apply(
        k("control-select"),
        Command::Adopt {
            source: k("control"),
            owner: k("owner"),
            expected_revision: 1,
            prepared: p0.reference,
        },
    )
    .unwrap();
    let guards = vec![Guard {
        source: k("control"),
        owner: k("owner"),
        expected_revision: 2,
    }];
    let old = progress_budget::reserve(
        &mut j,
        k("progress"),
        k("owner"),
        k("reserved"),
        0,
        guards.clone(),
    )
    .unwrap();
    let p = normalize(pair(&j));
    assert_eq!(
        reserve(&mut j, k("room"), k("owner"), p.clone()),
        Err(Error::Conflict)
    );
    progress_budget::release(&mut j, &old).unwrap();
    let r = reserve(&mut j, k("room"), k("owner"), p).unwrap();
    assert!(
        progress_budget::reserve(&mut j, k("overlap"), k("owner"), k("reserved"), 0, guards)
            .is_err()
    );
    release(&mut j, &r).unwrap();
}
