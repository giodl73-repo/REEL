use super::*;
use crate::ledger::{Checkpoint, Response};
use std::{
    fs,
    io::{self, Cursor, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};
use tempfile::TempDir;

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn request(id: &str, bytes: &[u8]) -> Request {
    Request {
        operation: key(id),
        owner: key("owner"),
        object: tig_core::Object::from_bytes(bytes),
        required: AckLevel::ProcessRestart,
    }
}
struct Fixture {
    _temp: TempDir,
    store: PathBuf,
    ledger: PathBuf,
    witness: Checkpoint,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let store = temp.path().join("store");
        let ledger = temp.path().join("ledger");
        LocalStore::create(&store, key("place")).unwrap();
        let l = CapacityLedger::create(&ledger, key("volume"), key("epoch"), 0).unwrap();
        Self {
            _temp: temp,
            store,
            ledger,
            witness: l.checkpoint(),
        }
    }
    fn open(&self) -> (LocalStore, CapacityLedger) {
        (
            LocalStore::open(&self.store).unwrap(),
            CapacityLedger::open(&self.ledger, &self.witness).unwrap(),
        )
    }
    fn scope(&self, id: &str) -> PathBuf {
        self.store
            .join("staging")
            .join(format!("attempt-{}", Digest::blob(id.as_bytes()).as_str()))
    }
}
fn job(l: &mut CapacityLedger, id: &str, peak: u64) -> JournalRef {
    let p = l.begin_probe(key(&format!("probe-{id}"))).unwrap();
    l.observe(
        key(&format!("sample-{id}")),
        p.reference(),
        1024 * 1024 * 1024,
    )
    .unwrap();
    l.reserve(key(id), key("owner"), peak)
        .unwrap()
        .reference()
        .clone()
}
struct NoRead;
impl Read for NoRead {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        panic!("must not read producer bytes")
    }
}

