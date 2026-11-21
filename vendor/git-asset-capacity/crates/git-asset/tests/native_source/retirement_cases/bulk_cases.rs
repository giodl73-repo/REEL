use super::*;

#[test]
fn native_block_runs_through_bulk_retirement_while_an_unselected_native_alias_keeps_shared_bytes() {
    use git_asset_store::{
        AckLevel, Request, accounted,
        final_retirement::{bulk, claims, provider},
        ledger::CapacityLedger,
        local::LocalStore,
        local_capacity::LocalCapacityProbe,
        orchestration::Place,
        retention,
        store_protection::{Binding as StoreBinding, Member},
    };
    struct Clock;
    impl provider::Clock for Clock {
        fn now(&self, id: &Digest) -> provider::Result<u64> {
            if id == &d("clock") {
                Ok(100)
            } else {
                Err(provider::Error::Clock)
            }
        }
    }
    let (f, mut snapshot) = fixture();
    let dir = tempfile::tempdir().unwrap();
    let mut nodes = metadata(&dir.path().join("metadata"));
    let all = roots(&snapshot);
    let root = retirement::build(
        &mut snapshot,
        all,
        profiles(&f),
        &mut nodes,
        BindingLimits::default(),
    )
    .unwrap();
    nodes.finish().unwrap();
    let inventory =
        retirement::inspect(&root, &root.context, &mut nodes, BindingLimits::default()).unwrap();
    let preview = retirement::select(
        &inventory,
        &Categories::new(),
        &filter(&snapshot, &["asset/a", "asset/c"]),
    )
    .unwrap();
    let mut namespaces = vec![namespace(&dir.path().join("namespace"), &nodes, &root)];
    let roster = claims::Roster::flat(
        d("final-authority"),
        d("epoch"),
        Some(d("owned-complete-scope")),
        vec![claims::NamespacePin::current(&namespaces[0]).unwrap()],
        protection_limits(),
    )
    .unwrap();
    let store = LocalStore::create_final(
        &dir.path().join("M"),
        key("M"),
        StoreBinding::new(
            1,
            d("fixture-boundary"),
            vec![Member {
                namespace: namespaces[0].identity().clone(),
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
    let mut ledger =
        CapacityLedger::create(&dir.path().join("ledger"), key("capacity"), key("epoch"), 0)
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
        let plan = retention::plan(
            &store,
            &retention::RetentionRequest {
                operation: key(&format!("retain-{name}")),
                owner: key("owner"),
                publication: publication.clone(),
                minimum_ack: AckLevel::ProcessRestart,
            },
        )
        .unwrap();
        retention::retain(&mut ledger, &store, &job, &plan).unwrap();
        copies.push(bulk::Copy {
            store: store.identity().clone(),
            capacity: ledger.identity().clone(),
            publication,
        });
    }
    copies.sort_by(|a, b| {
        a.publication
            .object
            .sha256
            .cmp(&b.publication.object.sha256)
    });
    let witness = LocalStore::create(&dir.path().join("witness"), key("witness")).unwrap();
    let mut witness_ledger = CapacityLedger::create(
        &dir.path().join("witness-ledger"),
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
        .reserve(key("job"), key("owner"), 64 * 1024 * 1024)
        .unwrap()
        .reference()
        .clone();
    let source = source_claims::subset::Enrollment {
        authority: d("source-authority"),
        namespace: namespaces[0].identity().clone(),
        policy: Digest::blob(
            &identity::canonical_json(&serde_json::to_value(namespaces[0].policy()).unwrap())
                .unwrap(),
        ),
        owner: key("owner"),
        classes: vec![Kind::Branch],
        limits: protection_limits(),
        writes: 10_000,
        write_bytes: 8 * 1024 * 1024,
    };
    let enrollment = bulk::Enrollment {
        authority: d("batch-authority"),
        epoch: d("epoch"),
        owner: key("owner"),
        context: d("batch-context"),
        scope: roster.scope().unwrap(),
        clock: d("clock"),
        witness: Place::local(&witness, 1).unwrap(),
        capacity: witness_ledger.identity().clone(),
        max_window: 1000,
        max_metadata_bytes: 16 * 1024 * 1024,
        sources: vec![source.clone()],
    };
    let request = bulk::Request {
        operation: key("native-block"),
        selections: vec![bulk::Selection {
            namespace: namespaces[0].identity().clone(),
            request: preview.request(key("main"), 3).unwrap(),
        }],
        copies,
        window: provider::Window {
            not_before: 50,
            expires: 500,
        },
    };
    let p = bulk::plan(
        &enrollment,
        request,
        &roster,
        &mut namespaces,
        &nodes.finished_reader().unwrap(),
        &[&store],
    )
    .unwrap();
    assert!(p.subjects().iter().any(
        |s| s.object == Object::from_bytes(b"shared") && s.hold == Some(bulk::Hold::Protected)
    ));
    assert_eq!(p.actions().len(), 1);
    let selected_paths = p
        .bindings()
        .iter()
        .filter(|b| b.object == Object::from_bytes(b"unique"))
        .map(|b| b.path.clone())
        .collect();
    let subset = source_claims::subset::plan(
        &source,
        &mut namespaces[0],
        source_claims::subset::Request {
            source: key("main"),
            predecessor_revision: 3,
            paths: selected_paths,
        },
        &nodes.finished_reader().unwrap(),
    )
    .unwrap();
    let survivor = subset.survivor_source().unwrap().unwrap();
    let jobs = vec![job.clone(); p.actions().len()];
    let mut places = [bulk::Local {
        store: &store,
        ledger: &mut ledger,
        job: &job,
    }];
    let result = bulk::Execution {
        enrollment: &enrollment,
        roster: &roster,
        namespaces: &mut namespaces,
        metadata: &mut nodes,
        places: &mut places,
        action_jobs: &jobs,
        witness: bulk::Local {
            store: &witness,
            ledger: &mut witness_ledger,
            job: &witness_job,
        },
        clock: &Clock,
        admitted_context: &enrollment.context,
    }
    .apply(&p, &p.authorize().unwrap())
    .unwrap();
    assert_eq!(result.phase, bulk::Phase::Completed);
    assert_eq!(result.historical_deleted_copy_bytes, 6);
    let status =
        provider::status::inspect(&store, &mut ledger, &job, p.actions().next().unwrap().1)
            .unwrap();
    assert!(matches!(status.observed, provider::Observation::Absent));
    let mut actual = Vec::new();
    store
        .read_object(&Object::from_bytes(b"shared"))
        .unwrap()
        .read_to_end(&mut actual)
        .unwrap();
    assert_eq!(actual, b"shared");
    let source = namespaces[0].source(&survivor).unwrap().unwrap();
    let remaining = &source.predecessors[0].selection;
    let inventory = retirement::inspect(
        remaining,
        &root.context,
        &mut nodes.finished_reader().unwrap(),
        BindingLimits::default(),
    )
    .unwrap();
    let a = retirement::select(
        &inventory,
        &Categories::new(),
        &filter(&snapshot, &["asset/a"]),
    )
    .unwrap();
    let c = retirement::select(
        &inventory,
        &Categories::new(),
        &filter(&snapshot, &["asset/c"]),
    )
    .unwrap();
    assert!(a.objects.contains(&Object::from_bytes(b"shared")));
    assert_eq!(c.assets[0].remaining_bindings, 3);
    assert!(!c.objects.contains(&Object::from_bytes(b"unique")));
}
