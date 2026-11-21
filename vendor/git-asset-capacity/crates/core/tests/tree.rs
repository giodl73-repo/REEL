use git_asset_core::{LogicalPath, Object, tree::*};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn lookup_and_edit_require_an_explicit_inventory_bound_without_calling_it_corruption() {
    let mut nodes = MemoryNodes::default();
    let bindings = BTreeMap::from([
        (LogicalPath::parse("a").unwrap(), Object::from_bytes(b"A")),
        (LogicalPath::parse("b").unwrap(), Object::from_bytes(b"B")),
    ]);
    let (tree, _) = Tree::build(&bindings, &mut nodes, TreeLimits::default()).unwrap();
    let small = TreeLimits {
        max_entries: 1,
        ..TreeLimits::default()
    };
    assert_eq!(
        tree.get(&LogicalPath::parse("a").unwrap(), &mut nodes, small)
            .unwrap_err(),
        TreeError::Budget
    );
    let changes = BTreeMap::from([(
        LogicalPath::parse("a").unwrap(),
        Some(Object::from_bytes(b"fixed")),
    )]);
    assert_eq!(
        tree.edit(tree.root(), &changes, &mut nodes, small)
            .unwrap_err(),
        TreeError::Budget
    );
    let admitted = TreeLimits {
        max_entries: 2,
        ..small
    };
    assert_eq!(
        tree.get(&LogicalPath::parse("a").unwrap(), &mut nodes, admitted)
            .unwrap()
            .0,
        Some(Object::from_bytes(b"A"))
    );
    assert!(
        tree.edit(tree.root(), &changes, &mut nodes, admitted)
            .is_ok()
    );
}

fn path(value: &str) -> LogicalPath {
    LogicalPath::parse(value).unwrap()
}
fn blob(value: &str) -> Object {
    Object::from_bytes(value.as_bytes())
}
fn build(values: &[(&str, &str)], nodes: &mut MemoryNodes) -> Tree {
    Tree::build(
        &values.iter().map(|(p, v)| (path(p), blob(v))).collect(),
        nodes,
        TreeLimits::default(),
    )
    .unwrap()
    .0
}

#[test]
fn design_one_wire_vectors_match_independently_hashed_bytes() {
    // Expected strings/hashes were composed and SHA-256 hashed independently
    // with PowerShell/.NET, not by git-asset's serializer or tree builder.
    let mut nodes = MemoryNodes::default();
    let empty = Tree::empty(&mut nodes).unwrap();
    assert_eq!(
        empty.root().sha256.as_str(),
        "8758bf7289ab9631f523aa740e20fc86551b5334fe1c1135d9e5cf82378844a0"
    );
    let one = build(&[("a", "A")], &mut nodes);
    assert_eq!(
        one.root().sha256.as_str(),
        "c22c76999ff64a883574461f7ba23d1b44a9ad7f59716cf281a1e8c83c003eb6"
    );
    assert_eq!(one.root().bytes, 155);
    assert_eq!(
        nodes.get(one.root()).unwrap().unwrap(),
        br#"{"root":{"entries":1,"object":{"bytes":192,"sha256":"782b61d025f83eaab3911f9f205aafb0122ab839638fc8a5aaafe5d5d61ca9dc"}},"schema":"tig.directory.design-1"}"#
    );
}

#[test]
fn failed_publication_keeps_the_old_root_readable_and_retryable() {
    struct FailingNodes {
        inner: MemoryNodes,
        remaining: usize,
    }
    impl NodeStore for FailingNodes {
        fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
            self.inner.get(object)
        }
        fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
            if self.remaining == 0 {
                return Err(TreeError::Store("injected metadata write failure".into()));
            }
            self.remaining -= 1;
            self.inner.put(bytes)
        }
    }
    let mut inner = MemoryNodes::default();
    let old = build(&[("scene/a", "A"), ("scene/b", "B")], &mut inner);
    let before = inner.objects().count();
    let changes = BTreeMap::from([(path("scene/b"), Some(blob("replacement")))]);
    let mut nodes = FailingNodes {
        inner,
        remaining: 1,
    };
    assert!(matches!(
        old.edit(old.root(), &changes, &mut nodes, TreeLimits::default()),
        Err(TreeError::Store(_))
    ));
    // Immutable orphan metadata can remain; failed edits never adopt a root.
    assert!(nodes.inner.objects().count() > before);
    assert_eq!(
        old.get(&path("scene/b"), &mut nodes, TreeLimits::default())
            .unwrap()
            .0,
        Some(blob("B"))
    );
    nodes.remaining = 100;
    let new = old
        .edit(old.root(), &changes, &mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    assert_eq!(
        new.get(&path("scene/b"), &mut nodes, TreeLimits::default())
            .unwrap()
            .0,
        Some(blob("replacement"))
    );
}

