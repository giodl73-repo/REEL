use super::*;
use crate::{
    AckLevel, Request, publish,
    source_claims::{Command, Kind, Places, Selection, admission::Policy},
};
use git_asset_core::{
    LogicalPath,
    tree::{MemoryNodes, Tree, TreeLimits, TreeResult},
};
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn digest(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn limits() -> Limits {
    Limits {
        millis: 60_000,
        roots: 32,
        bindings: 1000,
        associations: 10000,
        reads: 10000,
        read_bytes: 16 * 1024 * 1024,
    }
}
fn policy() -> Policy {
    let places = Places {
        current: vec![key("working"), key("backup")],
        pending: vec![key("working"), key("backup")],
        predecessor: vec![key("history"), key("backup")],
    };
    Policy {
        prefer_active: true,
        classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
            .into_iter()
            .map(|k| (k, places.clone()))
            .collect(),
    }
}
fn tree(nodes: &mut impl NodeStore, object: &Object) -> Selection {
    let (tree, _) = Tree::build(
        &[(LogicalPath::parse("item").unwrap(), object.clone())]
            .into_iter()
            .collect(),
        nodes,
        TreeLimits::default(),
    )
    .unwrap();
    Selection {
        root: tree.root().clone(),
        context: digest("context"),
    }
}
fn prepare(
    namespace: &mut Namespace,
    operation: &str,
    selection: Selection,
    nodes: &impl NodeStore,
) -> crate::source_claims::Receipt {
    let revision = namespace.source(&key("main")).unwrap().unwrap().revision;
    namespace
        .apply(
            key(operation),
            Command::Prepare {
                source: key("main"),
                owner: key("owner"),
                expected_revision: revision,
                selection,
            },
            nodes,
            &limits(),
        )
        .unwrap()
}
struct Fixture {
    root: tempfile::TempDir,
    store: LocalStore,
    enrollment: Enrollment,
    namespaces: Vec<Namespace>,
    nodes: MemoryNodes,
    object: Object,
    selection: Selection,
}
impl Fixture {
    fn new() -> Self {
        Self::places(vec![key("working")])
    }
    fn places(places: Vec<Key>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut namespaces = Vec::new();
        for n in 0..2 {
            let mut namespace = Namespace::create(
                &root.path().join(format!("namespace-{n}")),
                key(&format!("project-{n}")),
                key("epoch"),
                policy(),
            )
            .unwrap();
            namespace
                .apply(
                    key("enroll"),
                    Command::Enroll {
                        source: key("main"),
                        owner: key("owner"),
                        class: Kind::Branch,
                    },
                    &MemoryNodes::default(),
                    &limits(),
                )
                .unwrap();
            namespaces.push(namespace);
        }
        let binding = Binding::new(
            7,
            digest("admitted-managed-writers"),
            namespaces
                .iter()
                .map(|n| Member {
                    namespace: n.identity().clone(),
                    places: places.clone(),
                })
                .collect(),
        )
        .unwrap();
        let mut store = LocalStore::create_protected(
            &root.path().join("store"),
            key("working-provider"),
            binding,
        )
        .unwrap();
        let object = Object::from_bytes(b"target");
        publish(
            &mut store,
            &Request {
                operation: key("initial"),
                owner: key("owner"),
                object: object.clone(),
                required: AckLevel::ProcessRestart,
            },
            &mut Cursor::new(b"target"),
        )
        .unwrap();
        let enrollment = Enrollment::bind(&store).unwrap();
        let mut nodes = MemoryNodes::default();
        let selection = tree(&mut nodes, &object);
        Self {
            root,
            store,
            enrollment,
            namespaces,
            nodes,
            object,
            selection,
        }
    }
    fn path(&self) -> PathBuf {
        self.root.path().join("store")
    }
    fn peer(&self, index: usize) -> Namespace {
        Namespace::open(
            &self.root.path().join(format!("namespace-{index}")),
            &self.namespaces[index].checkpoint(),
        )
        .unwrap()
    }
    fn assess(&mut self) -> Result<Assessment<'_>> {
        assess(
            &self.store,
            &self.enrollment,
            &self.object,
            &mut self.namespaces,
            &self.nodes,
            &limits(),
        )
    }
}
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(root: &Path, path: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for item in fs::read_dir(path).unwrap() {
            let path = item.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push((
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
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
fn binding_normalization_bounds_and_identity_preserve_every_namespace_and_alias() {
    let a = Member {
        namespace: digest("a"),
        places: vec![key("working"), key("backup")],
    };
    let b = Member {
        namespace: digest("b"),
        places: vec![key("working")],
    };
    let one = Binding::new(1, digest("boundary"), vec![a.clone(), b.clone()]).unwrap();
    let mut reversed = a.clone();
    reversed.places.reverse();
    assert_eq!(
        one,
        Binding::new(1, digest("boundary"), vec![b.clone(), reversed]).unwrap()
    );
    assert_ne!(
        one.id().unwrap(),
        Binding::new(2, digest("boundary"), vec![a.clone(), b.clone()])
            .unwrap()
            .id()
            .unwrap()
    );
    assert_ne!(
        one.id().unwrap(),
        Binding::new(1, digest("other"), vec![a.clone(), b])
            .unwrap()
            .id()
            .unwrap()
    );
    assert!(Binding::new(0, digest("boundary"), vec![a.clone()]).is_err());
    assert!(Binding::new(1, digest("boundary"), vec![]).is_err());
    assert!(Binding::new(1, digest("boundary"), vec![a.clone(), a.clone()]).is_err());
    assert!(
        Binding::new(
            1,
            digest("boundary"),
            vec![Member {
                namespace: a.namespace.clone(),
                places: vec![key("working"), key("working")]
            }]
        )
        .is_err()
    );
    assert!(Binding::new(1, digest("boundary"), vec![a; 17]).is_err());
}

#[test]
fn independently_pinned_roster_binding_preserves_legacy_framing_and_rejects_unknown_scope() {
    // Independent Python json(sort_keys,separators) + hashlib SHA-256 outputs.
    let members = vec![Member {
        namespace: digest("namespace"),
        places: vec![key("history")],
    }];
    let old = Binding::new(7, digest("boundary"), members.clone()).unwrap();
    assert_eq!(
        old.id().unwrap().as_str(),
        "b3d86661dbb8678666a4703d6165bbc62a99825bd70509b07bfcf93a4de132f7"
    );
    let legacy = serde_json::to_value(&old).unwrap();
    assert!(legacy.get("history_scope").is_none());
    assert_eq!(serde_json::from_value::<Binding>(legacy).unwrap(), old);
    let roster = Binding::new_roster(7, digest("boundary"), members).unwrap();
    assert_eq!(
        roster.id().unwrap().as_str(),
        "e42b635a8df3709d35a43647c915e3edadae58ed6dc445ec5b0daf9333b5e909"
    );
    let mut wire = serde_json::to_value(&roster).unwrap();
    assert_eq!(wire["history_scope"], "roster");
    assert_eq!(
        serde_json::from_value::<Binding>(wire.clone()).unwrap(),
        roster
    );
    wire["history_scope"] = serde_json::json!("unknown");
    assert!(serde_json::from_value::<Binding>(wire).is_err());
}
#[test]
fn exact_provider_binding_reopens_and_legacy_marker_is_unchanged() {
    let f = Fixture::new();
    let before = snapshot(&f.path());
    let reopened = LocalStore::open(&f.path()).unwrap();
    assert_eq!(Enrollment::bind(&reopened).unwrap(), f.enrollment);
    assert_eq!(before, snapshot(&f.path()));
    let new: serde_json::Value =
        serde_json::from_slice(&fs::read(f.path().join("store.json")).unwrap()).unwrap();
    assert_eq!(new["schema"], "tig.local-store.design-6");
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Old {
        schema: String,
        place: Key,
        instance: Digest,
    }
    assert!(serde_json::from_value::<Old>(new).is_err());
    let legacy = LocalStore::create(&f.root.path().join("legacy"), key("legacy")).unwrap();
    let raw = fs::read(f.root.path().join("legacy/store.json")).unwrap();
    let old: Old = serde_json::from_slice(&raw).unwrap();
    assert_eq!(old.schema, "tig.local-store.design-5");
    assert_eq!(old.place, *legacy.place());
    assert_eq!(old.instance, *legacy.identity());
    assert_eq!(Enrollment::bind(&legacy), Err(Error::WrongEnrollment));
}
#[test]
fn every_namespace_is_required_and_foreign_duplicate_or_subset_rosters_refuse() {
    let mut f = Fixture::new();
    let n = f.namespaces.pop().unwrap();
    assert!(matches!(f.assess(), Err(Error::IncompleteRoster)));
    f.namespaces.push(n);
    let saved = f.namespaces.pop().unwrap();
    f.namespaces.push(f.peer(0));
    assert!(matches!(f.assess(), Err(Error::IncompleteRoster)));
    f.namespaces.pop();
    f.namespaces.push(saved);
    f.namespaces.reverse();
    let assessment = f.assess().unwrap();
    assert_eq!(assessment.summary().checked.len(), 2);
    assert!(!assessment.summary().removal_authority);
    assessment.check_current().unwrap();
}
#[test]
fn claim_from_either_namespace_prevents_store_absence_and_failure_releases_all_guards() {
    for index in 0..2 {
        let mut f = Fixture::new();
        prepare(
            &mut f.namespaces[index],
            "prepare",
            f.selection.clone(),
            &f.nodes,
        );
        assert!(matches!(f.assess(), Err(Error::Required)));
        for namespace in &f.namespaces {
            drop(namespace.fence(&f.object).unwrap());
        }
        drop(f.store.try_exclusive_object(&f.object).unwrap());
    }
}
#[test]
fn aliases_are_union_not_first_match_and_unknown_mapping_never_establishes_absence() {
    let mut f = Fixture::places(vec![key("working"), key("history")]);
    let initial = prepare(&mut f.namespaces[0], "old", f.selection.clone(), &f.nodes);
    f.namespaces[0]
        .apply(
            key("adopt"),
            Command::Adopt {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 2,
                prepared: initial.reference,
            },
            &f.nodes,
            &limits(),
        )
        .unwrap();
    f.namespaces[0]
        .apply(
            key("withdraw"),
            Command::Withdraw {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 3,
            },
            &f.nodes,
            &limits(),
        )
        .unwrap();
    // History alone protects this same provider despite absent working demand.
    assert!(matches!(f.assess(), Err(Error::Required)));
    let mut f = Fixture::places(vec![key("unknown-place")]);
    assert!(matches!(
        f.assess(),
        Err(Error::Admission(admission::Error::InvalidPolicy))
    ));
}
#[test]
fn held_assessment_blocks_claims_and_provider_readers_but_unrelated_admission_progresses() {
    let mut f = Fixture::new();
    let mut peer = f.peer(1);
    let target = f.selection.clone();
    let mut nodes = MemoryNodes::default();
    let unrelated = (0..1024)
        .map(|n| Object::from_bytes(format!("other-{n}").as_bytes()))
        .find(|o| o.sha256.as_str()[..2] != f.object.sha256.as_str()[..2])
        .unwrap();
    let other = tree(&mut nodes, &unrelated);
    let object = f.object.clone();
    let reopened = LocalStore::open(&f.path()).unwrap();
    let assessment = f.assess().unwrap();
    assert!(matches!(
        reopened.read_object(&object),
        Err(crate::Error::Busy)
    ));
    assert!(matches!(
        peer.apply(
            key("late"),
            Command::Prepare {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 1,
                selection: target.clone()
            },
            &nodes,
            &limits()
        ),
        Err(admission::Error::IncompleteClosure)
    ));
    // With the actual target metadata available, the effective fence refuses.
    tree(&mut nodes, &object);
    assert_eq!(
        peer.apply(
            key("late"),
            Command::Prepare {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 1,
                selection: target
            },
            &nodes,
            &limits()
        ),
        Err(admission::Error::Busy)
    );
    prepare(&mut peer, "unrelated", other, &nodes);
    assessment.check_current().unwrap();
    drop(assessment);
    assert!(reopened.read_object(&object).is_ok());
}
#[test]
fn active_reader_and_persistent_retention_refuse_store_assessment() {
    let mut f = Fixture::new();
    let reader = f.store.read_object(&f.object).unwrap();
    assert!(matches!(f.assess(), Err(Error::Busy)));
    drop(reader);
    let mut ledger = crate::ledger::CapacityLedger::create(
        &f.root.path().join("ledger"),
        key("capacity"),
        key("epoch"),
        0,
    )
    .unwrap();
    let publication = Request {
        operation: key("initial"),
        owner: key("owner"),
        object: f.object.clone(),
        required: AckLevel::ProcessRestart,
    };
    let plan = crate::retention::plan(
        &f.store,
        &crate::retention::RetentionRequest {
            operation: key("retain"),
            owner: key("owner"),
            publication,
            minimum_ack: AckLevel::ProcessRestart,
        },
    )
    .unwrap();
    let probe = ledger.begin_probe(key("probe")).unwrap();
    ledger
        .observe(key("sample"), probe.reference(), 1024 * 1024 * 1024)
        .unwrap();
    let job = ledger
        .reserve(key("job"), key("owner"), plan.required_peak() * 20)
        .unwrap()
        .reference()
        .clone();
    crate::retention::retain(&mut ledger, &f.store, &job, &plan).unwrap();
    f.store = LocalStore::open(&f.path()).unwrap();
    assert!(matches!(f.assess(), Err(Error::Retained)));
    for namespace in &f.namespaces {
        drop(namespace.fence(&f.object).unwrap());
    }
}
struct Missing;
impl NodeStore for Missing {
    fn get(&self, _: &Object) -> TreeResult<Option<Vec<u8>>> {
        Ok(None)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        panic!("read-only")
    }
}
#[test]
fn missing_closure_unknown_size_and_aggregate_cross_namespace_budget_refuse() {
    let mut f = Fixture::new();
    let other = Object::from_bytes(b"other");
    let selection = tree(&mut f.nodes, &other);
    for n in &mut f.namespaces {
        prepare(n, "prepare", selection.clone(), &f.nodes);
    }
    assert!(matches!(
        assess(
            &f.store,
            &f.enrollment,
            &f.object,
            &mut f.namespaces,
            &Missing,
            &limits()
        ),
        Err(Error::Admission(admission::Error::IncompleteClosure))
    ));
    let mut budget = limits();
    budget.roots = 1;
    assert!(matches!(
        assess(
            &f.store,
            &f.enrollment,
            &f.object,
            &mut f.namespaces,
            &f.nodes,
            &budget
        ),
        Err(Error::Admission(admission::Error::Budget))
    ));
    let assessment = f.assess().unwrap();
    assessment.check_current().unwrap();
    drop(assessment);
    let mut wrong = f.object.clone();
    wrong.bytes += 1;
    // A protected root with the same hash at another size cannot prove absence.
    let mut f = Fixture::new();
    prepare(
        &mut f.namespaces[0],
        "target",
        f.selection.clone(),
        &f.nodes,
    );
    assert!(matches!(
        assess(
            &f.store,
            &f.enrollment,
            &wrong,
            &mut f.namespaces,
            &f.nodes,
            &limits()
        ),
        Err(Error::Admission(admission::Error::ConflictingSize))
    ));
}
#[test]
fn assessment_is_readonly_and_missing_anchor_does_not_create_or_repair() {
    let mut f = Fixture::new();
    let before = snapshot(&f.path());
    let encoded = serde_json::to_vec(&f.enrollment).unwrap();
    let restored: Enrollment = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(restored, f.enrollment);
    let assessment = f.assess().unwrap();
    let summary = serde_json::to_string(&assessment.summary()).unwrap();
    assert!(!summary.contains("namespace-"));
    drop(assessment);
    assert_eq!(snapshot(&f.path()), before);
    let absent = Object::from_bytes(b"never-published");
    assert!(matches!(
        assess(
            &f.store,
            &f.enrollment,
            &absent,
            &mut f.namespaces,
            &f.nodes,
            &limits()
        ),
        Err(Error::Provider)
    ));
    assert_eq!(snapshot(&f.path()), before);
    let mut oversized = f.object.clone();
    oversized.bytes = crate::local::MAX_OBJECT_BYTES + 1;
    assert!(matches!(
        assess(
            &f.store,
            &f.enrollment,
            &oversized,
            &mut f.namespaces,
            &f.nodes,
            &limits()
        ),
        Err(Error::Provider)
    ));
    assert_eq!(snapshot(&f.path()), before);
}

#[test]
fn invalid_decoded_binding_refuses_before_provider_creation_and_deadline_covers_closure_io() {
    let mut f = Fixture::new();
    let path = f.root.path().join("must-not-create");
    let mut raw = serde_json::to_value(f.store.protection().unwrap()).unwrap();
    raw["members"][0]["places"] = serde_json::json!([]);
    let invalid: Binding = serde_json::from_value(raw).unwrap();
    assert!(LocalStore::create_protected(&path, key("invalid"), invalid).is_err());
    assert!(!path.exists());
    let selection = tree(&mut f.nodes, &Object::from_bytes(b"other"));
    prepare(&mut f.namespaces[0], "other", selection, &f.nodes);
    struct Slow<'a>(&'a MemoryNodes);
    impl NodeStore for Slow<'_> {
        fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
            std::thread::sleep(std::time::Duration::from_millis(20));
            self.0.get(object)
        }
        fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
            panic!("read-only")
        }
    }
    let mut budget = limits();
    budget.millis = 1;
    assert!(matches!(
        assess(
            &f.store,
            &f.enrollment,
            &f.object,
            &mut f.namespaces,
            &Slow(&f.nodes),
            &budget
        ),
        Err(Error::Admission(admission::Error::Budget))
    ));
    drop(f.assess().unwrap());
}
#[test]
fn stale_enrollment_replaced_store_and_marker_mutation_refuse() {
    let mut f = Fixture::new();
    let mut json = serde_json::to_value(&f.enrollment).unwrap();
    json["generation"] = 8.into();
    let stale: Enrollment = serde_json::from_value(json).unwrap();
    assert!(matches!(
        assess(
            &f.store,
            &stale,
            &f.object,
            &mut f.namespaces,
            &f.nodes,
            &limits()
        ),
        Err(Error::WrongEnrollment)
    ));
    let other = Fixture::new();
    assert_eq!(
        f.enrollment.check(&other.store),
        Err(Error::WrongEnrollment)
    );
    let marker = f.path().join("store.json");
    let raw = fs::read(&marker).unwrap();
    let mut changed: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    changed["protection"]["generation"] = 8.into();
    fs::write(&marker, identity::canonical_json(&changed).unwrap()).unwrap();
    assert!(matches!(f.assess(), Err(Error::Provider)));
    fs::write(&marker, raw).unwrap();
    assert!(f.assess().is_ok());
}

