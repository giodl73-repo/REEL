use git_asset_core::{LogicalPath, Object, tree::*, worktree::*};
use std::{cell::Cell, collections::BTreeMap};

fn path(p: &str) -> LogicalPath {
    LogicalPath::parse(p).unwrap()
}
fn blob(b: &str) -> Object {
    Object::from_bytes(b.as_bytes())
}
fn base(nodes: &mut MemoryNodes) -> Tree {
    Tree::build(
        &BTreeMap::from([
            (path("scene/cel-a"), blob("A1")),
            (path("scene/cel-b"), blob("B1")),
            (path("scene/cue"), blob("Q1")),
        ]),
        nodes,
        TreeLimits::default(),
    )
    .unwrap()
    .0
}
fn candidate(
    tree: &Tree,
    key: &str,
    value: Option<&str>,
    nodes: &mut impl NodeStore,
) -> SealedCandidate {
    let mut view = ByteWorktree::open(tree.clone());
    match value {
        Some(value) => view.put(path(key), blob(value)).unwrap(),
        None => view.remove(path(key)).unwrap(),
    }
    view.seal(nodes, TreeLimits::default()).unwrap().0
}
fn plan(c: &SealedCandidate, current: &Tree, nodes: &mut MemoryNodes) -> AdoptionPlan {
    match c
        .plan_adoption(current, nodes, TreeLimits::default())
        .unwrap()
        .0
    {
        Adoption::Ready(plan) => plan,
        Adoption::Conflicts(c) => panic!("unexpected conflict: {c:?}"),
    }
}
fn get(tree: &Tree, key: &str, nodes: &mut MemoryNodes) -> Option<Object> {
    tree.get(&path(key), nodes, TreeLimits::default())
        .unwrap()
        .0
}

#[test]
fn opening_views_shares_a_pin_without_reading_or_copying_base_nodes() {
    let mut nodes = MemoryNodes::default();
    let tree = base(&mut nodes);
    let count = nodes.objects().count();
    let left = ByteWorktree::open(tree.clone());
    let right = ByteWorktree::open(tree.clone());
    assert_eq!(left.base(), right.base());
    assert!(left.changes().is_empty());
    assert_eq!(nodes.objects().count(), count);
}

#[test]
fn private_put_remove_fallback_and_discard_are_isolated() {
    let mut nodes = MemoryNodes::default();
    let tree = base(&mut nodes);
    let mut left = ByteWorktree::open(tree.clone());
    let right = ByteWorktree::open(tree);
    left.put(path("scene/cel-a"), blob("A2")).unwrap();
    left.remove(path("scene/cel-b")).unwrap();
    assert_eq!(
        left.get(&path("scene/cel-a"), &mut nodes, TreeLimits::default())
            .unwrap()
            .0,
        Some(blob("A2"))
    );
    assert_eq!(
        left.get(&path("scene/cel-b"), &mut nodes, TreeLimits::default())
            .unwrap()
            .0,
        None
    );
    assert_eq!(
        left.get(&path("scene/cue"), &mut nodes, TreeLimits::default())
            .unwrap()
            .0,
        Some(blob("Q1"))
    );
    assert_eq!(
        right
            .get(&path("scene/cel-a"), &mut nodes, TreeLimits::default())
            .unwrap()
            .0,
        Some(blob("A1"))
    );
    assert!(left.discard(&path("scene/cel-a")));
    assert!(!left.discard(&path("scene/cel-a")));
    assert!(left.discard(&path("scene/cel-b")));
    assert_eq!(
        left.get(&path("scene/cel-b"), &mut nodes, TreeLimits::default())
            .unwrap()
            .0,
        Some(blob("B1"))
    );
}

#[test]
fn sealing_keeps_base_and_other_view_readable() {
    let mut nodes = MemoryNodes::default();
    let tree = base(&mut nodes);
    let left = candidate(&tree, "scene/cel-b", Some("B2"), &mut nodes);
    let right = candidate(&tree, "scene/cue", Some("Q2"), &mut nodes);
    assert_eq!(left.base(), &tree);
    assert_eq!(get(left.tree(), "scene/cue", &mut nodes), Some(blob("Q1")));
    assert_eq!(
        get(right.tree(), "scene/cel-b", &mut nodes),
        Some(blob("B1"))
    );
    assert_eq!(get(&tree, "scene/cel-b", &mut nodes), Some(blob("B1")));
}

