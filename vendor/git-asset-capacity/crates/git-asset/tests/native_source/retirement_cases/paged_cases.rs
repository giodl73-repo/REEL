use super::*;
#[path = "paged_cases/operator.rs"]
mod operator;
use git_asset::source::native::retirement::paged::{self as native, Occurrence, Session};
use git_asset_store::{
    final_retirement::{claims, paged as selection, provider},
    source_claims::age,
};
use std::cell::Cell;

struct Clock(Cell<u64>);
impl provider::Clock for Clock {
    fn now(&self, id: &Digest) -> provider::Result<u64> {
        if id == &d("clock") {
            Ok(self.0.get())
        } else {
            Err(provider::Error::Clock)
        }
    }
}
fn digest(v: &impl serde::Serialize) -> Digest {
    Digest::blob(&identity::canonical_json(&serde_json::to_value(v).unwrap()).unwrap())
}
fn ages(
    f: &mut Setup,
    recent: bool,
    capture: bool,
) -> (Namespace, source_claims::Checkpoint, age::Enrollment) {
    let mut registry = Namespace::create(
        &f.temp.path().join("ages"),
        key("ages"),
        key("epoch"),
        f.namespaces[0].policy().clone(),
    )
    .unwrap();
    registry
        .apply(
            key("enroll-records"),
            SourceCommand::Enroll {
                source: key("records"),
                owner: key("owner"),
                class: Kind::Retention,
            },
            &f.metadata,
            &protection_limits(),
        )
        .unwrap();
    let minimum = registry.checkpoint();
    let enrolled = age::Enrollment {
        authority: d("final-authority"),
        epoch: d("epoch"),
        metadata: f.enrollment.metadata.clone(),
        registry: registry.identity().clone(),
        registry_policy: digest(registry.policy()),
        source: key("records"),
        owner: key("owner"),
        clock: d("clock"),
        observers: vec![age::Observer {
            namespace: f.namespaces[0].identity().clone(),
            policy: digest(f.namespaces[0].policy()),
            owners: vec![key("owner")],
            classes: [Kind::Branch].into_iter().collect(),
        }],
    };
    let limits = age::Limits::default();
    age::Capture {
        enrollment: &enrolled,
        registry: &mut registry,
        minimum: &minimum,
        metadata: &mut f.metadata,
        limits: &limits,
    }
    .initialize()
    .unwrap();
    if capture {
        let mut order = (0..f.roots.len()).collect::<Vec<_>>();
        if recent {
            order.rotate_left(1);
        }
        for i in order {
            let r = &f.roots[i];
            let clock = Clock(Cell::new(if recent && i == 0 { 90 } else { 10 }));
            let min = f.namespaces[0].checkpoint();
            age::Capture {
                enrollment: &enrolled,
                registry: &mut registry,
                minimum: &minimum,
                metadata: &mut f.metadata,
                limits: &limits,
            }
            .capture(
                &age::Request {
                    namespace: r.namespace.clone(),
                    source: r.source.clone(),
                    predecessor_revision: r.predecessor_revision,
                },
                &mut f.namespaces[0],
                &min,
                &clock,
            )
            .unwrap();
        }
    }
    f.enrollment.age = Some(native::AgePolicy {
        enrollment: enrolled.clone(),
        required_age_millis: 50,
    });
    (registry, minimum, enrolled)
}
struct Setup {
    _fixture: Fixture,
    snapshot: AssetSnapshot,
    temp: tempfile::TempDir,
    metadata: MetadataStore,
    namespaces: Vec<Namespace>,
    roots: Vec<Occurrence>,
    enrollment: native::Enrollment,
}
impl Setup {
    fn new(count: usize) -> Self {
        let (f, mut snapshot) = fixture();
        let temp = tempfile::tempdir().unwrap();
        let mut metadata = metadata(&temp.path().join("metadata"));
        let names = roots(&snapshot);
        let original = retirement::build(
            &mut snapshot,
            names,
            profiles(&f),
            &mut metadata,
            BindingLimits::default(),
        )
        .unwrap();
        metadata.finish().unwrap();
        let mut ns = namespace(&temp.path().join("namespace"), &metadata, &original);
        let mut occurrences = vec![Occurrence {
            namespace: ns.identity().clone(),
            source: key("main"),
            predecessor_revision: 3,
            header: original.context.clone(),
        }];
        for i in 1..count {
            let revision = ns.source(&key("main")).unwrap().unwrap().revision;
            ns.select(
                &Select {
                    prepare: key(&format!("prepare-{i}")),
                    adopt: key(&format!("adopt-{i}")),
                    source: key("main"),
                    owner: key("owner"),
                    expected_revision: revision,
                    selection: original.clone(),
                },
                &metadata,
                &protection_limits(),
            )
            .unwrap();
            let revision = ns.source(&key("main")).unwrap().unwrap().revision;
            ns.apply(
                key(&format!("withdraw-{i}")),
                SourceCommand::Withdraw {
                    source: key("main"),
                    owner: key("owner"),
                    expected_revision: revision,
                },
                &metadata,
                &protection_limits(),
            )
            .unwrap();
            occurrences.push(Occurrence {
                namespace: ns.identity().clone(),
                source: key("main"),
                predecessor_revision: revision,
                header: original.context.clone(),
            });
        }
        let e = native::Enrollment {
            metadata: metadata.checkpoint().identity().clone(),
            owner: key("owner"),
            categories: category(&snapshot),
            age: None,
        };
        Self {
            _fixture: f,
            snapshot,
            temp,
            metadata,
            namespaces: vec![ns],
            roots: occurrences,
            enrollment: e,
        }
    }
    fn session(&self) -> Session {
        Session::new(BindingLimits::default()).unwrap()
    }
    fn input(&mut self) -> Object {
        let session = self.session();
        let midpoint = self.roots.len().div_ceil(2);
        let first = native::occurrences(
            None,
            self.roots[..midpoint].to_vec(),
            &mut self.metadata,
            &session,
        )
        .unwrap();
        let result = if midpoint == self.roots.len() {
            first
        } else {
            native::occurrences(
                Some(&first),
                self.roots[midpoint..].to_vec(),
                &mut self.metadata,
                &session,
            )
            .unwrap()
        };
        self.metadata.finish().unwrap();
        result
    }
    fn select(&mut self, input: &Object, chosen: &Filter) -> native::Candidate {
        let session = self.session();
        native::select(
            &self.enrollment,
            input,
            chosen,
            selection::Paging {
                max_bindings: 2,
                ..Default::default()
            },
            &mut self.namespaces,
            &mut self.metadata,
            None,
            &session,
        )
        .unwrap()
    }
}
#[test]
fn seventeen_native_roots_stage_complete_exact_requests_and_read_back_without_git() {
    let mut f = Setup::new(17);
    let input = f.input();
    let chosen = filter(&f.snapshot, &["asset/c"]);
    let p = f.select(&input, &chosen);
    let minimum = f.metadata.finish().unwrap();
    let session = f.session();
    let mut reader = MetadataReader::open(&f.temp.path().join("metadata"), &minimum).unwrap();
    let inspected =
        native::validate_current(&f.enrollment, &p, &mut f.namespaces, &mut reader, &session)
            .unwrap();
    assert_eq!(inspected.summary().roots, 17);
    assert_eq!(inspected.summary().selected_roots, 17);
    assert_eq!(inspected.summary().matched_assets, 17);
    assert!(!inspected.summary().authorizes_removal);
    let (_, requested) = selection::read_request(
        p.request().unwrap(),
        &mut reader,
        &selection::Limits::default(),
    )
    .unwrap();
    assert_eq!(requested.len(), 17);
    for s in requested {
        let map = Tree::from_root(s.bindings)
            .entries(&mut reader, TreeLimits::default())
            .unwrap()
            .0;
        assert!(map.values().any(|o| *o == Object::from_bytes(b"unique")));
        assert!(!map.values().any(|o| *o == Object::from_bytes(b"shared")));
    }
    let before = inspected
        .rows()
        .map(|r| r.source_checkpoint.clone())
        .collect::<Vec<_>>();
    std::fs::rename(
        f._fixture.root().join(".git"),
        f._fixture.root().join("offline-git"),
    )
    .unwrap();
    let detached = native::inspect(&f.enrollment, &p, &mut reader, &f.session()).unwrap();
    assert_eq!(
        before,
        detached
            .rows()
            .map(|r| r.source_checkpoint.clone())
            .collect::<Vec<_>>()
    );
}
#[test]
fn metadata_index_conflicts_source_drift_and_unknown_global_assets_refuse() {
    let mut f = Setup::new(2);
    let input = f.input();
    let mut conflicting = f.roots[0].clone();
    conflicting.header = d("wrong-header");
    assert!(
        native::occurrences(
            Some(&input),
            vec![conflicting],
            &mut f.metadata,
            &Session::new(BindingLimits::default()).unwrap()
        )
        .is_err()
    );
    let chosen = filter(&f.snapshot, &["asset/c"]);
    let p = f.select(&input, &chosen);
    f.metadata.finish().unwrap();
    let absent = Filter {
        assets: BTreeSet::from([Reference {
            id: "absent".into(),
            object: Object::from_bytes(b"absent"),
        }]),
        categories: BTreeSet::new(),
    };
    assert!(
        native::select(
            &f.enrollment,
            &input,
            &absent,
            selection::Paging::default(),
            &mut f.namespaces,
            &mut f.metadata,
            None,
            &Session::new(BindingLimits::default()).unwrap()
        )
        .is_err()
    );
    f.metadata.finish().unwrap();
    f.namespaces[0]
        .apply(
            key("new-job"),
            SourceCommand::Enroll {
                source: key("job"),
                owner: key("owner"),
                class: Kind::Job,
            },
            &f.metadata,
            &protection_limits(),
        )
        .unwrap();
    assert!(
        native::validate_current(
            &f.enrollment,
            &p,
            &mut f.namespaces,
            &mut f.metadata.finished_reader().unwrap(),
            &Session::new(BindingLimits::default()).unwrap()
        )
        .is_err()
    );
}