#[test]
fn admitted_publication_settles_a_labelled_measured_file_data_footprint() {
    for bytes in [b"AAA".to_vec(), Vec::new(), vec![0x5a; 150_003]] {
        let f = Fixture::new();
        let (s, mut l) = f.open();
        let req = request("one", &bytes);
        let p = plan(&s, &req).unwrap();
        let j = job(&mut l, "job", p.required_peak());
        let r = publish_accounted(&mut l, &s, &j, &p, &mut Cursor::new(&bytes)).unwrap();
        assert_eq!(r.basis(), "conservative-allocated-file-data-footprint");
        assert!(r.charged_footprint() <= p.required_peak());
        assert_eq!(
            l.accounting().unwrap().allocated_after_watermark,
            r.charged_footprint()
        );
        l.finish(key("finish"), &j).unwrap();
        assert_eq!(l.accounting().unwrap().reserved_remaining, 0);
        assert_eq!(
            fs::read(f.store.join("objects").join(req.object.sha256.as_str())).unwrap(),
            bytes
        );
        assert!(
            matches!(r.settlement().response(),Response::FootprintCharged(n) if *n==r.charged_footprint())
        );
    }
}
#[test]
fn insufficient_reservation_rejects_before_provider_mutation_or_source_read() {
    let f = Fixture::new();
    let (s, mut l) = f.open();
    let p = plan(&s, &request("one", b"AAA")).unwrap();
    let j = job(&mut l, "job", 1);
    assert!(matches!(
        publish_accounted(&mut l, &s, &j, &p, &mut NoRead),
        Err(AccountedError::Ledger(LedgerError::Capacity(_)))
    ));
    assert_eq!(fs::read_dir(f.store.join("objects")).unwrap().count(), 0);
    assert_eq!(fs::read_dir(f.store.join("staging")).unwrap().count(), 0);
    assert_eq!(fs::read_dir(f.store.join("fences")).unwrap().count(), 0);
}
#[test]
fn closed_reservation_cannot_authorize_new_publication() {
    let f = Fixture::new();
    let (s, mut l) = f.open();
    let p = plan(&s, &request("one", b"AAA")).unwrap();
    let j = job(&mut l, "job", p.required_peak());
    l.finish(key("finish"), &j).unwrap();
    assert!(publish_accounted(&mut l, &s, &j, &p, &mut NoRead).is_err());
    assert!(!f.scope("one").exists());
}
#[test]
fn wrong_owner_and_wrong_store_plan_reject_before_streaming() {
    let f = Fixture::new();
    let other = Fixture::new();
    let (s, mut l) = f.open();
    let (wrong, _) = other.open();
    let mut req = request("one", b"AAA");
    let p = plan(&s, &req).unwrap();
    let j = job(&mut l, "job", p.required_peak());
    assert!(matches!(
        publish_accounted(&mut l, &wrong, &j, &p, &mut NoRead),
        Err(AccountedError::InvalidPlan)
    ));
    req.owner = key("foreign");
    let p = plan(&s, &req).unwrap();
    assert!(matches!(
        publish_accounted(&mut l, &s, &j, &p, &mut NoRead),
        Err(AccountedError::Ledger(LedgerError::InvalidReference))
    ));
    assert!(!f.scope("one").exists());
}
#[test]
fn changed_request_cannot_reuse_an_already_bound_publication_operation() {
    let f = Fixture::new();
    let (s, mut l) = f.open();
    let p = plan(&s, &request("one", b"AAA")).unwrap();
    let j = job(&mut l, "job", p.required_peak());
    publish_accounted(&mut l, &s, &j, &p, &mut Cursor::new(b"AAA")).unwrap();
    let changed = plan(&s, &request("one", b"BBB")).unwrap();
    assert!(matches!(
        publish_accounted(&mut l, &s, &j, &changed, &mut NoRead),
        Err(AccountedError::Ledger(LedgerError::Conflict))
    ));
}
#[test]
fn settled_retry_after_job_close_is_readonly_and_does_not_double_charge() {
    let f = Fixture::new();
    let (s, mut l) = f.open();
    let p = plan(&s, &request("one", b"AAA")).unwrap();
    let j = job(&mut l, "job", p.required_peak());
    let r = publish_accounted(&mut l, &s, &j, &p, &mut Cursor::new(b"AAA")).unwrap();
    l.finish(key("finish"), &j).unwrap();
    let checkpoint = l.checkpoint();
    drop(l);
    let mut l = CapacityLedger::open(&f.ledger, &checkpoint).unwrap();
    let before = l.accounting().unwrap();
    let replay = publish_accounted(&mut l, &s, &j, &p, &mut NoRead).unwrap();
    assert_eq!(replay.charged_footprint(), r.charged_footprint());
    assert_eq!(l.accounting().unwrap(), before);
    assert_eq!(l.checkpoint(), checkpoint);
}
#[test]
fn settled_missing_or_corrupt_payload_never_uses_closed_budget_to_rebuild() {
    for corrupt in [false, true] {
        let f = Fixture::new();
        let (s, mut l) = f.open();
        let req = request("one", b"AAA");
        let p = plan(&s, &req).unwrap();
        let j = job(&mut l, "job", p.required_peak());
        publish_accounted(&mut l, &s, &j, &p, &mut Cursor::new(b"AAA")).unwrap();
        let path = f.store.join("objects").join(req.object.sha256.as_str());
        if corrupt {
            fs::write(&path, b"BAD").unwrap();
        } else {
            fs::remove_file(&path).unwrap();
        }
        assert!(publish_accounted(&mut l, &s, &j, &p, &mut NoRead).is_err());
        if corrupt {
            assert_eq!(fs::read(path).unwrap(), b"BAD");
        } else {
            assert!(!path.exists());
        }
    }
}
#[test]
fn failed_stream_keeps_capacity_inflight_and_exact_retry_can_finish() {
    let f = Fixture::new();
    let (s, mut l) = f.open();
    let p = plan(&s, &request("one", b"AAA")).unwrap();
    let j = job(&mut l, "job", p.required_peak());
    assert!(publish_accounted(&mut l, &s, &j, &p, &mut Cursor::new(b"BAD")).is_err());
    assert!(matches!(
        l.finish(key("finish"), &j),
        Err(LedgerError::Capacity(
            tig_core::capacity::CapacityError::InFlight
        ))
    ));
    assert_eq!(
        l.accounting().unwrap().reserved_remaining,
        p.required_peak()
    );
    publish_accounted(&mut l, &s, &j, &p, &mut Cursor::new(b"AAA")).unwrap();
    l.finish(key("finish"), &j).unwrap();
}
#[test]
fn known_partial_without_verified_final_blocks_another_staging_copy() {
    let f = Fixture::new();
    let (s, mut l) = f.open();
    let req = request("one", b"AAA");
    let p = plan(&s, &req).unwrap();
    let j = job(&mut l, "job", p.required_peak());
    assert!(
        publish_hook(&mut l, &s, &j, &p, &mut NoRead, |stage| {
            if stage == Stage::Bound {
                fs::write(f.scope("one").join("123-1.part"), b"unclosed").unwrap();
                return Err(AccountedError::Injected);
            }
            Ok(())
        })
        .is_err()
    );
    assert!(matches!(
        publish_accounted(&mut l, &s, &j, &p, &mut NoRead),
        Err(AccountedError::PendingPartial)
    ));
    assert_eq!(
        fs::read(f.scope("one").join("123-1.part")).unwrap(),
        b"unclosed"
    );
}
#[test]
fn unknown_scope_files_are_not_swept_or_silently_ignored() {
    let f = Fixture::new();
    let (s, mut l) = f.open();
    let p = plan(&s, &request("one", b"AAA")).unwrap();
    let j = job(&mut l, "job", p.required_peak());
    assert!(
        publish_hook(&mut l, &s, &j, &p, &mut NoRead, |stage| {
            if stage == Stage::Bound {
                fs::write(f.scope("one").join("unknown.txt"), b"keep").unwrap();
                return Err(AccountedError::Injected);
            }
            Ok(())
        })
        .is_err()
    );
    assert!(publish_accounted(&mut l, &s, &j, &p, &mut NoRead).is_err());
    assert_eq!(
        fs::read(f.scope("one").join("unknown.txt")).unwrap(),
        b"keep"
    );
}
#[test]
fn all_bridge_boundaries_can_retry_without_new_payload_or_double_settlement() {
    for cut in [
        Stage::Admitted,
        Stage::Bound,
        Stage::Published,
        Stage::Settled,
    ] {
        let f = Fixture::new();
        let (s, mut l) = f.open();
        let p = plan(&s, &request("one", b"AAA")).unwrap();
        let j = job(&mut l, "job", p.required_peak());
        assert!(
            publish_hook(&mut l, &s, &j, &p, &mut Cursor::new(b"AAA"), |stage| {
                if stage == cut {
                    Err(AccountedError::Injected)
                } else {
                    Ok(())
                }
            })
            .is_err()
        );
        drop(l);
        let (_, mut l) = f.open();
        let r = if matches!(cut, Stage::Published | Stage::Settled) {
            publish_accounted(&mut l, &s, &j, &p, &mut NoRead)
        } else {
            publish_accounted(&mut l, &s, &j, &p, &mut Cursor::new(b"AAA"))
        }
        .unwrap();
        assert_eq!(
            l.accounting().unwrap().allocated_after_watermark,
            r.charged_footprint()
        );
        l.finish(key("finish"), &j).unwrap();
    }
}
#[test]
fn generic_allocation_completion_cannot_change_a_publication_charge_basis() {
    let f = Fixture::new();
    let (s, mut l) = f.open();
    let p = plan(&s, &request("one", b"AAA")).unwrap();
    let j = job(&mut l, "job", p.required_peak());
    let r = publish_accounted(&mut l, &s, &j, &p, &mut Cursor::new(b"AAA")).unwrap();
    assert!(matches!(
        l.complete_allocation(key("wrong-kind"), r.allocation(), 0),
        Err(LedgerError::InvalidReference)
    ));
}
#[test]
fn same_attempt_is_nonblocking_while_unrelated_publication_proceeds() {
    let f = Fixture::new();
    let (s, mut l) = f.open();
    let p = plan(&s, &request("one", b"AAA")).unwrap();
    let j = job(&mut l, "job", p.required_peak());
    let p2 = plan(&s, &request("two", b"BBB")).unwrap();
    let j2 = job(&mut l, "other-job", p2.required_peak());
    let (ready_tx, ready_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel();
    struct Waiting {
        ready: Option<mpsc::Sender<()>>,
        go: mpsc::Receiver<()>,
        inner: Cursor<Vec<u8>>,
    }
    impl Read for Waiting {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            if let Some(tx) = self.ready.take() {
                tx.send(()).unwrap();
                self.go.recv().unwrap();
            }
            self.inner.read(b)
        }
    }
    let root = f.store.clone();
    let ledger_root = f.ledger.clone();
    let witness = f.witness.clone();
    let p1 = p.clone();
    let first = j.clone();
    let worker = std::thread::spawn(move || {
        let s = LocalStore::open(&root).unwrap();
        let mut l = CapacityLedger::open(&ledger_root, &witness).unwrap();
        publish_accounted(
            &mut l,
            &s,
            &first,
            &p1,
            &mut Waiting {
                ready: Some(ready_tx),
                go: go_rx,
                inner: Cursor::new(b"AAA".to_vec()),
            },
        )
        .unwrap()
    });
    ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(
        publish_accounted(&mut l, &s, &j, &p, &mut NoRead),
        Err(AccountedError::Store(Error::Busy))
    ));
    publish_accounted(&mut l, &s, &j2, &p2, &mut Cursor::new(b"BBB")).unwrap();
    go_tx.send(()).unwrap();
    worker.join().unwrap();
}

