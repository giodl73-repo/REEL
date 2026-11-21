use super::*;
use crate::{
    metadata,
    source_claims::{Places, admission},
};
use std::{fs, path::PathBuf, process::Command as Process};

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
#[test]
fn independent_framing_vectors_pin_occurrence_enrollment_and_both_origins() {
    let vectors: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/vectors/age-witnesses.design-1.json"
    )))
    .unwrap();
    for case in vectors["cases"].as_array().unwrap() {
        let raw = case["canonical"].as_str().unwrap().as_bytes();
        let value = identity::parse_json(raw).unwrap();
        let expected: Digest = serde_json::from_value(case["id"].clone()).unwrap();
        assert_eq!(Digest::blob(raw).as_str(), case["sha256"].as_str().unwrap());
        match case["kind"].as_str().unwrap() {
            "enrollment" => {
                let v: Enrollment = serde_json::from_value(value).unwrap();
                assert_eq!(encode(&v).unwrap(), raw);
                assert_eq!(v.id().unwrap(), expected);
            }
            "request" => {
                let v: Request = serde_json::from_value(value).unwrap();
                assert_eq!(encode(&v).unwrap(), raw);
                assert_eq!(v.id().unwrap(), expected);
                assert_eq!(
                    v.path().unwrap().as_str(),
                    format!("age/{}", expected.as_str())
                );
            }
            "identity" => {
                let enrollment: Enrollment =
                    serde_json::from_str(vectors["cases"][0]["canonical"].as_str().unwrap())
                        .unwrap();
                let (object, actual) = identity_object(&enrollment).unwrap();
                assert_eq!(actual, raw);
                assert_eq!(object.sha256, expected);
            }
            "captured" | "inherited" => {
                let v: Record = serde_json::from_value(value).unwrap();
                assert_eq!(encode(&v).unwrap(), raw);
                assert_eq!(hash(&v).unwrap(), expected);
            }
            "inheritance_key" => {
                let inherited = vectors["cases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|c| c["kind"] == "inherited")
                    .unwrap();
                let record: Record =
                    serde_json::from_str(inherited["canonical"].as_str().unwrap()).unwrap();
                assert_eq!(record.slot().unwrap(), expected);
                assert_ne!(record.slot().unwrap(), record.target.request.id().unwrap());
                assert!(
                    record
                        .accepts(&record.target.request.path().unwrap())
                        .unwrap()
                );
                assert!(record.accepts(&record.path().unwrap()).unwrap());
                assert!(!record.accepts(&path("age/unrelated").unwrap()).unwrap());
            }
            _ => panic!("unknown vector"),
        }
    }
}
fn policy() -> admission::Policy {
    let p = Places {
        current: vec![key("F"), key("M")],
        pending: vec![key("F"), key("M")],
        predecessor: vec![key("D"), key("M")],
    };
    admission::Policy {
        prefer_active: true,
        classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
            .into_iter()
            .map(|k| (k, p.clone()))
            .collect(),
    }
}
fn enroll(n: &mut Namespace, m: &MetadataStore, source: &str, class: Kind) {
    n.apply(
        key(&format!("enroll-{source}")),
        Command::Enroll {
            source: key(source),
            owner: key("owner"),
            class,
        },
        m,
        &Limits::default().source,
    )
    .unwrap();
}
fn select(n: &mut Namespace, m: &MetadataStore, source: &str, selection: Selection, label: &str) {
    let s = n.source(&key(source)).unwrap().unwrap();
    n.select(
        &Select {
            prepare: key(&format!("prepare-{label}")),
            adopt: key(&format!("adopt-{label}")),
            source: key(source),
            owner: s.owner,
            expected_revision: s.revision,
            selection,
        },
        m,
        &Limits::default().source,
    )
    .unwrap();
}
fn withdraw(n: &mut Namespace, m: &MetadataStore, source: &str, label: &str) {
    let s = n.source(&key(source)).unwrap().unwrap();
    n.apply(
        key(label),
        Command::Withdraw {
            source: key(source),
            owner: s.owner,
            expected_revision: s.revision,
        },
        m,
        &Limits::default().source,
    )
    .unwrap();
}
struct TestClock {
    id: Digest,
    stamp: Cell<Stamp>,
    calls: Cell<usize>,
}
impl TestClock {
    fn new(id: &Digest, earliest: u64, latest: u64) -> Self {
        Self {
            id: id.clone(),
            stamp: Cell::new(Stamp { earliest, latest }),
            calls: Cell::new(0),
        }
    }
}
impl Clock for TestClock {
    fn now(&self, _: &Digest) -> crate::final_retirement::provider::Result<u64> {
        panic!("interval must not be reduced to a point")
    }
    fn bounds(&self, id: &Digest) -> crate::final_retirement::provider::Result<TimeBounds> {
        self.calls.set(self.calls.get() + 1);
        if id != &self.id {
            return Err(crate::final_retirement::provider::Error::Clock);
        }
        let t = self.stamp.get();
        Ok(TimeBounds {
            earliest: t.earliest,
            latest: t.latest,
        })
    }
}
struct NoClock;
impl Clock for NoClock {
    fn now(&self, _: &Digest) -> crate::final_retirement::provider::Result<u64> {
        panic!("no clock allowed")
    }
}
struct Fixture {
    temp: tempfile::TempDir,
    observer: Namespace,
    registry: Namespace,
    metadata: MetadataStore,
    e: Enrollment,
    request: Request,
    original: Selection,
    observer_min: Checkpoint,
    registry_min: Checkpoint,
    limits: Limits,
}
impl Fixture {
    fn new() -> Self {
        Self::with_clock(Digest::blob(b"clock"))
    }
    fn with_clock(clock: Digest) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut observer = Namespace::create(
            &temp.path().join("observer"),
            key("project"),
            key("epoch"),
            policy(),
        )
        .unwrap();
        let mut registry = Namespace::create(
            &temp.path().join("registry"),
            key("age"),
            key("epoch"),
            policy(),
        )
        .unwrap();
        let mut metadata = MetadataStore::create(
            &temp.path().join("metadata"),
            metadata::Limits {
                nodes: 100000,
                bytes: 64 * 1024 * 1024,
                database_bytes: 128 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let map = [
            ("a", Object::from_bytes(b"shared")),
            ("b", Object::from_bytes(b"shared")),
            ("c", Object::from_bytes(b"unique")),
        ]
        .into_iter()
        .map(|(p, o)| (path(p).unwrap(), o))
        .collect();
        let original = Selection {
            root: Tree::build(&map, &mut metadata, TreeLimits::default())
                .unwrap()
                .0
                .root()
                .clone(),
            context: Digest::blob(b"native-header"),
        };
        metadata.finish().unwrap();
        enroll(&mut observer, &metadata, "main", Kind::Branch);
        select(
            &mut observer,
            &metadata,
            "main",
            original.clone(),
            "original",
        );
        withdraw(&mut observer, &metadata, "main", "end-original");
        enroll(&mut registry, &metadata, "records", Kind::Retention);
        let e = Enrollment {
            authority: Digest::blob(b"authority"),
            epoch: Digest::blob(b"epoch"),
            metadata: metadata.checkpoint().identity().clone(),
            registry: registry.identity().clone(),
            registry_policy: hash(registry.policy()).unwrap(),
            source: key("records"),
            owner: key("owner"),
            clock,
            observers: vec![Observer {
                namespace: observer.identity().clone(),
                policy: hash(observer.policy()).unwrap(),
                owners: vec![key("owner")],
                classes: [Kind::Branch, Kind::Draft].into_iter().collect(),
            }],
        };
        let request = Request {
            namespace: observer.identity().clone(),
            source: key("main"),
            predecessor_revision: 3,
        };
        let mut f = Self {
            observer_min: observer.checkpoint(),
            registry_min: registry.checkpoint(),
            temp,
            observer,
            registry,
            metadata,
            e,
            request,
            original,
            limits: Limits::default(),
        };
        f.capture_api().initialize().unwrap();
        f
    }
    fn capture_api(&mut self) -> Capture<'_> {
        Capture {
            enrollment: &self.e,
            registry: &mut self.registry,
            minimum: &self.registry_min,
            metadata: &mut self.metadata,
            limits: &self.limits,
        }
    }
    fn capture(&mut self, clock: &dyn Clock) -> Result<Status> {
        Capture {
            enrollment: &self.e,
            registry: &mut self.registry,
            minimum: &self.registry_min,
            metadata: &mut self.metadata,
            limits: &self.limits,
        }
        .capture(&self.request, &mut self.observer, &self.observer_min, clock)
    }
    fn status(&mut self, r: &Request) -> Result<Status> {
        let mut nodes = self.metadata.finished_reader()?;
        Inspection {
            enrollment: &self.e,
            registry: &mut self.registry,
            minimum: &self.registry_min,
            metadata: self.e.metadata.clone(),
            nodes: &mut nodes,
            limits: &self.limits,
        }
        .status(r, &mut self.observer, &self.observer_min)
    }
    fn assess(&mut self, r: &Request, clock: &dyn Clock, age: u64) -> Result<Assessment> {
        let mut nodes = self.metadata.finished_reader()?;
        Inspection {
            enrollment: &self.e,
            registry: &mut self.registry,
            minimum: &self.registry_min,
            metadata: self.e.metadata.clone(),
            nodes: &mut nodes,
            limits: &self.limits,
        }
        .assess(r, &mut self.observer, &self.observer_min, clock, age)
    }
    fn subset_enrollment(&self) -> subset::Enrollment {
        subset::Enrollment {
            authority: self.e.authority.clone(),
            namespace: self.request.namespace.clone(),
            policy: self.e.observers[0].policy.clone(),
            owner: key("owner"),
            classes: vec![Kind::Branch, Kind::Draft],
            limits: self.limits.source.clone(),
            writes: 10000,
            write_bytes: 8 * 1024 * 1024,
        }
    }
    fn inherit(
        &mut self,
        parent: &Request,
        e: &subset::Enrollment,
        p: &subset::Plan,
    ) -> Result<Status> {
        Capture {
            enrollment: &self.e,
            registry: &mut self.registry,
            minimum: &self.registry_min,
            metadata: &mut self.metadata,
            limits: &self.limits,
        }
        .inherit(parent, &mut self.observer, &self.observer_min, e, p)
    }
    fn reopen(&mut self) {
        self.observer =
            Namespace::open(&self.temp.path().join("observer"), &self.observer_min).unwrap();
        self.registry =
            Namespace::open(&self.temp.path().join("registry"), &self.registry_min).unwrap();
        self.metadata = MetadataStore::open(
            &self.temp.path().join("metadata"),
            &self.metadata.checkpoint(),
        )
        .unwrap();
    }
}

