use super::*;
use crate::source_claims::{Checkpoint, age};

pub(super) struct AgeFixture {
    registry: Namespace,
    pub(super) minimum: Checkpoint,
    limits: age::Limits,
}
impl AgeFixture {
    fn status(
        &mut self,
        f: &mut Fixture,
        e: &control::Enrollment,
        d: &control::Decision,
        l: &selection::Limits,
    ) -> control::Report {
        run(f, e, l, |x| {
            control::aged::status(
                &mut control::Inspection {
                    enrollment: x.enrollment,
                    roster: x.roster,
                    namespaces: x.namespaces,
                    metadata: Some(x.metadata.finished_reader().unwrap()),
                    places: x.places,
                    jobs: x.jobs,
                    witness: &mut x.witness,
                    admitted_context: x.admitted_context,
                    limits: x.limits,
                },
                d,
                &mut self.context(),
            )
        })
        .unwrap()
    }
    pub(super) fn new(f: &mut Fixture, e: &mut control::Enrollment) -> Self {
        let mut registry = Namespace::create(
            &f.temp.path().join("ages"),
            key("ages"),
            key("epoch"),
            policy(),
        )
        .unwrap();
        enroll(
            &mut registry,
            &f.metadata,
            "records",
            "owner",
            Kind::Retention,
        );
        let enrollment = age::Enrollment {
            authority: e.selection.authority.clone(),
            epoch: e.selection.epoch.clone(),
            metadata: e.selection.metadata.clone(),
            registry: registry.identity().clone(),
            registry_policy: Digest::blob(
                &git_asset_core::identity::canonical_json(
                    &serde_json::to_value(registry.policy()).unwrap(),
                )
                .unwrap(),
            ),
            source: key("records"),
            owner: key("owner"),
            clock: e.clock.clone(),
            observers: e
                .sources
                .iter()
                .map(|s| age::Observer {
                    namespace: s.namespace.clone(),
                    policy: s.policy.clone(),
                    owners: vec![s.owner.clone()],
                    classes: s.classes.iter().cloned().collect(),
                })
                .collect(),
        };
        e.age = Some(control::aged::Policy {
            enrollment,
            required_age_millis: 50,
            metadata_boundary: d("admitted-metadata-only"),
        });
        let mut result = Self {
            minimum: registry.checkpoint(),
            registry,
            limits: age::Limits::default(),
        };
        result.capture_api(f, e).initialize().unwrap();
        result
    }
    pub(super) fn context(&mut self) -> control::aged::Context<'_> {
        control::aged::Context {
            registry: &mut self.registry,
            minimum: &self.minimum,
            limits: &self.limits,
        }
    }
    fn capture_api<'a>(
        &'a mut self,
        f: &'a mut Fixture,
        e: &'a control::Enrollment,
    ) -> age::Capture<'a> {
        age::Capture {
            enrollment: &e.age.as_ref().unwrap().enrollment,
            registry: &mut self.registry,
            minimum: &self.minimum,
            metadata: &mut f.metadata,
            limits: &self.limits,
        }
    }
    pub(super) fn capture(
        &mut self,
        f: &mut Fixture,
        e: &control::Enrollment,
        d: &control::Decision,
    ) {
        let inventory = selection::inspect(
            &e.selection,
            &d.request().manifest,
            &mut f.metadata.finished_reader().unwrap(),
            &selection::Limits::default(),
        )
        .unwrap();
        for c in inventory.cohorts() {
            let at = f
                .namespaces
                .iter()
                .position(|n| n.identity() == &c.origin.namespace)
                .unwrap();
            let min = f.namespaces[at].checkpoint();
            age::Capture {
                enrollment: &e.age.as_ref().unwrap().enrollment,
                registry: &mut self.registry,
                minimum: &self.minimum,
                metadata: &mut f.metadata,
                limits: &self.limits,
            }
            .capture(
                &age::Request {
                    namespace: c.origin.namespace.clone(),
                    source: c.origin.source.clone(),
                    predecessor_revision: c.origin.predecessor_revision,
                },
                &mut f.namespaces[at],
                &min,
                &f.clock,
            )
            .unwrap();
        }
    }
    pub(super) fn plan(
        &mut self,
        f: &mut Fixture,
        e: &control::Enrollment,
        d: &control::Decision,
        l: &selection::Limits,
    ) -> control::Result<control::Decision> {
        let result = control::aged::plan(
            e,
            d.request().clone(),
            &f.roster,
            &mut f.namespaces,
            &mut f.metadata,
            &[&f.store],
            l,
            &mut self.context(),
            &f.clock,
        );
        f.metadata.finish().unwrap();
        result
    }
}
#[test]
fn aged_controller_finishes_twenty_pages_and_keeps_exact_survivor_age() {
    let mut f = Fixture::new();
    let (mut e, manual, l) = compile(&mut f, 15);
    let mut ages = AgeFixture::new(&mut f, &mut e);
    f.clock.0.set(10);
    ages.capture(&mut f, &e, &manual);
    f.clock.0.set(100);
    let p = ages.plan(&mut f, &e, &manual, &l).unwrap();
    assert_ne!(manual.id().unwrap(), p.id().unwrap());
    assert!(
        run(&mut f, &e, &l, |x| x.apply(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap()
        ))
        .is_err()
    );
    let mut acknowledged = Vec::new();
    loop {
        let r = run(&mut f, &e, &l, |x| {
            x.run_limits.pages = 3;
            x.apply_aged(
                &p,
                &p.authorize_claim_release().unwrap(),
                &p.authorize_final_loss().unwrap(),
                ages.context(),
                |s| {
                    if let control::Stage::AgeAcknowledged(i) = s {
                        acknowledged.push(i);
                    }
                    if let control::Stage::PageCompleted(i) = s {
                        assert_eq!(acknowledged.last(), Some(&i));
                    }
                    Ok(())
                },
            )
        })
        .unwrap();
        if r.phase == control::Phase::Completed {
            break;
        }
    }
    assert_eq!(acknowledged, (0..20).collect::<Vec<_>>());
    for name in ["a", "b", "c"] {
        assert!(f.store.read_object(&f.publications[name].object).is_err());
    }
    for name in ["shared", "keep"] {
        assert!(f.store.read_object(&f.publications[name].object).is_ok());
    }
}
#[test]
fn unknown_recent_and_wrong_registry_refuse_before_source_change() {
    let mut f = Fixture::new();
    let (mut e, manual, l) = compile(&mut f, 0);
    let mut ages = AgeFixture::new(&mut f, &mut e);
    let before = f.namespaces[0].checkpoint();
    assert!(ages.plan(&mut f, &e, &manual, &l).is_err());
    ages.capture(&mut f, &e, &manual);
    assert!(ages.plan(&mut f, &e, &manual, &l).is_err());
    f.clock.0.set(160);
    let p = ages.plan(&mut f, &e, &manual, &l).unwrap();
    let mut wrong = Namespace::create(
        &f.temp.path().join("wrong-age"),
        key("wrong-age"),
        key("epoch"),
        policy(),
    )
    .unwrap();
    let min = wrong.checkpoint();
    assert!(
        run(&mut f, &e, &l, |x| x.apply_aged(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
            control::aged::Context {
                registry: &mut wrong,
                minimum: &min,
                limits: &age::Limits::default(),
            },
            |_| Ok(())
        ))
        .is_err()
    );
    assert_eq!(f.namespaces[0].checkpoint(), before);
}
#[test]
fn retired_page_without_age_ack_resumes_and_ack_before_completion_is_replayable() {
    for stop in [
        control::Stage::Source(0, subset::Stage::Retired),
        control::Stage::AgeAcknowledged(0),
    ] {
        let mut f = Fixture::new();
        let (mut e, manual, l) = compile(&mut f, 0);
        let mut ages = AgeFixture::new(&mut f, &mut e);
        f.clock.0.set(10);
        ages.capture(&mut f, &e, &manual);
        f.clock.0.set(100);
        let p = ages.plan(&mut f, &e, &manual, &l).unwrap();
        assert!(
            run(&mut f, &e, &l, |x| x.apply_aged(
                &p,
                &p.authorize_claim_release().unwrap(),
                &p.authorize_final_loss().unwrap(),
                ages.context(),
                |s| {
                    if s == stop {
                        Err(control::Error::Injected)
                    } else {
                        Ok(())
                    }
                }
            ))
            .is_err()
        );
        let status = run(&mut f, &e, &l, |x| x.status(&p)).unwrap();
        assert_eq!(status.completed_pages, Some(0));
        assert_eq!(status.historical_retired_pages, Some(1));
        assert!(status.age_inheritance_unknown);
        let registry_before = ages.registry.checkpoint();
        assert_eq!(
            ages.status(&mut f, &e, &p, &l).age_inheritance_unknown,
            stop != control::Stage::AgeAcknowledged(0)
        );
        assert_eq!(registry_before, ages.registry.checkpoint());
        let r = run(&mut f, &e, &l, |x| {
            x.apply_aged(
                &p,
                &p.authorize_claim_release().unwrap(),
                &p.authorize_final_loss().unwrap(),
                ages.context(),
                |_| Ok(()),
            )
        })
        .unwrap();
        assert_eq!(r.phase, control::Phase::Completed);
    }
}

