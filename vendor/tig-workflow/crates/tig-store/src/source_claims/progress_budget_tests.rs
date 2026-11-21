use super::*;
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn selection() -> Selection {
    Selection {
        root: Object::from_bytes(b"public fixture"),
        context: Digest::blob(b"context"),
    }
}
fn fixture() -> (tempfile::TempDir, SourceJournal, Vec<Guard>) {
    let temp = tempfile::tempdir().unwrap();
    let mut j =
        SourceJournal::create(&temp.path().join("journal"), key("runtime"), key("epoch")).unwrap();
    let p = j
        .apply(
            key("parent-prepare"),
            Command::Prepare {
                source: key("parent"),
                owner: key("owner"),
                expected_revision: 0,
                selection: selection(),
            },
        )
        .unwrap();
    j.apply(
        key("parent-adopt"),
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
    (temp, j, guards)
}
#[test]
fn reserve_wrap_reopen_settle_and_historical_replay_keep_capacity_exact() {
    let (t, mut j, guards) = fixture();
    let original = capacity(&mut j).unwrap();
    let r = reserve(
        &mut j,
        key("quota"),
        key("owner"),
        key("work"),
        0,
        guards.clone(),
    )
    .unwrap();
    assert_eq!(capacity(&mut j).unwrap().held_events, EVENTS);
    let p = j
        .apply(
            key("begin-prepare"),
            r.wrap(Command::Prepare {
                source: key("work"),
                owner: key("owner"),
                expected_revision: 0,
                selection: selection(),
            }),
        )
        .unwrap();
    j.apply(
        key("begin-adopt"),
        r.wrap(Command::AdoptGuarded {
            source: key("work"),
            owner: key("owner"),
            expected_revision: 1,
            prepared: p.reference,
            guards,
        }),
    )
    .unwrap();
    assert_eq!(release(&mut j, &r), Err(Error::Pending));
    let cp = j.checkpoint();
    j = SourceJournal::open(&t.path().join("journal"), &cp).unwrap();
    let p = j
        .apply(
            key("outcome-prepare"),
            r.wrap(Command::Prepare {
                source: key("work"),
                owner: key("owner"),
                expected_revision: 2,
                selection: selection(),
            }),
        )
        .unwrap();
    j.apply(
        key("outcome-adopt"),
        r.wrap(Command::Adopt {
            source: key("work"),
            owner: key("owner"),
            expected_revision: 3,
            prepared: p.reference,
        }),
    )
    .unwrap();
    release(&mut j, &r).unwrap();
    assert_eq!(capacity(&mut j).unwrap().held_events, 0);
    assert_eq!(
        capacity(&mut j).unwrap().available_events,
        original.available_events - 6
    );
    let cp = j.checkpoint();
    release(&mut j, &r).unwrap();
    assert_eq!(j.checkpoint(), cp);
}
#[test]
fn wrong_scope_early_release_and_unwrapped_writes_cannot_spend_reserved_room() {
    let (_temp, mut j, guards) = fixture();
    let r = reserve(
        &mut j,
        key("quota"),
        key("owner"),
        key("work"),
        0,
        guards.clone(),
    )
    .unwrap();
    let before = j.checkpoint();
    assert!(
        j.apply(
            key("wrong-source"),
            r.wrap(Command::Prepare {
                source: key("other"),
                owner: key("owner"),
                expected_revision: 0,
                selection: selection()
            })
        )
        .is_err()
    );
    assert!(
        j.apply(
            key("unwrapped"),
            Command::Prepare {
                source: key("work"),
                owner: key("owner"),
                expected_revision: 0,
                selection: selection()
            }
        )
        .is_err()
    );
    assert!(reserve(&mut j, key("overlap"), key("owner"), key("work"), 0, guards).is_err());
    assert_eq!(j.checkpoint(), before);
    release(&mut j, &r).unwrap();
    assert!(
        j.apply(
            key("after-close"),
            r.wrap(Command::Prepare {
                source: key("work"),
                owner: key("owner"),
                expected_revision: 0,
                selection: selection()
            })
        )
        .is_err()
    );
}

#[test]
fn stale_peer_cannot_overlap_or_reopen_a_settled_reservation() {
    let (temp, mut j, guards) = fixture();
    let mut peer = SourceJournal::open(&temp.path().join("journal"), &j.checkpoint()).unwrap();
    let r = reserve(
        &mut j,
        key("quota"),
        key("owner"),
        key("work"),
        0,
        guards.clone(),
    )
    .unwrap();
    assert_eq!(
        reserve(
            &mut peer,
            key("other-quota"),
            key("owner"),
            key("work"),
            0,
            guards.clone()
        ),
        Err(Error::Conflict)
    );
    assert_eq!(
        reserve(
            &mut peer,
            key("quota"),
            key("owner"),
            key("work"),
            0,
            guards.clone()
        )
        .unwrap(),
        r
    );
    release(&mut peer, &r).unwrap();
    assert_eq!(
        reserve(&mut j, key("quota"), key("owner"), key("work"), 0, guards),
        Err(Error::Conflict)
    );
    assert_eq!(capacity(&mut j).unwrap().held_events, 0);
}

/// Assemble a valid prefix in one transaction. Normal reopen must validate every
/// event; this is byte pressure, not thousands of individually durable IO effects.
#[test]
fn byte_pressure_preserves_an_admitted_outcome_and_settlement() {
    use rusqlite::params;
    let (temp, mut j, guards) = fixture();
    let r = reserve(
        &mut j,
        key("quota"),
        key("owner"),
        key("work"),
        0,
        guards.clone(),
    )
    .unwrap();
    let p = j
        .apply(
            key("begin-prepare"),
            r.wrap(Command::Prepare {
                source: key("work"),
                owner: key("owner"),
                expected_revision: 0,
                selection: selection(),
            }),
        )
        .unwrap();
    j.apply(
        key("begin-adopt"),
        r.wrap(Command::AdoptGuarded {
            source: key("work"),
            owner: key("owner"),
            expected_revision: 1,
            prepared: p.reference,
            guards,
        }),
    )
    .unwrap();
    let mut guards = Vec::new();
    for i in 0..16 {
        let source = key(&format!("g{i:02}-{}", "s".repeat(120)));
        let owner = key(&"o".repeat(124));
        let p = j
            .apply(
                key(&format!("g{i}-prepare")),
                Command::Prepare {
                    source: source.clone(),
                    owner: owner.clone(),
                    expected_revision: 0,
                    selection: selection(),
                },
            )
            .unwrap();
        j.apply(
            key(&format!("g{i}-adopt")),
            Command::Adopt {
                source: source.clone(),
                owner: owner.clone(),
                expected_revision: 1,
                prepared: p.reference,
            },
        )
        .unwrap();
        guards.push(Guard {
            source,
            owner,
            expected_revision: 2,
        });
    }
    let mut state = j.state.clone();
    let tx = j.conn.transaction().unwrap();
    let mut count = 0;
    loop {
        let id = key(&format!("fill-{count}"));
        let creation = key(&format!("allocate-{count}"));
        let reference = BudgetRef {
            journal: j.state.head.genesis.clone(),
            id: id.clone(),
            owner: key("owner"),
            creation: creation.clone(),
        };
        let request = Request {
            operation: creation,
            command: Command::ReserveProgress {
                id,
                owner: key("owner"),
                source: key("filler-work"),
                first_revision: 0,
                guards: guards.clone(),
            },
        };
        let length = encode(&request).unwrap().len() as u64;
        if crate::source_claims::admit(&state, &request, length) == Err(Error::Budget) {
            break;
        }
        for request in [
            request,
            Request {
                operation: key(&format!("settle-{count}")),
                command: Command::ReleaseProgress { budget: reference },
            },
        ] {
            let body = encode(&request).unwrap();
            let (bytes, cancel_bytes) =
                crate::source_claims::admit(&state, &request, body.len() as u64).unwrap();
            state.transition(&request).unwrap();
            let sequence = state.head.sequence + 1;
            let hash = event_hash(sequence, &state.head.hash, &request).unwrap();
            tx.execute(
                "INSERT INTO events VALUES(?1,?2,?3,?4,?5)",
                params![
                    sequence as i64,
                    request.operation.as_str(),
                    body,
                    state.head.hash.as_str(),
                    hash.as_str()
                ],
            )
            .unwrap();
            state.head.sequence = sequence;
            state.head.hash = hash;
            state.bytes = bytes;
            state.cancel_bytes = cancel_bytes;
        }
        count += 1;
    }
    assert!(count > 1000);
    assert!(state.head.sequence < MAX_COMMANDS / 2);
    tx.execute(
        "UPDATE meta SET sequence=?1,hash=?2,bytes=?3 WHERE id=1",
        params![
            state.head.sequence as i64,
            state.head.hash.as_str(),
            state.bytes as i64
        ],
    )
    .unwrap();
    tx.commit().unwrap();
    j = SourceJournal::open(&temp.path().join("journal"), &state.head).unwrap();
    let room = capacity(&mut j).unwrap();
    assert_eq!(room.held_events, 3);
    assert!(room.available_bytes < BYTES + 8192);
    let p = j
        .apply(
            key("outcome-prepare"),
            r.wrap(Command::Prepare {
                source: key("work"),
                owner: key("owner"),
                expected_revision: 2,
                selection: selection(),
            }),
        )
        .unwrap();
    j.apply(
        key("outcome-adopt"),
        r.wrap(Command::Adopt {
            source: key("work"),
            owner: key("owner"),
            expected_revision: 3,
            prepared: p.reference,
        }),
    )
    .unwrap();
    release(&mut j, &r).unwrap();
    assert_eq!(capacity(&mut j).unwrap().held_events, 0);
}