#[test]
fn durable_capture_retry_status_and_conservative_interval() {
    let mut f = Fixture::new();
    let r = f.request.clone();
    let before = f.observer.checkpoint();
    let clock = TestClock::new(&f.e.clock, 100, 120);
    let captured = f.capture(&clock).unwrap();
    assert_eq!(clock.calls.get(), 1);
    assert_eq!(
        captured.observed,
        Some(Stamp {
            earliest: 100,
            latest: 120
        })
    );
    assert_eq!(before, f.observer.checkpoint());
    f.reopen();
    let registry = f.registry.checkpoint();
    let metadata = f.metadata.checkpoint();
    let replay = f.capture(&NoClock).unwrap();
    assert_eq!(captured.evidence, replay.evidence);
    f.capture_api().initialize().unwrap();
    assert_eq!(registry, f.registry.checkpoint());
    assert_eq!(metadata, f.metadata.checkpoint());
    assert_eq!(f.status(&r).unwrap().evidence, captured.evidence);
    assert_eq!(registry, f.registry.checkpoint());
    for (earliest, latest, age, old, lower) in [
        (110, 125, 0, false, 0),
        (129, 140, 10, false, 9),
        (130, 150, 10, true, 10),
    ] {
        clock.stamp.set(Stamp { earliest, latest });
        let a = f.assess(&r, &clock, age).unwrap();
        assert_eq!(a.old_enough, old);
        assert_eq!(a.age_lower_bound_millis, Some(lower));
        assert!(!a.authorizes_claim_release && !a.authorizes_removal);
    }
    clock.stamp.set(Stamp {
        earliest: 99,
        latest: 130,
    });
    assert!(matches!(f.assess(&r, &clock, 0), Err(Error::Clock)));
    assert_eq!(registry, f.registry.checkpoint());
    assert_eq!(metadata, f.metadata.checkpoint());
}