#[test]
fn cancellation_needs_no_registry_or_time_and_readonly_debt_inspection_does_not_create_facts() {
    for acknowledged in [false, true] {
        let mut f = Fixture::new();
        let (mut e, manual, l) = compile(&mut f, 0);
        let mut ages = AgeFixture::new(&mut f, &mut e);
        f.clock.0.set(10);
        ages.capture(&mut f, &e, &manual);
        f.clock.0.set(100);
        let p = ages.plan(&mut f, &e, &manual, &l).unwrap();
        let stop = if acknowledged {
            control::Stage::AgeAcknowledged(0)
        } else {
            control::Stage::Source(0, subset::Stage::Retired)
        };
        assert!(
            run(&mut f, &e, &l, |x| x.apply_aged(
                &p,
                &p.authorize_claim_release().unwrap(),
                &p.authorize_final_loss().unwrap(),
                ages.context(),
                |s| if s == stop {
                    Err(control::Error::Injected)
                } else {
                    Ok(())
                }
            ))
            .is_err()
        );
        f.clock.0.set(600);
        let stopped = run(&mut f, &e, &l, |x| {
            x.cancel(
                &p,
                &p.authorize_claim_release().unwrap(),
                &p.authorize_final_loss().unwrap(),
            )
        })
        .unwrap();
        assert_eq!(stopped.phase, control::Phase::Cancelled);
        assert!(stopped.age_inheritance_unknown);
        let registry_before = ages.registry.checkpoint();
        let clarified = ages.status(&mut f, &e, &p, &l);
        assert_eq!(clarified.phase, control::Phase::Cancelled);
        assert_eq!(clarified.age_inheritance_unknown, !acknowledged);
        assert_eq!(registry_before, ages.registry.checkpoint());
        for name in ["a", "b", "c"] {
            assert!(f.store.read_object(&f.publications[name].object).is_ok());
        }
    }
}

