use super::*;
use git_asset_core::tree::{NodeStore, TreeResult};
use std::cell::Cell;

#[test]
fn archive_acquisition_and_binding_share_one_read_budget_before_any_write() {
    struct Counted<'a> {
        nodes: &'a mut MemoryNodes,
        reads: Cell<usize>,
        writes: usize,
    }
    impl NodeStore for Counted<'_> {
        fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
            self.reads.set(self.reads.get() + 1);
            self.nodes.get(o)
        }
        fn put(&mut self, b: &[u8]) -> TreeResult<Object> {
            self.writes += 1;
            self.nodes.put(b)
        }
    }
    let (f, snapshot) = fixture();
    let mut nodes = MemoryNodes::default();
    let archive = snapshot
        .archive_metadata(&mut nodes, TreeLimits::default())
        .unwrap();
    let mut counted = Counted {
        nodes: &mut nodes,
        reads: Cell::new(0),
        writes: 0,
    };
    let limits = BindingLimits {
        reads: 1,
        ..BindingLimits::default()
    };
    assert!(matches!(
        retirement::build_from_archive(
            &archive,
            roots(&snapshot),
            profiles(&f),
            &mut counted,
            limits
        ),
        Err(retirement::Error::Budget)
    ));
    assert_eq!(counted.reads.get(), 1);
    assert_eq!(counted.writes, 0);
}

#[test]
fn one_thousand_native_assets_keep_independent_bindings_and_oversize_category_selection_refuses() {
    let f = Fixture::new();
    for n in 0..1000 {
        f.record(
            &format!("leaf-{n}"),
            &format!("asset/{n}"),
            true,
            json!([]),
            Some(b"shared"),
        );
    }
    let mut snapshot = capture(&reader(f.root()), commit(f.root()));
    let all = roots(&snapshot);
    let mut nodes = MemoryNodes::default();
    let root = retirement::build(
        &mut snapshot,
        all,
        profiles(&f),
        &mut nodes,
        BindingLimits::default(),
    )
    .unwrap();
    let inventory =
        retirement::inspect(&root, &root.context, &mut nodes, BindingLimits::default()).unwrap();
    assert_eq!(inventory.assets(), 1000);
    let definitions =
        BTreeSet::from([snapshot.resolve_id("asset/0").unwrap().definition().clone()]);
    let categories = BTreeMap::from([("drawing".into(), definitions)]);
    let all = Filter {
        assets: BTreeSet::new(),
        categories: BTreeSet::from(["drawing".into()]),
    };
    assert!(matches!(
        retirement::select(&inventory, &categories, &all),
        Err(retirement::Error::Budget)
    ));
    let block = Filter {
        assets: (0..10)
            .map(|n| {
                snapshot
                    .resolve_id(&format!("asset/{n}"))
                    .unwrap()
                    .reference()
                    .clone()
            })
            .collect(),
        categories: all.categories,
    };
    let preview = retirement::select(&inventory, &categories, &block).unwrap();
    assert_eq!(preview.assets.len(), 10);
    assert_eq!(preview.paths.len(), 40);
    assert_eq!(
        preview
            .objects
            .iter()
            .filter(|o| **o == Object::from_bytes(b"shared"))
            .count(),
        1
    );
    // The indexed path retains the complete category rather than flattening it
    // into the legacy1024-path response. Reuse this actual1000-record capture.
    use git_asset::source::native::retirement::paged::{self as paged, Occurrence, Session};
    let temp = tempfile::tempdir().unwrap();
    let mut persisted = metadata(&temp.path().join("metadata"));
    for (_, raw) in nodes.objects() {
        persisted.put(raw).unwrap();
    }
    persisted.finish().unwrap();
    let mut ns = vec![namespace(&temp.path().join("namespace"), &persisted, &root)];
    let session = Session::new(BindingLimits::default()).unwrap();
    let input = paged::occurrences(
        None,
        vec![Occurrence {
            namespace: ns[0].identity().clone(),
            source: key("main"),
            predecessor_revision: 3,
            header: root.context.clone(),
        }],
        &mut persisted,
        &session,
    )
    .unwrap();
    let e = paged::Enrollment {
        metadata: persisted.checkpoint().identity().clone(),
        owner: key("owner"),
        categories,
        age: None,
    };
    let all = Filter {
        assets: BTreeSet::new(),
        categories: BTreeSet::from(["drawing".into()]),
    };
    let candidate = paged::select(
        &e,
        &input,
        &all,
        git_asset_store::final_retirement::paged::Paging::default(),
        &mut ns,
        &mut persisted,
        None,
        &session,
    )
    .unwrap();
    let minimum = persisted.finish().unwrap();
    let mut reader = MetadataReader::open(&temp.path().join("metadata"), &minimum).unwrap();
    let checked = paged::validate_current(&e, &candidate, &mut ns, &mut reader, &session).unwrap();
    assert_eq!(checked.summary().matched_assets, 1000);
    assert_eq!(checked.summary().bindings, 4000);
    assert!(candidate.to_json().unwrap().len() < 65536);
    let (_, selected) = git_asset_store::final_retirement::paged::read_request(
        candidate.request().unwrap(),
        &mut reader,
        &Default::default(),
    )
    .unwrap();
    let exact = Tree::from_root(selected[0].bindings.clone())
        .entries(
            &mut reader,
            TreeLimits {
                max_entries: 65536,
                ..Default::default()
            },
        )
        .unwrap()
        .0;
    assert_eq!(exact.len(), 4000);
    assert_eq!(
        exact
            .values()
            .filter(|o| **o == Object::from_bytes(b"shared"))
            .count(),
        1000
    );
}

#[test]
fn conflicting_payload_sizes_across_separate_native_queries_refuse_before_metadata_publication() {
    let f = Fixture::new();
    f.record("a", "asset/a", true, json!([]), Some(b"shared"));
    let raw = json!({"schema":"tig.instance.design-1","id":"asset/b","definition":f.primary,
        "authored":{"deps":[],"text":"synthetic"},"streams":[],"selected":{"sha256":Object::from_bytes(b"shared").sha256,"bytes":7},"status":{},"metadata":{}});
    write(f.root(), "records/b.asset", &raw);
    let mut snapshot = capture(&reader(f.root()), commit(f.root()));
    let all = roots(&snapshot);
    let mut nodes = MemoryNodes::default();
    assert!(matches!(
        retirement::build(
            &mut snapshot,
            all,
            profiles(&f),
            &mut nodes,
            BindingLimits::default()
        ),
        Err(retirement::Error::Integrity)
    ));
    assert_eq!(nodes.objects().count(), 0);
}