#[test]
fn unknown_current_reactivation_and_missing_occurrence_do_not_reuse_age() {
    let mut f = Fixture::new();
    let old = f.request.clone();
    assert_eq!(
        f.assess(&old, &NoClock, 10).unwrap().hold,
        Some("unknown_age")
    );
    f.capture(&TestClock::new(&f.e.clock, 100, 110)).unwrap();
    select(
        &mut f.observer,
        &f.metadata,
        "main",
        f.original.clone(),
        "reactivated",
    );
    let current = f
        .observer
        .source(&key("main"))
        .unwrap()
        .unwrap()
        .current
        .unwrap();
    let recent = Request {
        predecessor_revision: current.revision,
        ..old.clone()
    };
    assert_eq!(
        f.assess(&recent, &NoClock, 10).unwrap().hold,
        Some("unknown_age")
    );
    // Even a known old occurrence is merely age evidence; the current alias is
    // still a source obligation, to be revalidated by final retirement.
    assert!(
        f.assess(&old, &TestClock::new(&f.e.clock, 1000, 1001), 10)
            .unwrap()
            .old_enough
    );
    let s = f.observer.source(&key("main")).unwrap().unwrap();
    f.observer
        .apply(
            key("retire-old"),
            Command::Retire {
                source: key("main"),
                owner: key("owner"),
                expected_revision: s.revision,
                predecessor_revision: 3,
            },
            &f.metadata,
            &f.limits.source,
        )
        .unwrap();
    assert!(f.status(&old).unwrap().observed.is_some());
    assert_eq!(
        f.assess(&old, &NoClock, 10).unwrap().hold,
        Some("occurrence_missing")
    );
    withdraw(&mut f.observer, &f.metadata, "main", "end-reactivated");
    assert_eq!(
        f.assess(&recent, &NoClock, 10).unwrap().hold,
        Some("unknown_age")
    );
}

#[test]
fn exact_scope_clock_and_record_budgets_refuse_without_observer_mutation() {
    let mut f = Fixture::new();
    let before = f.observer.checkpoint();
    assert!(matches!(
        f.capture(&TestClock::new(&Digest::blob(b"foreign"), 100, 110)),
        Err(Error::Clock)
    ));
    let mut invalid = f.e.clone();
    invalid.observers[0].namespace = invalid.registry.clone();
    assert!(invalid.id().is_err());
    invalid = f.e.clone();
    invalid.observers[0].owners.push(key("owner"));
    assert!(invalid.id().is_err());
    f.e.observers[0].classes = [Kind::Job].into_iter().collect();
    assert!(f.capture(&NoClock).is_err());
    f.e.observers[0].classes = [Kind::Branch, Kind::Draft].into_iter().collect();
    f.limits.writes = 0;
    assert!(matches!(
        f.capture(&TestClock::new(&f.e.clock, 100, 110)),
        Err(Error::Budget)
    ));
    assert_eq!(before, f.observer.checkpoint());
    assert!(f.metadata.pending_nodes() == 0);
    f.limits = Limits::default();
    f.capture(&TestClock::new(&f.e.clock, 100, 110)).unwrap();
    let r = f.request.clone();
    let registry = f.registry.checkpoint();
    f.limits.source.reads = 1;
    assert!(f.status(&r).is_err());
    assert_eq!(registry, f.registry.checkpoint());
    f.limits = Limits::default();
    f.e.epoch = Digest::blob(b"other-epoch");
    assert!(f.status(&r).is_err());
    assert_eq!(before, f.observer.checkpoint());
}

#[test]
fn concurrent_peer_acknowledgment_wins_without_timestamp_replacement() {
    let mut f = Fixture::new();
    let e = f.e.clone();
    let r = f.request.clone();
    let root = f.temp.path().to_path_buf();
    let registry_min = f.registry_min.clone();
    let observer_min = f.observer_min.clone();
    let metadata_min = f.metadata.checkpoint();
    let mut peer_evidence = None;
    Capture {
        enrollment: &f.e,
        registry: &mut f.registry,
        minimum: &f.registry_min,
        metadata: &mut f.metadata,
        limits: &f.limits,
    }
    .capture_hook(
        &r,
        &mut f.observer,
        &f.observer_min,
        &TestClock::new(&e.clock, 100, 110),
        |stage| {
            if stage == Stage::Sampled {
                let mut registry = Namespace::open(&root.join("registry"), &registry_min).unwrap();
                let mut observer = Namespace::open(&root.join("observer"), &observer_min).unwrap();
                let mut metadata =
                    MetadataStore::open(&root.join("metadata"), &metadata_min).unwrap();
                peer_evidence = Some(
                    Capture {
                        enrollment: &e,
                        registry: &mut registry,
                        minimum: &registry_min,
                        metadata: &mut metadata,
                        limits: &Limits::default(),
                    }
                    .capture(
                        &r,
                        &mut observer,
                        &observer_min,
                        &TestClock::new(&e.clock, 105, 115),
                    )
                    .unwrap()
                    .evidence,
                );
            }
            Ok(())
        },
    )
    .unwrap();
    let status = f.capture(&NoClock).unwrap();
    assert_eq!(Some(status.evidence), peer_evidence);
    assert_eq!(
        status.observed,
        Some(Stamp {
            earliest: 105,
            latest: 115
        })
    );
    assert!(
        f.registry
            .source(&f.e.source)
            .unwrap()
            .unwrap()
            .predecessors
            .is_empty()
    );
}

