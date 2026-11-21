use super::*;
use crate::{
    Error,
    metadata::{Limits, MetadataStore},
    retirement_intent as intent,
};
use std::io::{Cursor, Read};
use tig_core::{
    LogicalPath, Object,
    retirement::{
        review::{decision::*, *},
        *,
    },
};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn d(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn enrollment() -> Enrollment {
    Enrollment {
        owner: key("owner"),
        subject: d("subject"),
    }
}
pub(crate) struct Fixture {
    pub(crate) temp: tempfile::TempDir,
    pub(crate) store: LocalStore,
    pub(crate) ledger: CapacityLedger,
    pub(crate) job: JournalRef,
    pub(crate) grant: RetentionGrant,
    pub(crate) journal: SourceJournal,
    pub(crate) nodes: MetadataStore,
    pub(crate) ctx: ObservedIntent,
    pub(crate) plan: ReleasePlan,
}
impl Fixture {
    fn new() -> Self {
        Self::new_mode(false)
    }
    pub(crate) fn new_removable() -> Self {
        Self::new_mode(true)
    }
    pub(crate) fn new_large_removable() -> Self {
        Self::new_configured(true, &[], "D", true)
    }
    pub(crate) fn new_with_survivor() -> Self {
        Self::new_profile(true, true)
    }
    pub(crate) fn new_with_store_survivors(stores: &[Digest]) -> Self {
        Self::new_config(true, stores, "F")
    }
    fn new_mode(removable: bool) -> Self {
        Self::new_profile(removable, false)
    }
    fn new_profile(removable: bool, survivor: bool) -> Self {
        let stores = if survivor {
            vec![d("outside-policy-survivor")]
        } else {
            Vec::new()
        };
        Self::new_config(removable, &stores, "D")
    }
    fn new_config(removable: bool, survivors: &[Digest], alias: &str) -> Self {
        Self::new_configured(removable, survivors, alias, false)
    }
    fn new_configured(removable: bool, survivors: &[Digest], alias: &str, large: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let sample: serde_json::Value =
            serde_json::from_str(include_str!("../../../../examples/teaching/catalogue.json"))
                .unwrap();
        let asset = sample["assets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == "old")
            .unwrap();
        let generated = large.then(|| vec![0x5a; (crate::local::MAX_OBJECT_BYTES + 17) as usize]);
        let bytes = generated
            .as_deref()
            .unwrap_or_else(|| asset["content"].as_str().unwrap().as_bytes());
        let mut store = if large {
            LocalStore::create_profile(
                &root.join("store"),
                key(alias),
                crate::local::StoreProfile {
                    max_object_bytes: bytes.len() as u64,
                    removable,
                },
            )
        } else if removable {
            LocalStore::create_removable(&root.join("store"), key(alias))
        } else {
            LocalStore::create(&root.join("store"), key(alias))
        }
        .unwrap();
        let publication = Request {
            operation: key("old"),
            owner: key("owner"),
            object: Object::from_bytes(bytes),
            required: AckLevel::ProcessRestart,
        };
        crate::publish(&mut store, &publication, &mut Cursor::new(bytes)).unwrap();
        let mut ledger =
            CapacityLedger::create(&root.join("ledger"), key("volume"), key("epoch"), 0).unwrap();
        let rp = super::super::plan(
            &store,
            &RetentionRequest {
                operation: key("hold"),
                owner: key("owner"),
                publication: publication.clone(),
                minimum_ack: publication.required,
            },
        )
        .unwrap();
        let t = ledger.begin_probe(key("probe")).unwrap();
        ledger
            .observe(key("sample"), t.reference(), 1024 * 1024 * 1024)
            .unwrap();
        let job = ledger
            .reserve(key("job"), key("owner"), rp.required_peak() * 20)
            .unwrap()
            .reference()
            .clone();
        let grant = super::super::retain(&mut ledger, &store, &job, &rp)
            .unwrap()
            .grant;
        let mut journal =
            SourceJournal::create(&root.join("journal"), key("retirement"), key("epoch")).unwrap();
        let mut nodes = MetadataStore::create(
            &root.join("nodes"),
            Limits {
                nodes: 1000,
                bytes: 2 * 1024 * 1024,
                database_bytes: 4 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let object = publication.object;
        let path = LogicalPath::parse("cues/opening.tig").unwrap();
        let catalogue = Catalogue {
            snapshot: d("snapshot"),
            bindings: [(
                path.clone(),
                Binding {
                    object: object.clone(),
                    activity: Activity::Inactive(InactiveInterval {
                        clock: d("clock"),
                        identity: d("interval"),
                        started: 1,
                        through: 100,
                    }),
                },
            )]
            .into(),
        };
        let preview = Preview::create(
            &catalogue,
            [path].into(),
            Policy {
                identity: d("policy"),
                minimum_inactive_ticks: 10,
                places: [alias.into()].into(),
            },
            ClockPoint {
                era: d("clock"),
                tick: 100,
            },
        )
        .unwrap();
        let release = Release {
            claim: grant.claim_id().clone(),
            object: object.clone(),
        };
        let mut stores = std::collections::BTreeSet::from([store.identity().clone()]);
        let mut copies = vec![Copy {
            object: object.clone(),
            store: store.identity().clone(),
            state: CopyState::Verified {
                evidence: d("fixture"),
            },
        }];
        for survivor in survivors {
            stores.insert(survivor.clone());
            copies.push(Copy {
                object: object.clone(),
                store: survivor.clone(),
                state: CopyState::Verified {
                    evidence: d("admitted-survivor-fixture"),
                },
            });
        }
        let review = Review::create(
            preview,
            d("subject"),
            [(alias.into(), store.identity().clone())].into(),
            Protection {
                checkpoint: d("protection"),
                complete: true,
                obligations: vec![Obligation {
                    claim: release.claim.clone(),
                    object: object.clone(),
                    owner: d("subject"),
                    kind: Kind::Ordinary,
                }],
            },
            Inventory {
                checkpoint: d("inventory"),
                complete: true,
                stores,
                copies,
            },
            [release.clone()].into(),
        )
        .unwrap();
        let decision = Decision::authorize(
            &review,
            Approval {
                namespace: journal.identity().clone(),
                operation: d("retire"),
                owner: d("subject"),
                evidence: d("explicit-consent"),
            },
            Consent {
                release_owned_claims: [release].into(),
                remove_copies: [Target {
                    object: object.clone(),
                    store: store.identity().clone(),
                }]
                .into(),
                accept_intentional_loss: if !survivors.is_empty() {
                    Default::default()
                } else {
                    [object].into()
                },
            },
        )
        .unwrap();
        let candidate =
            intent::stage(&mut journal, &mut nodes, &enrollment(), &review, &decision).unwrap();
        let ready = intent::finish(&mut journal, &mut nodes, candidate).unwrap();
        let prepared = intent::prepare(&mut journal, &nodes, ready, key("prepare")).unwrap();
        intent::adopt(&mut journal, &nodes, &prepared, key("adopt")).unwrap();
        let ctx = intent::observe(&mut journal, &nodes, &enrollment(), &d("retire")).unwrap();
        let plan = plan(&store, &grant, &decision, &enrollment()).unwrap();
        Self {
            temp,
            store,
            ledger,
            job,
            grant,
            journal,
            nodes,
            ctx,
            plan,
        }
    }
    pub(crate) fn start(&mut self) -> u64 {
        progress::start(
            &mut self.journal,
            &mut self.nodes,
            &self.ctx,
            &self.grant.request().publication.object,
            progress::Action::Release {
                claim: self.grant.claim_id().clone(),
            },
        )
        .unwrap()
        .attempt
    }
    pub(crate) fn run(&mut self, attempt: u64) -> Result<ReleaseReceipt> {
        apply(
            &mut self.ledger,
            &self.store,
            &self.job,
            &self.plan,
            &mut self.journal,
            &self.nodes,
            &self.ctx,
            attempt,
        )
    }
    pub(crate) fn finish(&mut self, attempt: u64, evidence: Digest) {
        progress::finish(
            &mut self.journal,
            &mut self.nodes,
            &self.ctx,
            &self.grant.request().publication.object,
            attempt,
            progress::Action::Release {
                claim: self.grant.claim_id().clone(),
            },
            progress::Outcome::Released { evidence },
        )
        .unwrap();
    }
    pub(crate) fn reopen(&mut self) {
        self.store = LocalStore::open(&self.temp.path().join("store")).unwrap();
        self.ledger =
            CapacityLedger::open(&self.temp.path().join("ledger"), &self.ledger.checkpoint())
                .unwrap();
        self.journal = SourceJournal::open(
            &self.temp.path().join("journal"),
            &self.journal.checkpoint(),
        )
        .unwrap();
        self.nodes =
            MetadataStore::open(&self.temp.path().join("nodes"), &self.nodes.checkpoint()).unwrap();
    }
    fn bytes_present(&self) {
        let mut bytes = Vec::new();
        replica::verify(&self.store, &self.grant.request().publication)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(
            Object::from_bytes(&bytes),
            self.grant.request().publication.object
        );
    }
}
#[test]
fn portable_retirement_release_binds_admission_and_preserves_fresh_claim_on_replay() {
    use crate::provider::custody::release as port;
    fn execute(f: &mut Fixture, attempt: u64) -> std::result::Result<ReleaseReceipt, port::Error> {
        let request = port::native::retirement_request(&f.plan, attempt)?;
        let mut executor = port::native::retirement(port::native::Retirement {
            ledger: &mut f.ledger,
            store: &f.store,
            job: &f.job,
            plan: &f.plan,
            journal: &mut f.journal,
            nodes: &f.nodes,
            intent: &f.ctx,
            attempt,
        })?;
        Ok(port::release_exact(&mut executor, &request, true)?.evidence)
    }
    let mut f = Fixture::new();
    let n = f.start();
    let request = port::native::retirement_request(&f.plan, n).unwrap();
    let cp = f.ledger.checkpoint();
    {
        let mut executor = port::native::retirement(port::native::Retirement {
            ledger: &mut f.ledger,
            store: &f.store,
            job: &f.job,
            plan: &f.plan,
            journal: &mut f.journal,
            nodes: &f.nodes,
            intent: &f.ctx,
            attempt: n,
        })
        .unwrap();
        assert!(matches!(
            port::release_exact(&mut executor, &request, false),
            Err(port::Error::Unsupported)
        ));
        for field in 0..3 {
            let mut changed = request.clone();
            match &mut changed.authority {
                port::Authority::Retirement {
                    decision,
                    subject,
                    attempt,
                } => match field {
                    0 => *decision = d("changed-decision"),
                    1 => *subject = d("changed-subject"),
                    _ => *attempt += 1,
                },
                _ => unreachable!(),
            }
            assert!(matches!(
                port::release_exact(&mut executor, &changed, true),
                Err(port::Error::Invalid)
            ));
        }
    }
    assert_eq!(cp, f.ledger.checkpoint());
    let guard = super::super::verify(&mut f.ledger, &f.store, &f.grant).unwrap();
    assert!(matches!(execute(&mut f, n), Err(port::Error::Busy)));
    drop(guard);
    let released = execute(&mut f, n).unwrap();
    f.finish(n, released.evidence.clone());
    f.reopen();
    let mut fresh = f.grant.request().clone();
    fresh.operation = key("fresh-port-retention");
    let plan = super::super::plan(&f.store, &fresh).unwrap();
    let fresh = super::super::retain(&mut f.ledger, &f.store, &f.job, &plan)
        .unwrap()
        .grant;
    let cp = f.ledger.checkpoint();
    assert_eq!(execute(&mut f, n).unwrap().evidence, released.evidence);
    assert_eq!(cp, f.ledger.checkpoint());
    assert!(super::super::verify(&mut f.ledger, &f.store, &fresh).is_ok());
    assert!(matches!(
        super::super::verify(&mut f.ledger, &f.store, &f.grant),
        Err(RetentionError::Released)
    ));
    f.bytes_present();
}
#[test]
fn native_claim_release_requires_admission_and_live_readers_block_it() {
    let mut f = Fixture::new();
    assert!(matches!(f.run(1), Err(ReleaseError::NotAdmitted)));
    let n = f.start();
    assert!(matches!(f.run(n + 1), Err(ReleaseError::NotAdmitted)));
    let guard = super::super::verify(&mut f.ledger, &f.store, &f.grant).unwrap();
    assert!(matches!(
        f.run(n),
        Err(ReleaseError::Retention(RetentionError::Store(Error::Busy)))
    ));
    drop(guard);
    let result = f.run(n).unwrap();
    assert!(result.charged_footprint > 0);
    f.finish(n, result.evidence.clone());
    f.reopen();
    let checkpoint = f.ledger.checkpoint();
    assert_eq!(f.run(n).unwrap().evidence, result.evidence);
    assert_eq!(f.ledger.checkpoint(), checkpoint);
    assert!(matches!(
        super::super::verify(&mut f.ledger, &f.store, &f.grant),
        Err(RetentionError::Released)
    ));
    f.bytes_present();
    assert!(
        f.store
            .try_exclusive_object(&f.grant.request().publication.object)
            .is_ok()
    );
    let rp = super::super::plan(&f.store, f.grant.request()).unwrap();
    assert!(matches!(
        super::super::retain(&mut f.ledger, &f.store, &f.job, &rp),
        Err(RetentionError::Released)
    ));
    println!(
        "Native D claim released through admitted progress; reopened old-grant retry refuses; payload remains verified."
    );
}
#[test]
fn interrupted_release_recovers_marker_before_retiring_claim() {
    for cut in [Stage::Bound, Stage::Marked, Stage::Retired, Stage::Settled] {
        let mut f = Fixture::new();
        let n = f.start();
        assert!(matches!(
            apply_hook(
                &mut f.ledger,
                &f.store,
                &f.job,
                &f.plan,
                &mut f.journal,
                &f.nodes,
                &f.ctx,
                n,
                |stage| if stage == cut {
                    Err(ReleaseError::Injected)
                } else {
                    Ok(())
                }
            ),
            Err(ReleaseError::Injected)
        ));
        f.reopen();
        f.bytes_present();
        if cut == Stage::Marked || cut == Stage::Retired || cut == Stage::Settled {
            assert!(matches!(
                super::super::verify(&mut f.ledger, &f.store, &f.grant),
                Err(RetentionError::Released)
            ));
        }
        let r = f.run(n).unwrap();
        f.finish(n, r.evidence);
        assert!(f.store.handoff_claim_absent(&f.grant.record).unwrap());
        f.bytes_present();
    }
}
#[test]
fn cancellation_before_admission_refuses_but_admitted_release_can_reconcile() {
    for admitted in [false, true] {
        let mut f = Fixture::new();
        let n = if admitted { f.start() } else { 1 };
        intent::cancel(
            &mut f.journal,
            &key("owner"),
            &d("retire"),
            2,
            key("cancel"),
        )
        .unwrap();
        if admitted {
            let r = f.run(n).unwrap();
            f.finish(n, r.evidence);
        } else {
            assert!(f.run(n).is_err());
            assert!(super::super::verify(&mut f.ledger, &f.store, &f.grant).is_ok());
        }
        f.bytes_present();
    }
}
#[test]
fn foreign_mapping_other_claims_and_fresh_retention_are_preserved() {
    let mut f = Fixture::new();
    for enrollment in [
        Enrollment {
            owner: key("foreign"),
            subject: d("subject"),
        },
        Enrollment {
            owner: key("owner"),
            subject: d("foreign"),
        },
    ] {
        assert!(plan(&f.store, &f.grant, f.ctx.decision(), &enrollment).is_err());
    }
    let mut req = f.grant.request().clone();
    req.owner = key("other-owner");
    req.operation = key("other-hold");
    let rp = super::super::plan(&f.store, &req).unwrap();
    let t = f.ledger.begin_probe(key("other-probe")).unwrap();
    f.ledger
        .observe(key("other-sample"), t.reference(), 1024 * 1024 * 1024)
        .unwrap();
    let job = f
        .ledger
        .reserve(key("other-job"), req.owner.clone(), rp.required_peak())
        .unwrap()
        .reference()
        .clone();
    let other = super::super::retain(&mut f.ledger, &f.store, &job, &rp)
        .unwrap()
        .grant;
    assert!(plan(&f.store, &other, f.ctx.decision(), &enrollment()).is_err());
    let n = f.start();
    let r = f.run(n).unwrap();
    f.finish(n, r.evidence);
    assert!(super::super::verify(&mut f.ledger, &f.store, &other).is_ok());
    assert!(matches!(
        f.store.try_exclusive_object(&req.publication.object),
        Err(Error::Retained)
    ));
    req.owner = key("owner");
    req.operation = key("fresh-hold");
    let rp = super::super::plan(&f.store, &req).unwrap();
    let fresh = super::super::retain(&mut f.ledger, &f.store, &f.job, &rp)
        .unwrap()
        .grant;
    assert_ne!(fresh.claim_id(), f.grant.claim_id());
    assert!(super::super::verify(&mut f.ledger, &f.store, &fresh).is_ok());
    f.bytes_present();
}

#[test]
fn insufficient_budget_and_changed_or_corrupt_marker_refuse_without_payload_effect() {
    let mut f = Fixture::new();
    let n = f.start();
    let t = f.ledger.begin_probe(key("small-probe")).unwrap();
    f.ledger
        .observe(key("small-sample"), t.reference(), 1024 * 1024 * 1024)
        .unwrap();
    let small = f
        .ledger
        .reserve(key("small-job"), key("owner"), 1)
        .unwrap()
        .reference()
        .clone();
    assert!(
        apply(
            &mut f.ledger,
            &f.store,
            &small,
            &f.plan,
            &mut f.journal,
            &f.nodes,
            &f.ctx,
            n
        )
        .is_err()
    );
    assert!(super::super::verify(&mut f.ledger, &f.store, &f.grant).is_ok());
    let r = f.run(n).unwrap();
    f.finish(n, r.evidence);
    assert!(matches!(
        apply(
            &mut f.ledger,
            &f.store,
            &small,
            &f.plan,
            &mut f.journal,
            &f.nodes,
            &f.ctx,
            n
        ),
        Err(ReleaseError::AlreadyReleased)
    ));
    let marker = f
        .temp
        .path()
        .join("store/releases")
        .join(f.grant.request().publication.object.sha256.as_str())
        .join(format!(
            "{}.json",
            f.grant.request().pin_id().unwrap().as_str()
        ));
    // A previous reader cannot interpret final release as a successful handoff.
    assert!(crate::local::read_record::<super::super::handoff::HandoffRecord>(&marker).is_err());
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&marker).unwrap()).unwrap();
    value["schema"] = "unknown-release-format".into();
    std::fs::write(&marker, tig_core::identity::canonical_json(&value).unwrap()).unwrap();
    f.reopen();
    assert!(f.run(n).is_err());
    assert!(super::super::verify(&mut f.ledger, &f.store, &f.grant).is_err());
    f.bytes_present();
}

#[derive(Serialize, Deserialize)]
struct Controls {
    ledger: crate::ledger::Checkpoint,
    journal: crate::source_claims::Checkpoint,
    nodes: crate::metadata::Checkpoint,
    plan: ReleasePlan,
    job: JournalRef,
}
#[test]
fn progress_advanced_before_provider_fence_cannot_apply_an_old_attempt() {
    let mut f = Fixture::new();
    let n = f.start();
    let mut peer_journal =
        SourceJournal::open(&f.temp.path().join("journal"), &f.journal.checkpoint()).unwrap();
    let mut peer_nodes =
        MetadataStore::open(&f.temp.path().join("nodes"), &f.nodes.checkpoint()).unwrap();
    let ctx = f.ctx.clone();
    let object = f.grant.request().publication.object.clone();
    let action = progress::Action::Release {
        claim: f.grant.claim_id().clone(),
    };
    let result = apply_hook(
        &mut f.ledger,
        &f.store,
        &f.job,
        &f.plan,
        &mut f.journal,
        &f.nodes,
        &f.ctx,
        n,
        |stage| {
            if stage == Stage::Bound {
                progress::finish(
                    &mut peer_journal,
                    &mut peer_nodes,
                    &ctx,
                    &object,
                    n,
                    action.clone(),
                    progress::Outcome::NoEffect {
                        evidence: d("reconciled-before-provider-effect"),
                    },
                )
                .unwrap();
                progress::start(
                    &mut peer_journal,
                    &mut peer_nodes,
                    &ctx,
                    &object,
                    action.clone(),
                )
                .unwrap();
            }
            Ok(())
        },
    );
    assert!(matches!(result, Err(ReleaseError::NotAdmitted)));
    assert!(super::super::verify(&mut f.ledger, &f.store, &f.grant).is_ok());
    assert!(
        f.store
            .final_release_record(&f.grant.record)
            .unwrap()
            .is_none()
    );
    let r = f.run(n + 1).unwrap();
    f.finish(n + 1, r.evidence);
    f.bytes_present();
}
#[test]
fn release_child() {
    let Some(root) = std::env::var_os("TIG_FINAL_RELEASE_TEST_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let c: Controls =
        serde_json::from_slice(&std::fs::read(root.join("controls.json")).unwrap()).unwrap();
    let store = LocalStore::open(&root.join("store")).unwrap();
    let mut ledger = CapacityLedger::open(&root.join("ledger"), &c.ledger).unwrap();
    let mut journal = SourceJournal::open(&root.join("journal"), &c.journal).unwrap();
    let nodes = MetadataStore::open(&root.join("nodes"), &c.nodes).unwrap();
    let ctx = intent::observe(&mut journal, &nodes, &enrollment(), &d("retire")).unwrap();
    let cut = std::env::var("TIG_FINAL_RELEASE_TEST_CUT").unwrap();
    let _ = apply_hook(
        &mut ledger,
        &store,
        &c.job,
        &c.plan,
        &mut journal,
        &nodes,
        &ctx,
        1,
        |stage| {
            if (cut == "marked" && stage == Stage::Marked)
                || (cut == "retired" && stage == Stage::Retired)
            {
                std::process::exit(76);
            }
            Ok(())
        },
    );
    panic!("child failed to reach cut");
}
#[test]
fn child_exit_after_marker_or_claim_removal_resumes_exact_native_release() {
    for cut in ["marked", "retired"] {
        let mut f = Fixture::new();
        let n = f.start();
        std::fs::write(
            f.temp.path().join("controls.json"),
            serde_json::to_vec(&Controls {
                ledger: f.ledger.checkpoint(),
                journal: f.journal.checkpoint(),
                nodes: f.nodes.checkpoint(),
                plan: f.plan.clone(),
                job: f.job.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "retention::release::tests::release_child",
                "--nocapture",
            ])
            .env("TIG_FINAL_RELEASE_TEST_ROOT", f.temp.path())
            .env("TIG_FINAL_RELEASE_TEST_CUT", cut)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(76));
        f.reopen();
        assert!(matches!(
            super::super::verify(&mut f.ledger, &f.store, &f.grant),
            Err(RetentionError::Released)
        ));
        let r = f.run(n).unwrap();
        f.finish(n, r.evidence);
        assert!(f.store.handoff_claim_absent(&f.grant.record).unwrap());
        f.bytes_present();
    }
}