#[test]
fn same_bytes_and_removing_absent_leaf_are_noops() {
    let mut nodes = MemoryNodes::default();
    let tree = base(&mut nodes);
    let mut view = ByteWorktree::open(tree.clone());
    view.put(path("scene/cel-a"), blob("A1")).unwrap();
    view.remove(path("missing")).unwrap();
    let (sealed, metrics) = view.seal(&mut nodes, TreeLimits::default()).unwrap();
    assert_eq!(sealed.tree(), &tree);
    assert!(sealed.changes().is_empty());
    assert_eq!(metrics.writes, 0);
    assert_eq!(view.changes().len(), 2);
}

#[test]
fn repeated_seal_is_stable_and_does_not_clear_private_changes() {
    let mut nodes = MemoryNodes::default();
    let mut view = ByteWorktree::open(base(&mut nodes));
    view.put(path("scene/cel-b"), blob("B2")).unwrap();
    let one = view.seal(&mut nodes, TreeLimits::default()).unwrap().0;
    let count = nodes.objects().count();
    let two = view.seal(&mut nodes, TreeLimits::default()).unwrap().0;
    assert_eq!(one.tree(), two.tree());
    assert_eq!(one.changes(), two.changes());
    assert_eq!(nodes.objects().count(), count);
    assert_eq!(view.changes().len(), 1);
}

#[test]
fn disjoint_cel_and_cue_edits_adopt_without_losing_either() {
    let mut nodes = MemoryNodes::default();
    let tree = base(&mut nodes);
    let left = candidate(&tree, "scene/cel-b", Some("B2"), &mut nodes);
    let right = candidate(&tree, "scene/cue", Some("Q2"), &mut nodes);
    let (merged, _) = plan(&left, right.tree(), &mut nodes)
        .apply(right.tree(), &mut nodes, TreeLimits::default())
        .unwrap();
    assert_eq!(get(&merged, "scene/cel-b", &mut nodes), Some(blob("B2")));
    assert_eq!(get(&merged, "scene/cue", &mut nodes), Some(blob("Q2")));
    assert_eq!(get(&merged, "scene/cel-a", &mut nodes), Some(blob("A1")));
}

#[test]
fn same_leaf_different_bytes_conflicts_before_any_adoption_write() {
    let mut nodes = MemoryNodes::default();
    let tree = base(&mut nodes);
    let mut view = ByteWorktree::open(tree.clone());
    view.put(path("scene/cel-b"), blob("B2")).unwrap();
    view.put(path("new-file"), blob("new")).unwrap();
    let left = view.seal(&mut nodes, TreeLimits::default()).unwrap().0;
    let right = candidate(&tree, "scene/cel-b", Some("B3"), &mut nodes);
    let count = nodes.objects().count();
    let Adoption::Conflicts(conflicts) = left
        .plan_adoption(right.tree(), &mut nodes, TreeLimits::default())
        .unwrap()
        .0
    else {
        panic!("must conflict")
    };
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].change.before, Some(blob("B1")));
    assert_eq!(conflicts[0].change.after, Some(blob("B2")));
    assert_eq!(conflicts[0].current, Some(blob("B3")));
    assert_eq!(conflicts[0].kind, ConflictKind::Content);
    assert_eq!(nodes.objects().count(), count);
    assert_eq!(get(right.tree(), "new-file", &mut nodes), None);
}

#[test]
fn identical_concurrent_edit_is_already_applied() {
    let mut nodes = MemoryNodes::default();
    let tree = base(&mut nodes);
    let left = candidate(&tree, "scene/cel-b", Some("B2"), &mut nodes);
    let right = candidate(&tree, "scene/cel-b", Some("B2"), &mut nodes);
    let plan = plan(&left, right.tree(), &mut nodes);
    assert!(plan.edits().is_empty());
    let (same, metrics) = plan
        .apply(right.tree(), &mut nodes, TreeLimits::default())
        .unwrap();
    assert_eq!(&same, right.tree());
    assert_eq!(metrics.writes, 0);
}

