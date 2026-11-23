use super::*;
mod placement_cases;
use crate::{
    accounted,
    local_capacity::LocalCapacityProbe,
    retention,
    source_claims::{
        Command, Kind, Places, Selection,
        admission::{Policy as SourcePolicy, Select},
    },
    store_protection::{Binding, Member},
};
use git_asset_core::{
    LogicalPath,
    tree::{MemoryNodes, Tree, TreeLimits},
};
use std::{
    cell::Cell,
    fs,
    io::{Cursor, Read},
    path::PathBuf,
    process::Command as Process,
};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn d(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn limits() -> crate::protection::Limits {
    crate::protection::Limits {
        millis: 60000,
        roots: 128,
        bindings: 10000,
        associations: 100000,
        reads: 100000,
        read_bytes: 32 * 1024 * 1024,
    }
}
fn source_policy() -> SourcePolicy {
    let p = Places {
        current: vec![key("F")],
        pending: vec![key("F")],
        predecessor: vec![key("D")],
    };
    let m = Places {
        current: vec![key("M")],
        pending: vec![key("M")],
        predecessor: vec![key("M")],
    };
    SourcePolicy {
        prefer_active: true,
        classes: [
            (Kind::Branch, p.clone()),
            (Kind::Draft, p.clone()),
            (Kind::Job, p),
            (Kind::Retention, m),
        ]
        .into_iter()
        .collect(),
    }
}
struct TestClock {
    value: Cell<u64>,
    last: Cell<u64>,
}
struct BoundsClock(Cell<TimeBounds>);
impl Clock for BoundsClock {
    fn now(&self, _: &Digest) -> Result<u64> {
        panic!("provider bypassed clock bounds")
    }
    fn bounds(&self, _: &Digest) -> Result<TimeBounds> {
        Ok(self.0.get())
    }
}
#[test]
fn interval_expiry_holds_release_or_unlink_and_preserves_honest_grant_state() {
    for stop in [Stage::Intent, Stage::BeforeUnlink] {
        let mut f = Fixture::new();
        let p = f.plan();
        let clock = BoundsClock(Cell::new(TimeBounds {
            earliest: 100,
            latest: 101,
        }));
        let mut e = Execution {
            store: &f.store,
            ledger: &mut f.ledger,
            job: &f.job,
            roster: &f.roster,
            namespaces: &mut f.namespaces,
            nodes: &f.nodes,
            clock: &clock,
            admitted_context: &f.context,
        };
        let result = e.apply_hook(&p, &p.authorize().unwrap(), |stage| {
            if stage == stop {
                clock.0.set(TimeBounds {
                    earliest: 100,
                    latest: 500,
                });
            }
            Ok(())
        });
        assert!(matches!(result, Err(Error::Clock)));
        assert_eq!(f.bytes(), b"asset");
        let state = f.store.load_final_state(&p.birth).unwrap().unwrap();
        assert_eq!(state.release.is_some(), stop == Stage::BeforeUnlink);
        let active = f.store.active_final_grants(&p.birth.object).unwrap();
        if stop == Stage::Intent {
            assert_eq!(encode(&active).unwrap(), encode(&p.grants()).unwrap());
        } else {
            assert!(active.is_empty());
        }
    }
}
#[test]
fn actual_enrolled_local_clock_drives_final_retirement_without_request_supplied_now() {
    let temp = tempfile::tempdir().unwrap();
    let ledger = CapacityLedger::create_clock(
        &temp.path().join("clock"),
        key("clock"),
        key("epoch"),
        0,
        crate::clock::Policy {
            authority: d("time-authority"),
            epoch: d("time-epoch"),
            boundary: d("admitted-os-time"),
            max_sample_millis: 5000,
            max_drift_millis: 1000,
            max_session_millis: 60000,
        },
    )
    .unwrap();
    let enrollment = ledger.clock_enrollment().unwrap();
    let id = enrollment.id().unwrap();
    let clock = crate::clock::LocalClock::new(ledger, enrollment).unwrap();
    let mut f = Fixture::with_clock(id.clone(), 60000);
    let bounds = clock.sample(&id).unwrap();
    let mut request = f.plan().request().clone();
    request.window = Window {
        not_before: bounds.earliest.saturating_sub(1000),
        expires: bounds.latest + 30000,
    };
    let p = plan(&f.store, request).unwrap();
    let receipt = Execution {
        store: &f.store,
        ledger: &mut f.ledger,
        job: &f.job,
        roster: &f.roster,
        namespaces: &mut f.namespaces,
        nodes: &f.nodes,
        clock: &clock,
        admitted_context: &f.context,
    }
    .apply(&p, &p.authorize().unwrap())
    .unwrap();
    assert_eq!(receipt.outcome, Some(Outcome::Deleted));
    assert!(clock.status().unwrap().checkpoint.sequence() >= 4);
    assert!(f.store.read_object(&f.publication.object).is_err());
}

#[test]
fn reacquisition_birth_requires_the_whole_clock_interval_but_completion_can_resume() {
    let mut f = Fixture::new();
    let old = f.plan();
    f.apply(&old);
    let rp = reacquire::plan(
        &f.store,
        &old,
        reacquire::Request {
            publication: Publication {
                operation: key("bounds-reacquire"),
                ..f.publication.clone()
            },
            capacity: f.ledger.identity().clone(),
            context: f.context.clone(),
            window: Window {
                not_before: 50,
                expires: 500,
            },
        },
    )
    .unwrap();
    let clock = BoundsClock(Cell::new(TimeBounds {
        earliest: 100,
        latest: 500,
    }));
    let run = |f: &mut Fixture| {
        reacquire::Execution {
            store: &f.store,
            ledger: &mut f.ledger,
            job: &f.job,
            clock: &clock,
            admitted_context: &f.context,
        }
        .apply(&rp, &rp.authorize().unwrap(), &mut Cursor::new(b"asset"))
    };
    assert!(matches!(run(&mut f), Err(Error::Clock)));
    assert_eq!(
        digest(
            &f.store
                .life_head(&f.publication.object)
                .unwrap()
                .unwrap()
                .birth
        )
        .unwrap(),
        digest(&old.birth).unwrap()
    );
    clock.0.set(TimeBounds {
        earliest: 100,
        latest: 499,
    });
    run(&mut f).unwrap();
    assert_eq!(f.bytes(), b"asset");
    clock.0.set(TimeBounds {
        earliest: 500,
        latest: 501,
    });
    run(&mut f).unwrap();
    assert_eq!(f.bytes(), b"asset");
}
impl Clock for TestClock {
    fn now(&self, p: &Digest) -> Result<u64> {
        let n = self.value.get();
        if p != &d("clock") || n < self.last.get() {
            return Err(Error::Clock);
        }
        self.last.set(n);
        Ok(n)
    }
}
struct Fixture {
    root: tempfile::TempDir,
    store: LocalStore,
    ledger: CapacityLedger,
    job: JournalRef,
    publication: Publication,
    grant: RetentionGrant,
    roster: claims::Roster,
    namespaces: Vec<Namespace>,
    nodes: MemoryNodes,
    clock: TestClock,
    context: Digest,
}
impl Fixture {
    fn new() -> Self {
        Self::with_clock(d("clock"), 1000)
    }
    fn with_clock(clock: Digest, max_window: u64) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut namespaces = Vec::new();
        let nodes = MemoryNodes::default();
        for i in 0..2 {
            let mut n = Namespace::create(
                &root.path().join(format!("ns-{i}")),
                key("project"),
                key("epoch"),
                source_policy(),
            )
            .unwrap();
            n.apply(
                key("enroll"),
                Command::Enroll {
                    source: key("main"),
                    owner: key("owner"),
                    class: Kind::Branch,
                },
                &nodes,
                &limits(),
            )
            .unwrap();
            namespaces.push(n);
        }
        let roster = claims::Roster::flat(
            d("authority"),
            d("epoch"),
            Some(d("complete-flat-boundary")),
            namespaces
                .iter()
                .map(|n| claims::NamespacePin::current(n).unwrap())
                .collect(),
            limits(),
        )
        .unwrap();
        let binding = Binding::new(
            1,
            d("provider-boundary"),
            vec![Member {
                namespace: namespaces[0].identity().clone(),
                places: vec![key("M")],
            }],
        )
        .unwrap();
        let policy = Policy::new(
            d("authority"),
            d("epoch"),
            roster.scope().unwrap(),
            clock,
            vec![key("owner")],
            max_window,
        )
        .unwrap();
        let store = LocalStore::create_final(&root.path().join("store"), key("M"), binding, policy)
            .unwrap();
        let mut ledger = CapacityLedger::create(
            &root.path().join("ledger"),
            key("capacity"),
            key("epoch"),
            0,
        )
        .unwrap();
        LocalCapacityProbe::bind(&store, &mut ledger)
            .unwrap()
            .observe(&store, &mut ledger, key("probe"), key("sample"))
            .unwrap();
        let job = ledger
            .reserve(key("job"), key("owner"), 256 * 1024 * 1024)
            .unwrap()
            .reference()
            .clone();
        let publication = Publication {
            operation: key("ingest"),
            owner: key("owner"),
            object: Object::from_bytes(b"asset"),
            required: AckLevel::ProcessRestart,
        };
        let pp = accounted::plan(&store, &publication).unwrap();
        accounted::publish_accounted(&mut ledger, &store, &job, &pp, &mut Cursor::new(b"asset"))
            .unwrap();
        let rp = retention::plan(
            &store,
            &retention::RetentionRequest {
                operation: key("pin"),
                owner: key("owner"),
                publication: publication.clone(),
                minimum_ack: AckLevel::ProcessRestart,
            },
        )
        .unwrap();
        let grant = retention::retain(&mut ledger, &store, &job, &rp)
            .unwrap()
            .grant;
        Self {
            root,
            store,
            ledger,
            job,
            publication,
            grant,
            roster,
            namespaces,
            nodes,
            clock: TestClock {
                value: Cell::new(100),
                last: Cell::new(0),
            },
            context: d("private-context"),
        }
    }
    fn plan(&self) -> Plan {
        plan(
            &self.store,
            Request {
                operation: key("delete"),
                owner: key("owner"),
                publication: self.publication.clone(),
                capacity: self.ledger.identity().clone(),
                context: self.context.clone(),
                window: Window {
                    not_before: 50,
                    expires: 500,
                },
            },
        )
        .unwrap()
    }
    fn execution(&mut self) -> Execution<'_, MemoryNodes> {
        Execution {
            store: &self.store,
            ledger: &mut self.ledger,
            job: &self.job,
            roster: &self.roster,
            namespaces: &mut self.namespaces,
            nodes: &self.nodes,
            clock: &self.clock,
            admitted_context: &self.context,
        }
    }
    fn apply(&mut self, p: &Plan) -> Receipt {
        self.execution().apply(p, &p.authorize().unwrap()).unwrap()
    }
    fn bytes(&self) -> Vec<u8> {
        let mut r = self.store.read_object(&self.publication.object).unwrap();
        let mut b = Vec::new();
        r.read_to_end(&mut b).unwrap();
        b
    }
    fn selected(&mut self, index: usize) {
        let (tree, _) = Tree::build(
            &[(
                LogicalPath::parse("item").unwrap(),
                self.publication.object.clone(),
            )]
            .into_iter()
            .collect(),
            &mut self.nodes,
            TreeLimits::default(),
        )
        .unwrap();
        self.namespaces[index]
            .select(
                &Select {
                    prepare: key("prepare"),
                    adopt: key("adopt"),
                    source: key("main"),
                    owner: key("owner"),
                    expected_revision: 1,
                    selection: Selection {
                        root: tree.root().clone(),
                        context: d("selection"),
                    },
                },
                &self.nodes,
                &limits(),
            )
            .unwrap();
    }
}