#[test]
fn exact_block_spans_different_snapshots_and_request_substitution_is_rejected() {
    let mut f = Setup::new(1);
    f._fixture
        .record("d", "asset/d", true, json!([]), Some(b"later"));
    let mut newer = capture(&reader(f._fixture.root()), commit(f._fixture.root()));
    let dref = newer.resolve_id("asset/d").unwrap().reference().clone();
    let root = retirement::build(
        &mut newer,
        BTreeSet::from([dref.clone()]),
        profiles(&f._fixture),
        &mut f.metadata,
        BindingLimits::default(),
    )
    .unwrap();
    f.metadata.finish().unwrap();
    f.namespaces[0]
        .select(
            &Select {
                prepare: key("prepare-later"),
                adopt: key("adopt-later"),
                source: key("main"),
                owner: key("owner"),
                expected_revision: 4,
                selection: root.clone(),
            },
            &f.metadata,
            &protection_limits(),
        )
        .unwrap();
    f.namespaces[0]
        .apply(
            key("withdraw-later"),
            SourceCommand::Withdraw {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 6,
            },
            &f.metadata,
            &protection_limits(),
        )
        .unwrap();
    f.roots.push(Occurrence {
        namespace: f.namespaces[0].identity().clone(),
        source: key("main"),
        predecessor_revision: 6,
        header: root.context,
    });
    let input = f.input();
    let aref = f
        .snapshot
        .resolve_id("asset/a")
        .unwrap()
        .reference()
        .clone();
    let chosen = Filter {
        assets: BTreeSet::from([aref, dref]),
        categories: BTreeSet::new(),
    };
    let p = f.select(&input, &chosen);
    f.metadata.finish().unwrap();
    let session = f.session();
    let inspected = native::inspect(
        &f.enrollment,
        &p,
        &mut f.metadata.finished_reader().unwrap(),
        &session,
    )
    .unwrap();
    assert_eq!(inspected.summary().matched_assets, 2);
    assert_eq!(inspected.summary().selected_roots, 2);
    let only_c = filter(&f.snapshot, &["asset/c"]);
    let other = f.select(&input, &only_c);
    f.metadata.finish().unwrap();
    let mut forged = serde_json::to_value(&p).unwrap();
    forged["request"] = serde_json::to_value(other.request()).unwrap();
    let forged = native::Candidate::from_json(&identity::canonical_json(&forged).unwrap()).unwrap();
    assert!(
        native::inspect(
            &f.enrollment,
            &forged,
            &mut f.metadata.finished_reader().unwrap(),
            &f.session()
        )
        .is_err()
    );
}