#[test]
fn simultaneous_capture_peers_converge_on_one_immutable_observation() {
    let mut f = Fixture::new();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut threads = Vec::new();
    for offset in [0, 5] {
        let root = f.temp.path().to_path_buf();
        let e = f.e.clone();
        let r = f.request.clone();
        let rm = f.registry_min.clone();
        let om = f.observer_min.clone();
        let mm = f.metadata.checkpoint();
        let gate = barrier.clone();
        threads.push(std::thread::spawn(move || {
            let mut registry = Namespace::open(&root.join("registry"), &rm).unwrap();
            let mut observer = Namespace::open(&root.join("observer"), &om).unwrap();
            let mut metadata = MetadataStore::open(&root.join("metadata"), &mm).unwrap();
            Capture {
                enrollment: &e,
                registry: &mut registry,
                minimum: &rm,
                metadata: &mut metadata,
                limits: &Limits::default(),
            }
            .capture_hook(
                &r,
                &mut observer,
                &om,
                &TestClock::new(&e.clock, 100 + offset, 110 + offset),
                |stage| {
                    if stage == Stage::Sampled {
                        gate.wait();
                    }
                    Ok(())
                },
            )
        }));
    }
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    for error in results.iter().filter_map(|r| r.as_ref().err()) {
        assert!(
            matches!(
                error,
                Error::Metadata(crate::metadata::Error::Busy)
                    | Error::Admission(crate::source_claims::admission::Error::Busy)
                    | Error::Admission(crate::source_claims::admission::Error::Journal(
                        crate::source_claims::Error::Busy | crate::source_claims::Error::Conflict
                    ))
            ) || matches!(error, Error::Tree(TreeError::Store(message)) if message == "metadata busy"),
            "unexpected capture peer failure: {error:?}"
        );
    }
    let acknowledged: Vec<_> = results
        .into_iter()
        .filter_map(std::result::Result::ok)
        .collect();
    let request = f.request.clone();
    let retained = f.status(&request).unwrap();
    let clock = TestClock::new(&f.e.clock, 200, 210);
    let winner = f.capture(&clock).unwrap();
    if retained.evidence.is_some() {
        assert_eq!(winner.evidence, retained.evidence);
        assert_eq!(winner.observed, retained.observed);
        assert_eq!(clock.calls.get(), 0);
        assert!(matches!(
            winner.observed,
            Some(
                Stamp {
                    earliest: 100,
                    latest: 110
                } | Stamp {
                    earliest: 105,
                    latest: 115
                }
            )
        ));
    } else {
        assert!(
            acknowledged.is_empty(),
            "acknowledged capture lost its retained fact"
        );
        assert_eq!(clock.calls.get(), 1);
        assert_eq!(
            winner.observed,
            Some(Stamp {
                earliest: 200,
                latest: 210
            })
        );
    }
    for a in acknowledged {
        assert_eq!(a.evidence, winner.evidence);
        assert_eq!(a.observed, winner.observed);
    }
    assert_eq!(f.capture(&NoClock).unwrap().evidence, winner.evidence);
}

#[test]
fn both_busy_capture_peers_require_later_explicit_observation_then_replay_without_time() {
    use std::sync::{Arc, Barrier};
    let mut f = Fixture::new();
    let registry_before = f.registry.checkpoint();
    let observer_before = f.observer.checkpoint();
    let ready = Arc::new(Barrier::new(3));
    let release = Arc::new(Barrier::new(3));
    let mut peers = Vec::new();
    for offset in [0, 5] {
        let root = f.temp.path().to_path_buf();
        let (e, r, rm, om, mm) = (
            f.e.clone(),
            f.request.clone(),
            f.registry_min.clone(),
            f.observer_min.clone(),
            f.metadata.checkpoint(),
        );
        let (ready, release) = (ready.clone(), release.clone());
        peers.push(std::thread::spawn(move || {
            let mut registry = Namespace::open(&root.join("registry"), &rm).unwrap();
            let mut observer = Namespace::open(&root.join("observer"), &om).unwrap();
            let mut metadata = MetadataStore::open(&root.join("metadata"), &mm).unwrap();
            Capture {
                enrollment: &e,
                registry: &mut registry,
                minimum: &rm,
                metadata: &mut metadata,
                limits: &Limits::default(),
            }
            .capture_hook(
                &r,
                &mut observer,
                &om,
                &TestClock::new(&e.clock, 100 + offset, 110 + offset),
                |stage| {
                    if stage == Stage::Sampled {
                        ready.wait();
                        release.wait();
                    }
                    Ok(())
                },
            )
        }));
    }
    ready.wait();
    let blocked = Arc::new(Barrier::new(2));
    let unblock = Arc::new(Barrier::new(2));
    let root = f.temp.path().join("metadata");
    let minimum = f.metadata.checkpoint();
    let (writer_ready, writer_release) = (blocked.clone(), unblock.clone());
    let writer = std::thread::spawn(move || {
        let mut metadata = MetadataStore::open(&root, &minimum).unwrap();
        metadata
            .put(b"unrelated synthetic metadata writer")
            .unwrap();
        metadata
            .finish_hook(|stage| {
                if stage == crate::metadata::Stage::BeforeCommit {
                    writer_ready.wait();
                    writer_release.wait();
                }
                Ok(())
            })
            .unwrap();
    });
    blocked.wait();
    release.wait();
    let results = peers.into_iter().map(|p| p.join()).collect::<Vec<_>>();
    unblock.wait();
    writer.join().unwrap();
    for result in results {
        assert!(matches!(
            result.unwrap(),
            Err(Error::Metadata(crate::metadata::Error::Busy))
        ));
    }
    assert_eq!(f.registry.checkpoint(), registry_before);
    assert_eq!(f.observer.checkpoint(), observer_before);
    let request = f.request.clone();
    assert!(f.status(&request).unwrap().evidence.is_none());
    let clock = TestClock::new(&f.e.clock, 200, 210);
    let acknowledged = f.capture(&clock).unwrap();
    assert_eq!(clock.calls.get(), 1);
    assert_eq!(
        acknowledged.observed,
        Some(Stamp {
            earliest: 200,
            latest: 210
        })
    );
    let retry = f.capture(&NoClock).unwrap();
    assert_eq!(retry.evidence, acknowledged.evidence);
    assert_eq!(retry.observed, acknowledged.observed);
}