#[test]
fn exact_authorized_last_copy_deletion_releases_only_its_grants_and_replays() {
    let mut f = Fixture::new();
    assert_eq!(
        crate::orchestration::Place::local(&f.store, 1)
            .unwrap()
            .contract,
        Digest::blob(b"tig.local-store.design-8/process-restart/final-incarnation/v1")
    );
    let p = f.plan();
    assert_eq!(p.grants().len(), 1);
    let receipt = f.apply(&p);
    assert_eq!(receipt.outcome, Some(Outcome::Deleted));
    assert!(receipt.grants_released);
    assert_eq!(receipt.logical_bytes_removed, 5);
    assert_eq!(receipt.observed, Observation::Absent);
    assert!(matches!(
        f.store.read_object(&f.publication.object),
        Err(crate::Error::AuthorizedAbsence)
    ));
    assert!(matches!(
        retention::verify(&mut f.ledger, &f.store, &f.grant),
        Err(retention::RetentionError::Released)
    ));
    let checkpoint = f.ledger.checkpoint();
    assert!(f.apply(&p).historical);
    assert_eq!(f.ledger.checkpoint(), checkpoint);
    let pp = accounted::plan(&f.store, &f.publication).unwrap();
    assert!(
        accounted::publish_accounted(
            &mut f.ledger,
            &f.store,
            &f.job,
            &pp,
            &mut Cursor::new(b"asset")
        )
        .is_err()
    );
    assert!(!f.store.object_present(&f.publication.object).unwrap());
}

