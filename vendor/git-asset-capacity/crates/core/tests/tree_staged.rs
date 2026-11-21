use git_asset_core::{
    Digest, LogicalPath, Object,
    tree::{MemoryNodes, NodeStore, Tree, TreeError, TreeLimits, TreeResult},
};
use std::{cell::Cell, collections::BTreeMap};

fn path(s: &str) -> LogicalPath {
    LogicalPath::parse(s).unwrap()
}
fn object(s: &str) -> Object {
    Object::from_bytes(s.as_bytes())
}

#[derive(Default)]
struct Port {
    nodes: MemoryNodes,
    gets: Cell<usize>,
    published: Vec<Object>,
    fail_after: Option<usize>,
    wrong_ack: bool,
    corrupt: bool,
    missing: bool,
}
impl NodeStore for Port {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.gets.set(self.gets.get() + 1);
        if self.missing {
            return Ok(None);
        }
        if self.corrupt {
            return Ok(Some(b"bad".to_vec()));
        }
        self.nodes.get(o)
    }
    fn put(&mut self, b: &[u8]) -> TreeResult<Object> {
        if self.fail_after == Some(self.published.len()) {
            return Err(TreeError::Store("fixture-put".into()));
        }
        let value: serde_json::Value = serde_json::from_slice(b).unwrap();
        let mut children = Vec::new();
        if value["schema"] == "tig.directory.design-1" {
            if !value["root"].is_null() {
                children.push(value["root"]["object"].clone());
            }
        } else {
            for side in ["left", "right"] {
                if !value[side].is_null() {
                    children.push(value[side]["object"].clone());
                }
            }
            if value["value"]["kind"] == "directory" {
                children.push(value["value"]["object"].clone());
            }
        }
        for child in children {
            let child: Object = serde_json::from_value(child).unwrap();
            assert!(
                self.nodes.get(&child)?.is_some(),
                "children precede parents"
            );
        }
        let o = self.nodes.put(b)?;
        self.published.push(o.clone());
        Ok(if self.wrong_ack { object("wrong") } else { o })
    }
}

#[test]
fn exact_design_one_vector_and_staged_file_alias_stay_opaque() {
    let (base, mut port) = setup(&BTreeMap::new());
    let one = BTreeMap::from([(path("a"), Some(Object::from_bytes(b"A")))]);
    let (next, _) = base
        .edit_staged(base.root(), &one, &mut port, TreeLimits::default())
        .unwrap();
    assert_eq!(
        next.root().sha256.as_str(),
        "c22c76999ff64a883574461f7ba23d1b44a9ad7f59716cf281a1e8c83c003eb6"
    );
    port.published.clear();
    // Creating a/x stages an empty child directory. b's file object refers to
    // those same bytes, but file edges must not publish that metadata node.
    let changes = BTreeMap::from([
        (path("a/x"), Some(object("payload"))),
        (path("b"), Some(base.root().clone())),
    ]);
    let (nested, _) = base
        .edit_staged(base.root(), &changes, &mut port, TreeLimits::default())
        .unwrap();
    assert!(!port.published.contains(base.root()));
    assert_eq!(
        nested
            .get(&path("b"), &mut port, TreeLimits::default())
            .unwrap()
            .0,
        Some(base.root().clone())
    );
}
fn setup(bindings: &BTreeMap<LogicalPath, Object>) -> (Tree, Port) {
    let mut port = Port::default();
    let tree = Tree::build(bindings, &mut port, TreeLimits::default())
        .unwrap()
        .0;
    port.published.clear();
    (tree, port)
}