#[test]
fn delete_modify_conflicts_in_both_directions() {
    let mut nodes = MemoryNodes::default();
    let tree = base(&mut nodes);
    let deleted = candidate(&tree, "scene/cel-b", None, &mut nodes);
    let modified = candidate(&tree, "scene/cel-b", Some("B2"), &mut nodes);
    for (ours, current) in [(&deleted, modified.tree()), (&modified, deleted.tree())] {
        assert!(matches!(
            ours.plan_adoption(current, &mut nodes, TreeLimits::default())
                .unwrap()
                .0,
            Adoption::Conflicts(_)
        ));
    }
}

#[test]
fn matching_deletions_are_already_applied() {
    let mut nodes = MemoryNodes::default();
    let tree = base(&mut nodes);
    let left = candidate(&tree, "scene/cel-b", None, &mut nodes);
    let right = candidate(&tree, "scene/cel-b", None, &mut nodes);
    assert!(plan(&left, right.tree(), &mut nodes).edits().is_empty());
}

#[test]
fn add_add_different_content_conflicts_from_an_absent_base() {
    let mut nodes = MemoryNodes::default();
    let tree = Tree::empty(&mut nodes).unwrap();
    let left = candidate(&tree, "new", Some("left"), &mut nodes);
    let right = candidate(&tree, "new", Some("right"), &mut nodes);
    let Adoption::Conflicts(c) = left
        .plan_adoption(right.tree(), &mut nodes, TreeLimits::default())
        .unwrap()
        .0
    else {
        panic!("must conflict")
    };
    assert_eq!(c[0].change.before, None);
}

#[test]
fn rename_uses_the_same_object_and_only_exact_leaf_tombstones() {
    let mut nodes = MemoryNodes::default();
    let tree = base(&mut nodes);
    let mut view = ByteWorktree::open(tree);
    view.remove(path("scene/cel-b")).unwrap();
    view.put(path("scene/renamed"), blob("B1")).unwrap();
    let sealed = view.seal(&mut nodes, TreeLimits::default()).unwrap().0;
    assert_eq!(get(sealed.tree(), "scene/cel-b", &mut nodes), None);
    assert_eq!(
        get(sealed.tree(), "scene/renamed", &mut nodes),
        Some(blob("B1"))
    );
}

#[test]
fn prefix_overlaps_reject_without_changing_private_state() {
    let mut nodes = MemoryNodes::default();
    let tree = Tree::empty(&mut nodes).unwrap();
    for (a, b) in [("a", "a/b"), ("a/b", "a")] {
        let mut view = ByteWorktree::open(tree.clone());
        view.put(path(a), blob("A")).unwrap();
        assert_eq!(view.remove(path(b)), Err(WorktreeError::OverlappingPaths));
        assert_eq!(view.changes().len(), 1);
        assert_eq!(
            view.get(&path(b), &mut nodes, TreeLimits::default())
                .unwrap_err(),
            WorktreeError::OverlappingPaths
        );
    }
}

#[test]
fn directory_operations_are_rejected_rather_than_implicitly_recursive() {
    let mut nodes = MemoryNodes::default();
    let tree = base(&mut nodes);
    let mut view = ByteWorktree::open(tree);
    view.remove(path("scene")).unwrap();
    assert_eq!(
        view.seal(&mut nodes, TreeLimits::default()).unwrap_err(),
        WorktreeError::Tree(TreeError::PathConflict)
    );
}

#[test]
fn file_directory_shape_changes_are_reported_as_conflicts() {
    let mut nodes = MemoryNodes::default();
    let tree = Tree::empty(&mut nodes).unwrap();
    let ours = candidate(&tree, "a/file", Some("new"), &mut nodes);
    let current = candidate(&tree, "a", Some("file occupies parent"), &mut nodes);
    let Adoption::Conflicts(c) = ours
        .plan_adoption(current.tree(), &mut nodes, TreeLimits::default())
        .unwrap()
        .0
    else {
        panic!("must conflict")
    };
    assert_eq!(c[0].kind, ConflictKind::PathShape);
}

