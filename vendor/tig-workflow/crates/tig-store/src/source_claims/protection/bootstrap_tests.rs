use super::*;
fn d(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
struct Lab {
    temp: tempfile::TempDir,
    nodes: metadata::MetadataStore,
    index: Index,
    selections: BTreeMap<String, Selection>,
}
impl Lab {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut nodes = metadata::MetadataStore::create(
            &temp.path().join("nodes"),
            metadata::Limits {
                nodes: 10000,
                bytes: 16 << 20,
                database_bytes: 32 << 20,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let (tree, _) = Tree::build(
            &BTreeMap::from([(
                LogicalPath::parse("assets/a").unwrap(),
                Object::from_bytes(b"initial"),
            )]),
            &mut nodes,
            TreeLimits::default(),
        )
        .unwrap();
        nodes.finish().unwrap();
        let selections = BTreeMap::from([(
            "main".into(),
            Selection {
                root: tree.root().clone(),
                context: d("initial"),
            },
        )]);
        let index = Index::create(
            &temp.path().join("index"),
            key("epoch"),
            Enrollment {
                owner: key("controller"),
                subject: d("subject"),
                policies: vec![d("policy")],
                stores: vec![d("F")],
                actors: vec![Actor {
                    id: key("main"),
                    owner: key("owner"),
                    stores: vec![d("F")],
                }],
                max_bindings: 1000,
            },
        )
        .unwrap();
        Self {
            temp,
            nodes,
            index,
            selections,
        }
    }
}
#[test]
fn exact_bootstrap_resumes_after_prepared_cut_and_changed_inputs_do_not_rebind_it() {
    let mut f = Lab::new();
    assert!(matches!(
        f.index
            .bootstrap_inner(f.selections.clone(), &mut f.nodes, true, |s| {
                if s == Stage::Prepared {
                    Err(Error::Injected)
                } else {
                    Ok(())
                }
            }),
        Err(Error::Injected)
    ));
    let checkpoint = f.index.checkpoint();
    f.index = Index::open(&f.temp.path().join("index"), &checkpoint).unwrap();
    assert!(matches!(
        f.index.bootstrap(f.selections.clone(), &mut f.nodes),
        Err(Error::Conflict)
    ));
    f.index
        .bootstrap_recoverable(f.selections.clone(), &mut f.nodes)
        .unwrap();
    let witness = f.index.checkpoint();
    let replay = f
        .index
        .bootstrap_recoverable(f.selections.clone(), &mut f.nodes)
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(witness, f.index.checkpoint());
    let mut changed = f.selections.clone();
    changed.get_mut("main").unwrap().context = d("changed");
    assert!(
        f.index
            .bootstrap_recoverable(changed, &mut f.nodes)
            .is_err()
    );
    assert_eq!(witness, f.index.checkpoint());
    assert_eq!(
        f.index.actor(&mut f.nodes, &key("main")).unwrap().selection,
        f.selections["main"]
    );
}
#[test]
fn bootstrap_historical_replay_preserves_a_newer_actor_selection_and_adds_no_events() {
    let mut f = Lab::new();
    f.index
        .bootstrap_recoverable(f.selections.clone(), &mut f.nodes)
        .unwrap();
    let path = LogicalPath::parse("assets/a").unwrap();
    let object = Object::from_bytes(b"newer");
    let (tree, _) = Tree::from_root(f.selections["main"].root.clone())
        .edit(
            &f.selections["main"].root,
            &BTreeMap::from([(path.clone(), Some(object.clone()))]),
            &mut f.nodes,
            TreeLimits::default(),
        )
        .unwrap();
    let new = Selection {
        root: tree.root().clone(),
        context: d("newer"),
    };
    f.index
        .update(
            &Update {
                operation: key("newer"),
                actor: key("main"),
                owner: key("owner"),
                expected_revision: 1,
                selection: new.clone(),
                changes: BTreeMap::from([(path, Some(object))]),
                ready: true,
            },
            &mut f.nodes,
        )
        .unwrap();
    let witness = f.index.checkpoint();
    assert!(
        f.index
            .bootstrap_recoverable(f.selections.clone(), &mut f.nodes)
            .unwrap()
            .replayed
    );
    assert_eq!(witness, f.index.checkpoint());
    assert_eq!(
        f.index.actor(&mut f.nodes, &key("main")).unwrap().selection,
        new
    );
}