#[test]
fn failed_marker_revalidation_keeps_every_live_guard_until_assessment_drops() {
    let mut f = Fixture::new();
    let marker = f.root.path().join("namespace-1/claims.json");
    let raw = fs::read(&marker).unwrap();
    let mut peer = f.peer(1);
    let object = f.object.clone();
    let reopened = LocalStore::open(&f.path()).unwrap();
    let assessment = f.assess().unwrap();
    let mut changed: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    changed["epoch"] = "replaced".into();
    fs::write(&marker, identity::canonical_json(&changed).unwrap()).unwrap();
    assert!(assessment.check_current().is_err());
    assert!(matches!(
        reopened.read_object(&object),
        Err(crate::Error::Busy)
    ));
    fs::write(&marker, raw).unwrap();
    assert!(matches!(peer.fence(&object), Err(admission::Error::Busy)));
    assessment.check_current().unwrap();
    drop(assessment);
    drop(peer.fence(&object).unwrap());
    assert!(reopened.read_object(&object).is_ok());
    // Keep an actual mutable peer handle exercised after the protected epoch.
    assert!(peer.source(&key("main")).unwrap().is_some());
}

#[test]
fn bound_store_contract_and_generation_work_with_existing_copy_accounting_and_retention() {
    use crate::{
        ledger::CapacityLedger,
        local_capacity::LocalCapacityProbe,
        orchestration::{
            self, Basis, Budget, Copy, LocalExecution, Place, Plan, PlanRequest, ProtectionScope,
        },
    };
    let f = Fixture::new();
    assert!(matches!(
        Place::local(&f.store, 8),
        Err(orchestration::Error::StaleAdmission)
    ));
    let destination = LocalStore::create_protected(
        &f.root.path().join("destination"),
        key("destination"),
        f.store.protection().unwrap().clone(),
    )
    .unwrap();
    let mut ledger = CapacityLedger::create(
        &f.root.path().join("capacity"),
        key("capacity"),
        key("epoch"),
        0,
    )
    .unwrap();
    let request = Request {
        operation: key("initial"),
        owner: key("owner"),
        object: f.object.clone(),
        required: AckLevel::ProcessRestart,
    };
    let plan = Plan::new(PlanRequest {
        source: Place::local(&f.store, 7).unwrap(),
        destination: Place::local(&destination, 7).unwrap(),
        capacity: ledger.identity().clone(),
        owner: key("owner"),
        basis: Basis {
            source: digest("snapshot"),
            policy: digest("policy"),
            claims: digest("claims"),
            authority: digest("authority"),
            protection: ProtectionScope::Unknown {
                gaps: vec![key("copy-only")],
            },
        },
        budget: Budget {
            max_objects: 1,
            max_payload_bytes: 1024,
            max_peak_bytes: 64 * 1024 * 1024,
        },
        copies: vec![Copy {
            source: request,
            operation: key("copy"),
        }],
    })
    .unwrap();
    let peak = plan.local_peak(&destination).unwrap();
    LocalCapacityProbe::bind(&destination, &mut ledger)
        .unwrap()
        .observe(&destination, &mut ledger, key("probe"), key("sample"))
        .unwrap();
    let job = ledger
        .reserve(key("job"), key("owner"), peak)
        .unwrap()
        .reference()
        .clone();
    let mut bad = plan.request().clone();
    bad.source.contract = digest("tig.local-store.design-5/process-restart/copy-retain/v1");
    let stale = Plan::new(bad).unwrap();
    let before = ledger.checkpoint();
    let mut execution = LocalExecution {
        source: &f.store,
        destination: &destination,
        ledger: &mut ledger,
        admitted_source: &stale.request().source,
        admitted_destination: &stale.request().destination,
        admitted_basis: &stale.request().basis,
    };
    assert!(matches!(
        execution.apply(&stale, 0, &job),
        Err(orchestration::Error::StaleAdmission)
    ));
    assert_eq!(ledger.checkpoint(), before);
    let mut execution = LocalExecution {
        source: &f.store,
        destination: &destination,
        ledger: &mut ledger,
        admitted_source: &plan.request().source,
        admitted_destination: &plan.request().destination,
        admitted_basis: &plan.request().basis,
    };
    let copied = execution.apply(&plan, 0, &job).unwrap();
    drop(copied);
    assert!(f.store.read_object(&f.object).is_ok());
    assert!(matches!(
        destination.try_exclusive_object(&f.object),
        Err(crate::Error::Retained)
    ));
}