#[test]
fn a_peer_source_change_during_native_reads_invalidates_the_candidate() {
    use git_asset_core::tree::{NodeStore, TreeResult};
    struct Race<'a> {
        metadata: &'a mut MetadataStore,
        peer: std::cell::RefCell<Namespace>,
        once: Cell<bool>,
    }
    impl NodeStore for Race<'_> {
        fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
            if !self.once.replace(true) {
                self.peer
                    .borrow_mut()
                    .apply(
                        key("concurrent-job"),
                        SourceCommand::Enroll {
                            source: key("job"),
                            owner: key("owner"),
                            class: Kind::Job,
                        },
                        self.metadata,
                        &protection_limits(),
                    )
                    .unwrap();
            }
            self.metadata.get(o)
        }
        fn put(&mut self, b: &[u8]) -> TreeResult<Object> {
            self.metadata.put(b)
        }
    }
    let mut f = Setup::new(2);
    let input = f.input();
    let peer = Namespace::open(
        &f.temp.path().join("namespace"),
        &f.namespaces[0].checkpoint(),
    )
    .unwrap();
    let chosen = filter(&f.snapshot, &["asset/c"]);
    let session = f.session();
    let mut raced = Race {
        metadata: &mut f.metadata,
        peer: std::cell::RefCell::new(peer),
        once: Cell::new(false),
    };
    assert!(
        native::select(
            &f.enrollment,
            &input,
            &chosen,
            selection::Paging::default(),
            &mut f.namespaces,
            &mut raced,
            None,
            &session
        )
        .is_err()
    );
    assert!(raced.once.get());
}

