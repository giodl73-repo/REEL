use super::*;
use crate::{retirement_intent::Error, source_claims};
use std::fs;

fn fill_source_slots(j: &mut SourceJournal) {
    for index in 0..source_claims::MAX_SOURCES {
        let result = j.apply(
            key(&format!("pressure-stop-{index}")),
            Command::Cancel {
                source: key(&format!("pressure-slot-{index}")),
                owner: key("pressure-owner"),
                expected_revision: 0,
            },
        );
        if matches!(result, Err(source_claims::Error::Budget)) {
            return;
        }
        result.unwrap();
    }
    panic!("expected the real source-slot bound");
}
/// Construct a valid bounded prefix in one fixture transaction, then require
/// normal journal catch-up/replay to validate it. Not 16,384 runtime IO effects.
fn fill_ordinary_history(f: &mut Fixture) {
    use rusqlite::params;
    let checkpoint = serde_json::to_value(f.j.checkpoint()).unwrap();
    let genesis = f.j.identity().clone();
    let mut previous = Digest::parse(checkpoint["hash"].as_str().unwrap()).unwrap();
    let room = source_claims::progress_budget::capacity(&mut f.j).unwrap();
    let end = f.j.checkpoint().sequence() + room.available_events;
    let first = f.j.checkpoint().sequence() + 1;
    let mut connection =
        rusqlite::Connection::open(f.temp.path().join("journal/claims.sqlite")).unwrap();
    let tx = connection.transaction().unwrap();
    let mut bytes: u64 = tx
        .query_row("SELECT bytes FROM meta WHERE id=1", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap() as u64;
    for sequence in first..=end {
        let offset = sequence - first;
        let op = key(&format!("bulk-pressure-{sequence}"));
        let command = if offset.is_multiple_of(2) {
            Command::Prepare {
                source: key("ordinary-pressure"),
                owner: key("pressure-owner"),
                expected_revision: offset,
                selection: Selection {
                    root: Object::from_bytes(b"public control fixture"),
                    context: d("pressure"),
                },
            }
        } else {
            let reference = serde_json::from_value(serde_json::json!({"journal":genesis,
                "operation":format!("bulk-pressure-{}", sequence-1)}))
            .unwrap();
            Command::Abort {
                source: key("ordinary-pressure"),
                owner: key("pressure-owner"),
                expected_revision: offset,
                prepared: reference,
            }
        };
        let request = serde_json::json!({"operation":op,"command":command});
        let body = encode(&request).unwrap();
        let event = serde_json::json!({"schema":"tig.source-event.design-1","sequence":sequence,
            "previous":previous,"request":request});
        let hash = Digest::blob(&encode(&event).unwrap());
        tx.execute(
            "INSERT INTO events VALUES(?1,?2,?3,?4,?5)",
            params![
                sequence as i64,
                op.as_str(),
                body,
                previous.as_str(),
                hash.as_str()
            ],
        )
        .unwrap();
        bytes += body.len() as u64;
        previous = hash;
    }
    tx.execute(
        "UPDATE meta SET sequence=?1,hash=?2,bytes=?3 WHERE id=1",
        params![end as i64, previous.as_str(), bytes as i64],
    )
    .unwrap();
    tx.commit().unwrap();
}
#[test]
fn group_slot_is_acknowledged_before_members_and_full_source_table_cannot_block_stop() {
    let mut f = Fixture::new_block();
    let b = planned(&mut f);
    let (a, attempt) = anchor(&b);
    assert!(
        start_hook(&mut f.j, &mut f.n, &f.ctx, b.clone(), |j, _| {
            let groups = groups(j, &key("controller"), 16)?;
            assert_eq!(groups.len(), 1);
            assert_eq!(groups[0].revision, 1);
            fill_source_slots(j);
            Err(Error::Invalid)
        })
        .is_err()
    );
    let groups = groups(&mut f.j, &key("controller"), 16).unwrap();
    let stop = stop_group(&mut f.j, &key("controller"), &groups[0]).unwrap();
    assert_eq!(stop.source, *groups[0].source());
    assert!(f.j.source(groups[0].source()).unwrap().unwrap().cancelled);
    cancel(&mut f.j, &mut f.n, &f.ctx, a, attempt).unwrap();
    assert!(
        inspect(&mut f.j, &f.n, &f.ctx, a)
            .unwrap()
            .in_flight
            .is_none()
    );
    assert!(apply(&mut f.j, &f.n, &f.ctx, a, attempt).is_err());
    assert_eq!(f.retained(), f.old);
}
#[test]
fn full_source_table_before_reservation_refuses_every_member_admission() {
    let mut f = Fixture::new_block();
    let b = planned(&mut f);
    fill_source_slots(&mut f.j);
    let before = f.j.checkpoint();
    assert_eq!(
        start(&mut f.j, &mut f.n, &f.ctx, b.clone()),
        Err(Error::Journal(source_claims::Error::Budget))
    );
    assert_eq!(before, f.j.checkpoint());
    for member in &b.members {
        assert!(
            inspect(&mut f.j, &f.n, &f.ctx, &member.object)
                .unwrap()
                .in_flight
                .is_none()
        );
    }
    assert_eq!(f.retained(), f.old);
}
#[test]
fn prepared_discovery_is_scoped_bounded_and_stop_needs_no_manifest_store() {
    for admitted in [false, true] {
        let mut f = Fixture::new_block();
        let b = planned(&mut f);
        let root = put(
            &mut f.n,
            &encode(&BlockRecord {
                schema: BLOCK_SCHEMA.into(),
                block: b.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        prepare_group(&mut f.j, &mut f.n, &f.ctx, &root, &b).unwrap();
        if admitted {
            start(&mut f.j, &mut f.n, &f.ctx, b.clone()).unwrap();
        }
        assert!(
            groups(&mut f.j, &key("other-owner"), 16)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            groups(&mut f.j, &key("controller"), 0),
            Err(Error::Journal(source_claims::Error::Budget))
        );
        let p = groups(&mut f.j, &key("controller"), 16).unwrap().remove(0);
        let checkpoint = f.n.checkpoint();
        drop(f.n);
        fs::rename(
            f.temp.path().join("nodes"),
            f.temp.path().join("nodes.offline"),
        )
        .unwrap();
        assert!(matches!(
            stop_group(&mut f.j, &key("wrong-owner"), &p),
            Err(Error::Owner)
        ));
        let stopped = stop_group(&mut f.j, &key("controller"), &p).unwrap();
        assert_eq!(stopped.revision, if admitted { 3 } else { 2 });
        assert_eq!(
            stopped.revision,
            stop_group(&mut f.j, &key("controller"), &p)
                .unwrap()
                .revision
        );
        fs::rename(
            f.temp.path().join("nodes.offline"),
            f.temp.path().join("nodes"),
        )
        .unwrap();
        f.n = MetadataStore::open(&f.temp.path().join("nodes"), &checkpoint).unwrap();
        assert!(matches!(
            resume_group(&mut f.j, &mut f.n, &f.ctx, &p),
            Err(Error::Cancelled)
        ));
        assert_eq!(f.retained(), f.old);
    }
}

#[test]
fn ordinary_event_exhaustion_keeps_reserved_stop_and_step_outcome_capacity() {
    let mut f = Fixture::new_block();
    let b = planned(&mut f);
    let (a, attempt) = anchor(&b);
    assert!(
        start_hook(&mut f.j, &mut f.n, &f.ctx, b.clone(), |_, _| Err(
            Error::Invalid
        ))
        .is_err()
    );
    fill_ordinary_history(&mut f);
    f.reopen(); // Validate the constructed prefix through normal journal replay.
    assert_eq!(
        source_claims::progress_budget::capacity(&mut f.j)
            .unwrap()
            .available_events,
        0
    );
    assert_eq!(
        source_claims::progress_budget::capacity(&mut f.j)
            .unwrap()
            .held_events,
        3
    );
    let p = groups(&mut f.j, &key("controller"), 16).unwrap().remove(0);
    let stopped = stop_group(&mut f.j, &key("controller"), &p).unwrap();
    assert_eq!(
        stopped.checkpoint.sequence(),
        source_claims::MAX_COMMANDS - 2
    );
    cancel(&mut f.j, &mut f.n, &f.ctx, a, attempt).unwrap();
    assert_eq!(
        source_claims::progress_budget::capacity(&mut f.j)
            .unwrap()
            .held_events,
        0
    );
    assert_eq!(f.j.checkpoint().sequence(), source_claims::MAX_COMMANDS + 1);
    assert!(f.j.source(p.source()).unwrap().unwrap().cancelled);
    assert!(
        inspect(&mut f.j, &f.n, &f.ctx, a)
            .unwrap()
            .in_flight
            .is_none()
    );
    assert!(
        inspect(&mut f.j, &f.n, &f.ctx, a)
            .unwrap()
            .released
            .is_empty()
    );
    assert!(apply(&mut f.j, &f.n, &f.ctx, a, attempt).is_err());
    assert_eq!(f.retained(), f.old);
}
#[test]
fn exhausted_manifest_capacity_refuses_group_and_members_before_journal_mutation() {
    let mut f = Fixture::new_block();
    let b = planned(&mut f);
    for index in 0..2500 {
        if f.n.checkpoint().nodes() + f.n.pending_nodes() as u64 == 2000 {
            break;
        }
        f.n.put(format!("public metadata pressure {index}").as_bytes())
            .unwrap();
    }
    f.n.finish().unwrap();
    let metadata = f.n.checkpoint();
    let before = f.j.checkpoint();
    assert!(start(&mut f.j, &mut f.n, &f.ctx, b.clone()).is_err());
    assert_eq!(before, f.j.checkpoint());
    f.n = MetadataStore::open(&f.temp.path().join("nodes"), &metadata).unwrap();
    assert!(groups(&mut f.j, &key("controller"), 16).unwrap().is_empty());
    for m in b.members {
        assert!(
            inspect(&mut f.j, &f.n, &f.ctx, &m.object)
                .unwrap()
                .in_flight
                .is_none()
        );
    }
}
