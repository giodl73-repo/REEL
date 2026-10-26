use super::*;
use std::{
    io::{BufRead, BufReader, Read},
    process::{Child, Command as ProcessCommand, Stdio},
    sync::{Arc, Barrier},
};
use tempfile::TempDir;

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
struct Fixture {
    _temp: TempDir,
    root: PathBuf,
    witness: Checkpoint,
}
impl Fixture {
    fn new(floor: u64) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("ledger");
        let ledger = CapacityLedger::create(&root, key("volume"), key("epoch-1"), floor).unwrap();
        Self {
            _temp: temp,
            root,
            witness: ledger.checkpoint(),
        }
    }
    fn open(&self) -> CapacityLedger {
        CapacityLedger::open(&self.root, &self.witness).unwrap()
    }
}
fn probe(l: &mut CapacityLedger, id: &str, free: u64) {
    let p = l.begin_probe(key(&format!("probe-{id}"))).unwrap();
    l.observe(key(&format!("sample-{id}")), p.reference(), free)
        .unwrap();
}
fn reserved(l: &mut CapacityLedger, id: &str, peak: u64) -> Receipt {
    probe(l, id, 1000);
    l.reserve(key(id), key("owner"), peak).unwrap()
}

#[test]
fn reservations_survive_reopen_but_old_free_space_does_not_authorize_new_work() {
    let f = Fixture::new(100);
    let mut l = f.open();
    let r = reserved(&mut l, "job", 200);
    let witness = l.checkpoint();
    drop(l);
    let mut l = CapacityLedger::open(&f.root, &witness).unwrap();
    assert_eq!(l.accounting().unwrap().reserved_remaining, 200);
    assert!(matches!(
        l.reserve(key("new"), key("owner"), 1),
        Err(LedgerError::NeedsProbe)
    ));
    assert!(matches!(
        l.reserve(key("job"), key("owner"), 200).unwrap().response(),
        Response::Reserved
    ));
    l.finish(key("finish"), r.reference()).unwrap();
    assert_eq!(l.accounting().unwrap().available, 900);
}
#[test]
fn exact_operation_retries_return_original_checkpoint_and_reject_rebinding() {
    let f = Fixture::new(100);
    let mut l = f.open();
    let one = reserved(&mut l, "job", 200);
    let two = l.reserve(key("job"), key("owner"), 200).unwrap();
    assert!(!one.was_replayed());
    assert!(two.was_replayed());
    assert_eq!(one.checkpoint(), two.checkpoint());
    assert!(matches!(
        l.reserve(key("job"), key("other"), 200),
        Err(LedgerError::Conflict)
    ));
    assert!(matches!(
        l.reserve(key("job"), key("owner"), 201),
        Err(LedgerError::Conflict)
    ));
    assert_eq!(l.accounting().unwrap().reserved_remaining, 200);
}
#[test]
fn inflight_write_and_its_completion_replay_without_a_capacity_gap() {
    let f = Fixture::new(100);
    let mut l = f.open();
    let r = reserved(&mut l, "job", 200);
    let a = l
        .begin_allocation(key("allocate"), r.reference(), key("write"), 100)
        .unwrap();
    drop(l);
    let mut l = f.open();
    assert!(matches!(
        l.finish(key("finish"), r.reference()),
        Err(LedgerError::Capacity(capacity::CapacityError::InFlight))
    ));
    l.complete_allocation(key("complete"), a.reference(), 80)
        .unwrap();
    l.finish(key("finish"), r.reference()).unwrap();
    drop(l);
    let mut l = f.open();
    let s = l.accounting().unwrap();
    assert_eq!(
        (
            s.reserved_remaining,
            s.allocated_after_watermark,
            s.available
        ),
        (0, 80, 820)
    );
    probe(&mut l, "after", 920);
    assert_eq!(l.accounting().unwrap().available, 820);
}
#[test]
fn old_probe_references_cannot_be_promoted_to_fresh_after_restart() {
    let f = Fixture::new(0);
    let mut l = f.open();
    let p = l.begin_probe(key("old-probe")).unwrap();
    drop(l);
    let mut l = f.open();
    l.begin_probe(key("old-probe")).unwrap();
    assert!(matches!(
        l.observe(key("sample"), p.reference(), 1000),
        Err(LedgerError::NeedsProbe)
    ));
    probe(&mut l, "new", 1000);
    l.reserve(key("job"), key("owner"), 100).unwrap();
}
#[test]
fn each_new_admission_consumes_a_fresh_probe_and_expired_probes_do_not_count() {
    let f = Fixture::new(0);
    let mut l = f.open();
    probe(&mut l, "first", 1000);
    l.reserve(key("one"), key("owner"), 100).unwrap();
    assert!(matches!(
        l.reserve(key("two"), key("owner"), 100),
        Err(LedgerError::NeedsProbe)
    ));
    probe(&mut l, "aged", 1000);
    l.fresh = Some(Instant::now() - FRESH_FOR - Duration::from_secs(1));
    assert!(matches!(
        l.reserve(key("two"), key("owner"), 100),
        Err(LedgerError::NeedsProbe)
    ));
    let p = l.begin_probe(key("aged-probe")).unwrap();
    l.probes.insert(
        "aged-probe".into(),
        Instant::now() - FRESH_FOR - Duration::from_secs(1),
    );
    assert!(matches!(
        l.observe(key("sample-expired"), p.reference(), 1000),
        Err(LedgerError::NeedsProbe)
    ));
}
#[test]
fn references_bind_a_unique_ledger_instance_and_result_kind() {
    let a = Fixture::new(0);
    let b = Fixture::new(0);
    let mut left = a.open();
    let mut right = b.open();
    let r = reserved(&mut left, "job", 100);
    assert!(matches!(
        right.finish(key("finish"), r.reference()),
        Err(LedgerError::InvalidReference)
    ));
    let p = left.begin_probe(key("wrong-kind")).unwrap();
    let before = left.checkpoint();
    assert!(matches!(
        left.finish(key("finish"), p.reference()),
        Err(LedgerError::InvalidReference)
    ));
    assert_eq!(before, left.checkpoint());
    assert!(matches!(
        CapacityLedger::open(&b.root, &a.witness),
        Err(LedgerError::InvalidReference)
    ));
}
#[test]
fn failed_checked_transition_leaves_no_event_or_core_mutation() {
    let f = Fixture::new(0);
    let mut l = f.open();
    probe(&mut l, "first", 100);
    let before = l.checkpoint();
    assert!(matches!(
        l.reserve(key("job"), key("owner"), 101),
        Err(LedgerError::Capacity(capacity::CapacityError::Insufficient))
    ));
    assert_eq!(before, l.checkpoint());
    assert_eq!(l.accounting().unwrap().reserved_remaining, 0);
    l.reserve(key("job"), key("owner"), 100).unwrap();
}
#[test]
fn warm_handles_replay_only_unseen_tail_events() {
    let f = Fixture::new(0);
    let mut a = f.open();
    let mut b = f.open();
    let r = reserved(&mut a, "job", 100);
    assert_eq!(b.accounting().unwrap().reserved_remaining, 100);
    assert_eq!(b.last_replay_count(), 3);
    b.accounting().unwrap();
    assert_eq!(b.last_replay_count(), 0);
    a.finish(key("finish"), r.reference()).unwrap();
    b.accounting().unwrap();
    assert_eq!(b.last_replay_count(), 1);
}
#[test]
fn historical_retry_response_is_distinct_from_current_accounting() {
    let f = Fixture::new(0);
    let mut l = f.open();
    let p = l.begin_probe(key("probe")).unwrap();
    l.observe(key("sample"), p.reference(), 1000).unwrap();
    l.reserve(key("job"), key("owner"), 200).unwrap();
    let replay = l.observe(key("sample"), p.reference(), 1000).unwrap();
    let Response::Observed(old) = replay.response() else {
        panic!("wrong response")
    };
    assert_eq!(old.available, 1000);
    assert_eq!(l.accounting().unwrap().available, 800);
}
#[test]
fn concurrent_connections_cannot_reserve_the_same_headroom_twice() {
    let f = Fixture::new(0);
    let mut a = f.open();
    let mut b = f.open();
    probe(&mut a, "a", 100);
    probe(&mut b, "b", 100);
    let barrier = Arc::new(Barrier::new(2));
    let start = barrier.clone();
    let one = std::thread::spawn(move || {
        start.wait();
        let r = a.reserve(key("a"), key("owner"), 80);
        (a, r)
    });
    let two = std::thread::spawn(move || {
        barrier.wait();
        let r = b.reserve(key("b"), key("owner"), 80);
        (b, r)
    });
    let (mut a, ra) = one.join().unwrap();
    let (mut b, rb) = two.join().unwrap();
    for result in [&ra, &rb] {
        assert!(
            matches!(
                result,
                Ok(_)
                    | Err(LedgerError::Busy)
                    | Err(LedgerError::Capacity(capacity::CapacityError::Insufficient))
            ),
            "unexpected concurrent result: {result:?}"
        );
    }
    assert!(!(ra.is_ok() && rb.is_ok()));
    // Nonblocking rollback-journal contention may reject both attempts. Once
    // contention stops, exactly one serial retry must acquire the headroom.
    let neither = ra.is_err() && rb.is_err();
    if neither {
        assert!(matches!(ra, Err(LedgerError::Busy)));
        assert!(matches!(rb, Err(LedgerError::Busy)));
        probe(&mut a, "retry-a", 100);
        a.reserve(key("a"), key("owner"), 80).unwrap();
    } else if matches!(ra, Err(LedgerError::Busy)) {
        probe(&mut a, "retry-a", 100);
        assert!(matches!(
            a.reserve(key("a"), key("owner"), 80),
            Err(LedgerError::Capacity(capacity::CapacityError::Insufficient))
        ));
    }
    if matches!(rb, Err(LedgerError::Busy)) {
        probe(&mut b, "retry-b", 100);
        assert!(matches!(
            b.reserve(key("b"), key("owner"), 80),
            Err(LedgerError::Capacity(capacity::CapacityError::Insufficient))
        ));
    }
    assert_eq!(f.open().accounting().unwrap().reserved_remaining, 80);
}
#[test]
fn corrupted_or_missing_database_is_never_an_empty_replacement() {
    let f = Fixture::new(0);
    fs::write(f.root.join("ledger.sqlite"), b"not a database").unwrap();
    assert!(CapacityLedger::open(&f.root, &f.witness).is_err());
    assert_eq!(
        fs::read(f.root.join("ledger.sqlite")).unwrap(),
        b"not a database"
    );
    fs::remove_file(f.root.join("ledger.sqlite")).unwrap();
    assert!(CapacityLedger::open(&f.root, &f.witness).is_err());
    assert!(!f.root.join("ledger.sqlite").exists());
}
#[test]
fn gaps_tampered_events_and_oversize_records_block_recovery() {
    for sql in [
        "DELETE FROM events WHERE sequence=2",
        "UPDATE events SET body=x'7b' WHERE sequence=2",
        "UPDATE events SET body=zeroblob(65537) WHERE sequence=2",
    ] {
        let f = Fixture::new(0);
        let mut l = f.open();
        reserved(&mut l, "job", 100);
        drop(l);
        let conn = Connection::open(f.root.join("ledger.sqlite")).unwrap();
        conn.execute(sql, []).unwrap();
        drop(conn);
        assert!(CapacityLedger::open(&f.root, &f.witness).is_err(), "{sql}");
    }
}
#[test]
fn trusted_minimum_checkpoint_detects_a_consistent_old_backup() {
    let f = Fixture::new(0);
    let mut l = f.open();
    probe(&mut l, "first", 1000);
    drop(l);
    let backup = f._temp.path().join("backup.sqlite");
    fs::copy(f.root.join("ledger.sqlite"), &backup).unwrap();
    let mut l = f.open();
    l.reserve(key("job"), key("owner"), 100).unwrap_err();
    probe(&mut l, "second", 1000);
    l.reserve(key("job"), key("owner"), 100).unwrap();
    let newer = l.checkpoint();
    drop(l);
    fs::copy(backup, f.root.join("ledger.sqlite")).unwrap();
    assert!(matches!(
        CapacityLedger::open(&f.root, &newer),
        Err(LedgerError::Rollback)
    ));
}
#[test]
fn rewritten_witness_or_tail_head_mismatch_does_not_pass() {
    let f = Fixture::new(0);
    let mut l = f.open();
    reserved(&mut l, "job", 100);
    let mut witness = l.checkpoint();
    drop(l);
    witness.hash = Digest::parse(&"0".repeat(64)).unwrap();
    assert!(matches!(
        CapacityLedger::open(&f.root, &witness),
        Err(LedgerError::Rollback)
    ));
    let conn = Connection::open(f.root.join("ledger.sqlite")).unwrap();
    conn.execute("UPDATE meta SET sequence=0,hash=genesis,bytes=0", [])
        .unwrap();
    drop(conn);
    assert!(CapacityLedger::open(&f.root, &f.witness).is_err());
}
#[test]
fn injected_commit_uncertainty_requires_reopen_and_exact_retry() {
    for cut in [Stage::Validated, Stage::Written, Stage::Committed] {
        let f = Fixture::new(0);
        let mut l = f.open();
        probe(&mut l, "first", 1000);
        let witness = l.checkpoint();
        let result = l.apply_hook(
            key("job"),
            Command::Reserve {
                owner: key("owner"),
                peak: 100,
            },
            |stage| {
                if stage == cut {
                    Err(LedgerError::Injected)
                } else {
                    Ok(())
                }
            },
        );
        assert!(matches!(result, Err(LedgerError::Injected)));
        assert!(matches!(l.accounting(), Err(LedgerError::NeedsRecovery)));
        drop(l);
        let mut l = CapacityLedger::open(&f.root, &witness).unwrap();
        let expected = if cut == Stage::Committed { 100 } else { 0 };
        assert_eq!(l.accounting().unwrap().reserved_remaining, expected);
        if cut != Stage::Committed {
            probe(&mut l, "retry", 1000);
        }
        l.reserve(key("job"), key("owner"), 100).unwrap();
        assert_eq!(l.accounting().unwrap().reserved_remaining, 100);
    }
}
#[test]
fn unsafe_numeric_wire_values_reject_before_a_transaction_is_written() {
    let f = Fixture::new(0);
    let mut l = f.open();
    let before = l.checkpoint();
    assert!(l.reserve(key("huge"), key("owner"), u64::MAX).is_err());
    assert_eq!(before, l.checkpoint());
    assert!(
        CapacityLedger::create(&f._temp.path().join("bad"), key("v"), key("e"), u64::MAX).is_err()
    );
    assert!(!f._temp.path().join("bad").exists());
}