#[test]
fn registry_full_holds_new_occurrences_and_preserves_acknowledged_facts() {
    let mut f = Fixture::new();
    let first = f.request.clone();
    f.limits.entries = 1;
    let evidence = f
        .capture(&TestClock::new(&f.e.clock, 100, 110))
        .unwrap()
        .evidence;
    select(
        &mut f.observer,
        &f.metadata,
        "main",
        f.original.clone(),
        "again",
    );
    let v = f
        .observer
        .source(&key("main"))
        .unwrap()
        .unwrap()
        .current
        .unwrap();
    withdraw(&mut f.observer, &f.metadata, "main", "end-again");
    f.request.predecessor_revision = v.revision;
    let before = f.registry.checkpoint();
    assert!(matches!(
        f.capture(&TestClock::new(&f.e.clock, 200, 210)),
        Err(Error::Budget)
    ));
    assert_eq!(before, f.registry.checkpoint());
    assert_eq!(f.status(&first).unwrap().evidence, evidence);
    assert!(f.status(&f.request.clone()).unwrap().observed.is_none());
}

#[test]
fn phase_changes_cannot_reset_capture_read_or_elapsed_budgets() {
    let mut f = Fixture::new();
    let before = f.registry.checkpoint();
    let clock = TestClock::new(&f.e.clock, 100, 110);
    // Every individual small-tree phase fits four reads. Their composition
    // must share the same counter and refuse before adopting the new root.
    f.limits.source.reads = 4;
    assert!(matches!(f.capture(&clock), Err(Error::Budget)));
    assert_eq!(clock.calls.get(), 1);
    assert_eq!(before, f.registry.checkpoint());
    f.limits = Limits::default();
    let r = f.request.clone();
    assert!(f.status(&r).unwrap().evidence.is_none());
    // Simulate a caller pause after observing the actual historical source.
    // An expired operation cannot regain time by entering clock/write phases.
    f.limits.source.millis = 1000;
    let mut observed = false;
    let result = Capture {
        enrollment: &f.e,
        registry: &mut f.registry,
        minimum: &f.registry_min,
        metadata: &mut f.metadata,
        limits: &f.limits,
    }
    .capture_hook(&r, &mut f.observer, &f.observer_min, &NoClock, |stage| {
        if stage == Stage::Observed {
            observed = true;
            std::thread::sleep(Duration::from_millis(1010));
        }
        Ok(())
    });
    assert!(matches!(result, Err(Error::Budget)));
    assert!(observed);
    assert_eq!(before, f.registry.checkpoint());
    f.limits = Limits::default();
    assert_eq!(
        f.capture(&TestClock::new(&f.e.clock, 200, 210))
            .unwrap()
            .observed,
        Some(Stamp {
            earliest: 200,
            latest: 210
        })
    );
}