#[test]
fn exact_paths_reject_escape_reserved_and_ambiguous_components() {
    for invalid in [
        "",
        "/a",
        "a/",
        "a//b",
        "a/../b",
        "./a",
        "a\\b",
        "C:/a",
        ".git/config",
        ".tig/state",
        "AUX.txt",
        "COM1",
        "a.",
        "a ",
        "a\0b",
        "a?b",
    ] {
        assert!(LogicalPath::parse(invalid).is_err(), "{invalid:?}");
    }
    assert!(LogicalPath::parse(&"x/".repeat(33)).is_err());
    assert!(LogicalPath::parse(&"x".repeat(1025)).is_err());
    assert_ne!(path("é.png"), path("e\u{301}.png"));
    assert_eq!(path("scene-04/cel-b.png").as_str(), "scene-04/cel-b.png");
}

#[test]
fn empty_tree_and_missing_binding_are_explicit() {
    let mut nodes = MemoryNodes::default();
    let tree = Tree::empty(&mut nodes).unwrap();
    assert!(
        tree.entries(&mut nodes, TreeLimits::default())
            .unwrap()
            .0
            .is_empty()
    );
    assert_eq!(
        tree.get(&path("missing"), &mut nodes, TreeLimits::default())
            .unwrap()
            .0,
        None
    );
}

#[test]
fn tree_root_is_canonical_across_bulk_and_reverse_edit_order() {
    let mut nodes = MemoryNodes::default();
    let bulk = build(&[("s/a", "A"), ("s/b", "B"), ("q", "Q")], &mut nodes);
    let mut incremental = Tree::empty(&mut nodes).unwrap();
    for (p, v) in [("s/b", "B"), ("q", "Q"), ("s/a", "A")] {
        incremental = incremental
            .edit(
                incremental.root(),
                &BTreeMap::from([(path(p), Some(blob(v)))]),
                &mut nodes,
                TreeLimits::default(),
            )
            .unwrap()
            .0;
    }
    assert_eq!(incremental.root(), bulk.root());
}

#[test]
fn one_of_a_thousand_changed_bindings_reuses_immutable_nodes_and_old_view() {
    let mut nodes = MemoryNodes::default();
    let bindings: BTreeMap<_, _> = (0..1000)
        .map(|n| {
            (
                path(&format!("scene/cel-{n:04}.png")),
                blob(&format!("image-{n}")),
            )
        })
        .collect();
    let (old, built) = Tree::build(&bindings, &mut nodes, TreeLimits::default()).unwrap();
    assert_eq!(built.reads, 0);
    assert_eq!(built.writes, 1003);
    let original_nodes: BTreeSet<_> = nodes.objects().map(|(key, _)| key.clone()).collect();
    let changed = path("scene/cel-0500.png");
    let (new, delta) = old
        .edit(
            old.root(),
            &BTreeMap::from([(changed.clone(), Some(blob("new-image")))]),
            &mut nodes,
            TreeLimits::default(),
        )
        .unwrap();
    assert_ne!(new.root(), old.root());
    assert!(delta.writes < 40, "{delta:?}");
    assert!(delta.write_bytes < 20_000, "{delta:?}");
    let additions = nodes
        .objects()
        .filter(|(key, _)| !original_nodes.contains(*key))
        .count();
    eprintln!("1000-binding sparse update: {delta:?}; new stored metadata nodes={additions}");
    assert!(additions < 40);
    assert_eq!(
        old.get(&changed, &mut nodes, TreeLimits::default())
            .unwrap()
            .0,
        Some(blob("image-500"))
    );
    assert_eq!(
        new.get(&changed, &mut nodes, TreeLimits::default())
            .unwrap()
            .0,
        Some(blob("new-image"))
    );
    let (same, lookup) = new
        .get(
            &path("scene/cel-0001.png"),
            &mut nodes,
            TreeLimits::default(),
        )
        .unwrap();
    assert_eq!(same, Some(blob("image-1")));
    assert!(lookup.reads < 40);
}

