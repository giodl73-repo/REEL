use super::*;
use git_asset::source::native::job::{Dependencies, protection};
use git_asset_store::{
    protection::Limits as ProtectionLimits,
    source_claims::{
        Kind, Places,
        admission::{Namespace, Policy, Select},
    },
};
use std::cell::Cell;

fn declared(
    context: &ProductionContext,
    plan: &Plan,
    nodes: &mut impl NodeStore,
) -> job::JobContext {
    job::freeze(
        &Admission {
            context,
            plan,
            runtime: &names(),
            output: None,
        },
        nodes,
        TreeLimits::default(),
    )
    .unwrap()
}

#[test]
fn job_projection_preserves_every_role_without_retargeting_the_job() {
    let (f, source, plan, context, mut nodes) = fixture();
    let original = subject(&source, &f.profiles);
    let claim = Claim::create(&plan, pin(&context), vec![original.subject().clone()]).unwrap();
    let mut runtime = names();
    runtime.insert(
        LogicalPath::parse("shared-input").unwrap(),
        Object::from_bytes(b"rows-v1"),
    );
    let composition = BTreeMap::from([(
        LogicalPath::parse("component").unwrap(),
        Object::from_bytes(b"component"),
    )]);
    let evidence = BTreeMap::from([(
        LogicalPath::parse("request").unwrap(),
        Object::from_bytes(b"original request"),
    )]);
    let attachments = BTreeMap::from([(
        LogicalPath::parse("response").unwrap(),
        Object::from_bytes(b"original response"),
    )]);
    let job = job::freeze_with_dependencies(
        &Admission {
            context: &context,
            plan: &plan,
            runtime: &runtime,
            output: Some(OutputEvidence {
                claim: &claim,
                attachments: &attachments,
            }),
        },
        &Dependencies {
            composition: &composition,
            evidence: &evidence,
        },
        None,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let original = job.selection().clone();
    // Projection requires no source traversal or payload reads.
    let mut flat = MemoryNodes::default();
    let selection = protection::freeze(&job, &mut flat, TreeLimits::default()).unwrap();
    let (entries, _) = Tree::from_root(selection.root.clone())
        .entries(&mut flat, TreeLimits::default())
        .unwrap();
    assert_eq!(entries.len(), job.objects().len() + 1);
    for object in job.objects() {
        assert_eq!(
            entries[&LogicalPath::parse(&format!("objects/{}", object.sha256.as_str())).unwrap()],
            *object
        );
    }
    for role in [
        Role::Primary,
        Role::Runtime,
        Role::Composition,
        Role::ClaimedOutput,
        Role::Evidence,
    ] {
        assert!(!job.of(role).is_empty());
        assert!(
            job.of(role)
                .iter()
                .all(|o| entries.values().any(|v| v == o))
        );
    }
    protection::verify(&selection, &job, &mut flat, TreeLimits::default()).unwrap();
    assert_eq!(
        protection::freeze(&job, &mut flat, TreeLimits::default()).unwrap(),
        selection
    );
    assert_eq!(*job.selection(), original);
    assert_ne!(selection, original);
}

#[derive(Default)]
struct Counted {
    nodes: MemoryNodes,
    reads: Cell<usize>,
    read_bytes: Cell<usize>,
    writes: usize,
    write_bytes: usize,
}
impl NodeStore for Counted {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.reads.set(self.reads.get() + 1);
        self.read_bytes
            .set(self.read_bytes.get() + object.bytes as usize);
        self.nodes.get(object)
    }
    fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
        self.writes += 1;
        self.write_bytes += bytes.len();
        self.nodes.put(bytes)
    }
}