#[test]
fn leaf_update_uses_one_checked_traversal_and_preserves_zero_write_noops() {
    let (base, mut port) = setup(&BTreeMap::from([(path("a"), object("old"))]));
    let limits = TreeLimits {
        max_reads: 2,
        ..TreeLimits::default()
    };
    let changes = BTreeMap::from([(path("a"), Some(object("new")))]);
    let (next, metrics) = base
        .edit_staged(base.root(), &changes, &mut port, limits)
        .unwrap();
    assert_eq!(metrics.tree.reads, 2);
    let mut independent = MemoryNodes::default();
    let expected = Tree::build(
        &BTreeMap::from([(path("a"), object("new"))]),
        &mut independent,
        TreeLimits::default(),
    )
    .unwrap()
    .0;
    assert_eq!(next, expected);
    let (same, metrics) = base
        .edit_staged(
            base.root(),
            &BTreeMap::from([(path("z"), None)]),
            &mut port,
            TreeLimits {
                max_writes: 0,
                ..limits
            },
        )
        .unwrap();
    assert_eq!(same, base);
    assert_eq!(metrics.tree.writes, 0);
    let (empty, _) = base
        .edit_staged(
            base.root(),
            &BTreeMap::from([(path("a"), None)]),
            &mut port,
            limits,
        )
        .unwrap();
    assert_eq!(
        empty.root().sha256.as_str(),
        "8758bf7289ab9631f523aa740e20fc86551b5334fe1c1135d9e5cf82378844a0"
    );
    let (directory, mut port) = setup(&BTreeMap::from([(path("a/x"), object("old"))]));
    for value in [None, Some(object("conflict"))] {
        assert_eq!(
            directory
                .edit_staged(
                    directory.root(),
                    &BTreeMap::from([(path("a"), value)]),
                    &mut port,
                    TreeLimits::default()
                )
                .unwrap_err(),
            TreeError::PathConflict
        );
        assert!(port.published.is_empty());
    }
}

#[test]
fn conditional_insert_matches_bulk_without_a_separate_absence_traversal() {
    let (empty, mut port) = setup(&BTreeMap::new());
    let one = BTreeMap::from([(path("a"), object("A"))]);
    let (first, metrics) = empty
        .insert_staged(
            empty.root(),
            &one,
            &mut port,
            TreeLimits {
                max_reads: 1,
                ..TreeLimits::default()
            },
        )
        .unwrap();
    assert_eq!(metrics.tree.reads, 1);
    assert_eq!(
        first.root().sha256.as_str(),
        "c22c76999ff64a883574461f7ba23d1b44a9ad7f59716cf281a1e8c83c003eb6"
    );
    let mut bindings: BTreeMap<_, _> = (0..300)
        .map(|i| (path(&format!("stable/key{i:04}")), object("old")))
        .collect();
    let (base, mut port) = setup(&bindings);
    let insertions: BTreeMap<_, _> = (300..450)
        .map(|i| (path(&format!("stable/key{i:04}")), object("new")))
        .chain([(path("other/nested/key"), object("other"))])
        .collect();
    let (next, _) = base
        .insert_staged(base.root(), &insertions, &mut port, TreeLimits::default())
        .unwrap();
    bindings.extend(insertions);
    let mut independent = MemoryNodes::default();
    let expected = Tree::build(&bindings, &mut independent, TreeLimits::default())
        .unwrap()
        .0;
    assert_eq!(next, expected);
    assert_eq!(
        next.entries(&mut port, TreeLimits::default()).unwrap().0,
        bindings
    );
    assert_eq!(
        base.get(&path("stable/key0000"), &mut port, TreeLimits::default())
            .unwrap()
            .0,
        Some(object("old"))
    );
}

#[test]
fn conditional_insert_conflicts_even_on_identical_files_and_flushes_no_partial_batch() {
    let (base, mut port) = setup(&BTreeMap::from([
        (path("z"), object("old")),
        (path("directory/x"), object("nested")),
    ]));
    let before = port.nodes.objects().count();
    for insertions in [
        BTreeMap::from([
            (path("a"), object("staged first")),
            (path("z"), object("old")),
        ]),
        BTreeMap::from([(path("z"), object("replacement"))]),
        BTreeMap::from([(path("z/x"), object("under file"))]),
        BTreeMap::from([(path("directory"), object("over directory"))]),
        BTreeMap::from([
            (path("a"), object("parent")),
            (path("a/x"), object("child")),
        ]),
    ] {
        assert_eq!(
            base.insert_staged(base.root(), &insertions, &mut port, TreeLimits::default())
                .unwrap_err(),
            TreeError::PathConflict
        );
        assert!(port.published.is_empty());
        assert_eq!(port.nodes.objects().count(), before);
    }
    assert_eq!(
        base.edit_staged(
            base.root(),
            &BTreeMap::from([(path("z"), Some(object("old")))]),
            &mut port,
            TreeLimits::default()
        )
        .unwrap()
        .0,
        base
    );
}