#[test]
fn acknowledged_evidence_and_independent_minima_reject_restored_prefixes() {
    let mut f = Fixture::new();
    // Back up a source prefix before this occurrence becomes historical.
    select(
        &mut f.observer,
        &f.metadata,
        "main",
        f.original.clone(),
        "second",
    );
    let v = f
        .observer
        .source(&key("main"))
        .unwrap()
        .unwrap()
        .current
        .unwrap();
    fs::copy(
        f.temp.path().join("observer/claims.sqlite"),
        f.temp.path().join("source-prefix.sqlite"),
    )
    .unwrap();
    withdraw(&mut f.observer, &f.metadata, "main", "end-second");
    f.request.predecessor_revision = v.revision;
    f.capture(&TestClock::new(&f.e.clock, 100, 110)).unwrap();
    let latest = f.observer.checkpoint();
    let saved_metadata = f.metadata.checkpoint();
    let registry_min = f.registry.checkpoint();
    let root = f.temp.path().to_path_buf();
    let r = f.request.clone();
    let e = f.e.clone();
    let old_min = f.observer_min.clone();
    drop(f.observer);
    drop(f.registry);
    drop(f.metadata);
    fs::copy(
        root.join("source-prefix.sqlite"),
        root.join("observer/claims.sqlite"),
    )
    .unwrap();
    assert!(Namespace::open(&root.join("observer"), &latest).is_err());
    // Even if the caller's original minimum predates the observation, the
    // acknowledged record contributes its own stronger source checkpoint.
    let mut observer = Namespace::open(&root.join("observer"), &old_min).unwrap();
    let mut registry = Namespace::open(&root.join("registry"), &registry_min).unwrap();
    let mut metadata = MetadataStore::open(&root.join("metadata"), &saved_metadata).unwrap();
    let mut nodes = metadata.finished_reader().unwrap();
    assert!(
        Inspection {
            enrollment: &e,
            registry: &mut registry,
            minimum: &registry_min,
            metadata: e.metadata.clone(),
            nodes: &mut nodes,
            limits: &Limits::default()
        }
        .status(&r, &mut observer, &old_min)
        .is_err()
    );
    // Missing an acknowledged immutable record is recovery failure, not age 0.
    let source = registry.source(&e.source).unwrap().unwrap();
    let mut w = Work::new(&mut nodes, &f.limits).unwrap();
    let map = entries(&e, &source, &mut w).unwrap();
    let evidence = map.get(&r.path().unwrap()).unwrap();
    let mut masked = Mask {
        inner: &mut metadata,
        hidden: evidence.clone(),
    };
    assert!(matches!(
        Inspection {
            enrollment: &e,
            registry: &mut registry,
            minimum: &registry_min,
            metadata: e.metadata.clone(),
            nodes: &mut masked,
            limits: &f.limits
        }
        .status(&r, &mut observer, &old_min),
        Err(Error::Recovery)
    ));
}
struct Mask<'a> {
    inner: &'a mut MetadataStore,
    hidden: Object,
}
impl NodeStore for Mask<'_> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        if o == &self.hidden {
            Ok(None)
        } else {
            self.inner.get(o)
        }
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        panic!("inspection cannot write metadata")
    }
}

#[test]
fn executed_owned_subsets_inherit_age_and_unexecuted_plans_cannot() {
    let mut f = Fixture::new();
    let parent = f.request.clone();
    let e = f.subset_enrollment();
    let stamp = f
        .capture(&TestClock::new(&f.e.clock, 100, 110))
        .unwrap()
        .observed;
    let p = subset::plan(
        &e,
        &mut f.observer,
        subset::Request {
            source: parent.source.clone(),
            predecessor_revision: 3,
            paths: vec![path("a").unwrap()],
        },
        &f.metadata.finished_reader().unwrap(),
    )
    .unwrap();
    assert!(f.inherit(&parent, &e, &p).is_err());
    subset::apply(&e, &mut f.observer, &mut f.metadata, &p).unwrap();
    let first = f.inherit(&parent, &e, &p).unwrap();
    assert_eq!(first.observed, stamp);
    let r = first.request.clone();
    f.reopen();
    assert_eq!(f.status(&r).unwrap().observed, stamp);
    let p2 = subset::plan(
        &e,
        &mut f.observer,
        subset::Request {
            source: r.source.clone(),
            predecessor_revision: r.predecessor_revision,
            paths: vec![path("b").unwrap()],
        },
        &f.metadata.finished_reader().unwrap(),
    )
    .unwrap();
    subset::apply(&e, &mut f.observer, &mut f.metadata, &p2).unwrap();
    let second = f.inherit(&r, &e, &p2).unwrap();
    assert_eq!(second.observed, stamp);
    assert!(
        f.assess(&second.request, &TestClock::new(&f.e.clock, 120, 125), 10)
            .unwrap()
            .old_enough
    );
    assert_eq!(
        f.assess(&r, &NoClock, 10).unwrap().hold,
        Some("occurrence_missing")
    );
    f.limits.lineage = 2;
    assert!(matches!(f.status(&second.request), Err(Error::Budget)));
}

