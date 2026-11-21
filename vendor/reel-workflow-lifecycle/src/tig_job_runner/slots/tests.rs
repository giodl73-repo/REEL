use super::*;
use tig_store::source_claims::{
    coordinated,
    protection::{Enrollment, Routing},
};
fn k(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn d(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
struct Case {
    temp: tempfile::TempDir,
    index: Index,
    nodes: MetadataStore,
    scope: Scope,
    input: Selection,
    prepared: envelope::Envelope,
    marker: Vec<u8>,
}
impl Case {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir(root.join("coordination")).unwrap();
        let mut nodes = MetadataStore::create(
            &root.join("metadata"),
            tig_store::metadata::Limits {
                nodes: 100000,
                bytes: 32 * 1024 * 1024,
                database_bytes: 64 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let payload = Object::from_bytes(b"input");
        let input = Selection {
            root: Tree::build(
                &[(LogicalPath::parse("payload").unwrap(), payload.clone())].into(),
                &mut nodes,
                TreeLimits::default(),
            )
            .unwrap()
            .0
            .root()
            .clone(),
            context: d("input"),
        };
        let prepared = envelope::create(
            &Resources::admitted(&input, [payload].into()),
            &d("policy"),
            &mut nodes,
            Limits::default(),
        )
        .unwrap();
        let empty = Selection {
            root: Tree::empty(&mut nodes).unwrap().root().clone(),
            context: d("aggregate"),
        };
        let mut stores = vec![d("F"), d("M")];
        stores.sort();
        let mut index = Index::create_effects(
            &root.join("index"),
            k("epoch"),
            Enrollment {
                owner: k("index-owner"),
                subject: d("group"),
                policies: vec![d("policy")],
                stores: stores.clone(),
                actors: ["main", "retained", "slot-a", "slot-b", "weak"]
                    .into_iter()
                    .map(|id| Actor {
                        id: k(id),
                        owner: k("writer"),
                        stores: if id == "weak" {
                            vec![d("F")]
                        } else {
                            stores.clone()
                        },
                    })
                    .collect(),
                max_bindings: 8192,
            },
            Some(Routing {
                source: k("main"),
                actor: k("main"),
            }),
        )
        .unwrap();
        index
            .bootstrap(
                [
                    ("main".into(), empty.clone()),
                    ("retained".into(), empty.clone()),
                    ("slot-a".into(), idle(&index, &k("slot-a")).unwrap()),
                    ("slot-b".into(), idle(&index, &k("slot-b")).unwrap()),
                    ("weak".into(), empty.clone()),
                ]
                .into(),
                &mut nodes,
            )
            .unwrap();
        let mut source = SourceJournal::create_coordinated(
            &root.join("source"),
            k("project"),
            k("epoch"),
            coordinated::Binding {
                index: index.identity().clone(),
                source: k("main"),
                actor: k("main"),
                owner: k("writer"),
                initial: empty,
            },
        )
        .unwrap();
        work::adoption::enrollment::initialize(&mut index, &mut source, &mut nodes).unwrap();
        let scope = Scope {
            coordination_root: root.join("coordination"),
            actor: k("slot-a"),
            owner: k("writer"),
            operation: k("job-one"),
            policy: d("policy"),
            store: d("F"),
        };
        let marker = activation(
            &index,
            &scope,
            &input,
            &d("source"),
            &Object::from_bytes(b"candidate"),
        )
        .unwrap();
        Self {
            temp,
            index,
            nodes,
            scope,
            input,
            prepared,
            marker,
        }
    }
    fn accept(&mut self, apply: bool) -> Result<Accepted> {
        accept(
            &mut self.index,
            &mut self.nodes,
            &self.scope,
            &self.prepared,
            &self.marker,
            Limits::default(),
            apply,
        )
    }
    fn close(&mut self, apply: bool, cut: Option<Stage>) -> Result<Closed> {
        let root = self.temp.path();
        close(
            &mut self.index,
            &mut self.nodes,
            &self.scope,
            &Close {
                activation: &Object::from_bytes(&self.marker),
                operation: &k("close-one"),
                retained: &k("retained"),
                retained_owner: &k("writer"),
                index_root: &root.join("index"),
                metadata_root: &root.join("metadata"),
            },
            Limits::default(),
            apply,
            |stage| {
                if Some(stage) == cut {
                    bail!("injected")
                } else {
                    Ok(())
                }
            },
        )
    }
    fn reopen(&mut self) {
        let witness = self.nodes.finish().unwrap();
        self.nodes = MetadataStore::open(&self.temp.path().join("metadata"), &witness).unwrap();
        self.index =
            Index::open(&self.temp.path().join("index"), &self.index.checkpoint()).unwrap();
    }
    fn protected(&mut self, object: &Object, store: &str) -> Vec<Key> {
        self.index
            .inspect(&mut self.nodes, object, &d(store))
            .unwrap()
            .protected_by
    }
    fn replace(&mut self, actor: &str, selection: Selection, op: &str) {
        let old = self.index.actor(&mut self.nodes, &k(actor)).unwrap();
        let a = entries(&old.selection, &mut self.nodes, Limits::default()).unwrap();
        let b = entries(&selection, &mut self.nodes, Limits::default()).unwrap();
        let reference = work::stage(
            &mut self.index,
            &Update {
                operation: k(op),
                actor: k(actor),
                owner: k("writer"),
                expected_revision: old.revision,
                selection,
                changes: delta(&a, &b),
                ready: true,
            },
            None,
            &mut self.nodes,
        )
        .unwrap();
        work::execute(&mut self.index, &reference, &mut self.nodes).unwrap();
    }
}
#[test]
fn typed_idle_dry_run_acceptance_and_marker_verification_are_exact() {
    let mut c = Case::new();
    let before = (c.index.checkpoint(), c.nodes.checkpoint());
    let plan = c.accept(false).unwrap();
    assert!(!plan.applied);
    assert_eq!(before, (c.index.checkpoint(), c.nodes.checkpoint()));
    let accepted = c.accept(true).unwrap();
    verify_activation(
        &c.index,
        &c.scope,
        &c.input,
        &d("source"),
        &accepted.activation,
        &c.nodes,
    )
    .unwrap();
    assert!(
        verify_activation(
            &c.index,
            &c.scope,
            &c.input,
            &d("other-source"),
            &accepted.activation,
            &c.nodes
        )
        .is_err()
    );
    let before = c.index.checkpoint();
    c.accept(true).unwrap();
    assert_eq!(before, c.index.checkpoint());
    let wrong = activation(
        &c.index,
        &c.scope,
        &c.input,
        &d("source"),
        &Object::from_bytes(b"changed candidate"),
    )
    .unwrap();
    assert!(
        accept(
            &mut c.index,
            &mut c.nodes,
            &c.scope,
            &c.prepared,
            &wrong,
            Limits::default(),
            true
        )
        .is_err()
    );
    assert_eq!(before, c.index.checkpoint());
}
#[test]
fn handoff_boundaries_recover_and_historical_operations_cannot_reactivate_or_clear_reused_slot() {
    let mut c = Case::new();
    c.accept(true).unwrap();
    let object = Object::from_bytes(b"input");
    let before = (c.index.checkpoint(), c.nodes.checkpoint());
    assert!(!c.close(false, None).unwrap().applied);
    assert_eq!(before, (c.index.checkpoint(), c.nodes.checkpoint()));
    for cut in [
        Stage::HandoffStaged,
        Stage::HandoffAdopted,
        Stage::ClearStaged,
    ] {
        assert!(c.close(true, Some(cut)).is_err());
        c.reopen();
        for store in ["F", "M"] {
            let held = c.protected(&object, store);
            assert!(held.contains(&k("slot-a")));
            if cut != Stage::HandoffStaged {
                assert!(held.contains(&k("retained")));
            }
        }
    }
    assert!(c.close(true, None).unwrap().slot_idle);
    c.reopen();
    for store in ["F", "M"] {
        assert_eq!(c.protected(&object, store), vec![k("retained")]);
    }
    let mut next = c.scope.clone();
    next.operation = k("job-two");
    let marker = activation(
        &c.index,
        &next,
        &c.input,
        &d("source-two"),
        &Object::from_bytes(b"candidate-two"),
    )
    .unwrap();
    let new = accept(
        &mut c.index,
        &mut c.nodes,
        &next,
        &c.prepared,
        &marker,
        Limits::default(),
        true,
    )
    .unwrap();
    let state = c.index.actor(&mut c.nodes, &c.scope.actor).unwrap();
    assert!(c.accept(true).is_err());
    assert!(!c.close(true, None).unwrap().slot_idle);
    let after = c.index.actor(&mut c.nodes, &c.scope.actor).unwrap();
    assert_eq!(state.selection, after.selection);
    assert_eq!(state.revision, after.revision);
    assert!(
        entries(&after.selection, &mut c.nodes, Limits::default())
            .unwrap()
            .values()
            .any(|o| o == &new.activation)
    );
}
#[test]
fn lost_retained_coverage_blocks_clear_and_wrong_store_scope_is_not_a_handoff() {
    let mut c = Case::new();
    c.accept(true).unwrap();
    let root = c.temp.path();
    let marker = Object::from_bytes(&c.marker);
    assert!(
        close(
            &mut c.index,
            &mut c.nodes,
            &c.scope,
            &Close {
                activation: &marker,
                operation: &k("bad"),
                retained: &k("weak"),
                retained_owner: &k("writer"),
                index_root: &root.join("index"),
                metadata_root: &root.join("metadata")
            },
            Limits::default(),
            true,
            |_| Ok(())
        )
        .is_err()
    );
    assert!(c.close(true, Some(Stage::ClearStaged)).is_err());
    let held = c
        .index
        .actor(&mut c.nodes, &k("retained"))
        .unwrap()
        .selection;
    let empty = Selection {
        root: Tree::empty(&mut c.nodes).unwrap().root().clone(),
        context: d("temporary-withdrawal"),
    };
    c.replace("retained", empty, "withdraw-retained");
    let before = c.index.actor(&mut c.nodes, &k("slot-a")).unwrap();
    assert!(c.close(true, None).is_err());
    assert_eq!(
        before.selection,
        c.index.actor(&mut c.nodes, &k("slot-a")).unwrap().selection
    );
    assert_eq!(
        c.protected(&Object::from_bytes(b"input"), "F"),
        vec![k("slot-a")]
    );
    c.replace("retained", held, "repair-retained");
    assert!(c.close(true, None).unwrap().slot_idle);
}
#[test]
fn scoped_busy_and_original_change_budgets_fail_without_losing_claims() {
    let mut c = Case::new();
    let lease = actor_fence(
        c.index.identity(),
        &c.scope.coordination_root,
        &c.scope.actor,
    )
    .unwrap();
    assert!(c.accept(true).is_err());
    let mut other = c.scope.clone();
    other.actor = k("slot-b");
    let marker = activation(
        &c.index,
        &other,
        &c.input,
        &d("other"),
        &Object::from_bytes(b"candidate"),
    )
    .unwrap();
    accept(
        &mut c.index,
        &mut c.nodes,
        &other,
        &c.prepared,
        &marker,
        Limits::default(),
        true,
    )
    .unwrap();
    drop(lease);
    c.accept(true).unwrap();
    let current = c.index.actor(&mut c.nodes, &c.scope.actor).unwrap();
    let old = entries(&current.selection, &mut c.nodes, Limits::default()).unwrap();
    let mut objects: BTreeSet<_> = old.into_values().collect();
    objects.insert(current.selection.root);
    objects.extend((0..120).map(|i| Object::from_bytes(format!("bulk-{i}").as_bytes())));
    let target = envelope::create(
        &Resources::admitted(&c.input, objects),
        &c.scope.policy,
        &mut c.nodes,
        Limits::default(),
    )
    .unwrap();
    c.replace("slot-a", target.selection().clone(), "bulk-add");
    let before = c.index.checkpoint();
    assert!(c.close(true, None).is_err());
    assert_eq!(before, c.index.checkpoint());
    assert!(
        c.protected(&Object::from_bytes(b"input"), "M")
            .contains(&k("slot-a"))
    );
}