#[test]
fn conditional_insert_validates_exact_root_sizes_and_whole_operation_budgets() {
    let (base, mut port) = setup(&BTreeMap::new());
    let insertions = BTreeMap::from([
        (path("a"), object("A")),
        (path("b"), object("B")),
        (path("c"), object("C")),
    ]);
    let (_, used) = base
        .insert_staged(base.root(), &insertions, &mut port, TreeLimits::default())
        .unwrap();
    port.published.clear();
    for limits in [
        TreeLimits {
            max_reads: used.tree.reads - 1,
            ..TreeLimits::default()
        },
        TreeLimits {
            max_writes: used.tree.writes - 1,
            ..TreeLimits::default()
        },
        TreeLimits {
            max_read_bytes: used.tree.read_bytes - 1,
            ..TreeLimits::default()
        },
        TreeLimits {
            max_write_bytes: used.tree.write_bytes - 1,
            ..TreeLimits::default()
        },
    ] {
        assert_eq!(
            base.insert_staged(base.root(), &insertions, &mut port, limits)
                .unwrap_err(),
            TreeError::Budget
        );
        assert!(port.published.is_empty());
    }
    port.gets.set(0);
    assert_eq!(
        base.insert_staged(
            &object("stale"),
            &insertions,
            &mut port,
            TreeLimits::default()
        )
        .unwrap_err(),
        TreeError::StaleRoot
    );
    let inconsistent = BTreeMap::from([
        (path("a"), object("A")),
        (
            path("b"),
            Object {
                sha256: object("A").sha256,
                bytes: 99,
            },
        ),
    ]);
    assert_eq!(
        base.insert_staged(base.root(), &inconsistent, &mut port, TreeLimits::default())
            .unwrap_err(),
        TreeError::Integrity
    );
    assert_eq!(port.gets.get(), 0);
    port.missing = true;
    assert_eq!(
        base.insert_staged(base.root(), &insertions, &mut port, TreeLimits::default())
            .unwrap_err(),
        TreeError::MissingNode(base.root().clone())
    );
    port.missing = false;
    port.corrupt = true;
    assert_eq!(
        base.insert_staged(base.root(), &insertions, &mut port, TreeLimits::default())
            .unwrap_err(),
        TreeError::Integrity
    );
    assert!(port.published.is_empty());
}