#[test]
fn conflicting_payload_sizes_across_historical_namespaces_refuse_native_candidates() {
    let mut f = Setup::new(1);
    f._fixture
        .record("d", "asset/d", true, json!([]), Some(b"shared"));
    let record_path = f._fixture.root().join("records/d.asset");
    let mut raw: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    raw["selected"]["bytes"] = json!(7);
    std::fs::write(&record_path, serde_json::to_vec(&raw).unwrap()).unwrap();
    let mut newer = capture(&reader(f._fixture.root()), commit(f._fixture.root()));
    let dref = newer.resolve_id("asset/d").unwrap().reference().clone();
    let second = retirement::build(
        &mut newer,
        BTreeSet::from([dref.clone()]),
        profiles(&f._fixture),
        &mut f.metadata,
        BindingLimits::default(),
    )
    .unwrap();
    f.metadata.finish().unwrap();
    let mut ns = Namespace::create(
        &f.temp.path().join("second-namespace"),
        key("second-project"),
        key("epoch"),
        f.namespaces[0].policy().clone(),
    )
    .unwrap();
    ns.apply(
        key("enroll"),
        SourceCommand::Enroll {
            source: key("main"),
            owner: key("owner"),
            class: Kind::Branch,
        },
        &f.metadata,
        &protection_limits(),
    )
    .unwrap();
    ns.select(
        &Select {
            prepare: key("prepare"),
            adopt: key("adopt"),
            source: key("main"),
            owner: key("owner"),
            expected_revision: 1,
            selection: second.clone(),
        },
        &f.metadata,
        &protection_limits(),
    )
    .unwrap();
    ns.apply(
        key("withdraw"),
        SourceCommand::Withdraw {
            source: key("main"),
            owner: key("owner"),
            expected_revision: 3,
        },
        &f.metadata,
        &protection_limits(),
    )
    .unwrap();
    f.roots.push(Occurrence {
        namespace: ns.identity().clone(),
        source: key("main"),
        predecessor_revision: 3,
        header: second.context,
    });
    f.namespaces.push(ns);
    let input = f.input();
    let chosen = Filter {
        assets: BTreeSet::from([
            f.snapshot
                .resolve_id("asset/a")
                .unwrap()
                .reference()
                .clone(),
            dref,
        ]),
        categories: BTreeSet::new(),
    };
    let before = f
        .namespaces
        .iter()
        .map(Namespace::checkpoint)
        .collect::<Vec<_>>();
    assert!(matches!(
        native::select(
            &f.enrollment,
            &input,
            &chosen,
            selection::Paging::default(),
            &mut f.namespaces,
            &mut f.metadata,
            None,
            &Session::new(BindingLimits::default()).unwrap()
        ),
        Err(retirement::Error::Integrity)
    ));
    assert_eq!(
        before,
        f.namespaces
            .iter()
            .map(Namespace::checkpoint)
            .collect::<Vec<_>>()
    );
}