struct ChildScope(Child);
impl Drop for ChildScope {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn spawn_child(f: &Fixture, mode: &str) -> ChildScope {
    ChildScope(
        ProcessCommand::new(std::env::current_exe().unwrap())
            .args(["--exact", "ledger::tests::ledger_child", "--nocapture"])
            .env("TIG_LEDGER_TEST_ROOT", &f.root)
            .env(
                "TIG_LEDGER_TEST_WITNESS",
                serde_json::to_string(&f.witness).unwrap(),
            )
            .env("TIG_LEDGER_TEST_MODE", mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    )
}
#[test]
fn ledger_child() {
    let Some(root) = std::env::var_os("TIG_LEDGER_TEST_ROOT") else {
        return;
    };
    let witness: Checkpoint =
        serde_json::from_str(&std::env::var("TIG_LEDGER_TEST_WITNESS").unwrap()).unwrap();
    let mode = std::env::var("TIG_LEDGER_TEST_MODE").unwrap();
    let mut l = CapacityLedger::open(Path::new(&root), &witness).unwrap();
    if mode == "hold" {
        l.conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        println!("READY");
        std::io::stdout().flush().unwrap();
        let mut b = [0u8; 1];
        std::io::stdin().read_exact(&mut b).unwrap();
        std::process::exit(77);
    }
    probe(&mut l, "child", 1000);
    if mode == "inflight" {
        let r = l.reserve(key("job"), key("owner"), 100).unwrap();
        l.begin_allocation(key("write"), r.reference(), key("event"), 50)
            .unwrap();
        std::process::exit(77);
    }
    let cut = if mode == "before" {
        Stage::Written
    } else {
        Stage::Committed
    };
    let _ = l.apply_hook(
        key("job"),
        Command::Reserve {
            owner: key("owner"),
            peak: 100,
        },
        |stage| {
            if stage == cut {
                std::process::exit(77);
            }
            Ok(())
        },
    );
    panic!("did not hit crash point");
}
#[test]
fn actual_process_death_preserves_committed_reservations_and_rolls_back_uncommitted_ones() {
    for mode in ["before", "after", "inflight"] {
        let f = Fixture::new(0);
        let mut child = spawn_child(&f, mode);
        assert_eq!(child.0.wait().unwrap().code(), Some(77));
        let mut l = f.open();
        let expected = if mode == "before" { 0 } else { 100 };
        assert_eq!(l.accounting().unwrap().reserved_remaining, expected);
        if mode == "before" {
            probe(&mut l, "retry", 1000);
        }
        let r = l.reserve(key("job"), key("owner"), 100).unwrap();
        assert_eq!(l.accounting().unwrap().reserved_remaining, 100);
        if mode == "inflight" {
            assert!(matches!(
                l.finish(key("finish"), r.reference()),
                Err(LedgerError::Capacity(capacity::CapacityError::InFlight))
            ));
        }
    }
}
#[test]
fn busy_domain_is_nonblocking_and_an_independent_domain_keeps_progressing() {
    let f = Fixture::new(0);
    let other = Fixture::new(0);
    let mut l = f.open();
    probe(&mut l, "parent", 1000);
    let mut child = spawn_child(&f, "hold");
    let mut reader = BufReader::new(child.0.stdout.take().unwrap());
    let mut ready = false;
    for _ in 0..10 {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 {
            break;
        }
        if line.contains("READY") {
            ready = true;
            break;
        }
    }
    assert!(ready);
    assert!(matches!(
        l.reserve(key("job"), key("owner"), 100),
        Err(LedgerError::Busy)
    ));
    let mut second = other.open();
    reserved(&mut second, "independent", 100);
    child.0.stdin.take().unwrap().write_all(b"x").unwrap();
    assert_eq!(child.0.wait().unwrap().code(), Some(77));
    probe(&mut l, "parent-retry", 1000);
    l.reserve(key("job"), key("owner"), 100).unwrap();
}