#[test]
fn status_is_read_only_and_reports_release_without_implying_removal_authority() {
    for (stop, expected) in [
        (None, status::Phase::NotStarted),
        (Some(Stage::Admitted), status::Phase::Admitted),
        (Some(Stage::Intent), status::Phase::Intent),
        (Some(Stage::Released), status::Phase::Released),
        (Some(Stage::Unlinked), status::Phase::Released),
        (Some(Stage::Terminal), status::Phase::Deleted),
        (Some(Stage::Settled), status::Phase::Deleted),
    ] {
        let mut f = Fixture::new();
        let p = f.plan();
        if let Some(stop) = stop {
            assert!(matches!(
                f.execution().apply_hook(&p, &p.authorize().unwrap(), |s| {
                    if s == stop {
                        Err(Error::Injected)
                    } else {
                        Ok(())
                    }
                }),
                Err(Error::Injected)
            ));
        }
        let before = f.ledger.checkpoint();
        let s = status::inspect(&f.store, &mut f.ledger, &f.job, &p).unwrap();
        assert_eq!(s.phase, expected);
        assert!(!s.removal_authority);
        assert_eq!(f.ledger.checkpoint(), before);
        assert_eq!(
            s.observed,
            if matches!(
                stop,
                Some(Stage::Unlinked | Stage::Terminal | Stage::Settled)
            ) {
                Observation::Absent
            } else {
                Observation::PresentUnverified
            }
        );
    }
    let f = Fixture::new();
    let mut value = serde_json::to_value(f.plan()).unwrap();
    value["birth"]["index"] = serde_json::json!(u64::MAX);
    assert!(Plan::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
    let mut p = f.plan();
    p.birth.index = usize::MAX;
    assert!(p.id().is_err());
}

#[test]
fn cancel_before_or_after_release_preserves_bytes_and_never_rearms_old_operation() {
    for stop in [
        None,
        Some(Stage::Intent),
        Some(Stage::Released),
        Some(Stage::GrantsRemoved),
    ] {
        let mut f = Fixture::new();
        let p = f.plan();
        if let Some(stop) = stop {
            assert!(
                f.execution()
                    .apply_hook(&p, &p.authorize().unwrap(), |stage| if stage == stop {
                        Err(Error::Injected)
                    } else {
                        Ok(())
                    })
                    .is_err()
            );
        }
        f.clock.value.set(700);
        let cancelled = f.execution().cancel(&p, &p.authorize().unwrap()).unwrap();
        assert_eq!(cancelled.outcome, Some(Outcome::Cancelled));
        assert_eq!(f.bytes(), b"asset");
        assert_eq!(
            cancelled.grants_released,
            matches!(stop, Some(Stage::Released | Stage::GrantsRemoved))
        );
        let old_valid = retention::verify(&mut f.ledger, &f.store, &f.grant).is_ok();
        assert_eq!(old_valid, !cancelled.grants_released);
        f.clock.value.set(800);
        let checkpoint = f.ledger.checkpoint();
        let repeated = f.apply(&p);
        assert!(repeated.historical);
        assert_eq!(repeated.outcome, Some(Outcome::Cancelled));
        assert_eq!(f.ledger.checkpoint(), checkpoint);
        assert_eq!(f.bytes(), b"asset");
    }
}

#[test]
fn every_durable_boundary_resumes_exact_intent_and_absence_recovery_needs_no_fresh_clock() {
    for stop in [
        Stage::Admitted,
        Stage::Bound,
        Stage::Intent,
        Stage::Released,
        Stage::GrantsRemoved,
        Stage::BeforeUnlink,
        Stage::Unlinked,
        Stage::Terminal,
        Stage::Settled,
    ] {
        let mut f = Fixture::new();
        let p = f.plan();
        assert!(
            f.execution()
                .apply_hook(&p, &p.authorize().unwrap(), |s| if s == stop {
                    Err(Error::Injected)
                } else {
                    Ok(())
                })
                .is_err()
        );
        if matches!(stop, Stage::Unlinked | Stage::Terminal | Stage::Settled) {
            f.clock.value.set(900);
            f.selected(1);
        }
        let receipt = f.apply(&p);
        assert_eq!(receipt.outcome, Some(Outcome::Deleted));
        assert_eq!(receipt.observed, Observation::Absent);
    }
}

#[test]
fn source_elsewhere_readers_expiry_context_and_changed_grants_hold() {
    let mut f = Fixture::new();
    let p = f.plan();
    f.selected(1);
    assert!(matches!(
        f.execution().apply(&p, &p.authorize().unwrap()),
        Err(Error::Claims(claims::Error::Protected))
    ));
    assert_eq!(f.bytes(), b"asset");
    let mut f = Fixture::new();
    let p = f.plan();
    let reader = f.store.read_object(&f.publication.object).unwrap();
    assert!(f.execution().apply(&p, &p.authorize().unwrap()).is_err());
    drop(reader);
    assert_eq!(f.bytes(), b"asset");
    let mut f = Fixture::new();
    let p = f.plan();
    f.clock.value.set(501);
    assert!(matches!(
        f.execution().apply(&p, &p.authorize().unwrap()),
        Err(Error::Clock)
    ));
    assert_eq!(f.bytes(), b"asset");
    let mut f = Fixture::new();
    let p = f.plan();
    f.context = d("changed-context");
    assert!(matches!(
        f.execution().apply(&p, &p.authorize().unwrap()),
        Err(Error::WrongAuthority)
    ));
    assert_eq!(f.bytes(), b"asset");
    let mut f = Fixture::new();
    let p = f.plan();
    let r = retention::plan(
        &f.store,
        &retention::RetentionRequest {
            operation: key("additional"),
            owner: key("owner"),
            publication: f.publication.clone(),
            minimum_ack: AckLevel::ProcessRestart,
        },
    )
    .unwrap();
    retention::retain(&mut f.ledger, &f.store, &f.job, &r).unwrap();
    assert!(matches!(
        f.execution().apply(&p, &p.authorize().unwrap()),
        Err(Error::ProtectedGrant)
    ));
    assert_eq!(f.bytes(), b"asset");
}

#[test]
fn independent_vectors_pin_manifest_and_all_terminal_record_shapes() {
    let v: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/vectors/final-provider.design-1.json"
    )))
    .unwrap();
    for c in v["cases"].as_array().unwrap() {
        let raw = c["canonical"].as_str().unwrap().as_bytes();
        assert_eq!(Digest::blob(raw).as_str(), c["sha256"].as_str().unwrap());
        let normalized = match c["kind"].as_str().unwrap() {
            "manifest" => {
                LocalStore::fixture_manifest_valid(raw).unwrap();
                raw.to_vec()
            }
            "policy" => {
                let p: Policy = serde_json::from_slice(raw).unwrap();
                p.id().unwrap();
                encode(&p).unwrap()
            }
            "plan" => Plan::from_json(raw).unwrap().to_json().unwrap(),
            "reacquisition_plan" => reacquire::Plan::from_json(raw).unwrap().to_json().unwrap(),
            "intent" | "cancel_intent" => {
                encode(&serde_json::from_slice::<Intent>(raw).unwrap()).unwrap()
            }
            "release" => encode(&serde_json::from_slice::<Release>(raw).unwrap()).unwrap(),
            "reacquired_birth" => encode(&serde_json::from_slice::<Birth>(raw).unwrap()).unwrap(),
            _ => encode(&serde_json::from_slice::<Terminal>(raw).unwrap()).unwrap(),
        };
        assert_eq!(normalized, raw);
    }
}