#[test]
fn unknown_and_recent_age_remain_held_rows_and_cannot_grow_an_old_request() {
    let mut f = Setup::new(17);
    let input = f.input();
    let chosen = filter(&f.snapshot, &["asset/c"]);
    let (mut registry, minimum, e) = ages(&mut f, false, false);
    let clock = Clock(Cell::new(100));
    let limits = age::Limits::default();
    let session = f.session();
    let none = native::select(
        &f.enrollment,
        &input,
        &chosen,
        selection::Paging::default(),
        &mut f.namespaces,
        &mut f.metadata,
        Some(&mut native::AgeContext {
            registry: &mut registry,
            minimum: &minimum,
            clock: &clock,
            limits: &limits,
        }),
        &session,
    )
    .unwrap();
    f.metadata.finish().unwrap();
    assert!(none.request().is_none());
    let inspected = native::inspect(
        &f.enrollment,
        &none,
        &mut f.metadata.finished_reader().unwrap(),
        &session,
    )
    .unwrap();
    assert_eq!(inspected.summary().held_roots, 17);
    assert!(
        inspected
            .rows()
            .all(|r| r.hold == Some(native::Hold::AgeUnknown))
    );
    for i in (1..17).chain(std::iter::once(0)) {
        clock.0.set(if i == 0 { 90 } else { 10 });
        let min = f.namespaces[0].checkpoint();
        let r = &f.roots[i];
        age::Capture {
            enrollment: &e,
            registry: &mut registry,
            minimum: &minimum,
            metadata: &mut f.metadata,
            limits: &limits,
        }
        .capture(
            &age::Request {
                namespace: r.namespace.clone(),
                source: r.source.clone(),
                predecessor_revision: r.predecessor_revision,
            },
            &mut f.namespaces[0],
            &min,
            &clock,
        )
        .unwrap();
    }
    clock.0.set(100);
    let session = f.session();
    let p = native::select(
        &f.enrollment,
        &input,
        &chosen,
        selection::Paging::default(),
        &mut f.namespaces,
        &mut f.metadata,
        Some(&mut native::AgeContext {
            registry: &mut registry,
            minimum: &minimum,
            clock: &clock,
            limits: &limits,
        }),
        &session,
    )
    .unwrap();
    f.metadata.finish().unwrap();
    let inspected = native::inspect(
        &f.enrollment,
        &p,
        &mut f.metadata.finished_reader().unwrap(),
        &session,
    )
    .unwrap();
    assert_eq!(inspected.summary().selected_roots, 16);
    assert_eq!(inspected.summary().held_roots, 1);
    assert!(
        inspected
            .rows()
            .any(|r| r.occurrence == f.roots[0] && r.hold == Some(native::Hold::AgeRecent))
    );
    clock.0.set(200);
    let unchanged = native::inspect(
        &f.enrollment,
        &p,
        &mut f.metadata.finished_reader().unwrap(),
        &f.session(),
    )
    .unwrap();
    assert_eq!(unchanged.summary().selected_roots, 16);
    assert!(
        native::inspect(
            &f.enrollment,
            &none,
            &mut f.metadata.finished_reader().unwrap(),
            &f.session()
        )
        .unwrap()
        .summary()
        .request
        .is_none()
    );
    // Even after the recent root ages, it was excluded from this request and
    // still protects the shared object in the complete-source projection.
    let roster = claims::Roster::flat(
        d("final-authority"),
        d("epoch"),
        Some(d("complete-fixture-scope")),
        vec![claims::NamespacePin::current(&f.namespaces[0]).unwrap()],
        protection_limits(),
    )
    .unwrap();
    let enrolled = selection::Enrollment {
        authority: d("final-authority"),
        epoch: d("epoch"),
        owner: key("owner"),
        metadata: f.enrollment.metadata.clone(),
        scope: roster.scope().unwrap(),
        classes: [Kind::Branch].into_iter().collect(),
    };
    let limits = selection::Limits::default();
    let manifest = selection::build(
        &enrolled,
        p.request().unwrap(),
        &roster,
        &mut f.namespaces,
        &mut f.metadata,
        &limits,
    )
    .unwrap();
    f.metadata.finish().unwrap();
    let projected = selection::inspect(
        &enrolled,
        &manifest,
        &mut f.metadata.finished_reader().unwrap(),
        &limits,
    )
    .unwrap();
    assert!(
        projected
            .subjects()
            .any(|s| s.object == Object::from_bytes(b"unique") && s.held_by_surviving_claims)
    );
}

