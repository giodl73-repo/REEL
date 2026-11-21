use super::*;
use crate::{
    metadata::MetadataStore,
    retention::{self, release::tests::Fixture},
    retirement_intent as intent,
};
use std::{cell::RefCell, io::Cursor, rc::Rc};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn d(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}

// Deliberately finite synthetic protection adapter; SourceJournal completeness
// remains false. No production/global registry is inferred from this fixture.
#[derive(Default)]
pub(crate) struct Facts {
    pub(crate) unknown: bool,
    pub(crate) job: bool,
    pub(crate) held: bool,
}
#[derive(Default)]
pub(crate) struct Finite {
    pub(crate) facts: Rc<RefCell<Facts>>,
}
pub(crate) struct Lease {
    facts: Rc<RefCell<Facts>>,
    scope: Scope,
}
impl Protection for Finite {
    type Lease = Lease;
    fn acquire(&mut self, scope: &Scope) -> crate::Result<Lease> {
        let mut f = self.facts.borrow_mut();
        if f.unknown || f.job {
            return Err(Error::Retained);
        }
        if f.held {
            return Err(Error::Busy);
        }
        f.held = true;
        Ok(Lease {
            facts: self.facts.clone(),
            scope: scope.clone(),
        })
    }
}
impl ProtectionLease for Lease {
    fn revalidate(&mut self, scope: &Scope) -> crate::Result<Digest> {
        let f = self.facts.borrow();
        if !f.held || f.unknown || f.job || scope != &self.scope {
            return Err(Error::Retained);
        }
        Ok(Digest::blob(&encode(&(
            "finite-synthetic-protection",
            scope,
        ))?))
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.facts.borrow_mut().held = false;
    }
}
fn object(f: &Fixture) -> Object {
    f.grant.request().publication.object.clone()
}
fn release(f: &mut Fixture) {
    let n = f.start();
    let result = f.run(n).unwrap();
    f.finish(n, result.evidence);
}
fn start(f: &mut Fixture) -> Plan {
    let object = object(f);
    let n = progress::start(
        &mut f.journal,
        &mut f.nodes,
        &f.ctx,
        &object,
        progress::Action::Remove {
            store: f.store.identity().clone(),
        },
    )
    .unwrap()
    .attempt;
    plan(&f.store, &f.ctx, &object, n).unwrap()
}
fn execute<'a>(
    f: &'a mut Fixture,
    protection: &'a mut Finite,
) -> Execution<'a, MetadataStore, Finite> {
    Execution {
        store: &f.store,
        ledger: &mut f.ledger,
        job: &f.job,
        journal: &mut f.journal,
        nodes: &f.nodes,
        intent: &f.ctx,
        protection,
    }
}
fn apply_port(f: &mut Fixture, protection: &mut Finite, p: &Plan) -> RemovalReceipt {
    use crate::provider::removal as port;
    let mut executor = port::native::final_loss(execute(f, protection), p).unwrap();
    port::remove_exact(&mut executor, &port::native::request(p).unwrap(), true)
        .unwrap()
        .evidence
}
fn report(f: &mut Fixture, p: &Plan, result: &RemovalReceipt) {
    let object = object(f);
    progress::finish(
        &mut f.journal,
        &mut f.nodes,
        &f.ctx,
        &object,
        p.scope.attempt,
        progress::Action::Remove {
            store: f.store.identity().clone(),
        },
        progress::Outcome::Removed {
            evidence: result.evidence.clone(),
            allocation: progress::Allocation {
                before: Some(result.file_data_before),
                reclaimed: None,
            },
        },
    )
    .unwrap();
}
fn bytes(f: &Fixture) -> Vec<u8> {
    let sample: serde_json::Value =
        serde_json::from_str(include_str!("../../../../examples/teaching/catalogue.json")).unwrap();
    let bytes = sample["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "old")
        .unwrap()["content"]
        .as_str()
        .unwrap()
        .as_bytes()
        .to_vec();
    assert_eq!(Object::from_bytes(&bytes), object(f));
    bytes
}
fn ingest(f: &mut Fixture, operation: &str) -> crate::Result<crate::Receipt> {
    let req = Request {
        operation: key(operation),
        ..f.grant.request().publication.clone()
    };
    let data = bytes(f);
    crate::publish(&mut f.store, &req, &mut Cursor::new(data))
}

#[test]
fn configured_large_object_final_removal_reopens_and_fences_old_publication() {
    use std::io::Read;
    let mut f = Fixture::new_large_removable();
    let old = f.grant.request().publication.clone();
    assert!(old.object.bytes > MAX_OBJECT_BYTES);
    release(&mut f);
    let p = start(&mut f);
    let mut protection = Finite::default();
    let removed = apply_port(&mut f, &mut protection, &p);
    report(&mut f, &p, &removed);
    f.reopen();
    assert!(f.store.read_object(&old.object).is_err());
    assert!(crate::recover(&mut f.store, &old).is_err());
    let fresh = Request {
        operation: key("fresh-large"),
        ..old.clone()
    };
    crate::publish(
        &mut f.store,
        &fresh,
        &mut std::io::repeat(0x5a).take(old.object.bytes),
    )
    .unwrap();
    assert_eq!(
        apply_port(&mut f, &mut protection, &p).evidence,
        removed.evidence
    );
    assert!(crate::recover(&mut f.store, &fresh).unwrap().is_some());
    assert!(crate::recover(&mut f.store, &old).is_err());
}

#[test]
fn native_final_removal_reopens_skips_history_and_permits_only_fresh_ingest() {
    let mut f = Fixture::new_removable();
    release(&mut f);
    let p = start(&mut f);
    let mut protection = Finite::default();
    let result = apply_port(&mut f, &mut protection, &p);
    assert!(result.charged_metadata > 0);
    report(&mut f, &p, &result);
    f.reopen();
    assert!(f.store.read_object(&object(&f)).is_err());
    assert!(ingest(&mut f, "old").is_err());
    assert!(f.store.read_object(&object(&f)).is_err());
    ingest(&mut f, "fresh").unwrap();
    let checkpoint = f.ledger.checkpoint();
    assert_eq!(
        apply_port(&mut f, &mut protection, &p).evidence,
        result.evidence
    );
    assert_eq!(f.ledger.checkpoint(), checkpoint);
    assert!(f.store.read_object(&object(&f)).is_ok());
    assert!(ingest(&mut f, "old").is_err());
    let fresh = Request {
        operation: key("fresh"),
        ..f.grant.request().publication.clone()
    };
    let rp = retention::plan(
        &f.store,
        &retention::RetentionRequest {
            operation: key("fresh-pin"),
            owner: key("owner"),
            publication: fresh,
            minimum_ack: AckLevel::ProcessRestart,
        },
    )
    .unwrap();
    let grant = retention::retain(&mut f.ledger, &f.store, &f.job, &rp)
        .unwrap()
        .grant;
    assert!(retention::verify(&mut f.ledger, &f.store, &grant).is_ok());
    println!(
        "NATIVE FINAL REMOVAL: admitted claim release -> actual D unlink -> reopen -> old publication refused -> explicit fresh ingest preserved on historical removal replay. Complete protection is a finite synthetic adapter; net reclaim is unknown."
    );
}
#[test]
fn unknown_coverage_jobs_readers_and_remaining_native_retention_block_unlink() {
    let mut f = Fixture::new_removable();
    let mut other = f.grant.request().clone();
    other.operation = key("other-pin");
    let rp = retention::plan(&f.store, &other).unwrap();
    let grant = retention::retain(&mut f.ledger, &f.store, &f.job, &rp)
        .unwrap()
        .grant;
    release(&mut f);
    let p = start(&mut f);
    assert!(matches!(
        apply(execute(&mut f, &mut Finite::default()), &p),
        Err(RemovalError::Store(Error::Retained))
    ));
    assert!(retention::verify(&mut f.ledger, &f.store, &grant).is_ok());
    assert!(f.store.read_object(&object(&f)).is_ok());
    // Separate unretained fixture exercises each additional admission fence.
    let mut f = Fixture::new_removable();
    release(&mut f);
    let p = start(&mut f);
    let reader = f.store.read_object(&object(&f)).unwrap();
    assert!(matches!(
        apply(execute(&mut f, &mut Finite::default()), &p),
        Err(RemovalError::Store(Error::Busy))
    ));
    drop(reader);
    for unknown in [true, false] {
        let mut protection = Finite::default();
        protection.facts.borrow_mut().unknown = unknown;
        protection.facts.borrow_mut().job = !unknown;
        assert!(matches!(
            apply(execute(&mut f, &mut protection), &p),
            Err(RemovalError::Store(Error::Retained))
        ));
        assert!(f.store.read_object(&object(&f)).is_ok());
    }
}
#[test]
fn every_effect_boundary_reconciles_without_deleting_a_new_incarnation() {
    for cut in [
        Stage::Bound,
        Stage::Marked,
        Stage::Unlinked,
        Stage::Completed,
        Stage::Settled,
    ] {
        let mut f = Fixture::new_removable();
        release(&mut f);
        let p = start(&mut f);
        let mut protection = Finite::default();
        assert!(matches!(
            apply_hook(execute(&mut f, &mut protection), &p, |stage| {
                if stage == cut {
                    Err(RemovalError::Injected)
                } else {
                    Ok(())
                }
            }),
            Err(RemovalError::Injected)
        ));
        f.reopen();
        if cut == Stage::Marked || cut == Stage::Unlinked {
            assert!(ingest(&mut f, "cannot-ingest-pending").is_err());
        }
        if cut == Stage::Completed || cut == Stage::Settled {
            ingest(&mut f, "fresh-before-settlement").unwrap();
        }
        let r = apply(execute(&mut f, &mut protection), &p).unwrap();
        report(&mut f, &p, &r);
        if cut == Stage::Completed || cut == Stage::Settled {
            assert!(f.store.read_object(&object(&f)).is_ok());
        } else {
            assert!(f.store.read_object(&object(&f)).is_err());
        }
    }
}
#[test]
fn old_protected_but_uncommitted_publication_cannot_resurrect_retired_bytes() {
    let mut f = Fixture::new_removable();
    let request = Request {
        operation: key("old-pending"),
        ..f.grant.request().publication.clone()
    };
    f.store.protect(&request).unwrap();
    release(&mut f);
    let p = start(&mut f);
    apply(execute(&mut f, &mut Finite::default()), &p).unwrap();
    let data = bytes(&f);
    assert!(crate::publish(&mut f.store, &request, &mut Cursor::new(data)).is_err());
    assert!(f.store.read_object(&object(&f)).is_err());
    ingest(&mut f, "new-pending-era").unwrap();
    assert!(f.store.protect(&request).is_err());
}
#[test]
fn current_protection_failure_after_marker_preserves_bytes_and_blocks_publication() {
    let mut f = Fixture::new_removable();
    release(&mut f);
    let p = start(&mut f);
    let mut protection = Finite::default();
    let facts = protection.facts.clone();
    assert!(matches!(
        apply_hook(execute(&mut f, &mut protection), &p, |stage| {
            if stage == Stage::Marked {
                facts.borrow_mut().unknown = true;
            }
            Ok(())
        }),
        Err(RemovalError::Store(Error::Retained))
    ));
    assert!(path_present(&f.store.object_path(&object(&f))).unwrap());
    assert!(ingest(&mut f, "pending-not-an-ingest").is_err());
    facts.borrow_mut().unknown = false;
    apply(execute(&mut f, &mut protection), &p).unwrap();
    assert!(!path_present(&f.store.object_path(&object(&f))).unwrap());
}
#[test]
fn legacy_profile_foreign_scope_and_unadmitted_attempt_refuse() {
    let mut f = Fixture::new_removable();
    release(&mut f);
    assert!(plan(&f.store, &f.ctx, &object(&f), 0).is_err());
    let p = plan(&f.store, &f.ctx, &object(&f), 2).unwrap();
    assert!(matches!(
        apply(execute(&mut f, &mut Finite::default()), &p),
        Err(RemovalError::NotAdmitted)
    ));
    let mut p = start(&mut f);
    p.scope.store = d("foreign-store");
    assert!(apply(execute(&mut f, &mut Finite::default()), &p).is_err());
    let temp = tempfile::tempdir().unwrap();
    let legacy = LocalStore::create(&temp.path().join("legacy"), key("legacy")).unwrap();
    assert!(matches!(budget(&legacy), Err(Error::Unsupported)));
    assert!(LocalStore::open(&temp.path().join("legacy")).is_ok());
    assert!(!temp.path().join("legacy/removals").exists());
}
#[test]
fn a_decision_that_relies_on_survivors_cannot_use_the_final_loss_kernel() {
    let mut f = Fixture::new_with_survivor();
    release(&mut f);
    let object = object(&f);
    let a = progress::start(
        &mut f.journal,
        &mut f.nodes,
        &f.ctx,
        &object,
        progress::Action::Remove {
            store: f.store.identity().clone(),
        },
    )
    .unwrap();
    assert!(
        f.ctx
            .decision()
            .consent()
            .accept_intentional_loss
            .is_empty()
    );
    assert!(matches!(
        plan(&f.store, &f.ctx, &object, a.attempt),
        Err(RemovalError::InvalidPlan)
    ));
    assert!(f.store.read_object(&object).is_ok());
}

#[test]
fn unrelated_publication_runs_while_removal_holds_its_exact_scope() {
    let mut f = Fixture::new_removable();
    release(&mut f);
    let p = start(&mut f);
    let mut peer = LocalStore::open(&f.temp.path().join("store")).unwrap();
    let other = Request {
        operation: key("unrelated"),
        owner: key("owner"),
        object: Object::from_bytes(b"different object"),
        required: AckLevel::ProcessRestart,
    };
    let same = Request {
        operation: key("overlapping"),
        ..f.grant.request().publication.clone()
    };
    let data = bytes(&f);
    let mut protection = Finite::default();
    let facts = protection.facts.clone();
    apply_hook(execute(&mut f, &mut protection), &p, |stage| {
        if stage == Stage::Marked {
            assert!(facts.borrow().held);
            crate::publish(&mut peer, &other, &mut Cursor::new(b"different object")).unwrap();
            assert!(crate::publish(&mut peer, &same, &mut Cursor::new(&data)).is_err());
        }
        Ok(())
    })
    .unwrap();
    assert!(!facts.borrow().held);
    assert!(peer.read_object(&other.object).is_ok());
}
#[test]
fn no_effect_progress_advance_before_fence_refuses_old_attempt() {
    let mut f = Fixture::new_removable();
    release(&mut f);
    let p = start(&mut f);
    let mut journal =
        SourceJournal::open(&f.temp.path().join("journal"), &f.journal.checkpoint()).unwrap();
    let mut nodes =
        MetadataStore::open(&f.temp.path().join("nodes"), &f.nodes.checkpoint()).unwrap();
    let ctx = f.ctx.clone();
    let action = progress::Action::Remove {
        store: f.store.identity().clone(),
    };
    let object = object(&f);
    let result = apply_hook(execute(&mut f, &mut Finite::default()), &p, |stage| {
        if stage == Stage::Bound {
            progress::finish(
                &mut journal,
                &mut nodes,
                &ctx,
                &object,
                p.scope.attempt,
                action.clone(),
                progress::Outcome::NoEffect {
                    evidence: d("provider-reconciled-before-effect"),
                },
            )
            .unwrap();
            progress::start(&mut journal, &mut nodes, &ctx, &object, action.clone()).unwrap();
        }
        Ok(())
    });
    assert!(matches!(result, Err(RemovalError::NotAdmitted)));
    assert!(f.store.read_object(&object).is_ok());
    let p2 = plan(&f.store, &f.ctx, &object, p.scope.attempt + 1).unwrap();
    apply(execute(&mut f, &mut Finite::default()), &p2).unwrap();
}
#[test]
fn cancel_before_admission_refuses_and_admitted_removal_can_reconcile() {
    for admitted in [false, true] {
        let mut f = Fixture::new_removable();
        release(&mut f);
        let p = if admitted {
            start(&mut f)
        } else {
            plan(&f.store, &f.ctx, &object(&f), 2).unwrap()
        };
        intent::cancel(
            &mut f.journal,
            &key("owner"),
            &d("retire"),
            2,
            key("cancel"),
        )
        .unwrap();
        let result = apply(execute(&mut f, &mut Finite::default()), &p);
        if admitted {
            report(&mut f, &p, &result.unwrap());
            assert!(f.store.read_object(&object(&f)).is_err());
        } else {
            assert!(result.is_err());
            assert!(f.store.read_object(&object(&f)).is_ok());
        }
    }
}
#[test]
fn corrupt_bytes_budget_and_corrupt_disposition_never_become_success() {
    let mut f = Fixture::new_removable();
    release(&mut f);
    let p = start(&mut f);
    let t = f.ledger.begin_probe(key("small-probe")).unwrap();
    f.ledger
        .observe(key("small-sample"), t.reference(), 1024 * 1024 * 1024)
        .unwrap();
    let small = f
        .ledger
        .reserve(key("small"), key("owner"), 1)
        .unwrap()
        .reference()
        .clone();
    let mut protection = Finite::default();
    let mut execution = execute(&mut f, &mut protection);
    execution.job = &small;
    assert!(apply(execution, &p).is_err());
    assert!(f.store.read_object(&object(&f)).is_ok());
    let payload = f.store.object_path(&object(&f));
    fs::write(&payload, b"corrupt").unwrap();
    assert!(apply(execute(&mut f, &mut protection), &p).is_err());
    assert!(!path_present(&path(&f.store, &object(&f), &p.scope.incarnation, "begin")).unwrap());
    fs::write(&payload, bytes(&f)).unwrap();
    let alias = f.temp.path().join("external-hard-link");
    fs::hard_link(&payload, &alias).unwrap();
    let r = apply(execute(&mut f, &mut protection), &p).unwrap();
    assert_eq!(fs::read(&alias).unwrap(), bytes(&f));
    report(&mut f, &p, &r);
    assert!(
        progress::inspect(&mut f.journal, &f.nodes, &f.ctx, &p.scope.object)
            .unwrap()
            .removed[&p.scope.store]
            .reclaimed
            .is_none()
    );
    let dp = path(&f.store, &object(&f), &p.scope.incarnation, "done");
    fs::write(&dp, b"{}").unwrap();
    f.reopen();
    assert!(apply(execute(&mut f, &mut protection), &p).is_err());
    assert!(ingest(&mut f, "corrupt-disposition-not-new-incarnation").is_err());
    assert_eq!(fs::read(alias).unwrap(), bytes(&f));
}
#[test]
fn constructed_chain_bound_preserves_history_and_refuses_further_removal() {
    let mut f = Fixture::new_removable();
    release(&mut f);
    let mut p = start(&mut f);
    let object = object(&f);
    let dir = f.store.root.join("removals").join(object.sha256.as_str());
    fs::create_dir(&dir).unwrap();
    for _ in 0..MAX_REMOVALS {
        p.scope.incarnation = state(&f.store, &object).unwrap().incarnation;
        let begin = Begin {
            schema: BEGIN.into(),
            plan: p.clone(),
            job: f.job.clone(),
            allocation: f.job.clone(),
            file_data_before: 0,
            protection: d("constructed-chain-only"),
        };
        f.store
            .write_record(
                &path(&f.store, &object, &p.scope.incarnation, "begin"),
                &begin,
            )
            .unwrap();
        f.store
            .write_record(
                &path(&f.store, &object, &p.scope.incarnation, "done"),
                &disposition(&begin).unwrap(),
            )
            .unwrap();
    }
    f.reopen();
    assert_eq!(state(&f.store, &object).unwrap().depth, MAX_REMOVALS);
    assert!(plan(&f.store, &f.ctx, &object, p.scope.attempt).is_err());
    assert_eq!(fs::read_dir(dir).unwrap().count(), 2 * MAX_REMOVALS);
    // Constructed markers do not prove execution; this tests only bounded chain
    // handling. A genuinely fresh ingest is still admitted in the current era.
    ingest(&mut f, "fresh-after-chain-bound").unwrap();
}
#[derive(Serialize, Deserialize)]
struct Controls {
    ledger: crate::ledger::Checkpoint,
    journal: crate::source_claims::Checkpoint,
    nodes: crate::metadata::Checkpoint,
    plan: Plan,
    job: JournalRef,
}
#[test]
fn removal_child() {
    let Some(root) = std::env::var_os("TIG_NATIVE_REMOVAL_TEST_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let c: Controls =
        serde_json::from_slice(&fs::read(root.join("removal-controls.json")).unwrap()).unwrap();
    let store = LocalStore::open(&root.join("store")).unwrap();
    let mut ledger = CapacityLedger::open(&root.join("ledger"), &c.ledger).unwrap();
    let mut journal = SourceJournal::open(&root.join("journal"), &c.journal).unwrap();
    let nodes = MetadataStore::open(&root.join("nodes"), &c.nodes).unwrap();
    let ctx = intent::observe(
        &mut journal,
        &nodes,
        &intent::Enrollment {
            owner: key("owner"),
            subject: d("subject"),
        },
        &d("retire"),
    )
    .unwrap();
    let cut = std::env::var("TIG_NATIVE_REMOVAL_TEST_CUT").unwrap();
    let mut protection = Finite::default();
    let _ = apply_hook(
        Execution {
            store: &store,
            ledger: &mut ledger,
            job: &c.job,
            journal: &mut journal,
            nodes: &nodes,
            intent: &ctx,
            protection: &mut protection,
        },
        &c.plan,
        |stage| {
            if (cut == "marked" && stage == Stage::Marked)
                || (cut == "unlinked" && stage == Stage::Unlinked)
                || (cut == "completed" && stage == Stage::Completed)
            {
                std::process::exit(76);
            }
            Ok(())
        },
    );
    panic!("child failed to reach cut");
}
#[test]
fn actual_child_exits_reconcile_pending_unlinked_and_completed_native_copies() {
    for cut in ["marked", "unlinked", "completed"] {
        let mut f = Fixture::new_removable();
        release(&mut f);
        let p = start(&mut f);
        fs::write(
            f.temp.path().join("removal-controls.json"),
            serde_json::to_vec(&Controls {
                ledger: f.ledger.checkpoint(),
                journal: f.journal.checkpoint(),
                nodes: f.nodes.checkpoint(),
                plan: p.clone(),
                job: f.job.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "local::removal::tests::removal_child",
                "--nocapture",
            ])
            .env("TIG_NATIVE_REMOVAL_TEST_ROOT", f.temp.path())
            .env("TIG_NATIVE_REMOVAL_TEST_CUT", cut)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(76));
        f.reopen();
        if cut == "completed" {
            ingest(&mut f, "fresh-after-child-exit").unwrap();
        } else {
            assert!(ingest(&mut f, "pending-after-child-exit").is_err());
        }
        let r = apply(execute(&mut f, &mut Finite::default()), &p).unwrap();
        report(&mut f, &p, &r);
        assert!(ingest(&mut f, "old").is_err());
        assert_eq!(f.store.read_object(&object(&f)).is_ok(), cut == "completed");
    }
}

#[test]
fn portable_removal_admission_preserves_native_protection_and_exact_binding() {
    use crate::provider::removal as port;
    let mut f = Fixture::new_removable();
    release(&mut f);
    let p = start(&mut f);
    let request = port::native::request(&p).unwrap();
    let cp = f.ledger.checkpoint();
    let mut protection = Finite::default();
    {
        let mut executor = port::native::final_loss(execute(&mut f, &mut protection), &p).unwrap();
        assert!(matches!(
            port::remove_exact(&mut executor, &request, false),
            Err(port::Error::Unsupported)
        ));
        for field in 0..5 {
            let mut changed = request.clone();
            match field {
                0 => changed.owner = key("another-owner"),
                1 => changed.decision = d("another-decision"),
                2 => changed.attempt += 1,
                3 => changed.target.version = port::CopyVersion::new(b"another-copy").unwrap(),
                _ => changed.preservation = Some(d("another-preservation")),
            }
            assert!(matches!(
                port::remove_exact(&mut executor, &changed, true),
                Err(port::Error::Invalid)
            ));
        }
    }
    assert_eq!(cp, f.ledger.checkpoint());
    assert!(f.store.read_object(&object(&f)).is_ok());
    protection.facts.borrow_mut().unknown = true;
    {
        let mut executor = port::native::final_loss(execute(&mut f, &mut protection), &p).unwrap();
        assert!(matches!(
            port::remove_exact(&mut executor, &request, true),
            Err(port::Error::Protected)
        ));
    }
    assert!(f.store.read_object(&object(&f)).is_ok());
    protection.facts.borrow_mut().unknown = false;
    protection.facts.borrow_mut().job = true;
    {
        let mut executor = port::native::final_loss(execute(&mut f, &mut protection), &p).unwrap();
        assert!(matches!(
            port::remove_exact(&mut executor, &request, true),
            Err(port::Error::Protected)
        ));
    }
    protection.facts.borrow_mut().job = false;
    let removed = apply_port(&mut f, &mut protection, &p);
    report(&mut f, &p, &removed);
    f.reopen();
    ingest(&mut f, "new-after-port-removal").unwrap();
    assert_eq!(
        apply_port(&mut f, &mut protection, &p).evidence,
        removed.evidence
    );
    assert!(f.store.read_object(&object(&f)).is_ok());
}