#[test]
fn remove_and_restore_recovers_same_canonical_root_without_deleting_bytes() {
    let mut nodes = MemoryNodes::default();
    let old = build(&[("s/a", "A"), ("s/b", "B")], &mut nodes);
    let (removed, _) = old
        .edit(
            old.root(),
            &BTreeMap::from([(path("s/b"), None)]),
            &mut nodes,
            TreeLimits::default(),
        )
        .unwrap();
    assert_eq!(
        removed
            .get(&path("s/b"), &mut nodes, TreeLimits::default())
            .unwrap()
            .0,
        None
    );
    let (restored, _) = removed
        .edit(
            removed.root(),
            &BTreeMap::from([(path("s/b"), Some(blob("B")))]),
            &mut nodes,
            TreeLimits::default(),
        )
        .unwrap();
    assert_eq!(restored.root(), old.root());
    assert_eq!(
        old.entries(&mut nodes, TreeLimits::default())
            .unwrap()
            .0
            .len(),
        2
    );
}

#[test]
fn delete_last_leaf_prunes_only_logical_empty_directories() {
    let mut nodes = MemoryNodes::default();
    let old = build(&[("a/b/c", "value")], &mut nodes);
    let empty = Tree::empty(&mut nodes).unwrap();
    let (new, _) = old
        .edit(
            old.root(),
            &BTreeMap::from([(path("a/b/c"), None)]),
            &mut nodes,
            TreeLimits::default(),
        )
        .unwrap();
    assert_eq!(new.root(), empty.root());
    assert_eq!(
        old.get(&path("a/b/c"), &mut nodes, TreeLimits::default())
            .unwrap()
            .0,
        Some(blob("value"))
    );
}

#[test]
fn file_directory_collisions_are_rejected_in_build_and_edit() {
    let mut nodes = MemoryNodes::default();
    let conflicting = BTreeMap::from([(path("a"), blob("A")), (path("a/b"), blob("B"))]);
    assert_eq!(
        Tree::build(&conflicting, &mut nodes, TreeLimits::default()).unwrap_err(),
        TreeError::PathConflict
    );
    let tree = build(&[("a/b", "B")], &mut nodes);
    assert_eq!(
        tree.edit(
            tree.root(),
            &BTreeMap::from([(path("a"), Some(blob("A")))]),
            &mut nodes,
            TreeLimits::default()
        )
        .unwrap_err(),
        TreeError::PathConflict
    );
    let tree = build(&[("a", "A")], &mut nodes);
    assert_eq!(
        tree.edit(
            tree.root(),
            &BTreeMap::from([(path("a/b"), Some(blob("B")))]),
            &mut nodes,
            TreeLimits::default()
        )
        .unwrap_err(),
        TreeError::PathConflict
    );
}

#[test]
fn stale_root_noops_and_budgets_do_not_replace_the_snapshot() {
    let mut nodes = MemoryNodes::default();
    let tree = build(&[("a", "A")], &mut nodes);
    let edits = BTreeMap::from([(path("a"), Some(blob("B")))]);
    assert_eq!(
        tree.edit(
            &blob("wrong-root"),
            &edits,
            &mut nodes,
            TreeLimits::default()
        )
        .unwrap_err(),
        TreeError::StaleRoot
    );
    let limited = TreeLimits {
        max_writes: 0,
        ..TreeLimits::default()
    };
    assert_eq!(
        tree.edit(tree.root(), &edits, &mut nodes, limited)
            .unwrap_err(),
        TreeError::Budget
    );
    assert_eq!(
        tree.get(&path("a"), &mut nodes, TreeLimits::default())
            .unwrap()
            .0,
        Some(blob("A"))
    );
    let (same, metrics) = tree
        .edit(
            tree.root(),
            &BTreeMap::from([(path("a"), Some(blob("A")))]),
            &mut nodes,
            TreeLimits::default(),
        )
        .unwrap();
    assert_eq!(same.root(), tree.root());
    assert_eq!(metrics.writes, 0);
    let no_reads = TreeLimits {
        max_reads: 0,
        ..TreeLimits::default()
    };
    assert_eq!(
        tree.get(&path("a"), &mut nodes, no_reads).unwrap_err(),
        TreeError::Budget
    );
}