#[test]
fn native_seventeen_root_age_block_executes_paged_claims_and_only_eligible_copy_loss() {
    use git_asset_store::{
        AckLevel, Request, accounted,
        final_retirement::{bulk, paged::control},
        ledger::CapacityLedger,
        local::LocalStore,
        local_capacity::LocalCapacityProbe,
        orchestration::Place,
        placement::Local,
        retention,
        store_protection::{Binding as StoreBinding, Member},
    };
    let mut f = Setup::new(17);
    let input = f.input();
    let chosen = filter(&f.snapshot, &["asset/a", "asset/c"]);
    let (mut registry, minimum, age_enrollment) = ages(&mut f, false, true);
    let clock = Clock(Cell::new(100));
    let age_limits = age::Limits::default();
    let session = f.session();
    let candidate = native::select(
        &f.enrollment,
        &input,
        &chosen,
        selection::Paging {
            max_bindings: 4,
            ..Default::default()
        },
        &mut f.namespaces,
        &mut f.metadata,
        Some(&mut native::AgeContext {
            registry: &mut registry,
            minimum: &minimum,
            clock: &clock,
            limits: &age_limits,
        }),
        &session,
    )
    .unwrap();
    f.metadata.finish().unwrap();
    native::validate_current(
        &f.enrollment,
        &candidate,
        &mut f.namespaces,
        &mut f.metadata.finished_reader().unwrap(),
        &session,
    )
    .unwrap();
    let roster = claims::Roster::flat(
        d("final-authority"),
        d("epoch"),
        Some(d("owned-complete-scope")),
        vec![claims::NamespacePin::current(&f.namespaces[0]).unwrap()],
        protection_limits(),
    )
    .unwrap();
    let store = LocalStore::create_final(
        &f.temp.path().join("M"),
        key("M"),
        StoreBinding::new(
            1,
            d("fixture-boundary"),
            vec![Member {
                namespace: f.namespaces[0].identity().clone(),
                places: vec![key("M")],
            }],
        )
        .unwrap(),
        provider::Policy::new(
            d("final-authority"),
            d("epoch"),
            roster.scope().unwrap(),
            d("clock"),
            vec![key("owner")],
            1000,
        )
        .unwrap(),
    )
    .unwrap();
    let mut ledger = CapacityLedger::create(
        &f.temp.path().join("ledger"),
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
        .reserve(key("job"), key("owner"), 128 * 1024 * 1024)
        .unwrap()
        .reference()
        .clone();
    let mut copies = Vec::new();
    for name in ["shared", "unique"] {
        let publication = Request {
            operation: key(&format!("publish-{name}")),
            owner: key("owner"),
            object: Object::from_bytes(name.as_bytes()),
            required: AckLevel::ProcessRestart,
        };
        let plan = accounted::plan(&store, &publication).unwrap();
        accounted::publish_accounted(
            &mut ledger,
            &store,
            &job,
            &plan,
            &mut std::io::Cursor::new(name.as_bytes()),
        )
        .unwrap();
        let grant = retention::plan(
            &store,
            &retention::RetentionRequest {
                operation: key(&format!("retain-{name}")),
                owner: key("owner"),
                publication: publication.clone(),
                minimum_ack: AckLevel::ProcessRestart,
            },
        )
        .unwrap();
        retention::retain(&mut ledger, &store, &job, &grant).unwrap();
        copies.push(bulk::Copy {
            store: store.identity().clone(),
            capacity: ledger.identity().clone(),
            publication,
        });
    }
    let witness = LocalStore::create(&f.temp.path().join("witness"), key("witness")).unwrap();
    let mut witness_ledger = CapacityLedger::create(
        &f.temp.path().join("witness-ledger"),
        key("witness-capacity"),
        key("epoch"),
        0,
    )
    .unwrap();
    LocalCapacityProbe::bind(&witness, &mut witness_ledger)
        .unwrap()
        .observe(&witness, &mut witness_ledger, key("probe"), key("sample"))
        .unwrap();
    let witness_job = witness_ledger
        .reserve(key("job"), key("owner"), 512 * 1024 * 1024)
        .unwrap()
        .reference()
        .clone();
    let limits = selection::Limits::default();
    let control = control::Enrollment {
        age: Some(control::aged::Policy {
            enrollment: age_enrollment,
            required_age_millis: 50,
            metadata_boundary: d("admitted-metadata-only"),
        }),
        selection: selection::Enrollment {
            authority: d("final-authority"),
            epoch: d("epoch"),
            owner: key("owner"),
            metadata: f.enrollment.metadata.clone(),
            scope: roster.scope().unwrap(),
            classes: [Kind::Branch].into_iter().collect(),
        },
        sources: vec![source_claims::subset::Enrollment {
            authority: d("final-authority"),
            namespace: f.namespaces[0].identity().clone(),
            policy: digest(f.namespaces[0].policy()),
            owner: key("owner"),
            classes: vec![Kind::Branch],
            limits: protection_limits(),
            writes: 10000,
            write_bytes: 8 * 1024 * 1024,
        }],
        context: d("native-paged-context"),
        clock: d("clock"),
        witness: Place::local(&witness, 1).unwrap(),
        capacity: witness_ledger.identity().clone(),
        max_window: 1000,
        max_metadata_bytes: 512 * 1024 * 1024,
    };
    let manifest = selection::build(
        &control.selection,
        candidate.request().unwrap(),
        &roster,
        &mut f.namespaces,
        &mut f.metadata,
        &limits,
    )
    .unwrap();
    let copies = control::copies(copies, &mut f.metadata, &limits).unwrap();
    f.metadata.finish().unwrap();
    let p = control::aged::plan(
        &control,
        control::Request {
            operation: key("native-block"),
            manifest,
            copies,
            window: provider::Window {
                not_before: 50,
                expires: 500,
            },
        },
        &roster,
        &mut f.namespaces,
        &mut f.metadata,
        &[&store],
        &limits,
        &mut control::aged::Context {
            registry: &mut registry,
            minimum: &minimum,
            limits: &age_limits,
        },
        &clock,
    )
    .unwrap();
    f.metadata.finish().unwrap();
    let jobs = [control::StoreJob {
        store: store.identity().clone(),
        job: job.clone(),
    }];
    let mut completed_pages = 0;
    loop {
        let mut places = [Local {
            store: &store,
            ledger: &mut ledger,
            job: &job,
        }];
        let report = control::Execution {
            enrollment: &control,
            roster: &roster,
            namespaces: &mut f.namespaces,
            metadata: &mut f.metadata,
            places: &mut places,
            jobs: &jobs,
            witness: Local {
                store: &witness,
                ledger: &mut witness_ledger,
                job: &witness_job,
            },
            clock: &clock,
            admitted_context: &control.context,
            limits: &limits,
            run_limits: control::RunLimits::default(),
        }
        .apply_aged(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
            control::aged::Context {
                registry: &mut registry,
                minimum: &minimum,
                limits: &age_limits,
            },
            |_| Ok(()),
        )
        .unwrap();
        assert!(report.completed_pages.unwrap() >= completed_pages);
        completed_pages = report.completed_pages.unwrap();
        if report.phase == control::Phase::Completed {
            assert_eq!(report.historical_deleted_actions, Some(1));
            break;
        }
    }
    assert!(completed_pages > 16);
    assert!(store.read_object(&Object::from_bytes(b"unique")).is_err());
    let mut bytes = Vec::new();
    store
        .read_object(&Object::from_bytes(b"shared"))
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, b"shared");
}
#[test]
fn cli_index_and_selection_are_disabled_by_default_and_finish_readback_before_reporting() {
    let f = Setup::new(17);
    let root = f.temp.path();
    let mut profile = json!({"schema":"tig.native-paged-profile.design-1","enrollment":f.enrollment,
        "metadata":{"root":root.join("metadata"),"minimum":f.metadata.checkpoint()},
        "namespaces":[{"root":root.join("namespace"),"minimum":f.namespaces[0].checkpoint()}]});
    let profile_path = root.join("paged-profile.json");
    let input_path = root.join("paged-input.json");
    std::fs::write(&profile_path, serde_json::to_vec(&profile).unwrap()).unwrap();
    std::fs::write(
        &input_path,
        serde_json::to_vec(
            &json!({"schema":"tig.native-paged-index-request.design-1","occurrences":f.roots}),
        )
        .unwrap(),
    )
    .unwrap();
    let invoke = |command| {
        std::process::Command::new(env!("CARGO_BIN_EXE_git-asset-retirement"))
            .args([
                command,
                profile_path.to_str().unwrap(),
                input_path.to_str().unwrap(),
            ])
            .output()
            .unwrap()
    };
    let denied = invoke("index-paged");
    assert!(!denied.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&denied.stdout).unwrap()["code"],
        "staging_disabled"
    );
    profile["allow_stage"] = json!(true);
    std::fs::write(&profile_path, serde_json::to_vec(&profile).unwrap()).unwrap();
    let indexed = invoke("index-paged");
    assert!(
        indexed.status.success(),
        "{}",
        String::from_utf8_lossy(&indexed.stdout)
    );
    let indexed: serde_json::Value = serde_json::from_slice(&indexed.stdout).unwrap();
    assert_eq!(indexed["count"], 17);
    profile["metadata"]["minimum"] = indexed["metadata_checkpoint"].clone();
    std::fs::write(&profile_path, serde_json::to_vec(&profile).unwrap()).unwrap();
    std::fs::write(
        &input_path,
        serde_json::to_vec(&json!({"schema":"tig.native-paged-select-request.design-1",
        "occurrences":indexed["occurrences"],"filter":{"assets":[],"categories":["drawing"]}}))
        .unwrap(),
    )
    .unwrap();
    let selected = invoke("select-paged");
    assert!(
        selected.status.success(),
        "{}",
        String::from_utf8_lossy(&selected.stdout)
    );
    assert!(selected.stdout.len() < 65536);
    let selected: serde_json::Value = serde_json::from_slice(&selected.stdout).unwrap();
    assert_eq!(selected["preview"]["selected_roots"], 17);
    assert_eq!(selected["authorizes_removal"], false);
    let candidate: native::Candidate =
        serde_json::from_value(selected["candidate"].clone()).unwrap();
    let min = serde_json::from_value(selected["metadata_checkpoint"].clone()).unwrap();
    let mut reader = MetadataReader::open(&root.join("metadata"), &min).unwrap();
    assert_eq!(
        native::inspect(&f.enrollment, &candidate, &mut reader, &f.session())
            .unwrap()
            .summary()
            .roots,
        17
    );
}