#[test]
fn stale_adoption_plan_requires_replanning_and_preserves_candidates() {
    let mut nodes = MemoryNodes::default();
    let tree = base(&mut nodes);
    let ours = candidate(&tree, "scene/cel-b", Some("B2"), &mut nodes);
    let planned = plan(&ours, &tree, &mut nodes);
    let newer = candidate(&tree, "scene/cue", Some("Q2"), &mut nodes);
    let count = nodes.objects().count();
    assert_eq!(
        planned
            .apply(newer.tree(), &mut nodes, TreeLimits::default())
            .unwrap_err(),
        WorktreeError::StaleTarget
    );
    assert_eq!(nodes.objects().count(), count);
    let merged = plan(&ours, newer.tree(), &mut nodes)
        .apply(newer.tree(), &mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    assert_eq!(get(&merged, "scene/cue", &mut nodes), Some(blob("Q2")));
}

#[test]
fn pending_change_limit_is_bounded_but_existing_leaf_can_be_revised() {
    let mut nodes = MemoryNodes::default();
    let mut view = ByteWorktree::open(Tree::empty(&mut nodes).unwrap());
    for n in 0..MAX_CHANGES {
        view.put(path(&format!("file-{n}")), blob("A")).unwrap();
    }
    assert_eq!(view.remove(path("extra")), Err(WorktreeError::Budget));
    view.put(path("file-0"), blob("B")).unwrap();
    assert_eq!(view.changes().len(), MAX_CHANGES);
}

#[test]
fn one_seal_budget_covers_all_leaf_reads_and_final_metadata_edits() {
    let mut nodes = MemoryNodes::default();
    let tree = base(&mut nodes);
    let mut view = ByteWorktree::open(tree.clone());
    view.put(path("scene/cel-a"), blob("A2")).unwrap();
    view.put(path("scene/cel-b"), blob("B2")).unwrap();
    let first_cost = tree
        .get(&path("scene/cel-a"), &mut nodes, TreeLimits::default())
        .unwrap()
        .1
        .reads;
    let limit = TreeLimits {
        max_reads: first_cost,
        ..TreeLimits::default()
    };
    let count = nodes.objects().count();
    assert_eq!(
        view.seal(&mut nodes, limit).unwrap_err(),
        WorktreeError::Tree(TreeError::Budget)
    );
    assert_eq!(nodes.objects().count(), count);
    assert_eq!(view.changes().len(), 2);
    assert!(view.seal(&mut nodes, TreeLimits::default()).is_ok());
}

struct CountReads<'a> {
    inner: &'a mut MemoryNodes,
    reads: Cell<usize>,
}
impl NodeStore for CountReads<'_> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.reads.set(self.reads.get() + 1);
        self.inner.get(object)
    }
    fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
        self.inner.put(bytes)
    }
}

#[test]
fn shape_conflict_reads_still_consume_the_shared_adoption_budget() {
    let mut nodes = MemoryNodes::default();
    let empty = Tree::empty(&mut nodes).unwrap();
    let mut view = ByteWorktree::open(empty.clone());
    view.put(path("a/leaf"), blob("A")).unwrap();
    view.put(path("b/leaf"), blob("B")).unwrap();
    let ours = view.seal(&mut nodes, TreeLimits::default()).unwrap().0;
    let mut view = ByteWorktree::open(empty);
    view.put(path("a"), blob("parent A")).unwrap();
    view.put(path("b"), blob("parent B")).unwrap();
    let current = view.seal(&mut nodes, TreeLimits::default()).unwrap().0;
    let mut counter = CountReads {
        inner: &mut nodes,
        reads: Cell::new(0),
    };
    assert_eq!(
        current
            .tree()
            .get(&path("a/leaf"), &mut counter, TreeLimits::default())
            .unwrap_err(),
        TreeError::PathConflict
    );
    let limit = TreeLimits {
        max_reads: counter.reads.get(),
        ..TreeLimits::default()
    };
    assert_eq!(
        ours.plan_adoption(current.tree(), &mut counter, limit)
            .unwrap_err(),
        WorktreeError::Tree(TreeError::Budget)
    );
    let Adoption::Conflicts(c) = ours
        .plan_adoption(current.tree(), &mut nodes, TreeLimits::default())
        .unwrap()
        .0
    else {
        panic!("must conflict")
    };
    assert_eq!(c.len(), 2);
}