#[test]
fn combined_leaf_traversal_refuses_rehashed_bst_and_heap_corruption() {
    for heap in [false, true] {
        let (base, mut port) = setup(&BTreeMap::from([
            (path("a"), object("A")),
            (path("z"), object("Z")),
        ]));
        let mut directory: serde_json::Value =
            serde_json::from_slice(&port.nodes.get(base.root()).unwrap().unwrap()).unwrap();
        let map_object: Object =
            serde_json::from_value(directory["root"]["object"].clone()).unwrap();
        let mut parent: serde_json::Value =
            serde_json::from_slice(&port.nodes.get(&map_object).unwrap().unwrap()).unwrap();
        let old_side = if parent["left"].is_null() {
            "right"
        } else {
            "left"
        };
        let child_object: Object =
            serde_json::from_value(parent[old_side]["object"].clone()).unwrap();
        let mut child: serde_json::Value =
            serde_json::from_slice(&port.nodes.get(&child_object).unwrap().unwrap()).unwrap();
        let original_parent = parent["name"].clone();
        let original_child = child["name"].clone();
        child["name"] = original_parent.clone();
        let side = if heap {
            parent["name"] = original_child;
            let link = parent[old_side].take();
            let opposite = if old_side == "left" { "right" } else { "left" };
            parent[opposite] = link;
            opposite
        } else {
            old_side
        };
        let changed = port
            .nodes
            .put(&git_asset_core::identity::canonical_json(&child).unwrap())
            .unwrap();
        parent[side]["object"] = serde_json::to_value(changed).unwrap();
        let changed = port
            .nodes
            .put(&git_asset_core::identity::canonical_json(&parent).unwrap())
            .unwrap();
        directory["root"]["object"] = serde_json::to_value(changed).unwrap();
        let bad = Tree::from_root(
            port.nodes
                .put(&git_asset_core::identity::canonical_json(&directory).unwrap())
                .unwrap(),
        );
        let target = if heap {
            original_parent.as_str().unwrap()
        } else if side == "left" {
            "0"
        } else {
            "zzzz"
        };
        assert_eq!(
            bad.get(&path(target), &mut port, TreeLimits::default())
                .unwrap_err(),
            TreeError::InvalidNode
        );
        assert_eq!(
            bad.edit_staged(
                bad.root(),
                &BTreeMap::from([(path(target), Some(object("replacement")))]),
                &mut port,
                TreeLimits::default()
            )
            .unwrap_err(),
            TreeError::InvalidNode
        );
        assert_eq!(
            bad.insert_staged(
                bad.root(),
                &BTreeMap::from([(path(target), object("new"))]),
                &mut port,
                TreeLimits::default()
            )
            .unwrap_err(),
            TreeError::InvalidNode
        );
        assert!(port.published.is_empty());
    }
}

#[test]
fn staged_root_matches_independent_build_and_sequential_edits() {
    for seed in 0..12 {
        let mut bindings: BTreeMap<_, _> = (0..60)
            .map(|i| (path(&format!("group{}/key{i:03}", i % 4)), object("old")))
            .collect();
        let (base, mut staged) = setup(&bindings);
        let (_, mut sequential) = setup(&bindings);
        // Reversed/rotated generation still goes through exact ordered paths.
        let changes: BTreeMap<_, _> = (0..100)
            .rev()
            .map(|n| {
                let i = (n + seed * 7) % 100;
                let key = path(&format!("group{}/key{i:03}", i % 4));
                let value = (i % 3 != 0).then(|| object(&format!("new{i}")));
                (key, value)
            })
            .collect();
        for (key, value) in &changes {
            if let Some(value) = value {
                bindings.insert(key.clone(), value.clone());
            } else {
                bindings.remove(key);
            }
        }
        let (next, metrics) = base
            .edit_staged(base.root(), &changes, &mut staged, TreeLimits::default())
            .unwrap();
        let (ordinary, logical) = base
            .edit(
                base.root(),
                &changes,
                &mut sequential,
                TreeLimits::default(),
            )
            .unwrap();
        let mut built = MemoryNodes::default();
        let independent = Tree::build(&bindings, &mut built, TreeLimits::default())
            .unwrap()
            .0;
        assert_eq!(next, ordinary);
        assert_eq!(next, independent);
        assert_eq!(metrics.tree, logical);
        assert_eq!(
            next.entries(&mut staged, TreeLimits::default()).unwrap().0,
            bindings
        );
        let closure: BTreeMap<_, _> = built.objects().map(|(d, b)| (d.clone(), b.len())).collect();
        assert!(
            staged
                .published
                .iter()
                .all(|o| closure.contains_key(&o.sha256))
        );
        let unique: std::collections::BTreeSet<_> = staged.published.iter().collect();
        assert_eq!(unique.len(), staged.published.len());
        assert_eq!(metrics.published_nodes, staged.published.len());
        assert_eq!(
            metrics.published_bytes as u64,
            staged.published.iter().map(|o| o.bytes).sum::<u64>()
        );
        assert!(metrics.published_nodes * 3 < metrics.tree.writes);
    }
}