#[test]
fn existing_shared_payload_reuses_bytes_and_labels_its_conservative_charge() {
    let f = Fixture::new();
    let (s, mut l) = f.open();
    for (id, source) in [("one", true), ("two", false)] {
        let p = plan(&s, &request(id, b"AAA")).unwrap();
        let j = job(&mut l, &format!("job-{id}"), p.required_peak());
        let r = if source {
            publish_accounted(&mut l, &s, &j, &p, &mut Cursor::new(b"AAA"))
        } else {
            publish_accounted(&mut l, &s, &j, &p, &mut NoRead)
        }
        .unwrap();
        assert_eq!(r.basis(), p.basis());
        assert!(r.charged_footprint() <= p.required_peak());
    }
    assert_eq!(fs::read_dir(f.store.join("objects")).unwrap().count(), 1);
}
#[test]
fn unsupported_and_forged_plans_do_not_grant_write_authority() {
    let f = Fixture::new();
    let (s, mut l) = f.open();
    let mut req = request("one", b"AAA");
    req.required = AckLevel::ProviderDurable;
    assert!(matches!(
        plan(&s, &req),
        Err(AccountedError::Store(Error::Unsupported))
    ));
    req.required = AckLevel::ProcessRestart;
    req.object.bytes = crate::local::MAX_OBJECT_BYTES + 1;
    assert!(plan(&s, &req).is_err());
    let mut p = plan(&s, &request("one", b"AAA")).unwrap();
    let j = job(&mut l, "job", p.required_peak());
    p.limit -= 1;
    assert!(matches!(
        publish_accounted(&mut l, &s, &j, &p, &mut NoRead),
        Err(AccountedError::InvalidPlan)
    ));
    assert!(!f.scope("one").exists());
}
#[test]
fn stream_io_failure_never_settles_or_forgets_the_allocation() {
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::StorageFull.into())
        }
    }
    let f = Fixture::new();
    let (s, mut l) = f.open();
    let p = plan(&s, &request("one", b"AAA")).unwrap();
    let j = job(&mut l, "job", p.required_peak());
    assert!(matches!(
        publish_accounted(&mut l, &s, &j, &p, &mut Broken),
        Err(AccountedError::Store(Error::Io(
            io::ErrorKind::StorageFull,
            _
        )))
    ));
    assert_eq!(
        l.accounting().unwrap().reserved_remaining,
        p.required_peak()
    );
    assert_eq!(l.accounting().unwrap().allocated_after_watermark, 0);
    assert_eq!(fs::read_dir(f.store.join("objects")).unwrap().count(), 0);
}
#[test]
fn excessive_observed_footprint_blocks_ack_and_keeps_the_job_inflight() {
    let f = Fixture::new();
    let (s, mut l) = f.open();
    let p = plan(&s, &request("one", b"AAA")).unwrap();
    let j = job(&mut l, "job", p.required_peak());
    let unexpected = f.scope("one").join("123-2.part");
    let result = publish_hook(&mut l, &s, &j, &p, &mut Cursor::new(b"AAA"), |stage| {
        if stage == Stage::Published {
            let file = fs::File::create(&unexpected).unwrap();
            fs2::FileExt::allocate(&file, p.required_peak() * 2).unwrap();
        }
        Ok(())
    });
    assert!(matches!(result, Err(AccountedError::FootprintExceedsLimit)));
    assert!(unexpected.exists());
    assert!(l.finish(key("finish"), &j).is_err());
    assert_eq!(
        l.accounting().unwrap().reserved_remaining,
        p.required_peak()
    );
}
#[test]
fn damaged_attempt_binding_is_not_repaired_or_rebound_automatically() {
    let f = Fixture::new();
    let (s, mut l) = f.open();
    let p = plan(&s, &request("one", b"AAA")).unwrap();
    let j = job(&mut l, "job", p.required_peak());
    assert!(
        publish_hook(&mut l, &s, &j, &p, &mut NoRead, |stage| {
            if stage == Stage::Bound {
                Err(AccountedError::Injected)
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    fs::write(f.scope("one").join("binding.json"), b"foreign").unwrap();
    assert!(publish_accounted(&mut l, &s, &j, &p, &mut NoRead).is_err());
    assert_eq!(
        fs::read(f.scope("one").join("binding.json")).unwrap(),
        b"foreign"
    );
}

struct ChildScope(Child);
impl Drop for ChildScope {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn accounted_child() {
    let Some(root) = std::env::var_os("TIG_ACCOUNTED_TEST_ROOT") else {
        return;
    };
    let root = Path::new(&root);
    let witness: Checkpoint =
        serde_json::from_str(&std::env::var("TIG_ACCOUNTED_TEST_WITNESS").unwrap()).unwrap();
    let j: JournalRef =
        serde_json::from_str(&std::env::var("TIG_ACCOUNTED_TEST_JOB").unwrap()).unwrap();
    let mode = std::env::var("TIG_ACCOUNTED_TEST_MODE").unwrap();
    let s = LocalStore::open(&root.join("store")).unwrap();
    let mut l = CapacityLedger::open(&root.join("ledger"), &witness).unwrap();
    let p = plan(&s, &request("one", b"AAA")).unwrap();
    if mode == "partial" {
        struct Crash(bool);
        impl Read for Crash {
            fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
                if self.0 {
                    std::process::exit(77);
                }
                self.0 = true;
                b[0] = b'A';
                Ok(1)
            }
        }
        let _ = publish_accounted(&mut l, &s, &j, &p, &mut Crash(false));
    } else {
        let cut = if mode == "published" {
            Stage::Published
        } else {
            Stage::Settled
        };
        let _ = publish_hook(&mut l, &s, &j, &p, &mut Cursor::new(b"AAA"), |stage| {
            if stage == cut {
                std::process::exit(77);
            }
            Ok(())
        });
    }
    panic!("did not reach crash point");
}
#[test]
fn process_death_keeps_partial_attempt_charged_and_recovers_verified_completion() {
    for mode in ["partial", "published", "settled"] {
        let f = Fixture::new();
        let (s, mut l) = f.open();
        let p = plan(&s, &request("one", b"AAA")).unwrap();
        let j = job(&mut l, "job", p.required_peak());
        drop(l);
        let mut child = ChildScope(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "accounted::tests::accounted_child",
                    "--nocapture",
                ])
                .env("TIG_ACCOUNTED_TEST_ROOT", f._temp.path())
                .env(
                    "TIG_ACCOUNTED_TEST_WITNESS",
                    serde_json::to_string(&f.witness).unwrap(),
                )
                .env("TIG_ACCOUNTED_TEST_JOB", serde_json::to_string(&j).unwrap())
                .env("TIG_ACCOUNTED_TEST_MODE", mode)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        assert_eq!(child.0.wait().unwrap().code(), Some(77));
        let (_, mut l) = f.open();
        if mode == "partial" {
            assert!(matches!(
                publish_accounted(&mut l, &s, &j, &p, &mut NoRead),
                Err(AccountedError::PendingPartial)
            ));
            assert_eq!(
                l.accounting().unwrap().reserved_remaining,
                p.required_peak()
            );
        } else {
            publish_accounted(&mut l, &s, &j, &p, &mut NoRead).unwrap();
            l.finish(key("finish"), &j).unwrap();
        }
    }
}