#[test]
fn conflicting_sizes_in_private_pins_do_not_mutate_the_overlay() {
    let mut nodes = MemoryNodes::default();
    let mut view = ByteWorktree::open(Tree::empty(&mut nodes).unwrap());
    view.put(path("one"), blob("A")).unwrap();
    let mut wrong = blob("A");
    wrong.bytes += 1;
    assert_eq!(
        view.put(path("two"), wrong),
        Err(WorktreeError::Tree(TreeError::Integrity))
    );
    assert_eq!(view.changes().len(), 1);
}

#[test]
fn day_one_scene_and_day_two_replacement_keep_exact_history() {
    let mut nodes = MemoryNodes::default();
    let empty = Tree::empty(&mut nodes).unwrap();
    let mut day_one = ByteWorktree::open(empty.clone());
    for (key, bytes) in [
        ("scene/cel-a", "A1"),
        ("scene/cel-b", "B1"),
        ("scene/cue", "Q1"),
    ] {
        day_one.put(path(key), blob(bytes)).unwrap();
    }
    let first = day_one.seal(&mut nodes, TreeLimits::default()).unwrap().0;
    let main_v1 = plan(&first, &empty, &mut nodes)
        .apply(&empty, &mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    let second = candidate(&main_v1, "scene/cel-b", Some("B2"), &mut nodes);
    let main_v2 = plan(&second, &main_v1, &mut nodes)
        .apply(&main_v1, &mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    assert_eq!(second.changes().len(), 1);
    assert_eq!(get(&main_v1, "scene/cel-b", &mut nodes), Some(blob("B1")));
    assert_eq!(get(&main_v2, "scene/cel-b", &mut nodes), Some(blob("B2")));
    for key in ["scene/cel-a", "scene/cue"] {
        assert_eq!(
            get(&main_v1, key, &mut nodes),
            get(&main_v2, key, &mut nodes)
        );
    }
}

#[test]
fn generic_dataset_edits_commute_and_failed_seal_can_retry() {
    struct FailOnce<'a> {
        inner: &'a mut MemoryNodes,
        writes_left: usize,
    }
    impl NodeStore for FailOnce<'_> {
        fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
            self.inner.get(object)
        }
        fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
            if self.writes_left == 0 {
                return Err(TreeError::Store("injected".into()));
            }
            self.writes_left -= 1;
            self.inner.put(bytes)
        }
    }
    let mut nodes = MemoryNodes::default();
    let tree = Tree::empty(&mut nodes).unwrap();
    let mut left = ByteWorktree::open(tree.clone());
    left.put(path("training/partition-01"), blob("dataset shard 1"))
        .unwrap();
    {
        let mut failed = FailOnce {
            inner: &mut nodes,
            writes_left: 1,
        };
        assert!(matches!(
            left.seal(&mut failed, TreeLimits::default()),
            Err(WorktreeError::Tree(TreeError::Store(_)))
        ));
    }
    assert_eq!(left.changes().len(), 1);
    assert!(
        tree.entries(&mut nodes, TreeLimits::default())
            .unwrap()
            .0
            .is_empty()
    );
    let left = left.seal(&mut nodes, TreeLimits::default()).unwrap().0;
    let right = candidate(
        &tree,
        "training/partition-02",
        Some("dataset shard 2"),
        &mut nodes,
    );
    let lr = plan(&left, right.tree(), &mut nodes)
        .apply(right.tree(), &mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    let rl = plan(&right, left.tree(), &mut nodes)
        .apply(left.tree(), &mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    assert_eq!(lr, rl);
    assert_eq!(
        lr.entries(&mut nodes, TreeLimits::default())
            .unwrap()
            .0
            .len(),
        2
    );
}