#[test]
fn whole_operation_limits_refuse_before_any_provider_put() {
    let (base, mut port) = setup(&BTreeMap::from([(path("a"), object("old"))]));
    let changes = BTreeMap::from([
        (path("a"), Some(object("new"))),
        (path("b"), Some(object("b"))),
    ]);
    let mut reference = MemoryNodes::default();
    let (other, _) = Tree::build(
        &BTreeMap::from([(path("a"), object("old"))]),
        &mut reference,
        TreeLimits::default(),
    )
    .unwrap();
    let (_, work) = other
        .edit(
            other.root(),
            &changes,
            &mut reference,
            TreeLimits::default(),
        )
        .unwrap();
    for limits in [
        TreeLimits {
            max_reads: work.reads - 1,
            ..TreeLimits::default()
        },
        TreeLimits {
            max_writes: work.writes - 1,
            ..TreeLimits::default()
        },
        TreeLimits {
            max_read_bytes: work.read_bytes - 1,
            ..TreeLimits::default()
        },
        TreeLimits {
            max_write_bytes: work.write_bytes - 1,
            ..TreeLimits::default()
        },
        TreeLimits {
            max_entries: 1,
            ..TreeLimits::default()
        },
        TreeLimits {
            max_map_depth: 0,
            ..TreeLimits::default()
        },
    ] {
        assert_eq!(
            base.edit_staged(base.root(), &changes, &mut port, limits)
                .unwrap_err(),
            TreeError::Budget
        );
        assert!(port.published.is_empty());
    }
    let exact = TreeLimits {
        max_reads: work.reads,
        max_writes: work.writes,
        max_read_bytes: work.read_bytes,
        max_write_bytes: work.write_bytes,
        ..TreeLimits::default()
    };
    assert!(
        base.edit_staged(base.root(), &changes, &mut port, exact)
            .is_ok()
    );
}

#[test]
fn staging_errors_and_noops_never_publish_or_adopt() {
    let (base, mut port) = setup(&BTreeMap::from([(path("a/x"), object("old"))]));
    let empty = BTreeMap::new();
    let (same, metrics) = base
        .edit_staged(base.root(), &empty, &mut port, TreeLimits::default())
        .unwrap();
    assert_eq!(same, base);
    assert_eq!(metrics.published_nodes, 0);
    assert_eq!(port.gets.get(), 0);
    let noop = BTreeMap::from([
        (path("a/x"), Some(object("old"))),
        (path("absent/child"), None),
    ]);
    assert_eq!(
        base.edit_staged(base.root(), &noop, &mut port, TreeLimits::default())
            .unwrap()
            .0,
        base
    );
    let conflict = BTreeMap::from([
        (path("0"), Some(object("staged first"))),
        (path("a"), Some(object("bad"))),
    ]);
    assert_eq!(
        base.edit_staged(base.root(), &conflict, &mut port, TreeLimits::default())
            .unwrap_err(),
        TreeError::PathConflict
    );
    assert!(port.published.is_empty());
    port.gets.set(0);
    assert_eq!(
        base.edit_staged(
            &object("stale"),
            &conflict,
            &mut port,
            TreeLimits::default()
        )
        .unwrap_err(),
        TreeError::StaleRoot
    );
    assert_eq!(port.gets.get(), 0);
    let inconsistent = BTreeMap::from([
        (path("x"), Some(object("old"))),
        (
            path("y"),
            Some(Object {
                sha256: object("old").sha256,
                bytes: 99,
            }),
        ),
    ]);
    assert_eq!(
        base.edit_staged(base.root(), &inconsistent, &mut port, TreeLimits::default())
            .unwrap_err(),
        TreeError::Integrity
    );
    assert!(port.published.is_empty());
}

#[test]
fn missing_and_corrupt_base_nodes_hold_without_provider_puts() {
    let (base, mut port) = setup(&BTreeMap::from([(path("a"), object("old"))]));
    let changes = BTreeMap::from([(path("a"), Some(object("new")))]);
    port.missing = true;
    assert_eq!(
        base.edit_staged(base.root(), &changes, &mut port, TreeLimits::default())
            .unwrap_err(),
        TreeError::MissingNode(base.root().clone())
    );
    port.missing = false;
    port.corrupt = true;
    assert_eq!(
        base.edit_staged(base.root(), &changes, &mut port, TreeLimits::default())
            .unwrap_err(),
        TreeError::Integrity
    );
    assert!(port.published.is_empty());
}