#[test]
fn missing_corrupt_or_unknown_nodes_never_become_empty_success() {
    let mut nodes = MemoryNodes::default();
    let tree = build(&[("a", "A")], &mut nodes);
    let mut missing = MemoryNodes::default();
    assert!(matches!(
        tree.entries(&mut missing, TreeLimits::default()),
        Err(TreeError::MissingNode(_))
    ));
    struct Corrupt;
    impl NodeStore for Corrupt {
        fn get(&self, _: &Object) -> TreeResult<Option<Vec<u8>>> {
            Ok(Some(b"wrong bytes".to_vec()))
        }
        fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
            Ok(blob("wrong returned identity"))
        }
    }
    assert_eq!(
        tree.entries(&mut Corrupt, TreeLimits::default())
            .unwrap_err(),
        TreeError::Integrity
    );
    assert_eq!(Tree::empty(&mut Corrupt).unwrap_err(), TreeError::Integrity);
    let unknown = nodes
        .put(br#"{"schema":"future-unsupported","root":null}"#)
        .unwrap();
    assert_eq!(
        Tree::from_root(unknown)
            .entries(&mut nodes, TreeLimits::default())
            .unwrap_err(),
        TreeError::InvalidNode
    );
}

#[test]
fn conflicting_object_sizes_and_complete_traversal_limits_reject() {
    let mut nodes = MemoryNodes::default();
    let mut wrong = blob("A");
    wrong.bytes += 1;
    assert_eq!(
        Tree::build(
            &BTreeMap::from([(path("a"), blob("A")), (path("b"), wrong)]),
            &mut nodes,
            TreeLimits::default()
        )
        .unwrap_err(),
        TreeError::Integrity
    );
    let tree = build(&[("a", "A"), ("b", "B")], &mut nodes);
    let mut wrong = blob("same replacement");
    wrong.bytes += 1;
    let writes_before = nodes.objects().count();
    assert_eq!(
        tree.edit(
            tree.root(),
            &BTreeMap::from([
                (path("a"), Some(blob("same replacement"))),
                (path("b"), Some(wrong)),
            ]),
            &mut nodes,
            TreeLimits::default(),
        )
        .unwrap_err(),
        TreeError::Integrity
    );
    assert_eq!(nodes.objects().count(), writes_before);
    let tiny = TreeLimits {
        max_entries: 1,
        ..TreeLimits::default()
    };
    assert!(tree.entries(&mut nodes, tiny).is_err());
    let tiny = TreeLimits {
        max_map_depth: 0,
        ..TreeLimits::default()
    };
    assert_eq!(
        tree.get(&path("a"), &mut nodes, tiny).unwrap_err(),
        TreeError::Budget
    );
}

#[test]
fn deterministic_edit_sequences_match_a_reference_map_and_bulk_root() {
    let mut nodes = MemoryNodes::default();
    let mut tree = Tree::empty(&mut nodes).unwrap();
    let mut expected = BTreeMap::new();
    let mut seed = 7u64;
    for step in 0..120 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let key = path(&format!(
            "scene-{}/file-{}",
            (seed >> 32) % 3,
            (seed >> 16) % 17
        ));
        let value = if seed.is_multiple_of(4) {
            None
        } else {
            Some(blob(&format!("revision-{step}")))
        };
        tree = tree
            .edit(
                tree.root(),
                &BTreeMap::from([(key.clone(), value.clone())]),
                &mut nodes,
                TreeLimits::default(),
            )
            .unwrap()
            .0;
        if let Some(value) = value {
            expected.insert(key, value);
        } else {
            expected.remove(&key);
        }
        assert_eq!(
            tree.entries(&mut nodes, TreeLimits::default()).unwrap().0,
            expected
        );
        let rebuilt = Tree::build(&expected, &mut nodes, TreeLimits::default())
            .unwrap()
            .0;
        assert_eq!(tree.root(), rebuilt.root());
    }
}