#[test]
fn later_survivor_capture_cannot_suppress_older_verified_inheritance() {
    let mut f = Fixture::new();
    let parent = f.request.clone();
    let e = f.subset_enrollment();
    let old = f.capture(&TestClock::new(&f.e.clock, 100, 110)).unwrap();
    let plan = subset::plan(
        &e,
        &mut f.observer,
        subset::Request {
            source: parent.source.clone(),
            predecessor_revision: 3,
            paths: vec![path("a").unwrap()],
        },
        &f.metadata.finished_reader().unwrap(),
    )
    .unwrap();
    subset::apply(&e, &mut f.observer, &mut f.metadata, &plan).unwrap();
    let child = Request {
        namespace: parent.namespace.clone(),
        source: plan.survivor_source().unwrap().unwrap(),
        predecessor_revision: 3,
    };
    f.request = child.clone();
    let young = f.capture(&TestClock::new(&f.e.clock, 1000, 1010)).unwrap();
    assert_ne!(young.observed, old.observed);
    let before = f.observer.checkpoint();
    let inherited = f.inherit(&parent, &e, &plan).unwrap();
    assert_eq!(inherited.observed, old.observed);
    assert_ne!(inherited.evidence, young.evidence);
    let mut pinned_reader = f.metadata.finished_reader().unwrap();
    let pinned = Inspection {
        enrollment: &f.e,
        registry: &mut f.registry,
        minimum: &f.registry_min,
        metadata: f.e.metadata.clone(),
        nodes: &mut pinned_reader,
        limits: &f.limits,
    }
    .proof(
        &child,
        young.evidence.as_ref().unwrap(),
        &mut f.observer,
        &f.observer_min,
    )
    .unwrap();
    assert_eq!(pinned.observed, young.observed);
    assert_eq!(before, f.observer.checkpoint());
    f.reopen();
    assert_eq!(f.capture(&NoClock).unwrap().observed, old.observed);
    // The exact first capture document/binding survives the stronger proof.
    let source = f.registry.source(&f.e.source).unwrap().unwrap();
    let mut reader = f.metadata.finished_reader().unwrap();
    let mut work = Work::new(&mut reader, &f.limits).unwrap();
    let map = entries(&f.e, &source, &mut work).unwrap();
    assert_eq!(map.get(&child.path().unwrap()), young.evidence.as_ref());
    assert_eq!(
        work.record(young.evidence.as_ref().unwrap(), &f.e)
            .unwrap()
            .origin,
        Origin::Captured {
            checkpoint: before.clone(),
            observed: Stamp {
                earliest: 1000,
                latest: 1010
            }
        }
    );
    let next = subset::plan(
        &e,
        &mut f.observer,
        subset::Request {
            source: child.source.clone(),
            predecessor_revision: 3,
            paths: vec![path("b").unwrap()],
        },
        &f.metadata.finished_reader().unwrap(),
    )
    .unwrap();
    subset::apply(&e, &mut f.observer, &mut f.metadata, &next).unwrap();
    assert_eq!(
        f.inherit(&parent, &e, &plan).unwrap().observed,
        old.observed
    );
    let grandchild = f.inherit(&child, &e, &next).unwrap();
    assert_eq!(grandchild.observed, old.observed);
    let budget = crate::protection::budget::Budget::new(
        &f.limits.source,
        f.limits.writes,
        f.limits.write_bytes,
    )
    .unwrap();
    let pinned_child = Capture {
        enrollment: &f.e,
        registry: &mut f.registry,
        minimum: &f.registry_min,
        metadata: &mut f.metadata,
        limits: &f.limits,
    }
    .inherit_from_metered(
        &child,
        young.evidence.as_ref().unwrap(),
        &mut f.observer,
        &f.observer_min,
        &e,
        &next,
        &budget,
    )
    .unwrap();
    assert_eq!(pinned_child.observed, young.observed);
    assert_ne!(pinned_child.evidence, grandchild.evidence);
    assert!(budget.usage().0 > 1);
    let mut small = f.limits.source.clone();
    small.reads = 1;
    let exhausted =
        crate::protection::budget::Budget::new(&small, f.limits.writes, f.limits.write_bytes)
            .unwrap();
    exhausted
        .read(&Object::from_bytes(b"already-consumed-failed-read"))
        .unwrap();
    let registry_before = f.registry.checkpoint();
    assert!(matches!(
        Capture {
            enrollment: &f.e,
            registry: &mut f.registry,
            minimum: &f.registry_min,
            metadata: &mut f.metadata,
            limits: &f.limits,
        }
        .inherit_from_metered(
            &child,
            young.evidence.as_ref().unwrap(),
            &mut f.observer,
            &f.observer_min,
            &e,
            &next,
            &exhausted
        ),
        Err(Error::Budget)
    ));
    assert_eq!(registry_before, f.registry.checkpoint());
    assert_eq!(
        f.status(&grandchild.request).unwrap().observed,
        old.observed
    );
    assert_eq!(
        f.assess(&child, &NoClock, 10).unwrap().hold,
        Some("occurrence_missing")
    );
    assert!(
        f.assess(
            &grandchild.request,
            &TestClock::new(&f.e.clock, 120, 130),
            10
        )
        .unwrap()
        .old_enough
    );
}

#[test]
fn historical_inheritance_after_leaf_retirement_is_evidence_only() {
    let mut f = Fixture::new();
    let parent = f.request.clone();
    let e = f.subset_enrollment();
    let observed = f
        .capture(&TestClock::new(&f.e.clock, 100, 110))
        .unwrap()
        .observed;
    let p = subset::plan(
        &e,
        &mut f.observer,
        subset::Request {
            source: parent.source.clone(),
            predecessor_revision: 3,
            paths: vec![path("a").unwrap()],
        },
        &f.metadata.finished_reader().unwrap(),
    )
    .unwrap();
    subset::apply(&e, &mut f.observer, &mut f.metadata, &p).unwrap();
    let child = Request {
        namespace: parent.namespace.clone(),
        source: p.survivor_source().unwrap().unwrap(),
        predecessor_revision: 3,
    };
    let q = subset::plan(
        &e,
        &mut f.observer,
        subset::Request {
            source: child.source.clone(),
            predecessor_revision: 3,
            paths: vec![path("b").unwrap(), path("c").unwrap()],
        },
        &f.metadata.finished_reader().unwrap(),
    )
    .unwrap();
    subset::apply(&e, &mut f.observer, &mut f.metadata, &q).unwrap();
    let before = f.observer.checkpoint();
    let inherited = f.inherit(&parent, &e, &p).unwrap();
    assert_eq!(inherited.observed, observed);
    assert_eq!(before, f.observer.checkpoint());
    assert_eq!(
        f.assess(&child, &NoClock, 10).unwrap().hold,
        Some("occurrence_missing")
    );
}