#[test]
fn job_projection_refuses_substitution_missing_corrupt_and_aggregate_budget_overruns() {
    let (_f, _source, plan, context, mut nodes) = fixture();
    let job = declared(&context, &plan, &mut nodes);
    let mut flat = Counted::default();
    let selection = protection::freeze(&job, &mut flat, TreeLimits::default()).unwrap();
    let writes = flat.writes;
    let write_bytes = flat.write_bytes;
    protection::verify(&selection, &job, &mut flat, TreeLimits::default()).unwrap();
    let reads = flat.reads.get();
    let read_bytes = flat.read_bytes.get();
    assert!(reads > 1 && writes > 1);
    assert_eq!(flat.writes, writes);
    for limits in [
        TreeLimits {
            max_entries: job.objects().len(),
            ..TreeLimits::default()
        },
        TreeLimits {
            max_reads: 0,
            ..TreeLimits::default()
        },
        TreeLimits {
            max_read_bytes: 0,
            ..TreeLimits::default()
        },
    ] {
        let before = flat.reads.get();
        assert_eq!(
            protection::verify(&selection, &job, &mut flat, limits),
            Err(Error::Budget)
        );
        assert_eq!(flat.reads.get(), before);
    }
    for limits in [
        TreeLimits {
            max_writes: writes - 1,
            ..TreeLimits::default()
        },
        TreeLimits {
            max_write_bytes: write_bytes - 1,
            ..TreeLimits::default()
        },
    ] {
        let mut short = Counted::default();
        assert_eq!(
            protection::freeze(&job, &mut short, limits),
            Err(Error::Budget)
        );
        assert!(short.writes <= limits.max_writes);
        assert!(short.write_bytes <= limits.max_write_bytes);
    }
    for limits in [
        TreeLimits {
            max_reads: reads - 1,
            ..TreeLimits::default()
        },
        TreeLimits {
            max_read_bytes: read_bytes - 1,
            ..TreeLimits::default()
        },
    ] {
        flat.reads.set(0);
        flat.read_bytes.set(0);
        assert_eq!(
            protection::verify(&selection, &job, &mut flat, limits),
            Err(Error::Budget)
        );
        assert!(flat.reads.get() <= limits.max_reads);
        assert!(flat.read_bytes.get() <= limits.max_read_bytes);
    }
    assert_eq!(
        protection::verify(
            &selection,
            &job,
            &mut MemoryNodes::default(),
            TreeLimits::default()
        ),
        Err(Error::Incomplete)
    );
    let (mut entries, _) = Tree::from_root(selection.root.clone())
        .entries(&mut flat, TreeLimits::default())
        .unwrap();
    entries.remove(
        &LogicalPath::parse(&format!(
            "objects/{}",
            job.objects().iter().next().unwrap().sha256.as_str()
        ))
        .unwrap(),
    );
    let substituted = Selection {
        root: Tree::build(&entries, &mut flat, TreeLimits::default())
            .unwrap()
            .0
            .root()
            .clone(),
        context: selection.context.clone(),
    };
    assert_eq!(
        protection::verify(&substituted, &job, &mut flat, TreeLimits::default()),
        Err(Error::Integrity)
    );
    struct Corrupt<'a>(&'a MemoryNodes, Digest);
    impl NodeStore for Corrupt<'_> {
        fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
            if object.sha256 == self.1 {
                return Ok(Some(b"corrupt header".to_vec()));
            }
            self.0.get(object)
        }
        fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
            panic!("verification writes nothing")
        }
    }
    assert_eq!(
        protection::verify(
            &selection,
            &job,
            &mut Corrupt(&flat.nodes, selection.context.clone()),
            TreeLimits::default()
        ),
        Err(Error::Integrity)
    );
    let other_runtime = BTreeMap::new();
    let other = job::freeze(
        &Admission {
            context: &context,
            plan: &plan,
            runtime: &other_runtime,
            output: None,
        },
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let before = flat.reads.get();
    assert_eq!(
        protection::verify(&selection, &other, &mut flat, TreeLimits::default()),
        Err(Error::Integrity)
    );
    assert_eq!(flat.reads.get(), before);
}

fn namespace_policy() -> Policy {
    let active = Places {
        current: vec![key("working")],
        pending: vec![key("working")],
        predecessor: vec![key("archive")],
    };
    let retained = Places {
        current: vec![key("archive")],
        pending: vec![key("archive")],
        predecessor: vec![key("archive")],
    };
    Policy {
        prefer_active: true,
        classes: [
            (Kind::Job, active.clone()),
            (Kind::Branch, active.clone()),
            (Kind::Draft, active),
            (Kind::Retention, retained),
        ]
        .into_iter()
        .collect(),
    }
}
fn protection_limits() -> ProtectionLimits {
    ProtectionLimits {
        millis: 60_000,
        roots: 64,
        bindings: 10_000,
        associations: 100_000,
        reads: 10_000,
        read_bytes: 32 * 1024 * 1024,
    }
}
fn enroll(namespace: &mut Namespace, source: &str, class: Kind, nodes: &impl NodeStore) {
    namespace
        .apply(
            key(&format!("enroll-{source}")),
            OwnerCommand::Enroll {
                source: key(source),
                owner: key("fixture"),
                class,
            },
            nodes,
            &protection_limits(),
        )
        .unwrap();
}
fn select(source: &str, operation: &str, revision: u64, selection: &Selection) -> Select {
    Select {
        prepare: key(&format!("{operation}-prepare")),
        adopt: key(&format!("{operation}-adopt")),
        source: key(source),
        owner: key("fixture"),
        expected_revision: revision,
        selection: selection.clone(),
    }
}

