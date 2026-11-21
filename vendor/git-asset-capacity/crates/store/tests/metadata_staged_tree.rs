use git_asset_core::{
    LogicalPath, Object,
    tree::{Tree, TreeLimits},
};
use git_asset_store::metadata::{Limits, MetadataStore};
use std::collections::BTreeMap;

fn path(n: usize) -> LogicalPath {
    LogicalPath::parse(&format!("op/{n:08}")).unwrap()
}

#[test]
fn full_segment_staged_index_finishes_reopens_and_preserves_original_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("metadata");
    let mut store = MetadataStore::create(
        &root,
        Limits {
            nodes: 50_000,
            bytes: 32 * 1024 * 1024,
            database_bytes: 128 * 1024 * 1024,
            min_free_bytes: 65_536,
        },
    )
    .unwrap();
    let old = Object::from_bytes(b"original historical receipt");
    let new = Object::from_bytes(b"next historical receipt");
    let bindings: BTreeMap<_, _> = (0..4096).map(|n| (path(n), old.clone())).collect();
    let (base, _) = Tree::build(&bindings, &mut store, TreeLimits::default()).unwrap();
    let original = store.finish().unwrap();
    let changes = (4096..20480)
        .map(|n| (path(n), Some(new.clone())))
        .collect();
    let limits = TreeLimits {
        max_reads: 2_000_000,
        max_writes: 1_000_000,
        max_read_bytes: 512 * 1024 * 1024,
        max_write_bytes: 256 * 1024 * 1024,
        ..TreeLimits::default()
    };
    let (next, metrics) = base
        .edit_staged(base.root(), &changes, &mut store, limits)
        .unwrap();
    let finished = store.finish().unwrap();
    assert!(finished.nodes() > original.nodes());
    assert!(finished.nodes() - original.nodes() <= metrics.published_nodes as u64);
    assert!(finished.bytes() - original.bytes() <= metrics.published_bytes as u64);
    assert!(metrics.published_nodes <= 20484);
    drop(store);
    let mut reopened = MetadataStore::open(&root, &finished).unwrap();
    let expected = (0..20480)
        .map(|n| (path(n), if n < 4096 { old.clone() } else { new.clone() }))
        .collect();
    assert_eq!(
        next.entries(&mut reopened, TreeLimits::default())
            .unwrap()
            .0,
        expected
    );
    assert_eq!(
        base.entries(&mut reopened, TreeLimits::default())
            .unwrap()
            .0,
        bindings
    );
    assert_eq!(reopened.finish().unwrap(), finished);
    eprintln!(
        "physical staged index: nodes {} -> {}, bytes {} -> {}, {metrics:?}",
        original.nodes(),
        finished.nodes(),
        original.bytes(),
        finished.bytes()
    );
}