#[test]
fn foreign_grants_and_insufficient_metadata_budget_hold_before_release() {
    let mut f = Fixture::new();
    let p = f.plan();
    LocalCapacityProbe::bind(&f.store, &mut f.ledger)
        .unwrap()
        .observe(
            &f.store,
            &mut f.ledger,
            key("probe-small"),
            key("sample-small"),
        )
        .unwrap();
    let small = f
        .ledger
        .reserve(key("small-job"), key("owner"), 1)
        .unwrap()
        .reference()
        .clone();
    let mut execution = f.execution();
    execution.job = &small;
    assert!(execution.apply(&p, &p.authorize().unwrap()).is_err());
    assert!(f.store.load_final_state(&p.birth).unwrap().is_none());
    assert_eq!(f.bytes(), b"asset");
    retention::verify(&mut f.ledger, &f.store, &f.grant).unwrap();

    let mut f = Fixture::new();
    LocalCapacityProbe::bind(&f.store, &mut f.ledger)
        .unwrap()
        .observe(
            &f.store,
            &mut f.ledger,
            key("probe-other"),
            key("sample-other"),
        )
        .unwrap();
    let foreign = retention::plan(
        &f.store,
        &retention::RetentionRequest {
            operation: key("foreign-pin"),
            owner: key("other"),
            publication: f.publication.clone(),
            minimum_ack: AckLevel::ProcessRestart,
        },
    )
    .unwrap();
    let job = f
        .ledger
        .reserve(key("other-job"), key("other"), foreign.required_peak())
        .unwrap()
        .reference()
        .clone();
    let grant = retention::retain(&mut f.ledger, &f.store, &job, &foreign)
        .unwrap()
        .grant;
    let request = Request {
        operation: key("delete"),
        owner: key("owner"),
        publication: f.publication.clone(),
        capacity: f.ledger.identity().clone(),
        context: f.context.clone(),
        window: Window {
            not_before: 50,
            expires: 500,
        },
    };
    assert!(matches!(
        plan(&f.store, request),
        Err(Error::ProtectedGrant)
    ));
    retention::verify(&mut f.ledger, &f.store, &grant).unwrap();
    assert_eq!(f.bytes(), b"asset");
}

