use super::*;
use crate::ledger::{Checkpoint, LedgerError, Response};
use std::{
    fs,
    io::{self, Cursor},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tempfile::TempDir;
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
struct NoRead;
impl Read for NoRead {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        panic!("must not read new producer bytes")
    }
}
struct Fixture {
    temp: TempDir,
    store: LocalStore,
    ledger: CapacityLedger,
    witness: Checkpoint,
    publication: PublicationPlan,
    job: JournalRef,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let store = LocalStore::create(&temp.path().join("store"), key("place")).unwrap();
        let mut ledger =
            CapacityLedger::create(&temp.path().join("ledger"), key("volume"), key("epoch"), 0)
                .unwrap();
        let witness = ledger.checkpoint();
        let publication = plan(
            &store,
            &Request {
                operation: key("one"),
                owner: key("owner"),
                object: tig_core::Object::from_bytes(b"AAA"),
                required: AckLevel::ProcessRestart,
            },
        )
        .unwrap();
        // Durable parallel fixture IO can outlive a probe. Resample rather than
        // weakening production freshness or spending the expired observation.
        let job = (0..8)
            .find_map(|attempt| {
                let p = ledger
                    .begin_probe(key(&format!("probe-{attempt}")))
                    .unwrap();
                match ledger.observe(
                    key(&format!("sample-{attempt}")),
                    p.reference(),
                    1024 * 1024 * 1024,
                ) {
                    Err(LedgerError::NeedsProbe) => return None,
                    result => {
                        result.unwrap();
                    }
                }
                match ledger.reserve(key("job"), key("owner"), publication.limit) {
                    Err(LedgerError::NeedsProbe) => None,
                    result => Some(result.unwrap().reference().clone()),
                }
            })
            .expect("fixture could not obtain a fresh reservation after eight probes");
        Self {
            temp,
            store,
            ledger,
            witness,
            publication,
            job,
        }
    }
    fn seed(&mut self) {
        assert!(
            super::super::publish_hook(
                &mut self.ledger,
                &self.store,
                &self.job,
                &self.publication,
                &mut NoRead,
                |s| {
                    if s == super::super::Stage::Bound {
                        Err(AccountedError::Injected)
                    } else {
                        Ok(())
                    }
                }
            )
            .is_err()
        );
        self.store.protect(&self.publication.request).unwrap();
        fs::write(self.scope().join("123-1.part"), b"partial-one").unwrap();
        fs::write(self.scope().join("123-2.part"), b"partial-two").unwrap();
    }
    fn scope(&self) -> PathBuf {
        self.temp
            .path()
            .join("store/staging")
            .join(format!("attempt-{}", Digest::blob(b"one").as_str()))
    }
    fn inspect(&mut self) -> CleanupPlan {
        inspect(&mut self.ledger, &self.store, &self.job, &self.publication).unwrap()
    }
    fn reopen(&mut self) {
        self.ledger =
            CapacityLedger::open(&self.temp.path().join("ledger"), &self.witness).unwrap();
    }
    fn inflight(&mut self) {
        assert!(self.ledger.finish(key("finish"), &self.job).is_err());
    }
    fn object(&self) -> PathBuf {
        self.temp
            .path()
            .join("store/objects")
            .join(self.publication.request.object.sha256.as_str())
    }
}
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(root: &Path, path: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for e in fs::read_dir(path).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                out.push((
                    p.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(p).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}
#[test]
fn preview_is_readonly_and_records_exact_private_evidence() {
    let mut f = Fixture::new();
    f.seed();
    let before = snapshot(f.temp.path());
    let seq = f.ledger.checkpoint().sequence();
    let p = f.inspect();
    assert_eq!(p.files().len(), 2);
    assert_eq!(p.files()[0].sha256, Digest::blob(b"partial-one"));
    assert_eq!(f.ledger.checkpoint().sequence(), seq);
    assert_eq!(snapshot(f.temp.path()), before);
}
#[test]
fn approved_cleanup_retains_metadata_and_releases_only_after_disposition() {
    let mut f = Fixture::new();
    f.seed();
    let p = f.inspect();
    let a = p.authorize_discard().unwrap();
    let r = apply(&mut f.ledger, &f.store, &p, &a).unwrap();
    assert_eq!(r.reviewed_partial_count, 2);
    assert!(!f.scope().join("123-1.part").exists());
    assert!(f.scope().join("binding.json").exists());
    assert!(
        matches!(r.disposition.response(),Response::CleanupCompleted(n) if *n==r.remaining_footprint)
    );
    assert_eq!(
        f.ledger.accounting().unwrap().reserved_remaining,
        f.publication.limit - r.remaining_footprint
    );
    f.ledger.finish(key("finish"), &f.job).unwrap();
    assert_eq!(f.ledger.accounting().unwrap().reserved_remaining, 0);
    assert_eq!(
        f.ledger.accounting().unwrap().allocated_after_watermark,
        r.remaining_footprint
    );
}
#[test]
fn authorization_is_bound_to_the_exact_reviewed_plan() {
    let mut f = Fixture::new();
    f.seed();
    let p = f.inspect();
    let mut wrong = p.clone();
    wrong.schema = "other".into();
    let a = wrong.authorize_discard().unwrap();
    assert!(matches!(
        apply(&mut f.ledger, &f.store, &p, &a),
        Err(CleanupError::WrongAuthorization)
    ));
    assert!(f.scope().join("123-1.part").exists());
    f.inflight();
}
#[test]
fn changed_bytes_invalidate_approval_before_cancellation() {
    let mut f = Fixture::new();
    f.seed();
    let p = f.inspect();
    let a = p.authorize_discard().unwrap();
    let seq = f.ledger.checkpoint().sequence();
    fs::write(f.scope().join("123-1.part"), b"changed-one").unwrap();
    assert!(matches!(
        apply(&mut f.ledger, &f.store, &p, &a),
        Err(CleanupError::StalePlan)
    ));
    assert_eq!(f.ledger.checkpoint().sequence(), seq);
    f.inflight();
}
#[test]
fn missing_before_cancellation_is_not_assumed_to_be_a_retry() {
    let mut f = Fixture::new();
    f.seed();
    let p = f.inspect();
    let a = p.authorize_discard().unwrap();
    fs::remove_file(f.scope().join("123-1.part")).unwrap();
    assert!(matches!(
        apply(&mut f.ledger, &f.store, &p, &a),
        Err(CleanupError::StalePlan)
    ));
    assert!(f.scope().join("123-2.part").exists());
    f.inflight();
}
#[test]
fn unknown_scope_files_or_directories_are_preserved() {
    for directory in [false, true] {
        let mut f = Fixture::new();
        f.seed();
        let p = f.inspect();
        let a = p.authorize_discard().unwrap();
        let extra = f.scope().join("foreign");
        if directory {
            fs::create_dir(&extra).unwrap();
        } else {
            fs::write(&extra, b"foreign").unwrap();
        }
        assert!(apply(&mut f.ledger, &f.store, &p, &a).is_err());
        assert!(extra.exists());
        assert!(f.scope().join("123-1.part").exists());
        f.inflight();
    }
}
#[test]
fn scope_count_and_read_budgets_fail_closed() {
    for oversized in [false, true] {
        let mut f = Fixture::new();
        f.seed();
        if oversized {
            fs::File::create(f.scope().join("123-3.part"))
                .unwrap()
                .set_len(f.publication.limit + 1)
                .unwrap();
        } else {
            for i in 3..20 {
                fs::write(f.scope().join(format!("123-{i}.part")), b"x").unwrap();
            }
        }
        assert!(inspect(&mut f.ledger, &f.store, &f.job, &f.publication).is_err());
        assert!(f.scope().join("123-1.part").exists());
        f.inflight();
    }
}
#[test]
fn damaged_binding_cannot_authorize_any_removal() {
    for binding in [true, false] {
        let mut f = Fixture::new();
        f.seed();
        let p = f.inspect();
        let a = p.authorize_discard().unwrap();
        let path = if binding {
            f.scope().join("binding.json")
        } else {
            f.temp
                .path()
                .join("store/operations")
                .join(format!("{}.intent", Digest::blob(b"one").as_str()))
        };
        fs::write(path, b"{}").unwrap();
        assert!(apply(&mut f.ledger, &f.store, &p, &a).is_err());
        assert!(f.scope().join("123-1.part").exists());
        f.inflight();
    }
}
#[test]
fn final_objects_are_preserved_and_require_recovery_even_if_corrupt() {
    for bytes in [b"AAA".as_slice(), b"bad".as_slice()] {
        let mut f = Fixture::new();
        f.seed();
        let p = f.inspect();
        let a = p.authorize_discard().unwrap();
        fs::write(f.object(), bytes).unwrap();
        assert!(matches!(
            apply(&mut f.ledger, &f.store, &p, &a),
            Err(CleanupError::RecoverPublication)
        ));
        assert_eq!(fs::read(f.object()).unwrap(), bytes);
        assert!(f.scope().join("123-1.part").exists());
        f.inflight();
    }
}
#[test]
fn completed_publication_is_never_a_private_cleanup_candidate() {
    let mut f = Fixture::new();
    publish_accounted(
        &mut f.ledger,
        &f.store,
        &f.job,
        &f.publication,
        &mut Cursor::new(b"AAA"),
    )
    .unwrap();
    let before = snapshot(f.temp.path());
    assert!(inspect(&mut f.ledger, &f.store, &f.job, &f.publication).is_err());
    assert_eq!(snapshot(f.temp.path()), before);
}
#[test]
fn same_attempt_fence_is_nonblocking_and_other_objects_can_publish() {
    let mut f = Fixture::new();
    f.seed();
    let held = f.store.existing_attempt(&f.publication.request).unwrap();
    assert!(matches!(
        inspect(&mut f.ledger, &f.store, &f.job, &f.publication),
        Err(CleanupError::Accounted(AccountedError::Store(Error::Busy)))
    ));
    let mut other = LocalStore::open(&f.temp.path().join("store")).unwrap();
    let req = Request {
        operation: key("other"),
        owner: key("owner"),
        object: tig_core::Object::from_bytes(b"BBB"),
        required: AckLevel::ProcessRestart,
    };
    publish(&mut other, &req, &mut Cursor::new(b"BBB")).unwrap();
    drop(held);
    assert_eq!(f.inspect().files().len(), 2);
}
#[test]
fn cancellation_is_durable_before_removal_and_prevents_old_publication() {
    let mut f = Fixture::new();
    f.seed();
    let p = f.inspect();
    let a = p.authorize_discard().unwrap();
    assert!(
        apply_hook(&mut f.ledger, &f.store, &p, &a, |s| {
            if s == Stage::Cancelled {
                Err(CleanupError::Injected)
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    f.reopen();
    assert!(f.scope().join("123-1.part").exists());
    f.inflight();
    assert!(matches!(
        publish_accounted(&mut f.ledger, &f.store, &f.job, &f.publication, &mut NoRead),
        Err(AccountedError::Ledger(LedgerError::Cancelled))
    ));
    assert!(matches!(
        f.ledger.settle_footprint(
            key("wrong"),
            &p.allocation,
            Digest::blob(&encode(&(&p.publication, &p.job)).unwrap()),
            0
        ),
        Err(LedgerError::Cancelled)
    ));
    apply(&mut f.ledger, &f.store, &p, &a).unwrap();
}
#[test]
fn interrupted_removal_retries_only_the_remaining_reviewed_subset() {
    let mut f = Fixture::new();
    f.seed();
    let p = f.inspect();
    let a = p.authorize_discard().unwrap();
    assert!(
        apply_hook(
            &mut f.ledger,
            &f.store,
            &p,
            &a,
            |s| if s == Stage::Removed {
                Err(CleanupError::Injected)
            } else {
                Ok(())
            }
        )
        .is_err()
    );
    assert!(!f.scope().join("123-1.part").exists());
    assert!(f.scope().join("123-2.part").exists());
    f.reopen();
    f.inflight();
    let persisted = CleanupPlan::from_json(&p.to_json().unwrap()).unwrap();
    apply(&mut f.ledger, &f.store, &persisted, &a).unwrap();
    f.ledger.finish(key("finish"), &f.job).unwrap();
    let before = snapshot(f.temp.path());
    let r = apply(&mut f.ledger, &f.store, &persisted, &a).unwrap();
    assert!(r.disposition.was_replayed());
    assert_eq!(snapshot(f.temp.path()), before);
}
#[test]
fn new_or_changed_files_after_cancellation_are_never_swept() {
    for changed in [false, true] {
        let mut f = Fixture::new();
        f.seed();
        let p = f.inspect();
        let a = p.authorize_discard().unwrap();
        assert!(
            apply_hook(&mut f.ledger, &f.store, &p, &a, |_| Err(
                CleanupError::Injected
            ))
            .is_err()
        );
        let path = f
            .scope()
            .join(if changed { "123-1.part" } else { "123-3.part" });
        fs::write(&path, b"new unreviewed bytes").unwrap();
        assert!(matches!(
            apply(&mut f.ledger, &f.store, &p, &a),
            Err(CleanupError::StalePlan)
        ));
        assert!(path.exists());
        f.inflight();
    }
}
#[test]
fn terminal_disposition_cannot_silently_remove_new_files() {
    let mut f = Fixture::new();
    f.seed();
    let p = f.inspect();
    let a = p.authorize_discard().unwrap();
    apply(&mut f.ledger, &f.store, &p, &a).unwrap();
    let path = f.scope().join("123-3.part");
    fs::write(&path, b"x").unwrap();
    assert!(apply(&mut f.ledger, &f.store, &p, &a).is_err());
    assert!(path.exists());
    assert!(matches!(
        publish_accounted(&mut f.ledger, &f.store, &f.job, &f.publication, &mut NoRead),
        Err(AccountedError::Ledger(LedgerError::Cancelled))
    ));
}
#[test]
fn forged_paths_and_mismatched_stores_cannot_escape_the_reviewed_scope() {
    let mut f = Fixture::new();
    f.seed();
    let mut p = f.inspect();
    let outside = f.temp.path().join("outside");
    fs::write(&outside, b"keep").unwrap();
    p.files[0].name = "../../outside".into();
    let a = p.authorize_discard().unwrap();
    assert!(apply(&mut f.ledger, &f.store, &p, &a).is_err());
    assert_eq!(fs::read(outside).unwrap(), b"keep");
    assert!(f.scope().join("123-1.part").exists());
    let other = LocalStore::create(&f.temp.path().join("other"), key("place")).unwrap();
    assert!(apply(&mut f.ledger, &other, &p, &a).is_err());
    f.inflight();
}
#[test]
fn plan_decoder_rejects_oversize_duplicate_and_unknown_fields() {
    assert!(CleanupPlan::from_json(&vec![b' '; 65_537]).is_err());
    assert!(CleanupPlan::from_json(br#"{"schema":"a","schema":"b"}"#).is_err());
    let mut f = Fixture::new();
    f.seed();
    let p = f.inspect();
    let mut v: serde_json::Value = serde_json::from_slice(&p.to_json().unwrap()).unwrap();
    v["unknown"] = true.into();
    assert!(CleanupPlan::from_json(&serde_json::to_vec(&v).unwrap()).is_err());
}
#[test]
fn preview_of_an_unstarted_operation_creates_nothing() {
    let mut f = Fixture::new();
    let before = snapshot(f.temp.path());
    assert!(inspect(&mut f.ledger, &f.store, &f.job, &f.publication).is_err());
    assert_eq!(snapshot(f.temp.path()), before);
}
#[test]
fn no_partial_failure_can_be_cancelled_without_deleting_its_records() {
    let mut f = Fixture::new();
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::Other.into())
        }
    }
    assert!(
        publish_accounted(&mut f.ledger, &f.store, &f.job, &f.publication, &mut Broken).is_err()
    );
    let p = f.inspect();
    assert!(p.files().is_empty());
    let a = p.authorize_discard().unwrap();
    apply(&mut f.ledger, &f.store, &p, &a).unwrap();
    assert!(f.scope().join("binding.json").exists());
}
#[test]
fn publisher_rechecks_cancellation_after_a_stale_status_snapshot() {
    let mut f = Fixture::new();
    f.seed();
    let p = f.inspect();
    let a = p.authorize_discard().unwrap();
    let mut cleaner = CapacityLedger::open(&f.temp.path().join("ledger"), &f.witness).unwrap();
    let r = super::super::publish_hook(
        &mut f.ledger,
        &f.store,
        &f.job,
        &f.publication,
        &mut NoRead,
        |s| {
            if s == super::super::Stage::Checked {
                apply(&mut cleaner, &f.store, &p, &a).unwrap();
            }
            Ok(())
        },
    );
    assert!(matches!(
        r,
        Err(AccountedError::Ledger(LedgerError::Cancelled))
    ));
    assert!(!f.object().exists());
}
#[test]
fn cleanup_child() {
    let Ok(root) = std::env::var("TIG_CLEANUP_TEST_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let cut = std::env::var("TIG_CLEANUP_TEST_CUT").unwrap();
    let witness: Checkpoint =
        serde_json::from_slice(&fs::read(root.join("witness.json")).unwrap()).unwrap();
    if cut == "producer" {
        let publication: PublicationPlan =
            serde_json::from_slice(&fs::read(root.join("publication.json")).unwrap()).unwrap();
        let job: JournalRef =
            serde_json::from_slice(&fs::read(root.join("job.json")).unwrap()).unwrap();
        let store = LocalStore::open(&root.join("store")).unwrap();
        let mut ledger = CapacityLedger::open(&root.join("ledger"), &witness).unwrap();
        struct ExitAfterBytes(bool);
        impl Read for ExitAfterBytes {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                if self.0 {
                    std::process::exit(73);
                }
                self.0 = true;
                buffer[..3].copy_from_slice(b"AAA");
                Ok(3)
            }
        }
        publish_accounted(
            &mut ledger,
            &store,
            &job,
            &publication,
            &mut ExitAfterBytes(false),
        )
        .unwrap();
        panic!("producer must exit while writing");
    }
    let p = CleanupPlan::from_json(&fs::read(root.join("plan.json")).unwrap()).unwrap();
    let a = p.authorize_discard().unwrap();
    let store = LocalStore::open(&root.join("store")).unwrap();
    let mut ledger = CapacityLedger::open(&root.join("ledger"), &witness).unwrap();
    apply_hook(&mut ledger, &store, &p, &a, |s| {
        if matches!(
            (s, cut.as_str()),
            (Stage::Cancelled, "cancelled")
                | (Stage::Removed, "removed")
                | (Stage::Disposed, "disposed")
        ) {
            std::process::exit(73);
        }
        Ok(())
    })
    .unwrap();
    panic!("child must exit at a boundary");
}
#[test]
fn actual_process_exit_preserves_cancellation_and_idempotent_disposition() {
    for cut in ["cancelled", "removed", "disposed"] {
        let mut f = Fixture::new();
        f.seed();
        let p = f.inspect();
        let a = p.authorize_discard().unwrap();
        fs::write(
            f.temp.path().join("witness.json"),
            serde_json::to_vec(&f.witness).unwrap(),
        )
        .unwrap();
        fs::write(f.temp.path().join("plan.json"), p.to_json().unwrap()).unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "accounted::cleanup::tests::cleanup_child",
                "--nocapture",
            ])
            .env("TIG_CLEANUP_TEST_ROOT", f.temp.path())
            .env("TIG_CLEANUP_TEST_CUT", cut)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(73));
        f.reopen();
        if cut != "disposed" {
            f.inflight();
        }
        assert!(matches!(
            publish_accounted(&mut f.ledger, &f.store, &f.job, &f.publication, &mut NoRead),
            Err(AccountedError::Ledger(LedgerError::Cancelled))
        ));
        let r = apply(&mut f.ledger, &f.store, &p, &a).unwrap();
        f.ledger.finish(key("finish"), &f.job).unwrap();
        assert_eq!(
            f.ledger.accounting().unwrap().allocated_after_watermark,
            r.remaining_footprint
        );
    }
}

#[test]
fn actual_interrupted_producer_partial_can_be_reviewed_and_discarded() {
    let mut f = Fixture::new();
    for (name, bytes) in [
        ("witness.json", serde_json::to_vec(&f.witness).unwrap()),
        (
            "publication.json",
            serde_json::to_vec(&f.publication).unwrap(),
        ),
        ("job.json", serde_json::to_vec(&f.job).unwrap()),
    ] {
        fs::write(f.temp.path().join(name), bytes).unwrap();
    }
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "accounted::cleanup::tests::cleanup_child",
            "--nocapture",
        ])
        .env("TIG_CLEANUP_TEST_ROOT", f.temp.path())
        .env("TIG_CLEANUP_TEST_CUT", "producer")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(73));
    f.reopen();
    f.inflight();
    let p = f.inspect();
    assert_eq!(p.files().len(), 1);
    assert_eq!(p.files()[0].bytes, 3);
    assert_eq!(p.files()[0].sha256, Digest::blob(b"AAA"));
    let a = p.authorize_discard().unwrap();
    apply(&mut f.ledger, &f.store, &p, &a).unwrap();
    f.ledger.finish(key("finish"), &f.job).unwrap();
    assert!(!f.object().exists());
}
