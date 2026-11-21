use super::*;
use crate::clock::{Enrollment, Policy};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn d(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn policy() -> Policy {
    Policy {
        authority: d("authority"),
        epoch: d("epoch"),
        boundary: d("native-boundary"),
        max_sample_millis: 20,
        max_drift_millis: 5,
        max_session_millis: 1000,
    }
}
fn clock(root: &Path) -> CapacityLedger {
    CapacityLedger::create_clock(root, key("clock"), key("epoch"), 0, policy()).unwrap()
}
#[test]
fn independent_clock_marker_policy_enrollment_and_event_vectors_pin_legacy_bytes() {
    let vectors: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/vectors/local-clock.design-1.json"
    ))
    .unwrap();
    for case in vectors["cases"].as_array().unwrap() {
        let raw = case["canonical"].as_str().unwrap().as_bytes();
        let value: serde_json::Value = identity::parse_json(raw).unwrap();
        let expected = case["id"].as_str().unwrap();
        match case["kind"].as_str().unwrap() {
            "policy" => {
                let p: Policy = serde_json::from_value(value).unwrap();
                assert_eq!(p.id().unwrap().as_str(), expected);
                assert_eq!(encode(&p).unwrap(), raw);
            }
            "enrollment" => {
                let e: Enrollment = serde_json::from_value(value).unwrap();
                assert_eq!(e.id().unwrap().as_str(), expected);
                assert_eq!(encode(&e).unwrap(), raw);
            }
            "event" => {
                let r: Request = serde_json::from_value(value["request"].clone()).unwrap();
                let previous: Digest = serde_json::from_value(value["previous"].clone()).unwrap();
                assert_eq!(event_hash(1, &previous, &r).unwrap().as_str(), expected);
                assert_eq!(
                    encode(&HashEnvelope {
                        schema: "tig.capacity-event.design-1",
                        sequence: 1,
                        previous: &previous,
                        request: &r
                    })
                    .unwrap(),
                    raw
                );
            }
            _ => {
                let config: Config = serde_json::from_value(value).unwrap();
                assert_eq!(Digest::blob(&encode(&config).unwrap()).as_str(), expected);
                assert_eq!(encode(&config).unwrap(), raw);
            }
        }
    }
}
#[test]
fn clock_floor_replay_conflicts_and_capacity_invalidation_never_restore_old_time() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("clock");
    let mut ledger = clock(&root);
    let initial = ledger.checkpoint();
    assert!(ledger.clock_high_water().unwrap().is_none());
    let first = ledger.observe_clock(key("one"), 100, true).unwrap();
    assert!(!first.was_replayed());
    ledger.observe_clock(key("two"), 101, true).unwrap();
    let replay = ledger.observe_clock(key("one"), 100, true).unwrap();
    assert!(replay.was_replayed());
    assert!(matches!(replay.response(), Response::TimeObserved(100)));
    assert_eq!(ledger.clock_high_water().unwrap(), Some(101));
    assert!(matches!(
        ledger.observe_clock(key("one"), 102, true),
        Err(LedgerError::Conflict)
    ));
    assert!(matches!(
        ledger.observe_clock(key("back"), 100, false),
        Err(LedgerError::Clock)
    ));
    ledger.invalidate(key("invalidate")).unwrap();
    assert_eq!(ledger.clock_high_water().unwrap(), Some(101));
    assert!(matches!(
        ledger.observe_clock(key("equal"), 101, true),
        Err(LedgerError::Clock)
    ));
    ledger
        .observe_clock(key("same-invocation"), 101, false)
        .unwrap();
    drop(ledger);
    let mut reopened = CapacityLedger::open(&root, &initial).unwrap();
    assert_eq!(reopened.clock_high_water().unwrap(), Some(101));
    let mut legacy =
        CapacityLedger::create(&temp.path().join("legacy"), key("legacy"), key("epoch"), 0)
            .unwrap();
    assert!(matches!(
        legacy.observe_clock(key("forbidden"), 100, true),
        Err(LedgerError::Clock)
    ));
    assert_eq!(legacy.checkpoint().sequence(), 0);
}
#[test]
fn known_clock_checkpoint_refuses_restored_database_prefix() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("clock");
    let mut ledger = clock(&root);
    ledger.observe_clock(key("one"), 100, true).unwrap();
    let old = ledger.checkpoint();
    drop(ledger);
    std::fs::copy(
        root.join("ledger.sqlite"),
        temp.path().join("before.sqlite"),
    )
    .unwrap();
    let mut ledger = CapacityLedger::open(&root, &old).unwrap();
    ledger.observe_clock(key("two"), 101, true).unwrap();
    let latest = ledger.checkpoint();
    drop(ledger);
    std::fs::copy(
        temp.path().join("before.sqlite"),
        root.join("ledger.sqlite"),
    )
    .unwrap();
    assert!(matches!(
        CapacityLedger::open(&root, &latest),
        Err(LedgerError::Rollback)
    ));
}
#[test]
fn clock_command_budget_exhaustion_preserves_floor_and_historical_replies() {
    let temp = tempfile::tempdir().unwrap();
    let mut ledger = clock(&temp.path().join("clock"));
    for n in 1..=MAX_COMMANDS {
        ledger
            .observe_clock(key(&format!("tick-{n}")), n, true)
            .unwrap();
    }
    let before = ledger.checkpoint();
    assert!(matches!(
        ledger.observe_clock(key("overflow"), MAX_COMMANDS + 1, true),
        Err(LedgerError::Budget)
    ));
    assert_eq!(ledger.clock_high_water().unwrap(), Some(MAX_COMMANDS));
    assert_eq!(ledger.checkpoint(), before);
    assert!(
        ledger
            .observe_clock(key("tick-1"), 1, true)
            .unwrap()
            .was_replayed()
    );
    let e = ledger.clock_enrollment().unwrap();
    let id = e.id().unwrap();
    let port = crate::clock::LocalClock::new(ledger, e).unwrap();
    assert!(matches!(
        port.sample(&id),
        Err(crate::clock::Error::Ledger(LedgerError::Budget))
    ));
    assert_eq!(port.status().unwrap().checkpoint, before);
}
#[test]
fn clock_peers_serialize_only_their_authority_and_never_lower_the_floor() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("clock");
    let ledger = clock(&root);
    let initial = ledger.checkpoint();
    drop(ledger);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads = (0..2)
        .map(|n| {
            let root = root.clone();
            let initial = initial.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let opened = CapacityLedger::open(&root, &initial);
                barrier.wait();
                opened.and_then(|mut l| l.observe_clock(key(&format!("peer-{n}")), 100 + n, true))
            })
        })
        .collect::<Vec<_>>();
    for t in threads {
        assert!(matches!(
            t.join().unwrap(),
            Ok(_) | Err(LedgerError::Busy | LedgerError::Clock)
        ));
    }
    let mut ledger = CapacityLedger::open(&root, &initial).unwrap();
    ledger.observe_clock(key("serial"), 102, true).unwrap();
    assert_eq!(ledger.clock_high_water().unwrap(), Some(102));
}
#[test]
fn clock_exit_child() {
    let Some(root) = std::env::var_os("GIT_ASSET_CLOCK_CHILD_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let minimum: Checkpoint =
        serde_json::from_slice(&fs::read(root.join("minimum.json")).unwrap()).unwrap();
    let mut ledger = CapacityLedger::open(&root.join("clock"), &minimum).unwrap();
    let mode = std::env::var("GIT_ASSET_CLOCK_CHILD_MODE").unwrap();
    ledger
        .apply_hook(
            key("child-time"),
            Command::ObserveClock {
                millis: 100,
                strict: true,
            },
            |stage| {
                if mode
                    == match stage {
                        Stage::Validated => "validated",
                        Stage::Written => "written",
                        Stage::Committed => "committed",
                        Stage::Guarded => "guarded",
                    }
                {
                    std::process::exit(77);
                }
                Ok(())
            },
        )
        .unwrap();
    panic!("child did not reach requested boundary");
}
#[test]
fn actual_clock_child_exits_recover_only_committed_floor_before_time_can_be_used() {
    for mode in ["validated", "written", "committed"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("clock");
        let ledger = clock(&root);
        let initial = ledger.checkpoint();
        drop(ledger);
        fs::write(
            temp.path().join("minimum.json"),
            serde_json::to_vec(&initial).unwrap(),
        )
        .unwrap();
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ledger::clock_tests::clock_exit_child",
                "--nocapture",
            ])
            .env("GIT_ASSET_CLOCK_CHILD_ROOT", temp.path())
            .env("GIT_ASSET_CLOCK_CHILD_MODE", mode)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(77), "{out:?}");
        let mut ledger = CapacityLedger::open(&root, &initial).unwrap();
        assert_eq!(
            ledger.clock_high_water().unwrap(),
            if mode == "committed" { Some(100) } else { None }
        );
        let reply = ledger.observe_clock(key("child-time"), 100, true).unwrap();
        assert_eq!(reply.was_replayed(), mode == "committed");
        assert_eq!(ledger.clock_high_water().unwrap(), Some(100));
    }
}
