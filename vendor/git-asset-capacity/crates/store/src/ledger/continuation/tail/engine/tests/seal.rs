use super::*;

pub(super) fn intent(l: &CapacityLedger, c: &ActiveControl) -> SealIntent {
    SealIntent {
        schema: "tig.capacity-activation-intent.design-2".into(),
        operation: key("next-activation"),
        authority: l.identity().clone(),
        generation: c.preparation.tail.generation + 1,
        source: l.checkpoint(),
        prepared: Object::from_bytes(b"explicit synthetic second preparation"),
        predecessor: Some(c.identity().unwrap()),
    }
}

#[test]
fn actual_continued_source_seal_is_inspectable_and_fences_every_mutation() {
    let mut f = fixture();
    let c = activate(&mut f);
    let mut l = reopened(&f, &c, &minimum(&c));
    l.invalidate(key("continued-write")).unwrap();
    let next = intent(&l, &c);
    activation::seal(&mut l, &next, |_| Ok(())).unwrap();
    assert!(matches!(
        l.invalidate(key("after-seal")),
        Err(LedgerError::NeedsRecovery)
    ));
    assert!(
        open(
            &l.root,
            &c,
            &minimum(&c),
            f.nodes.finished_reader().unwrap()
        )
        .is_err()
    );
    let conn = connect(&l.root.join(c.preparation.tail.relative_name().unwrap())).unwrap();
    let raw = next.to_json().unwrap();
    activation::verify_seal(&conn, &raw, &Digest::blob(&raw)).unwrap();
    let mut conflict = next.clone();
    conflict.operation = key("conflict");
    let raw = conflict.to_json().unwrap();
    assert!(activation::verify_seal(&conn, &raw, &Digest::blob(&raw)).is_err());
    for table in ["events", "meta", "git_asset_activation"] {
        for action in ["INSERT", "UPDATE", "DELETE"] {
            let sql = match action {
                "INSERT" => format!("INSERT INTO {table} SELECT * FROM {table} LIMIT 1"),
                "UPDATE" => format!(
                    "UPDATE {table} SET {}={}",
                    if table == "events" { "sequence" } else { "id" },
                    if table == "events" { "sequence" } else { "id" }
                ),
                _ => format!("DELETE FROM {table}"),
            };
            assert!(conn.execute(&sql, []).is_err(), "{table}/{action}");
        }
    }
    activation::verify_seal(
        &conn,
        &next.to_json().unwrap(),
        &Digest::blob(&next.to_json().unwrap()),
    )
    .unwrap();
}

#[test]
fn wrong_predecessor_or_generation_does_not_seal_the_tail() {
    let mut f = fixture();
    let c = activate(&mut f);
    let mut l = reopened(&f, &c, &minimum(&c));
    let valid = intent(&l, &c);
    for change in 0..4 {
        let mut next = valid.clone();
        match change {
            0 => next.predecessor.as_mut().unwrap().digest = Digest::blob(b"other"),
            1 => next.generation += 1,
            2 => next.predecessor = None,
            _ => {
                next.schema = "tig.capacity-activation-intent.design-1".into();
                next.predecessor = None;
            }
        }
        assert!(activation::seal(&mut l, &next, |_| Ok(())).is_err());
        schema(&l.conn).unwrap();
    }
    l.invalidate(key("still-writable")).unwrap();
}