#[test]
fn guarded_job_projection_survives_detached_source_restart_peer_advance_and_cancellation() {
    let (f, source, plan, context, initial) = fixture();
    let disk = tempfile::tempdir().unwrap();
    let metadata = disk.path().join("metadata");
    let mut store = MetadataStore::create(
        &metadata,
        StoreLimits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65_536,
        },
    )
    .unwrap();
    for (_, raw) in initial.objects() {
        store.put(raw).unwrap();
    }
    let runtime = names();
    let composition = BTreeMap::new();
    let evidence = BTreeMap::from([(
        LogicalPath::parse("request").unwrap(),
        Object::from_bytes(b"outstanding manual request"),
    )]);
    let dependencies = Dependencies {
        composition: &composition,
        evidence: &evidence,
    };
    let planned = job::freeze_with_dependencies(
        &Admission {
            context: &context,
            plan: &plan,
            runtime: &runtime,
            output: None,
        },
        &dependencies,
        None,
        &mut store,
        TreeLimits::default(),
    )
    .unwrap();
    let current = subject(&source, &f.profiles);
    let claim = Claim::create(&plan, pin(&context), vec![current.subject().clone()]).unwrap();
    let attachments = BTreeMap::from([(
        LogicalPath::parse("response").unwrap(),
        Object::from_bytes(b"unselected returned bytes"),
    )]);
    let claimed = job::freeze_with_dependencies(
        &Admission {
            context: &context,
            plan: &plan,
            runtime: &runtime,
            output: Some(OutputEvidence {
                claim: &claim,
                attachments: &attachments,
            }),
        },
        &dependencies,
        None,
        &mut store,
        TreeLimits::default(),
    )
    .unwrap();
    let old = protection::freeze(&planned, &mut store, TreeLimits::default()).unwrap();
    let new = protection::freeze(&claimed, &mut store, TreeLimits::default()).unwrap();
    let witness = store.finish().unwrap();
    drop(store);
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut nodes = MetadataReader::open(&metadata, &witness).unwrap();
    protection::verify(&old, &planned, &mut nodes, TreeLimits::default()).unwrap();
    protection::verify(&new, &claimed, &mut nodes, TreeLimits::default()).unwrap();
    let path = disk.path().join("namespace");
    let mut namespace =
        Namespace::create(&path, key("project"), key("epoch"), namespace_policy()).unwrap();
    let minimum = namespace.checkpoint();
    for (name, kind) in [
        ("job", Kind::Job),
        ("peer-job", Kind::Job),
        ("main", Kind::Branch),
    ] {
        enroll(&mut namespace, name, kind, &nodes);
    }
    let start = select("job", "start", 1, &old);
    namespace
        .select(&start, &nodes, &protection_limits())
        .unwrap();
    namespace
        .select(
            &select("peer-job", "peer-start", 1, &old),
            &nodes,
            &protection_limits(),
        )
        .unwrap();
    namespace
        .select(
            &select("main", "main-start", 1, &old),
            &nodes,
            &protection_limits(),
        )
        .unwrap();
    let mut peer = Namespace::open(&path, &minimum).unwrap();
    peer.select(
        &select("main", "main-advance", 3, &new),
        &nodes,
        &protection_limits(),
    )
    .unwrap();
    let returned = select("job", "returned", 3, &new);
    let before_failure = peer.checkpoint();
    assert!(
        peer.select(&returned, &MemoryNodes::default(), &protection_limits())
            .is_err()
    );
    assert_eq!(peer.checkpoint(), before_failure);
    assert_eq!(
        peer.source(&key("job"))
            .unwrap()
            .unwrap()
            .current
            .unwrap()
            .selection,
        old
    );
    peer.select(&returned, &nodes, &protection_limits())
        .unwrap();
    let checkpoint = peer.checkpoint();
    drop(peer);
    drop(namespace);
    let mut namespace = Namespace::open(&path, &checkpoint).unwrap();
    let before = namespace.checkpoint();
    assert!(
        namespace
            .select(&returned, &nodes, &protection_limits())
            .unwrap()
            .replayed
    );
    assert_eq!(namespace.checkpoint(), before);
    assert!(matches!(
        namespace.select(
            &select("job", "stale", 3, &old),
            &nodes,
            &protection_limits()
        ),
        Err(git_asset_store::source_claims::admission::Error::Journal(
            git_asset_store::source_claims::Error::Stale
        ))
    ));
    assert_eq!(namespace.checkpoint(), before);
    assert_eq!(
        namespace
            .source(&key("peer-job"))
            .unwrap()
            .unwrap()
            .current
            .unwrap()
            .selection,
        old
    );
    namespace
        .apply(
            key("cancel-job"),
            OwnerCommand::Withdraw {
                source: key("job"),
                owner: key("fixture"),
                expected_revision: 5,
            },
            &nodes,
            &protection_limits(),
        )
        .unwrap();
    let job_state = namespace.source(&key("job")).unwrap().unwrap();
    assert!(job_state.current.is_none());
    assert!(job_state.predecessors.iter().any(|p| p.selection == new));
    assert!(
        namespace
            .selection_status(&returned)
            .unwrap()
            .unwrap()
            .replayed
    );
    let requirements = namespace
        .requirements(&nodes, &protection_limits())
        .unwrap();
    for object in planned.objects().iter().chain(claimed.objects()) {
        assert!(
            requirements
                .entries
                .iter()
                .any(|e| e.object == *object && !e.places.is_empty())
        );
    }
    let request = evidence.values().next().unwrap();
    assert!(
        requirements
            .entries
            .iter()
            .any(|e| e.object == *request && e.places.contains(&key("working")))
    );
    let after = namespace.checkpoint();
    assert!(
        namespace
            .apply(
                key("foreign-cancel"),
                OwnerCommand::Withdraw {
                    source: key("peer-job"),
                    owner: key("foreign"),
                    expected_revision: 3
                },
                &nodes,
                &protection_limits()
            )
            .is_err()
    );
    assert_eq!(namespace.checkpoint(), after);
}