#[test]
fn fresh_same_hash_reacquisition_survives_old_delete_and_released_grant_replay() {
    for stop in [
        None,
        Some(reacquire::Stage::Born),
        Some(reacquire::Stage::MetadataSettled),
        Some(reacquire::Stage::Published),
    ] {
        let mut f = Fixture::new();
        let old = f.plan();
        f.apply(&old);
        let publication = Publication {
            operation: key("fresh-ingest"),
            ..f.publication.clone()
        };
        let rp = reacquire::plan(
            &f.store,
            &old,
            reacquire::Request {
                publication: publication.clone(),
                capacity: f.ledger.identity().clone(),
                context: f.context.clone(),
                window: Window {
                    not_before: 50,
                    expires: 500,
                },
            },
        )
        .unwrap();
        let raw = rp.to_json().unwrap();
        let rp = reacquire::Plan::from_json(&raw).unwrap();
        let apply = |f: &mut Fixture, stop| {
            reacquire::Execution {
                store: &f.store,
                ledger: &mut f.ledger,
                job: &f.job,
                clock: &f.clock,
                admitted_context: &f.context,
            }
            .apply_hook(
                &rp,
                &rp.authorize().unwrap(),
                &mut Cursor::new(b"asset"),
                |stage| {
                    if Some(stage) == stop {
                        Err(Error::Injected)
                    } else {
                        Ok(())
                    }
                },
            )
        };
        // The original deletion cookie cannot authorize a new placement.
        assert!(matches!(
            reacquire::Execution {
                store: &f.store,
                ledger: &mut f.ledger,
                job: &f.job,
                clock: &f.clock,
                admitted_context: &f.context,
            }
            .apply(&rp, &old.authorize().unwrap(), &mut Cursor::new(b"asset")),
            Err(Error::WrongAuthority)
        ));
        if let Some(stop) = stop {
            assert!(matches!(apply(&mut f, Some(stop)), Err(Error::Injected)));
            // A durably admitted birth can finish source-preserving copy after expiry.
            f.clock.value.set(700);
        }
        apply(&mut f, None).unwrap();
        assert_eq!(f.bytes(), b"asset");
        let retention_plan = retention::plan(
            &f.store,
            &retention::RetentionRequest {
                operation: key("fresh-pin"),
                owner: key("owner"),
                publication,
                minimum_ack: AckLevel::ProcessRestart,
            },
        )
        .unwrap();
        let grant = retention::retain(&mut f.ledger, &f.store, &f.job, &retention_plan)
            .unwrap()
            .grant;
        let checkpoint = f.ledger.checkpoint();
        let receipt = f.apply(&old);
        assert!(receipt.historical);
        assert_eq!(receipt.outcome, Some(Outcome::Deleted));
        assert_eq!(receipt.observed, Observation::PresentUnverified);
        assert_eq!(f.ledger.checkpoint(), checkpoint);
        assert_eq!(f.bytes(), b"asset");
        retention::verify(&mut f.ledger, &f.store, &grant).unwrap();
        assert!(matches!(
            retention::verify(&mut f.ledger, &f.store, &f.grant),
            Err(retention::RetentionError::Released)
        ));
    }
}

