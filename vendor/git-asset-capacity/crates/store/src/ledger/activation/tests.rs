use super::*;
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn setup() -> (tempfile::TempDir, PathBuf, CapacityLedger, Intent) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    let mut ledger = CapacityLedger::create(&root, key("fixture"), key("epoch"), 0).unwrap();
    let p = ledger.begin_probe(key("probe")).unwrap();
    ledger.observe(key("sample"), p.reference(), 10000).unwrap();
    let job = ledger.reserve(key("job"), key("owner"), 1000).unwrap();
    let a = ledger
        .begin_allocation(key("done"), job.reference(), key("done"), 100)
        .unwrap();
    ledger
        .complete_allocation(key("complete"), a.reference(), 25)
        .unwrap();
    ledger
        .begin_allocation(key("pending"), job.reference(), key("pending"), 200)
        .unwrap();
    let source = ledger.checkpoint();
    let intent = Intent {
        schema: "tig.capacity-activation-intent.design-1".into(),
        operation: key("activate"),
        authority: source.genesis.clone(),
        generation: 1,
        source,
        prepared: Object::from_bytes(b"public prepared fixture metadata"),
        predecessor: None,
    };
    (temp, root, ledger, intent)
}
#[test]
fn atomic_guards_preserve_nonempty_prefix_and_exact_retry_is_readonly() {
    let (_temp, root, mut ledger, intent) = setup();
    let before = ledger.accounting().unwrap();
    assert!(inspect(&ledger.conn).unwrap().is_none());
    let receipt = seal(&mut ledger, &intent, |_| Ok(())).unwrap();
    assert_eq!(ledger.checkpoint(), intent.source);
    assert!(matches!(
        ledger.accounting(),
        Err(LedgerError::NeedsRecovery)
    ));
    let mut reopened = CapacityLedger::open(&root, &intent.source).unwrap();
    assert_eq!(reopened.accounting().unwrap(), before);
    let retry = seal(&mut reopened, &intent, |_| {
        panic!("retry must not execute transition hooks")
    })
    .unwrap();
    assert_eq!(retry, receipt);
    let mut conflict = CapacityLedger::open(&root, &intent.source).unwrap();
    let mut other = intent.clone();
    other.generation = 2;
    assert!(matches!(
        seal(&mut conflict, &other, |_| Ok(())),
        Err(LedgerError::Conflict)
    ));
    let mut old = CapacityLedger::open(&root, &intent.source).unwrap();
    assert!(matches!(
        old.invalidate(key("forbidden")),
        Err(LedgerError::Sql(_))
    ));
    assert_eq!(old.checkpoint(), intent.source);
    assert_eq!(inspect(&old.conn).unwrap(), Some(receipt));
}
#[test]
fn source_advance_and_invalid_intent_never_install_controls() {
    let (_temp, _root, mut ledger, mut intent) = setup();
    intent.generation = 0;
    assert!(seal(&mut ledger, &intent, |_| Ok(())).is_err());
    assert!(inspect(&ledger.conn).unwrap().is_none());
    intent.generation = 1;
    ledger.invalidate(key("advance")).unwrap();
    assert!(matches!(
        seal(&mut ledger, &intent, |_| Ok(())),
        Err(LedgerError::Conflict)
    ));
    assert!(inspect(&ledger.conn).unwrap().is_none());
}
#[test]
fn partial_missing_or_corrupt_controls_refuse_without_repair() {
    let (_temp, root, mut ledger, intent) = setup();
    ledger.conn.execute(TABLE, []).unwrap();
    assert!(matches!(inspect(&ledger.conn), Err(LedgerError::Corrupt)));
    assert!(matches!(
        seal(&mut ledger, &intent, |_| Ok(())),
        Err(LedgerError::Corrupt)
    ));
    drop(ledger);
    let mut fresh = CapacityLedger::open(&root, &intent.source).unwrap();
    assert!(matches!(
        seal(&mut fresh, &intent, |_| Ok(())),
        Err(LedgerError::Corrupt)
    ));
    let (_temp2, _root2, mut ledger, intent) = setup();
    seal(&mut ledger, &intent, |_| Ok(())).unwrap();
    ledger
        .conn
        .execute("DROP TRIGGER git_asset_activation_events_INSERT", [])
        .unwrap();
    assert!(matches!(inspect(&ledger.conn), Err(LedgerError::Corrupt)));
}
#[test]
fn injection_before_commit_rolls_back_and_lost_reply_replays_the_original_fence() {
    for boundary in [Stage::Checked, Stage::Written, Stage::Committed] {
        let (_temp, root, mut ledger, intent) = setup();
        assert!(matches!(
            seal(&mut ledger, &intent, |s| if s == boundary {
                Err(LedgerError::Injected)
            } else {
                Ok(())
            }),
            Err(LedgerError::Injected)
        ));
        drop(ledger);
        let mut reopened = CapacityLedger::open(&root, &intent.source).unwrap();
        assert_eq!(
            inspect(&reopened.conn).unwrap().is_some(),
            boundary == Stage::Committed
        );
        let first = seal(&mut reopened, &intent, |_| Ok(())).unwrap();
        let mut again = CapacityLedger::open(&root, &intent.source).unwrap();
        assert_eq!(seal(&mut again, &intent, |_| Ok(())).unwrap(), first);
    }
}
#[test]
fn fence_exit_child() {
    let Ok(root) = std::env::var("GIT_ASSET_FENCE_CHILD_ROOT") else {
        return;
    };
    let intent: Intent = decode(
        std::env::var("GIT_ASSET_FENCE_CHILD_INTENT")
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    let stage = std::env::var("GIT_ASSET_FENCE_CHILD_STAGE").unwrap();
    let mut ledger = CapacityLedger::open(Path::new(&root), &intent.source).unwrap();
    seal(&mut ledger, &intent, |s| {
        if format!("{s:?}") == stage {
            std::process::exit(77)
        }
        Ok(())
    })
    .unwrap();
    panic!("child must exit at requested boundary")
}
#[test]
fn actual_process_exits_recover_precommit_rollback_and_postcommit_fence() {
    for stage in [Stage::Written, Stage::Committed] {
        let (_temp, root, ledger, intent) = setup();
        drop(ledger);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ledger::activation::tests::fence_exit_child",
                "--nocapture",
            ])
            .env("GIT_ASSET_FENCE_CHILD_ROOT", &root)
            .env(
                "GIT_ASSET_FENCE_CHILD_INTENT",
                String::from_utf8(intent.bytes().unwrap()).unwrap(),
            )
            .env("GIT_ASSET_FENCE_CHILD_STAGE", format!("{stage:?}"))
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(77),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut recovered = CapacityLedger::open(&root, &intent.source).unwrap();
        assert_eq!(
            inspect(&recovered.conn).unwrap().is_some(),
            stage == Stage::Committed
        );
        assert_eq!(recovered.accounting().unwrap().reserved_remaining, 975);
        seal(&mut recovered, &intent, |_| Ok(())).unwrap();
    }
}
#[test]
fn older_transaction_owns_write_slot_and_other_domain_still_progresses() {
    let (temp, root, mut ledger, intent) = setup();
    let mut peer = connect(&root.join("ledger.sqlite")).unwrap();
    let tx = peer
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    assert!(matches!(
        seal(&mut ledger, &intent, |_| Ok(())),
        Err(LedgerError::Busy)
    ));
    let mut other =
        CapacityLedger::create(&temp.path().join("other"), key("other"), key("epoch"), 0).unwrap();
    other.invalidate(key("independent-progress")).unwrap();
    let request = Request {
        operation: key("peer-progress"),
        command: Command::Invalidate,
    };
    let body = encode(&request).unwrap();
    let next = intent.source.sequence + 1;
    let hash = event_hash(next, &intent.source.hash, &request).unwrap();
    tx.execute(
        "INSERT INTO events VALUES(?1,?2,?3,?4,?5)",
        params![
            sql_number(next).unwrap(),
            request.operation.as_str(),
            body,
            intent.source.hash.as_str(),
            hash.as_str()
        ],
    )
    .unwrap();
    tx.execute(
        "UPDATE meta SET sequence=?1,hash=?2,bytes=bytes+?3 WHERE id=1",
        params![sql_number(next).unwrap(), hash.as_str(), body.len() as i64],
    )
    .unwrap();
    tx.commit().unwrap();
    assert!(matches!(
        seal(&mut ledger, &intent, |_| Ok(())),
        Err(LedgerError::Conflict)
    ));
    assert!(inspect(&ledger.conn).unwrap().is_none());
}

