use super::super::tests::Finite;
use super::*;
use crate::{
    metadata::MetadataStore,
    retention::{handoff, release::tests::Fixture},
    retirement_intent as intent,
};
use std::io::Cursor;
#[path = "custody/portable_tests.rs"]
mod portable_tests;
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn d(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn seed(id: &str) -> Vec<u8> {
    let catalogue: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../examples/teaching/catalogue.json"
    ))
    .unwrap();
    catalogue["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)
        .unwrap()["content"]
        .as_str()
        .unwrap()
        .as_bytes()
        .to_vec()
}
struct Backup {
    temp: tempfile::TempDir,
    store: LocalStore,
    ledger: CapacityLedger,
    job: JournalRef,
    grant: RetentionGrant,
    domain: Key,
}
impl Backup {
    fn new(place: &str, domain: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut store = LocalStore::create(&temp.path().join("store"), key(place)).unwrap();
        let request = Request {
            operation: key("old"),
            owner: key("owner"),
            object: Object::from_bytes(&seed("old")),
            required: AckLevel::ProcessRestart,
        };
        crate::publish(&mut store, &request, &mut Cursor::new(seed("old"))).unwrap();
        let mut ledger = CapacityLedger::create(
            &temp.path().join("ledger"),
            key("one-test-volume"),
            key("epoch"),
            0,
        )
        .unwrap();
        let plan = retention::plan(
            &store,
            &retention::RetentionRequest {
                operation: key("hold"),
                owner: key("owner"),
                publication: request,
                minimum_ack: AckLevel::ProcessRestart,
            },
        )
        .unwrap();
        let t = ledger.begin_probe(key("probe")).unwrap();
        ledger
            .observe(key("sample"), t.reference(), 1024 * 1024 * 1024)
            .unwrap();
        let job = ledger
            .reserve(key("job"), key("owner"), plan.required_peak() * 20)
            .unwrap()
            .reference()
            .clone();
        let grant = retention::retain(&mut ledger, &store, &job, &plan)
            .unwrap()
            .grant;
        Self {
            temp,
            store,
            ledger,
            job,
            grant,
            domain: key(domain),
        }
    }
    fn reopen(&mut self) {
        self.store = LocalStore::open(&self.temp.path().join("store")).unwrap();
        self.ledger =
            CapacityLedger::open(&self.temp.path().join("ledger"), &self.ledger.checkpoint())
                .unwrap();
    }
}
struct Case {
    f: Fixture,
    backups: Vec<Backup>,
    p: Plan,
}
impl Case {
    fn new() -> Self {
        let mut backups = vec![
            Backup::new("M", "backup-domain"),
            Backup::new("D", "working-domain"),
        ];
        let mut f = Fixture::new_with_store_survivors(
            &backups
                .iter()
                .map(|b| b.store.identity().clone())
                .collect::<Vec<_>>(),
        );
        let object = f.grant.request().publication.object.clone();
        let action = progress::Action::Release {
            claim: f.grant.claim_id().clone(),
        };
        let a = progress::start(
            &mut f.journal,
            &mut f.nodes,
            &f.ctx,
            &object,
            action.clone(),
        )
        .unwrap();
        let hp = handoff::plan(
            &f.store,
            &handoff::HandoffRequest {
                operation: key("handoff-F-to-M"),
                owner: key("owner"),
                previous: f.grant.clone(),
                successor: backups[0].grant.clone(),
                source_domain: key("working-domain"),
                successor_domain: key("backup-domain"),
            },
        )
        .unwrap();
        let authorization = hp.authorize_handoff().unwrap();
        let m = &mut backups[0];
        let receipt = handoff::apply(
            &mut f.ledger,
            &f.store,
            &mut m.ledger,
            &m.store,
            &f.job,
            &hp,
            &authorization,
        )
        .unwrap();
        progress::finish(
            &mut f.journal,
            &mut f.nodes,
            &f.ctx,
            &object,
            a.attempt,
            action,
            progress::Outcome::Released {
                evidence: receipt.marker,
            },
        )
        .unwrap();
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
        let mut profile = Profile {
            source_domain: key("working-domain"),
            retained: backups
                .iter()
                .map(|b| Survivor {
                    grant: b.grant.clone(),
                    failure_domain: b.domain.clone(),
                })
                .collect(),
            required_domains: 1,
            minimum_ack: AckLevel::ProcessRestart,
        };
        profile
            .retained
            .sort_by(|a, b| a.grant.store().cmp(b.grant.store()));
        let p = plan(&f.store, &f.ctx, &object, a.attempt, &profile).unwrap();
        Self { f, backups, p }
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
    fn apply(&mut self, protection: &mut Finite) -> Result<RemovalReceipt> {
        let mut endpoints = self
            .backups
            .iter_mut()
            .map(|b| Endpoint {
                store: &b.store,
                ledger: &mut b.ledger,
            })
            .collect::<Vec<_>>();
        apply(
            Self::execute(&mut self.f, protection),
            &self.p,
            &mut endpoints,
        )
    }
    fn run(
        &mut self,
        protection: &mut Finite,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<RemovalReceipt> {
        let mut endpoints = self
            .backups
            .iter_mut()
            .map(|b| Endpoint {
                store: &b.store,
                ledger: &mut b.ledger,
            })
            .collect::<Vec<_>>();
        run(
            Self::execute(&mut self.f, protection),
            &self.p,
            &mut endpoints,
            hook,
        )
    }
    fn report(&mut self, r: &RemovalReceipt) {
        progress::finish(
            &mut self.f.journal,
            &mut self.f.nodes,
            &self.f.ctx,
            &self.p.scope.object,
            self.p.scope.attempt,
            progress::Action::Remove {
                store: self.f.store.identity().clone(),
            },
            progress::Outcome::Removed {
                evidence: r.evidence.clone(),
                allocation: progress::Allocation {
                    before: Some(r.file_data_before),
                    reclaimed: None,
                },
            },
        )
        .unwrap();
    }
    fn reopen(&mut self) {
        self.f.reopen();
        for b in &mut self.backups {
            b.reopen();
        }
    }
}
#[test]
fn portable_executor_preserves_legacy_native_custody_and_replay() {
    use crate::provider::removal as port;
    let mut c = Case::new();
    let r = {
        let mut protection = Finite::default();
        let mut endpoints = c
            .backups
            .iter_mut()
            .map(|b| Endpoint {
                store: &b.store,
                ledger: &mut b.ledger,
            })
            .collect::<Vec<_>>();
        let mut executor = port::native::retained_native(
            Case::execute(&mut c.f, &mut protection),
            &c.p,
            &mut endpoints,
        )
        .unwrap();
        port::remove_exact(&mut executor, &port::native::request(&c.p).unwrap(), true)
            .unwrap()
            .evidence
    };
    c.report(&r);
    c.reopen();
    assert!(c.f.store.read_object(&c.p.scope.object).is_err());
    for b in &mut c.backups {
        assert!(retention::verify(&mut b.ledger, &b.store, &b.grant).is_ok());
    }
    let mut protection = Finite::default();
    let mut endpoints = [];
    let mut executor = port::native::retained_native(
        Case::execute(&mut c.f, &mut protection),
        &c.p,
        &mut endpoints,
    )
    .unwrap();
    assert_eq!(
        port::remove_exact(&mut executor, &port::native::request(&c.p).unwrap(), true)
            .unwrap()
            .evidence
            .evidence,
        r.evidence
    );
}
#[test]
fn native_f_d_m_eviction_keeps_backups_and_allows_actual_reacquisition() {
    let mut c = Case::new();
    let fixed = Request {
        operation: key("fixed"),
        owner: key("owner"),
        object: Object::from_bytes(&seed("fixed")),
        required: AckLevel::ProcessRestart,
    };
    crate::publish(&mut c.f.store, &fixed, &mut Cursor::new(seed("fixed"))).unwrap();
    crate::publish(
        &mut c.backups[0].store,
        &fixed,
        &mut Cursor::new(seed("fixed")),
    )
    .unwrap();
    let hold = retention::plan(
        &c.backups[0].store,
        &retention::RetentionRequest {
            operation: key("hold-fixed"),
            owner: key("owner"),
            publication: fixed.clone(),
            minimum_ack: AckLevel::ProcessRestart,
        },
    )
    .unwrap();
    let m = &mut c.backups[0];
    let fixed_grant = retention::retain(&mut m.ledger, &m.store, &m.job, &hold)
        .unwrap()
        .grant;
    // Finite teaching placement, not globally complete SourceJournal coverage.
    let placement = |object: &Object, place: &str| tig_core::Placement {
        object: object.clone(),
        place: place.into(),
    };
    let desired = [
        placement(&c.p.scope.object, "D"),
        placement(&c.p.scope.object, "M"),
        placement(&fixed.object, "F"),
        placement(&fixed.object, "M"),
    ]
    .into();
    let view = tig_core::View::new(
        2,
        true,
        vec![tig_core::Claim {
            id: "accepted-finite-golden-policy".into(),
            placements: desired,
        }],
    )
    .unwrap();
    let observed = [
        placement(&c.p.scope.object, "F"),
        placement(&c.p.scope.object, "D"),
        placement(&c.p.scope.object, "M"),
        placement(&fixed.object, "F"),
        placement(&fixed.object, "M"),
    ]
    .into();
    assert_eq!(
        view.plan(&observed).unwrap().release_candidates,
        [placement(&c.p.scope.object, "F")].into()
    );
    assert!(
        c.f.ctx
            .decision()
            .consent()
            .accept_intentional_loss
            .is_empty()
    );
    assert!(
        !c.f.journal
            .obligations(100)
            .unwrap()
            .complete_claim_coverage()
    );
    let r = c.apply(&mut Finite::default()).unwrap();
    c.report(&r);
    c.reopen();
    assert!(c.f.store.read_object(&c.p.scope.object).is_err());
    assert!(c.f.store.read_object(&fixed.object).is_ok());
    for b in &mut c.backups {
        assert!(retention::verify(&mut b.ledger, &b.store, &b.grant).is_ok());
    }
    let m = &mut c.backups[0];
    assert!(retention::verify(&mut m.ledger, &m.store, &fixed_grant).is_ok());
    assert!(
        crate::publish(
            &mut c.f.store,
            &c.f.grant.request().publication,
            &mut Cursor::new(seed("old"))
        )
        .is_err()
    );
    let fresh = Request {
        operation: key("explicit-reacquire"),
        ..c.f.grant.request().publication.clone()
    };
    let mut reader = crate::replica::verify(
        &c.backups[0].store,
        &c.backups[0].grant.request().publication,
    )
    .unwrap();
    crate::publish(&mut c.f.store, &fresh, &mut reader).unwrap();
    drop(reader);
    let before = c.f.ledger.checkpoint();
    // Historical completion needs no new custody or current deletion permission.
    let mut protection = Finite::default();
    protection.facts.borrow_mut().unknown = true;
    let replay = apply(Case::execute(&mut c.f, &mut protection), &c.p, &mut []).unwrap();
    assert_eq!(replay.evidence, r.evidence);
    assert_eq!(before, c.f.ledger.checkpoint());
    assert!(c.f.store.read_object(&fresh.object).is_ok());
    println!(
        "NATIVE F/D/M: old F retained in D/M -> native F-to-M claim handoff -> native F eviction; fixed F/M and old D/M preserved -> explicit reacquire streams M back to F. Historical eviction cannot remove it. Domains/protection are finite synthetic admissions; net reclaim is unknown."
    );
}
#[test]
fn missing_corrupt_or_released_custody_never_unlinks_source() {
    let mut c = Case::new();
    let moved = c.backups[0].temp.path().join("offline-store");
    let root = c.backups[0].temp.path().join("store");
    fs::rename(&root, &moved).unwrap();
    assert!(c.apply(&mut Finite::default()).is_err());
    assert!(c.f.store.read_object(&c.p.scope.object).is_ok());
    fs::rename(&moved, &root).unwrap();
    let payload = c.backups[0].store.object_path(&c.p.scope.object);
    fs::write(&payload, b"corrupt-backup").unwrap();
    assert!(c.apply(&mut Finite::default()).is_err());
    assert!(c.f.store.read_object(&c.p.scope.object).is_ok());
    fs::write(&payload, seed("old")).unwrap(); // reset the controlled corruption fault
    let hp = handoff::plan(
        &c.backups[0].store,
        &handoff::HandoffRequest {
            operation: key("retire-M-claim"),
            owner: key("owner"),
            previous: c.backups[0].grant.clone(),
            successor: c.backups[1].grant.clone(),
            source_domain: key("backup-domain"),
            successor_domain: key("working-domain"),
        },
    )
    .unwrap();
    let auth = hp.authorize_handoff().unwrap();
    let (m, d) = c.backups.split_at_mut(1);
    handoff::apply(
        &mut m[0].ledger,
        &m[0].store,
        &mut d[0].ledger,
        &d[0].store,
        &m[0].job,
        &hp,
        &auth,
    )
    .unwrap();
    assert!(c.apply(&mut Finite::default()).is_err());
    assert!(c.f.store.read_object(&c.p.scope.object).is_ok());
}
#[test]
fn profile_bounds_domains_ack_and_reviewed_survivors_are_exact() {
    let c = Case::new();
    let profile = c.p.custody.as_ref().unwrap();
    let mut bad = Vec::new();
    let mut p = profile.clone();
    p.required_domains = 0;
    bad.push(p);
    let mut p = profile.clone();
    p.required_domains = 2;
    bad.push(p);
    let mut p = profile.clone();
    p.minimum_ack = AckLevel::VerifiedLocal;
    bad.push(p);
    let mut p = profile.clone();
    p.minimum_ack = AckLevel::ProviderDurable;
    bad.push(p);
    let mut p = profile.clone();
    p.retained.clear();
    bad.push(p);
    let mut p = profile.clone();
    p.retained.reverse();
    bad.push(p);
    let mut p = profile.clone();
    p.retained[1] = p.retained[0].clone();
    bad.push(p);
    let mut p = profile.clone();
    p.retained = vec![p.retained[0].clone(); 17];
    bad.push(p);
    let mut p = profile.clone();
    for r in &mut p.retained {
        r.failure_domain = p.source_domain.clone()
    }
    bad.push(p);
    let mut p = profile.clone();
    p.retained.remove(0);
    bad.push(p);
    for p in bad {
        assert!(
            plan(
                &c.f.store,
                &c.f.ctx,
                &c.p.scope.object,
                c.p.scope.attempt,
                &p
            )
            .is_err()
        );
    }
}
#[test]
fn eviction_and_final_loss_entry_points_cannot_substitute_for_each_other() {
    let mut c = Case::new();
    assert!(matches!(
        super::super::apply(Case::execute(&mut c.f, &mut Finite::default()), &c.p),
        Err(RemovalError::InvalidPlan)
    ));
    let mut modified = c.p.clone();
    modified.scope.preservation = Some(d("wrong-profile"));
    assert!(
        apply(
            Case::execute(&mut c.f, &mut Finite::default()),
            &modified,
            &mut []
        )
        .is_err()
    );
    assert!(c.f.store.read_object(&c.p.scope.object).is_ok());
    let mut f = Fixture::new_removable();
    let n = f.start();
    let r = f.run(n).unwrap();
    f.finish(n, r.evidence);
    let object = f.grant.request().publication.object.clone();
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
    let final_plan = super::super::plan(&f.store, &f.ctx, &object, a.attempt).unwrap();
    assert!(
        apply(
            Case::execute(&mut f, &mut Finite::default()),
            &final_plan,
            &mut []
        )
        .is_err()
    );
    assert!(
        plan(
            &f.store,
            &f.ctx,
            &object,
            a.attempt,
            c.p.custody.as_ref().unwrap()
        )
        .is_err()
    );
}
#[test]
fn native_custody_guards_block_concurrent_release_through_completion() {
    let mut c = Case::new();
    let peer = LocalStore::open(&c.backups[0].temp.path().join("store")).unwrap();
    let object = c.p.scope.object.clone();
    c.run(&mut Finite::default(), |stage| {
        if stage == Stage::Marked || stage == Stage::Completed {
            assert!(matches!(
                peer.try_exclusive_object(&object),
                Err(Error::Busy)
            ));
        }
        Ok(())
    })
    .unwrap();
    assert!(matches!(
        peer.try_exclusive_object(&object),
        Err(Error::Retained)
    ));
}
#[test]
fn each_native_boundary_reopens_without_repeating_completed_eviction() {
    for cut in [
        Stage::Bound,
        Stage::Marked,
        Stage::Unlinked,
        Stage::Completed,
        Stage::Settled,
    ] {
        let mut c = Case::new();
        assert!(matches!(
            c.run(&mut Finite::default(), |stage| if stage == cut {
                Err(RemovalError::Injected)
            } else {
                Ok(())
            }),
            Err(RemovalError::Injected)
        ));
        c.reopen();
        let r = c.apply(&mut Finite::default()).unwrap();
        c.report(&r);
        assert!(c.f.store.read_object(&c.p.scope.object).is_err());
        for b in &mut c.backups {
            assert!(retention::verify(&mut b.ledger, &b.store, &b.grant).is_ok());
        }
    }
}

#[test]
fn custody_revalidation_after_begin_preserves_source_until_fault_is_repaired() {
    let mut c = Case::new();
    let payload = c.backups[0].store.object_path(&c.p.scope.object);
    let result = c.run(&mut Finite::default(), |stage| {
        if stage == Stage::Marked {
            fs::write(&payload, b"controlled-corruption-after-begin").unwrap();
        }
        Ok(())
    });
    assert!(result.is_err());
    assert!(
        state(&c.f.store, &c.p.scope.object)
            .unwrap()
            .pending
            .is_some()
    );
    assert!(verify_payload(&c.f.store, &c.p.scope.object).is_ok());
    assert!(c.f.store.read_object(&c.p.scope.object).is_err());
    fs::write(payload, seed("old")).unwrap(); // reset the injected raw-writer fault
    c.reopen();
    let r = c.apply(&mut Finite::default()).unwrap();
    c.report(&r);
    assert!(c.f.store.read_object(&c.p.scope.object).is_err());
}
#[test]
fn missing_duplicate_and_wrong_ledger_endpoints_refuse_without_source_effect() {
    let mut c = Case::new();
    let (m, d) = c.backups.split_at_mut(1);
    let mut short = [Endpoint {
        store: &m[0].store,
        ledger: &mut m[0].ledger,
    }];
    assert!(
        apply(
            Case::execute(&mut c.f, &mut Finite::default()),
            &c.p,
            &mut short
        )
        .is_err()
    );
    let mut duplicate = [
        Endpoint {
            store: &m[0].store,
            ledger: &mut m[0].ledger,
        },
        Endpoint {
            store: &m[0].store,
            ledger: &mut d[0].ledger,
        },
    ];
    assert!(
        apply(
            Case::execute(&mut c.f, &mut Finite::default()),
            &c.p,
            &mut duplicate
        )
        .is_err()
    );
    let mut swapped = [
        Endpoint {
            store: &m[0].store,
            ledger: &mut d[0].ledger,
        },
        Endpoint {
            store: &d[0].store,
            ledger: &mut m[0].ledger,
        },
    ];
    assert!(
        apply(
            Case::execute(&mut c.f, &mut Finite::default()),
            &c.p,
            &mut swapped
        )
        .is_err()
    );
    assert!(
        state(&c.f.store, &c.p.scope.object)
            .unwrap()
            .pending
            .is_none()
    );
    assert!(c.f.store.read_object(&c.p.scope.object).is_ok());
}
#[derive(Serialize, Deserialize)]
struct BackupControl {
    root: PathBuf,
    ledger: crate::ledger::Checkpoint,
}
#[derive(Serialize, Deserialize)]
struct Controls {
    ledger: crate::ledger::Checkpoint,
    journal: crate::source_claims::Checkpoint,
    nodes: crate::metadata::Checkpoint,
    job: JournalRef,
    plan: Plan,
    backups: Vec<BackupControl>,
    #[serde(default)]
    provider_grants: Vec<RetentionGrant>,
}
#[test]
fn custody_child() {
    let Some(root) = std::env::var_os("TIG_RETAINED_EVICTION_TEST_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let c: Controls =
        serde_json::from_slice(&fs::read(root.join("eviction-controls.json")).unwrap()).unwrap();
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
    let stores = c
        .backups
        .iter()
        .map(|b| LocalStore::open(&b.root.join("store")).unwrap())
        .collect::<Vec<_>>();
    let mut ledgers = c
        .backups
        .iter()
        .map(|b| CapacityLedger::open(&b.root.join("ledger"), &b.ledger).unwrap())
        .collect::<Vec<_>>();
    let cut = std::env::var("TIG_RETAINED_EVICTION_TEST_CUT").unwrap();
    let mut protection = Finite::default();
    let execution = Execution {
        store: &store,
        ledger: &mut ledger,
        job: &c.job,
        journal: &mut journal,
        nodes: &nodes,
        intent: &ctx,
        protection: &mut protection,
    };
    let hook = |stage| {
        if (cut == "marked" && stage == Stage::Marked)
            || (cut == "unlinked" && stage == Stage::Unlinked)
            || (cut == "completed" && stage == Stage::Completed)
        {
            std::process::exit(76);
        }
        Ok(())
    };
    if c.plan.provider_custody.is_some() {
        use crate::provider::custody::{Retainer, native::Native};
        let mut providers = stores
            .iter()
            .zip(ledgers.iter_mut())
            .zip(&c.provider_grants)
            .map(|((store, ledger), grant)| Native {
                store,
                ledger,
                grant,
            })
            .collect::<Vec<_>>();
        let mut endpoints = providers
            .iter_mut()
            .map(|p| p as &mut dyn Retainer)
            .collect::<Vec<_>>();
        let _ = portable::run(execution, &c.plan, &mut endpoints, hook);
    } else {
        let mut endpoints = stores
            .iter()
            .zip(ledgers.iter_mut())
            .map(|(store, ledger)| Endpoint { store, ledger })
            .collect::<Vec<_>>();
        let _ = run(execution, &c.plan, &mut endpoints, hook);
    }
    panic!("child failed to reach cut");
}
#[test]
fn child_exits_reacquire_custody_or_replay_completed_eviction_without_new_effect() {
    for cut in ["marked", "unlinked", "completed"] {
        let mut c = Case::new();
        let controls = Controls {
            ledger: c.f.ledger.checkpoint(),
            journal: c.f.journal.checkpoint(),
            nodes: c.f.nodes.checkpoint(),
            job: c.f.job.clone(),
            plan: c.p.clone(),
            provider_grants: Vec::new(),
            backups: c
                .backups
                .iter()
                .map(|b| BackupControl {
                    root: b.temp.path().to_owned(),
                    ledger: b.ledger.checkpoint(),
                })
                .collect(),
        };
        fs::write(
            c.f.temp.path().join("eviction-controls.json"),
            serde_json::to_vec(&controls).unwrap(),
        )
        .unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "local::removal::custody::tests::custody_child",
                "--nocapture",
            ])
            .env("TIG_RETAINED_EVICTION_TEST_ROOT", c.f.temp.path())
            .env("TIG_RETAINED_EVICTION_TEST_CUT", cut)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(76));
        c.reopen();
        if cut == "marked" {
            let root = c.backups[0].temp.path().join("store");
            let offline = c.backups[0].temp.path().join("offline");
            fs::rename(&root, &offline).unwrap();
            assert!(c.apply(&mut Finite::default()).is_err());
            assert!(verify_payload(&c.f.store, &c.p.scope.object).is_ok());
            fs::rename(&offline, &root).unwrap();
        }
        let r = c.apply(&mut Finite::default()).unwrap();
        c.report(&r);
        assert!(c.f.store.read_object(&c.p.scope.object).is_err());
        for b in &mut c.backups {
            assert!(retention::verify(&mut b.ledger, &b.store, &b.grant).is_ok());
        }
    }
}