#[test]
fn consent_expiry_or_rollback_after_release_holds_unlink_and_cancellation_preserves_bytes() {
    for time in [99, 500] {
        let mut f = Fixture::new();
        let p = f.plan();
        let clock = &f.clock;
        let mut execution = Execution {
            store: &f.store,
            ledger: &mut f.ledger,
            job: &f.job,
            roster: &f.roster,
            namespaces: &mut f.namespaces,
            nodes: &f.nodes,
            clock,
            admitted_context: &f.context,
        };
        assert!(matches!(
            execution.apply_hook(&p, &p.authorize().unwrap(), |s| {
                if s == Stage::BeforeUnlink {
                    clock.value.set(time);
                }
                Ok(())
            }),
            Err(Error::Clock)
        ));
        let receipt = execution.cancel(&p, &p.authorize().unwrap()).unwrap();
        assert_eq!(receipt.outcome, Some(Outcome::Cancelled));
        assert!(receipt.grants_released);
        assert_eq!(f.bytes(), b"asset");
        assert!(matches!(
            retention::verify(&mut f.ledger, &f.store, &f.grant),
            Err(retention::RetentionError::Released)
        ));
    }
}

#[test]
fn malformed_or_missing_release_evidence_cannot_explain_loss_or_be_repaired() {
    let mut f = Fixture::new();
    let p = f.plan();
    f.execution()
        .apply_hook(&p, &p.authorize().unwrap(), |s| {
            if s == Stage::Intent {
                Err(Error::Injected)
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    fs::remove_file(
        f.root
            .path()
            .join("store/objects")
            .join(f.publication.object.sha256.as_str()),
    )
    .unwrap();
    assert!(matches!(
        f.execution().apply(&p, &p.authorize().unwrap()),
        Err(Error::Recovery)
    ));
    let mut f = Fixture::new();
    let p = f.plan();
    f.apply(&p);
    let path = f
        .root
        .path()
        .join("store/lifecycle")
        .join(f.publication.object.sha256.as_str())
        .join("00.final-release.json");
    fs::remove_file(path).unwrap();
    assert!(f.execution().apply(&p, &p.authorize().unwrap()).is_err());
}

#[derive(Serialize, Deserialize)]
struct Saved {
    plan: Plan,
    ledger: crate::ledger::Checkpoint,
    job: JournalRef,
    roster: claims::Roster,
    namespaces: Vec<crate::source_claims::Checkpoint>,
}
#[test]
fn final_child() {
    let Some(root) = std::env::var_os("GIT_ASSET_FINAL_PROVIDER_CHILD") else {
        return;
    };
    let root = PathBuf::from(root);
    let saved: Saved = serde_json::from_slice(&fs::read(root.join("saved.json")).unwrap()).unwrap();
    let store = LocalStore::open(&root.join("store")).unwrap();
    let mut ledger = CapacityLedger::open(&root.join("ledger"), &saved.ledger).unwrap();
    let mut ns: Vec<_> = saved
        .namespaces
        .iter()
        .enumerate()
        .map(|(i, c)| Namespace::open(&root.join(format!("ns-{i}")), c).unwrap())
        .collect();
    let clock = TestClock {
        value: Cell::new(100),
        last: Cell::new(0),
    };
    let nodes = MemoryNodes::default();
    let context = d("private-context");
    let mut e = Execution {
        store: &store,
        ledger: &mut ledger,
        job: &saved.job,
        roster: &saved.roster,
        namespaces: &mut ns,
        nodes: &nodes,
        clock: &clock,
        admitted_context: &context,
    };
    let stop = std::env::var("GIT_ASSET_FINAL_PROVIDER_STAGE").unwrap();
    e.apply_hook(&saved.plan, &saved.plan.authorize().unwrap(), |s| {
        if format!("{s:?}") == stop {
            std::process::exit(76);
        }
        Ok(())
    })
    .unwrap();
    panic!("exit boundary not reached");
}
#[test]
fn actual_child_exits_resume_release_unlink_and_completion_without_repeating_loss() {
    for stop in [
        Stage::Admitted,
        Stage::Bound,
        Stage::Intent,
        Stage::Released,
        Stage::GrantsRemoved,
        Stage::BeforeUnlink,
        Stage::Unlinked,
        Stage::Terminal,
        Stage::Settled,
    ] {
        let mut f = Fixture::new();
        let p = f.plan();
        let saved = Saved {
            plan: p.clone(),
            ledger: f.ledger.checkpoint(),
            job: f.job.clone(),
            roster: f.roster.clone(),
            namespaces: f.namespaces.iter().map(|n| n.checkpoint()).collect(),
        };
        fs::write(
            f.root.path().join("saved.json"),
            serde_json::to_vec(&saved).unwrap(),
        )
        .unwrap();
        let o = Process::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "final_retirement::provider::tests::final_child",
                "--nocapture",
            ])
            .env("GIT_ASSET_FINAL_PROVIDER_CHILD", f.root.path())
            .env("GIT_ASSET_FINAL_PROVIDER_STAGE", format!("{stop:?}"))
            .output()
            .unwrap();
        assert_eq!(
            o.status.code(),
            Some(76),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
        drop(f.store);
        drop(f.ledger);
        f.store = LocalStore::open(&f.root.path().join("store")).unwrap();
        f.ledger = CapacityLedger::open(&f.root.path().join("ledger"), &saved.ledger).unwrap();
        assert_eq!(f.apply(&p).outcome, Some(Outcome::Deleted));
    }
}