#[test]
fn actual_age_child_exits_recover_exact_source_registry_page_and_unlink_boundaries() {
    for stop in [
        control::Stage::Source(0, subset::Stage::Retired),
        control::Stage::AgeAcknowledged(0),
        control::Stage::PageCompleted(0),
        control::Stage::Provider(0, provider::Stage::Unlinked),
    ] {
        let mut f = Fixture::new();
        let (mut e, manual, l) = compile(&mut f, 0);
        let mut ages = AgeFixture::new(&mut f, &mut e);
        f.clock.0.set(10);
        ages.capture(&mut f, &e, &manual);
        f.clock.0.set(100);
        let p = ages.plan(&mut f, &e, &manual, &l).unwrap();
        let saved = PagedSaved {
            age: Some(ages.registry.checkpoint()),
            enrollment: e.clone(),
            decision: p.clone(),
            roster: f.roster.clone(),
            namespaces: f.namespaces.iter().map(|n| n.checkpoint()).collect(),
            metadata: f.metadata.checkpoint(),
            ledger: f.ledger.checkpoint(),
            witness: f.witness_ledger.checkpoint(),
            job: f.job.clone(),
            witness_job: f.witness_job.clone(),
        };
        std::fs::write(
            f.temp.path().join("paged-saved.json"),
            serde_json::to_vec(&saved).unwrap(),
        )
        .unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "final_retirement::bulk::tests::paged::paged_child",
                "--nocapture",
            ])
            .env("GIT_ASSET_PAGED_CHILD", f.temp.path())
            .env("GIT_ASSET_PAGED_STOP", format!("{stop:?}"))
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(76),
            "{stop:?}: {}",
            String::from_utf8_lossy(&child.stderr)
        );
        f.namespaces = saved
            .namespaces
            .iter()
            .enumerate()
            .map(|(i, c)| Namespace::open(&f.temp.path().join(format!("ns-{i}")), c).unwrap())
            .collect();
        f.metadata = MetadataStore::open(&f.temp.path().join("metadata"), &saved.metadata).unwrap();
        f.ledger = CapacityLedger::open(&f.temp.path().join("ledger"), &saved.ledger).unwrap();
        f.witness_ledger =
            CapacityLedger::open(&f.temp.path().join("witness-ledger"), &saved.witness).unwrap();
        ages.registry =
            Namespace::open(&f.temp.path().join("ages"), saved.age.as_ref().unwrap()).unwrap();
        let r = run(&mut f, &e, &l, |x| {
            x.apply_aged(
                &p,
                &p.authorize_claim_release().unwrap(),
                &p.authorize_final_loss().unwrap(),
                ages.context(),
                |_| Ok(()),
            )
        })
        .unwrap();
        assert_eq!(r.phase, control::Phase::Completed);
        assert_eq!(r.historical_deleted_actions, Some(3));
    }
}