#[test]
fn five_replacements_in_a_thousand_touch_only_the_changed_tree_paths() {
    let mut nodes = MemoryNodes::default();
    let mut bindings: BTreeMap<_, _> = (0..1000)
        .map(|n| (path(&format!("s/cel-{n:04}")), blob(&format!("old-{n}"))))
        .collect();
    let old = Tree::build(&bindings, &mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    let changes: BTreeMap<_, _> = [2, 101, 450, 723, 998]
        .into_iter()
        .map(|n| {
            let key = path(&format!("s/cel-{n:04}"));
            let value = blob(&format!("new-{n}"));
            bindings.insert(key.clone(), value.clone());
            (key, Some(value))
        })
        .collect();
    let (new, metrics) = old
        .edit(old.root(), &changes, &mut nodes, TreeLimits::default())
        .unwrap();
    assert!(metrics.writes < 150, "{metrics:?}");
    assert!(metrics.write_bytes < 60_000, "{metrics:?}");
    assert_eq!(
        new.entries(&mut nodes, TreeLimits::default()).unwrap().0,
        bindings
    );
    let rebuilt = Tree::build(&bindings, &mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    assert_eq!(new.root(), rebuilt.root());
    assert_eq!(
        old.get(&path("s/cel-0450"), &mut nodes, TreeLimits::default())
            .unwrap()
            .0,
        Some(blob("old-450"))
    );
    eprintln!("1000-binding five replacements: {metrics:?}");
}

#[test]
fn missing_required_fields_noncanonical_bytes_and_false_counts_reject() {
    let mut nodes = MemoryNodes::default();
    for bad in [
        br#"{"schema":"tig.directory.design-1"}"#.as_slice(),
        br#"{ "root": null, "schema": "tig.directory.design-1" }"#,
        br#"{"root":null,"schema":"tig.directory.design-1","unknown":true}"#,
    ] {
        let object = nodes.put(bad).unwrap();
        assert_eq!(
            Tree::from_root(object)
                .entries(&mut nodes, TreeLimits::default())
                .unwrap_err(),
            TreeError::InvalidNode
        );
    }
    let tree = build(&[("a", "A")], &mut nodes);
    let bytes = nodes.get(tree.root()).unwrap().unwrap();
    let mut value = git_asset_core::identity::parse_json(&bytes).unwrap();
    value["root"]["entries"] = serde_json::json!(2);
    let malformed = nodes
        .put(&git_asset_core::identity::canonical_json(&value).unwrap())
        .unwrap();
    assert_eq!(
        Tree::from_root(malformed)
            .entries(&mut nodes, TreeLimits::default())
            .unwrap_err(),
        TreeError::InvalidNode
    );
}

#[test]
fn byte_and_count_admission_happens_before_node_publication() {
    let mut nodes = MemoryNodes::default();
    let limit = TreeLimits {
        max_write_bytes: 0,
        ..TreeLimits::default()
    };
    assert_eq!(
        Tree::build(&BTreeMap::new(), &mut nodes, limit).unwrap_err(),
        TreeError::Budget
    );
    assert_eq!(nodes.objects().count(), 0);
    let values = BTreeMap::from([(path("a"), blob("A")), (path("b"), blob("B"))]);
    let limit = TreeLimits {
        max_entries: 1,
        ..TreeLimits::default()
    };
    assert_eq!(
        Tree::build(&values, &mut nodes, limit).unwrap_err(),
        TreeError::Budget
    );
    assert_eq!(nodes.objects().count(), 0);
    let oversized = Object {
        sha256: blob("x").sha256,
        bytes: 65_537,
    };
    assert_eq!(
        Tree::from_root(oversized)
            .entries(&mut nodes, TreeLimits::default())
            .unwrap_err(),
        TreeError::Budget
    );
}