#[derive(Serialize, Deserialize)]
struct ChildInput {
    enrollment: Enrollment,
    minimums: Vec<Checkpoint>,
    object: Object,
}
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn protection_child() {
    let Some(root) = std::env::var_os("GIT_ASSET_STORE_PROTECTION_CHILD_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let input: ChildInput =
        serde_json::from_slice(&fs::read(root.join("child-input.json")).unwrap()).unwrap();
    let store = LocalStore::open(&root.join("store")).unwrap();
    let mut namespaces = input
        .minimums
        .iter()
        .enumerate()
        .map(|(n, min)| Namespace::open(&root.join(format!("namespace-{n}")), min).unwrap())
        .collect::<Vec<_>>();
    let _assessment = assess(
        &store,
        &input.enrollment,
        &input.object,
        &mut namespaces,
        &MemoryNodes::default(),
        &limits(),
    )
    .unwrap();
    fs::write(root.join("child-ready"), b"ready").unwrap();
    let start = Instant::now();
    while start.elapsed().as_secs() < 20 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("test parent did not terminate helper");
}
#[test]
fn actual_process_exit_releases_provider_and_all_namespace_fences_and_restart_reassesses() {
    let mut f = Fixture::new();
    fs::write(
        f.root.path().join("child-input.json"),
        serde_json::to_vec(&ChildInput {
            enrollment: f.enrollment.clone(),
            minimums: f.namespaces.iter().map(Namespace::checkpoint).collect(),
            object: f.object.clone(),
        })
        .unwrap(),
    )
    .unwrap();
    let mut child = Child(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "store_protection::tests::protection_child",
                "--nocapture",
            ])
            .env("GIT_ASSET_STORE_PROTECTION_CHILD_ROOT", f.root.path())
            .spawn()
            .unwrap(),
    );
    let start = Instant::now();
    while !f.root.path().join("child-ready").exists() {
        assert!(child.0.try_wait().unwrap().is_none(), "helper exited early");
        assert!(
            start.elapsed().as_secs() < 10,
            "helper did not acquire guards"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    for n in &mut f.namespaces {
        assert_eq!(
            n.apply(
                key("late"),
                Command::Prepare {
                    source: key("main"),
                    owner: key("owner"),
                    expected_revision: 1,
                    selection: f.selection.clone()
                },
                &f.nodes,
                &limits()
            ),
            Err(admission::Error::Busy)
        );
        n.apply(
            key("unrelated-enroll"),
            Command::Enroll {
                source: key("another"),
                owner: key("owner"),
                class: Kind::Job,
            },
            &f.nodes,
            &limits(),
        )
        .unwrap();
    }
    assert!(matches!(
        f.store.read_object(&f.object),
        Err(crate::Error::Busy)
    ));
    assert!(child.0.try_wait().unwrap().is_none());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    f.store = LocalStore::open(&f.path()).unwrap();
    for index in 0..f.namespaces.len() {
        f.namespaces[index] = f.peer(index);
    }
    f.assess().unwrap().check_current().unwrap();
}