struct Actor(Option<std::process::Child>);
impl Drop for Actor {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
#[test]
#[ignore = "requires explicitly hashed timing-instrumented pinned legacy executable"]
fn pinned_legacy_writer_after_guard_is_fenced_by_rust_sealer() {
    let exe = PathBuf::from(
        std::env::var("GIT_ASSET_ACTIVATION_LEGACY_EXE").expect("explicit legacy executable"),
    );
    let proof_path = PathBuf::from(
        std::env::var("GIT_ASSET_ACTIVATION_LEGACY_REPORT").expect("explicit legacy proof"),
    );
    assert!(fs::metadata(&exe).unwrap().len() <= 128 * 1024 * 1024);
    assert!(fs::metadata(&proof_path).unwrap().len() <= 65536);
    let proof: serde_json::Value = serde_json::from_slice(&fs::read(proof_path).unwrap()).unwrap();
    assert_eq!(
        proof["legacy_source"],
        "c833add85f9a9c79d3387a78e469bc13a2570403"
    );
    assert_eq!(
        proof["original_ledger_sha256"],
        "98a043d49d73d193a07a65cfb06c05f8b672ee8a0e385f30b8f3ef3ffda435ed"
    );
    assert_eq!(
        proof["patched_ledger_sha256"],
        "1f8760d6bea94440bdd4da24dde2ff64a93858213922ac3ba036b0d405cbb991"
    );
    assert_eq!(proof["timing_instrumented"], true);
    assert_eq!(
        proof["executable_sha256"],
        Digest::blob(&fs::read(&exe).unwrap()).as_str()
    );
    let temp = tempfile::tempdir().unwrap();
    let case = temp.path().join("case");
    fs::create_dir(&case).unwrap();
    fs::write(case.join("owner.json"),serde_json::to_vec(&serde_json::json!({"schema":"tig.activation-legacy-case.design-1","legacy_source":"c833add85f9a9c79d3387a78e469bc13a2570403"})).unwrap()).unwrap();
    let prepared = std::process::Command::new(&exe)
        .args(["--exact", "prepare_legacy_source", "--nocapture"])
        .env("TIG_ACTIVATION_CASE", &case)
        .env_remove("TIG_ACTIVATION_READY")
        .env_remove("TIG_ACTIVATION_RELEASE")
        .output()
        .unwrap();
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(case.join("prepared-source.json")).unwrap()).unwrap();
    let source: Checkpoint = serde_json::from_value(record["minimum"].clone()).unwrap();
    let root = case.join("source");
    let mut ledger = CapacityLedger::open(&root, &source).unwrap();
    let intent = Intent {
        schema: "tig.capacity-activation-intent.design-1".into(),
        operation: key("rust-seal"),
        authority: source.genesis.clone(),
        generation: 1,
        source: source.clone(),
        prepared: Object::from_bytes(b"explicit admitted synthetic prepared metadata"),
        predecessor: None,
    };
    let ready = case.join("ready");
    let release = case.join("release");
    let child = std::process::Command::new(&exe)
        .args(["--exact", "delayed_legacy_writer", "--nocapture"])
        .env("TIG_ACTIVATION_CASE", &case)
        .env("TIG_ACTIVATION_READY", &ready)
        .env("TIG_ACTIVATION_RELEASE", &release)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = Actor(Some(child));
    let started = std::time::Instant::now();
    while !ready.is_file()
        || fs::read(&ready).unwrap_or_default() != b"after-guard-before-transaction"
    {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(15),
            "old writer barrier timeout"
        );
        assert!(
            child.0.as_mut().unwrap().try_wait().unwrap().is_none(),
            "old writer exited before barrier"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let receipt = seal(&mut ledger, &intent, |_| Ok(())).unwrap();
    fs::write(release, b"released-after-rust-seal").unwrap();
    let started = std::time::Instant::now();
    while child.0.as_mut().unwrap().try_wait().unwrap().is_none() {
        assert!(started.elapsed() < std::time::Duration::from_secs(30));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let output = child.0.take().unwrap().wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value =
        serde_json::from_slice(&fs::read(case.join("writer-result.json")).unwrap()).unwrap();
    assert_eq!(result["sql_guard_refused"], true);
    assert_eq!(result["ordinary_prefix_unchanged"], true);
    assert_eq!(result["inflight_preserved"], true);
    assert_eq!(result["fresh_probe_required"], true);
    assert_eq!(head(&ledger.conn).unwrap().0, source);
    assert_eq!(inspect(&ledger.conn).unwrap(), Some(receipt));
    let count: i64 = ledger
        .conn
        .query_row(
            "SELECT count(*) FROM events WHERE operation='after-guard-write'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}
#[test]
fn canonical_intent_and_control_corruption_are_bounded_and_not_repaired() {
    for mode in 0..5 {
        let (_temp, _root, mut ledger, intent) = setup();
        seal(&mut ledger, &intent, |_| Ok(())).unwrap();
        ledger
            .conn
            .execute(
                "DROP TRIGGER git_asset_activation_git_asset_activation_UPDATE",
                [],
            )
            .unwrap();
        let mut value = serde_json::to_value(&intent).unwrap();
        match mode {
            0 => value["generation"] = 0.into(),
            1 => value["prepared"]["bytes"] = 0.into(),
            2 => value["extra"] = true.into(),
            3 => value["schema"] = "future".into(),
            4 => value["source"]["hash"] = Digest::blob(b"other-head").as_str().into(),
            _ => unreachable!(),
        }
        let raw = identity::canonical_json(&value).unwrap();
        let hash = Digest::blob(&raw);
        ledger
            .conn
            .execute(
                "UPDATE git_asset_activation SET body=?1,hash=?2 WHERE id=1",
                params![raw, hash.as_str()],
            )
            .unwrap();
        ledger
            .conn
            .execute(
                &definitions()["git_asset_activation_git_asset_activation_UPDATE"].1,
                [],
            )
            .unwrap();
        assert!(
            matches!(inspect(&ledger.conn), Err(LedgerError::Corrupt)),
            "mode {mode}"
        );
    }
}

#[test]
fn independently_pinned_canonical_intent_does_not_self_hash() {
    let raw=br#"{"authority":"0000000000000000000000000000000000000000000000000000000000000000","generation":1,"operation":"fixture","prepared":{"bytes":2,"sha256":"44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"},"schema":"tig.capacity-activation-intent.design-1","source":{"genesis":"0000000000000000000000000000000000000000000000000000000000000000","hash":"0000000000000000000000000000000000000000000000000000000000000000","sequence":0}}"#;
    let intent: Intent = decode(raw).unwrap();
    assert_eq!(intent.bytes().unwrap(), raw);
    assert_eq!(
        Digest::blob(raw).as_str(),
        "b56af8979fd269c86b6586e2f9548af9fc70d952d3c5b473c8e85d9b17c6b94e"
    );
}
#[test]
fn full_original_journal_seals_without_an_extra_event_or_counter_reset() {
    let (_temp, root, mut ledger, mut intent) = setup();
    while ledger.checkpoint.sequence < MAX_COMMANDS {
        let op = key(&format!("full-{}", ledger.checkpoint.sequence));
        ledger.invalidate(op).unwrap();
    }
    intent.source = ledger.checkpoint();
    let before = head(&ledger.conn).unwrap();
    assert!(matches!(
        ledger.invalidate(key("over-original-cap")),
        Err(LedgerError::Budget)
    ));
    seal(&mut ledger, &intent, |_| Ok(())).unwrap();
    assert_eq!(head(&ledger.conn).unwrap(), before);
    drop(ledger);
    let mut reopened = CapacityLedger::open(&root, &intent.source).unwrap();
    let token = reopened.domain.reserve("job", "owner", 1000).unwrap();
    assert_eq!(
        reopened.domain.finish(&token),
        Err(capacity::CapacityError::InFlight)
    );
    assert_eq!(inspect(&reopened.conn).unwrap().unwrap().intent, intent);
}