#[test]
fn source_advance_and_writer_paused_after_file_guard_are_fenced() {
    let mut f = fixture();
    let c = activate(&mut f);
    let mut l = reopened(&f, &c, &minimum(&c));
    let mut peer = reopened(&f, &c, &minimum(&c));
    let stale = intent(&l, &c);
    peer.invalidate(key("peer-advance")).unwrap();
    assert!(matches!(
        activation::seal(&mut l, &stale, |_| Ok(())),
        Err(LedgerError::Conflict)
    ));
    let next = intent(&peer, &c);
    let result = l.apply_hook(key("paused-write"), Command::Invalidate, |stage| {
        if stage == super::super::Stage::Guarded {
            activation::seal(&mut peer, &next, |_| Ok(())).unwrap();
        }
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(l.checkpoint(), next.source);
    let conn = connect(&l.root.join(c.preparation.tail.relative_name().unwrap())).unwrap();
    activation::verify_seal(
        &conn,
        &next.to_json().unwrap(),
        &Digest::blob(&next.to_json().unwrap()),
    )
    .unwrap();
}

#[test]
fn continued_seal_rollback_and_lost_reply_preserve_observable_source() {
    for boundary in [
        activation::Stage::Checked,
        activation::Stage::Written,
        activation::Stage::Committed,
    ] {
        let mut f = fixture();
        let c = activate(&mut f);
        let mut l = reopened(&f, &c, &minimum(&c));
        l.invalidate(key("continued-write")).unwrap();
        let next = intent(&l, &c);
        assert!(matches!(
            activation::seal(&mut l, &next, |stage| {
                if stage == boundary {
                    Err(LedgerError::Injected)
                } else {
                    Ok(())
                }
            }),
            Err(LedgerError::Injected)
        ));
        assert!(matches!(l.accounting(), Err(LedgerError::NeedsRecovery)));
        let conn = connect(&l.root.join(c.preparation.tail.relative_name().unwrap())).unwrap();
        if boundary == activation::Stage::Committed {
            activation::verify_seal(
                &conn,
                &next.to_json().unwrap(),
                &Digest::blob(&next.to_json().unwrap()),
            )
            .unwrap();
        } else {
            schema(&conn).unwrap();
            drop(conn);
            let mut again = reopened(&f, &c, &minimum(&c));
            assert_eq!(again.checkpoint(), next.source);
            activation::seal(&mut again, &next, |_| Ok(())).unwrap();
        }
    }
}

#[test]
fn continued_seal_exit_child() {
    let Ok(root) = std::env::var("GIT_ASSET_CONTINUED_SEAL_ROOT") else {
        return;
    };
    let c = ActiveControl::from_json(
        std::env::var("GIT_ASSET_CONTINUED_SEAL_CONTROL")
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    let metadata = std::env::var("GIT_ASSET_CONTINUED_SEAL_METADATA").unwrap();
    let reader = MetadataReader::open(Path::new(&metadata), &c.preparation.metadata).unwrap();
    let mut l = open(Path::new(&root), &c, &minimum(&c), reader).unwrap();
    let next: SealIntent = decode(
        std::env::var("GIT_ASSET_CONTINUED_SEAL_INTENT")
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    let boundary = std::env::var("GIT_ASSET_CONTINUED_SEAL_STAGE").unwrap();
    activation::seal(&mut l, &next, |stage| {
        if format!("{stage:?}") == boundary {
            std::process::exit(77)
        }
        Ok(())
    })
    .unwrap();
    panic!("child must exit at requested seal boundary")
}

#[test]
fn actual_continued_process_exits_preserve_rollback_and_committed_seal() {
    for boundary in ["Written", "Committed"] {
        let mut f = fixture();
        let c = activate(&mut f);
        let mut l = reopened(&f, &c, &minimum(&c));
        l.invalidate(key("continued-child-prefix")).unwrap();
        let next = intent(&l, &c);
        let root = l.root.clone();
        drop(l);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ledger::continuation::tail::engine::tests::seal::continued_seal_exit_child",
                "--nocapture",
            ])
            .env("GIT_ASSET_CONTINUED_SEAL_ROOT", &root)
            .env(
                "GIT_ASSET_CONTINUED_SEAL_METADATA",
                f._temp.path().join("metadata"),
            )
            .env(
                "GIT_ASSET_CONTINUED_SEAL_CONTROL",
                String::from_utf8(c.to_json().unwrap()).unwrap(),
            )
            .env(
                "GIT_ASSET_CONTINUED_SEAL_INTENT",
                String::from_utf8(next.to_json().unwrap()).unwrap(),
            )
            .env("GIT_ASSET_CONTINUED_SEAL_STAGE", boundary)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(77),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let conn = connect(&root.join(c.preparation.tail.relative_name().unwrap())).unwrap();
        assert_eq!(head(&conn, &c.preparation.tail).unwrap().0, next.source);
        if boundary == "Written" {
            schema(&conn).unwrap();
            drop(conn);
            let mut recovered = reopened(&f, &c, &minimum(&c));
            activation::seal(&mut recovered, &next, |_| Ok(())).unwrap();
        } else {
            activation::verify_seal(
                &conn,
                &next.to_json().unwrap(),
                &Digest::blob(&next.to_json().unwrap()),
            )
            .unwrap();
            assert!(open(&root, &c, &minimum(&c), f.nodes.finished_reader().unwrap()).is_err());
        }
    }
}

#[test]
fn partial_guard_extra_schema_and_rebound_header_are_not_observed_seals() {
    for change in 0..3 {
        let mut f = fixture();
        let c = activate(&mut f);
        let mut l = reopened(&f, &c, &minimum(&c));
        let next = intent(&l, &c);
        activation::seal(&mut l, &next, |_| Ok(())).unwrap();
        match change {
            0 => l
                .conn
                .execute_batch("DROP TRIGGER git_asset_activation_meta_UPDATE")
                .unwrap(),
            1 => l
                .conn
                .execute_batch("CREATE TABLE unexpected(value TEXT)")
                .unwrap(),
            _ => {
                l.conn
                    .execute_batch("DROP TRIGGER git_asset_tail_UPDATE")
                    .unwrap();
                let mut wrong = c.preparation.tail.clone();
                wrong.generation += 1;
                let raw = bytes(&wrong).unwrap();
                l.conn
                    .execute(
                        "UPDATE git_asset_tail SET body=?1,hash=?2",
                        params![raw, Digest::blob(&raw).as_str()],
                    )
                    .unwrap();
                l.conn
                    .execute(&definitions()["git_asset_tail_UPDATE"].1, [])
                    .unwrap();
            }
        }
        assert!(
            activation::verify_seal(
                &l.conn,
                &next.to_json().unwrap(),
                &Digest::blob(&next.to_json().unwrap())
            )
            .is_err()
        );
    }
}