#[test]
#[ignore = "requires GIT_ASSET_PRE_ALTERNATE_AGE_CLI from the pinned pre-alternate source"]
fn pre_alternate_reader_and_writer_preserve_stronger_inherited_facts() {
    let executable = std::env::var_os("GIT_ASSET_PRE_ALTERNATE_AGE_CLI")
        .expect("explicit old artifact required");
    assert!(PathBuf::from(&executable).is_absolute());
    let clock_temp = tempfile::tempdir().unwrap();
    let clock_root = clock_temp.path().join("clock");
    let ledger = crate::ledger::CapacityLedger::create_clock(
        &clock_root,
        key("clock"),
        key("epoch"),
        0,
        crate::clock::Policy {
            authority: Digest::blob(b"authority"),
            epoch: Digest::blob(b"epoch"),
            boundary: Digest::blob(b"native-os"),
            max_sample_millis: 1000,
            max_drift_millis: 1000,
            max_session_millis: 60000,
        },
    )
    .unwrap();
    let clock_enrollment = ledger.clock_enrollment().unwrap();
    let clock_min = ledger.checkpoint();
    drop(ledger);
    let mut f = Fixture::with_clock(clock_enrollment.id().unwrap());
    let parent = f.request.clone();
    let e = f.subset_enrollment();
    let old = f.capture(&TestClock::new(&f.e.clock, 100, 110)).unwrap();
    let plan = subset::plan(
        &e,
        &mut f.observer,
        subset::Request {
            source: parent.source.clone(),
            predecessor_revision: 3,
            paths: vec![path("a").unwrap()],
        },
        &f.metadata.finished_reader().unwrap(),
    )
    .unwrap();
    subset::apply(&e, &mut f.observer, &mut f.metadata, &plan).unwrap();
    f.request = Request {
        namespace: parent.namespace.clone(),
        source: plan.survivor_source().unwrap().unwrap(),
        predecessor_revision: 3,
    };
    let young = f.capture(&TestClock::new(&f.e.clock, 1000, 1010)).unwrap();
    f.inherit(&parent, &e, &plan).unwrap();
    let root = f.temp.path();
    let profile = root.join("old-profile.json");
    let request = root.join("old-request.json");
    fs::write(
        &profile,
        serde_json::to_vec(&serde_json::json!({
            "schema":"tig.age-profile.design-1","enrollment":f.e,
            "registry":{"root":root.join("registry"),"minimum":f.registry.checkpoint()},
            "observers":[{"root":root.join("observer"),"minimum":f.observer.checkpoint()}],
            "metadata":{"root":root.join("metadata"),"minimum":f.metadata.checkpoint()},
        "clock":{"root":clock_root,"minimum":clock_min,"enrollment":clock_enrollment},
            "permissions":{"capture":true}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(&request, serde_json::to_vec(&f.request).unwrap()).unwrap();
    let before = f.observer.checkpoint();
    for op in ["status", "capture"] {
        let out = Process::new(&executable)
            .args(["age", op])
            .arg(&profile)
            .arg(&request)
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(out.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let value = if op == "capture" {
            &value["status"]
        } else {
            &value
        };
        assert_eq!(
            value["observed"],
            serde_json::to_value(young.observed).unwrap()
        );
        assert_eq!(
            value["evidence"],
            serde_json::to_value(&young.evidence).unwrap()
        );
    }
    f.reopen();
    assert_eq!(f.status(&f.request.clone()).unwrap().observed, old.observed);
    assert_eq!(f.observer.checkpoint(), before);
    assert_eq!(
        crate::ledger::CapacityLedger::open(&clock_root, &clock_min)
            .unwrap()
            .checkpoint(),
        clock_min
    );
}

#[derive(Serialize, Deserialize)]
struct Saved {
    e: Enrollment,
    request: Request,
    observer: Checkpoint,
    registry: Checkpoint,
    metadata: metadata::Checkpoint,
}
#[test]
fn abrupt_exit_helper() {
    let Some(root) = std::env::var_os("GIT_ASSET_AGE_EXIT_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let s: Saved = serde_json::from_slice(&fs::read(root.join("saved.json")).unwrap()).unwrap();
    let mut observer = Namespace::open(&root.join("observer"), &s.observer).unwrap();
    let mut registry = Namespace::open(&root.join("registry"), &s.registry).unwrap();
    let mut metadata = MetadataStore::open(&root.join("metadata"), &s.metadata).unwrap();
    let stop = std::env::var("GIT_ASSET_AGE_EXIT_STAGE").unwrap();
    Capture {
        enrollment: &s.e,
        registry: &mut registry,
        minimum: &s.registry,
        metadata: &mut metadata,
        limits: &Limits::default(),
    }
    .capture_hook(
        &s.request,
        &mut observer,
        &s.observer,
        &TestClock::new(&s.e.clock, 100, 110),
        |stage| {
            if format!("{stage:?}") == stop {
                std::process::exit(79)
            }
            Ok(())
        },
    )
    .unwrap();
    panic!("exit stage missing")
}
#[test]
fn actual_process_exits_preserve_acked_age_or_retry_conservatively() {
    for stage in [
        Stage::Observed,
        Stage::Sampled,
        Stage::MetadataFinished,
        Stage::RegistryAdopted,
        Stage::BeforeRetire,
        Stage::Retired,
    ] {
        let mut f = Fixture::new();
        let s = Saved {
            e: f.e.clone(),
            request: f.request.clone(),
            observer: f.observer.checkpoint(),
            registry: f.registry.checkpoint(),
            metadata: f.metadata.checkpoint(),
        };
        fs::write(
            f.temp.path().join("saved.json"),
            serde_json::to_vec(&s).unwrap(),
        )
        .unwrap();
        let out = Process::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "source_claims::age::tests::abrupt_exit_helper",
                "--nocapture",
            ])
            .env("GIT_ASSET_AGE_EXIT_ROOT", f.temp.path())
            .env("GIT_ASSET_AGE_EXIT_STAGE", format!("{stage:?}"))
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(79),
            "{stage:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        f.reopen();
        let clock = TestClock::new(&f.e.clock, 200, 210);
        let replay = f.capture(&clock).unwrap();
        let adopted = matches!(
            stage,
            Stage::RegistryAdopted | Stage::BeforeRetire | Stage::Retired
        );
        assert_eq!(clock.calls.get(), usize::from(!adopted));
        assert_eq!(
            replay.observed,
            Some(if adopted {
                Stamp {
                    earliest: 100,
                    latest: 110,
                }
            } else {
                Stamp {
                    earliest: 200,
                    latest: 210,
                }
            })
        );
        assert!(
            f.registry
                .source(&f.e.source)
                .unwrap()
                .unwrap()
                .predecessors
                .is_empty()
        );
        assert_eq!(s.observer, f.observer.checkpoint());
    }
}