#[test]
fn completed_page_cannot_recreate_inheritance_lost_to_a_registry_restore() {
    let mut f = Fixture::new();
    let (mut e, manual, l) = compile(&mut f, 0);
    let mut ages = AgeFixture::new(&mut f, &mut e);
    f.clock.0.set(10);
    ages.capture(&mut f, &e, &manual);
    f.clock.0.set(100);
    let p = ages.plan(&mut f, &e, &manual, &l).unwrap();
    let source = f.temp.path().join("ages/claims.sqlite");
    let backup = f.temp.path().join("before-age-page.sqlite");
    std::fs::copy(&source, &backup).unwrap();
    let first = run(&mut f, &e, &l, |x| {
        x.run_limits.pages = 1;
        x.apply_aged(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
            ages.context(),
            |_| Ok(()),
        )
    })
    .unwrap();
    assert_eq!(first.completed_pages, Some(1));
    let AgeFixture {
        registry,
        minimum,
        limits,
    } = ages;
    drop(registry);
    std::fs::copy(&backup, &source).unwrap();
    ages = AgeFixture {
        registry: Namespace::open(&f.temp.path().join("ages"), &minimum).unwrap(),
        minimum,
        limits,
    };
    let before = f.namespaces[0].checkpoint();
    let registry_before = ages.registry.checkpoint();
    assert!(ages.status(&mut f, &e, &p, &l).age_inheritance_unknown);
    assert!(
        run(&mut f, &e, &l, |x| x.apply_aged(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
            ages.context(),
            |_| Ok(())
        ))
        .is_err()
    );
    assert_eq!(before, f.namespaces[0].checkpoint());
    assert_eq!(registry_before, ages.registry.checkpoint());
    for name in ["a", "b", "c"] {
        assert!(f.store.read_object(&f.publications[name].object).is_ok());
    }
}