#[test]
fn failed_flush_and_wrong_ack_return_no_success_old_root_survives() {
    for wrong_ack in [false, true] {
        let (base, mut port) = setup(&BTreeMap::from([(path("a"), object("old"))]));
        port.wrong_ack = wrong_ack;
        port.fail_after = (!wrong_ack).then_some(1);
        let changes = BTreeMap::from([(path("b"), Some(object("new")))]);
        let error = base
            .edit_staged(base.root(), &changes, &mut port, TreeLimits::default())
            .unwrap_err();
        assert_eq!(
            error,
            if wrong_ack {
                TreeError::Integrity
            } else {
                TreeError::Store("fixture-put".into())
            }
        );
        assert_eq!(port.published.len(), 1);
        assert_eq!(
            base.get(&path("a"), &mut port, TreeLimits::default())
                .unwrap()
                .0,
            Some(object("old"))
        );
        port.wrong_ack = false;
        port.fail_after = None;
        assert!(
            base.edit_staged(base.root(), &changes, &mut port, TreeLimits::default())
                .is_ok()
        );
    }
}

#[test]
fn sparse_edit_reuses_opaque_base_and_never_reads_file_values() {
    let bindings = (0..2000)
        .map(|i| (path(&format!("stable/key{i:04}")), object("payload")))
        .chain([(path("changed/key"), object("old"))])
        .collect();
    let (base, mut port) = setup(&bindings);
    let changes = BTreeMap::from([(path("changed/key"), Some(object("new")))]);
    let (next, metrics) = base
        .edit_staged(base.root(), &changes, &mut port, TreeLimits::default())
        .unwrap();
    assert!(port.gets.get() < 30);
    assert!(metrics.published_nodes < 10);
    // No payload blobs are enrolled in the port; a payload read would fail.
    assert!(port.nodes.get(&object("payload")).unwrap().is_none());
    assert_eq!(
        next.get(&path("stable/key1999"), &mut port, TreeLimits::default())
            .unwrap()
            .0,
        Some(object("payload"))
    );
}

#[test]
fn full_segment_delta_persists_only_final_nodes_against_nonempty_index() {
    let bindings = (0..4096)
        .map(|i| (path(&format!("op/{i:08}")), object("old receipt")))
        .collect();
    let (base, mut port) = setup(&bindings);
    let changes = (4096..20480)
        .map(|i| (path(&format!("op/{i:08}")), Some(object("new receipt"))))
        .collect();
    let limits = TreeLimits {
        max_reads: 2_000_000,
        max_writes: 1_000_000,
        max_read_bytes: 512 * 1024 * 1024,
        max_write_bytes: 256 * 1024 * 1024,
        ..TreeLimits::default()
    };
    let (next, metrics) = base
        .edit_staged(base.root(), &changes, &mut port, limits)
        .unwrap();
    let expected: BTreeMap<_, _> = (0..20480)
        .map(|i| {
            (
                path(&format!("op/{i:08}")),
                object(if i < 4096 {
                    "old receipt"
                } else {
                    "new receipt"
                }),
            )
        })
        .collect();
    let mut independent = MemoryNodes::default();
    let built = Tree::build(&expected, &mut independent, TreeLimits::default())
        .unwrap()
        .0;
    assert_eq!(next, built);
    let closure: BTreeMap<Digest, _> = independent
        .objects()
        .map(|(d, b)| (d.clone(), b.len()))
        .collect();
    assert!(
        port.published
            .iter()
            .all(|o| closure.contains_key(&o.sha256))
    );
    assert!(metrics.published_nodes <= 20484);
    assert!(metrics.tree.writes > metrics.published_nodes * 5);
    assert_eq!(
        next.entries(&mut port, TreeLimits::default()).unwrap().0,
        expected
    );
    eprintln!(
        "staged full delta: {metrics:?}; base provider gets={}",
        port.gets.get()
    );
}