struct IntervalClock(Digest);
impl provider::Clock for IntervalClock {
    fn now(&self, _: &Digest) -> provider::Result<u64> {
        panic!("use complete bounds");
    }
    fn bounds(&self, id: &Digest) -> provider::Result<provider::TimeBounds> {
        if id != &self.0 {
            return Err(provider::Error::Clock);
        }
        Ok(provider::TimeBounds {
            earliest: 59,
            latest: 100,
        })
    }
}
#[test]
fn age_uses_earliest_current_bound_and_rechecks_on_execution() {
    let mut f = Fixture::new();
    let (mut e, manual, l) = compile(&mut f, 0);
    let mut ages = AgeFixture::new(&mut f, &mut e);
    f.clock.0.set(10);
    ages.capture(&mut f, &e, &manual);
    f.clock.0.set(100);
    assert!(
        control::aged::plan(
            &e,
            manual.request().clone(),
            &f.roster,
            &mut f.namespaces,
            &mut f.metadata,
            &[&f.store],
            &l,
            &mut ages.context(),
            &IntervalClock(e.clock.clone())
        )
        .is_err()
    );
    f.metadata.finish().unwrap();
    let p = ages.plan(&mut f, &e, &manual, &l).unwrap();
    f.clock.0.set(59);
    let before = f.namespaces[0].checkpoint();
    assert!(
        run(&mut f, &e, &l, |x| x.apply_aged(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
            ages.context(),
            |_| Ok(())
        ))
        .is_err()
    );
    assert_eq!(before, f.namespaces[0].checkpoint());
}

#[test]
fn age_deadline_is_rechecked_at_source_retirement_and_final_provider_clearance() {
    for stop in [
        control::Stage::Source(0, subset::Stage::BeforeRetire),
        control::Stage::PageCompleted(4),
    ] {
        let mut f = Fixture::new();
        let (mut e, manual, l) = compile(&mut f, 0);
        let mut ages = AgeFixture::new(&mut f, &mut e);
        f.clock.0.set(10);
        ages.capture(&mut f, &e, &manual);
        f.clock.0.set(100);
        let p = ages.plan(&mut f, &e, &manual, &l).unwrap();
        let steering = Clock(Cell::new(100), e.clock.clone());
        let jobs = [control::StoreJob {
            store: f.store.identity().clone(),
            job: f.job.clone(),
        }];
        let mut places = [Local {
            store: &f.store,
            ledger: &mut f.ledger,
            job: &f.job,
        }];
        let result = control::Execution {
            enrollment: &e,
            roster: &f.roster,
            namespaces: &mut f.namespaces,
            metadata: &mut f.metadata,
            places: &mut places,
            jobs: &jobs,
            witness: Local {
                store: &f.witness,
                ledger: &mut f.witness_ledger,
                job: &f.witness_job,
            },
            clock: &steering,
            admitted_context: &e.context,
            limits: &l,
            run_limits: control::RunLimits::default(),
        }
        .apply_aged(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
            ages.context(),
            |s| {
                if s == stop {
                    steering.0.set(59);
                }
                Ok(())
            },
        );
        assert_eq!(steering.0.get(), 59, "boundary must be reached");
        if stop == control::Stage::PageCompleted(4) {
            let report = result.unwrap();
            assert_eq!(report.phase, control::Phase::Intent);
            assert_eq!(report.completed_pages, Some(5));
            assert_eq!(report.historical_deleted_actions, Some(0));
            assert!(!report.holds.is_empty());
        } else {
            assert!(result.is_err());
        }
        for name in ["a", "b", "c"] {
            assert!(f.store.read_object(&f.publications[name].object).is_ok());
        }
    }
}
