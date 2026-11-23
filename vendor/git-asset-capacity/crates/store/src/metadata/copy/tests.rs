use super::*;

fn bounds(nodes: u64) -> Limits {
    Limits {
        nodes,
        bytes: 16 * 1024 * 1024,
        database_bytes: 32 * 1024 * 1024,
        min_free_bytes: 65536,
    }
}
fn budget() -> Budget {
    Budget {
        nodes: 10000,
        bytes: 16 * 1024 * 1024,
        millis: 60000,
    }
}
fn source(path: &Path, count: u64) -> (Checkpoint, Vec<Object>) {
    let mut store = MetadataStore::create(path, bounds(count + 10)).unwrap();
    let objects = (0..count)
        .map(|n| store.put(format!("synthetic-node-{n}").as_bytes()).unwrap())
        .collect();
    let checkpoint = store.finish().unwrap();
    (checkpoint, objects)
}

#[test]
fn exact_copy_reopens_every_node_preserves_source_and_uses_new_target_identity() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    let (checkpoint, objects) = source(&a, 2300);
    let config_before = fs::read(a.join("metadata.json")).unwrap();
    let mut target = MetadataStore::create(&b, bounds(5000)).unwrap();
    let result = copy_to_empty(&a, &checkpoint, &b, &mut target, budget()).unwrap();
    assert_eq!(result.source, checkpoint);
    assert_ne!(result.source.identity, result.target.identity);
    assert_eq!(result.nodes, 2300);
    assert_eq!(result.target.nodes, 2300);
    assert!(result.fresh_target_readback_verified);
    drop(target);
    verify_target(&b, &result, budget()).unwrap();
    let old = MetadataReader::open(&a, &checkpoint).unwrap();
    let new = MetadataReader::open(&b, &result.target).unwrap();
    for object in objects {
        assert_eq!(old.get(&object).unwrap(), new.get(&object).unwrap());
    }
    assert_eq!(old.checkpoint(), &checkpoint);
    assert_eq!(fs::read(a.join("metadata.json")).unwrap(), config_before);
}

#[test]
fn missing_foreign_nonempty_and_nested_targets_refuse_without_overwrite() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    let c = root.path().join("c");
    let (checkpoint, _) = source(&a, 3);
    let mut target = MetadataStore::create(&b, bounds(10)).unwrap();
    let other = MetadataStore::create(&c, bounds(10)).unwrap();
    drop(other);
    assert_eq!(
        copy_to_empty(&a, &checkpoint, &c, &mut target, budget()),
        Err(Error::Foreign)
    );
    assert_eq!(
        copy_to_empty(
            &root.path().join("missing"),
            &checkpoint,
            &b,
            &mut target,
            budget()
        ),
        Err(Error::Io)
    );
    target.put(b"unrelated").unwrap();
    target.finish().unwrap();
    let before = target.checkpoint();
    assert_eq!(
        copy_to_empty(&a, &checkpoint, &b, &mut target, budget()),
        Err(Error::Foreign)
    );
    assert_eq!(target.checkpoint(), before);
    let nested = a.join("nested");
    let mut nested_store = MetadataStore::create(&nested, bounds(10)).unwrap();
    assert_eq!(
        copy_to_empty(&a, &checkpoint, &nested, &mut nested_store, budget()),
        Err(Error::Foreign)
    );
}

#[test]
fn insufficient_target_and_copy_budgets_refuse_before_node_writes() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    let (checkpoint, _) = source(&a, 3);
    let mut target = MetadataStore::create(&b, bounds(2)).unwrap();
    let empty = target.checkpoint();
    assert_eq!(
        copy_to_empty(&a, &checkpoint, &b, &mut target, budget()),
        Err(Error::Budget)
    );
    assert_eq!(target.checkpoint(), empty);
    let c = root.path().join("c");
    let mut large = MetadataStore::create(&c, bounds(10)).unwrap();
    for small in [
        Budget {
            nodes: 2,
            ..budget()
        },
        Budget {
            bytes: 1,
            ..budget()
        },
        Budget {
            millis: 0,
            ..budget()
        },
    ] {
        assert_eq!(
            copy_to_empty(&a, &checkpoint, &c, &mut large, small),
            Err(Error::Budget)
        );
        assert_eq!(large.checkpoint().nodes, 0);
    }
}

#[test]
fn changed_source_refuses_completion_without_a_transfer_long_read_transaction() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    let (checkpoint, objects) = source(&a, 4);
    let mut target = MetadataStore::create(&b, bounds(20)).unwrap();
    let result = copy_inner(&a, &checkpoint, &b, &mut target, budget(), |phase| {
        if matches!(phase, Phase::BeforeSourceRecheck) {
            // The writer can commit while the copy's reader is still open.
            let mut writer = MetadataStore::open(&a, &checkpoint)?;
            writer
                .put(b"concurrent-source-node")
                .map_err(|_| Error::Corrupt)?;
            writer.finish()?;
        }
        Ok(())
    });
    assert_eq!(result, Err(Error::Foreign));
    let current = MetadataReader::open(&a, &checkpoint).unwrap();
    for object in objects {
        assert!(current.get(&object).unwrap().is_some());
    }
}

#[test]
fn failed_copy_preserves_source_and_leaves_an_unselected_committed_prefix() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    let (checkpoint, objects) = source(&a, 2300);
    let mut target = MetadataStore::create(&b, bounds(5000)).unwrap();
    let empty = target.checkpoint();
    let mut copied = 0;
    let result = copy_inner(&a, &checkpoint, &b, &mut target, budget(), |phase| {
        if matches!(phase, Phase::CopiedNode) {
            copied += 1;
            if copied == 1100 {
                return Err(Error::Injected);
            }
        }
        Ok(())
    });
    assert_eq!(result, Err(Error::Injected));
    drop(target);
    let prefix = MetadataReader::open(&b, &empty).unwrap();
    assert!(prefix.checkpoint().nodes > 0 && prefix.checkpoint().nodes < 2300);
    let old = MetadataReader::open(&a, &checkpoint).unwrap();
    assert_eq!(old.checkpoint(), &checkpoint);
    for object in objects {
        assert!(old.get(&object).unwrap().is_some());
    }
}

#[test]
fn corrupt_source_and_corrupt_reopened_target_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    let (checkpoint, objects) = source(&a, 4);
    let mut target = MetadataStore::create(&b, bounds(10)).unwrap();
    let observation = copy_to_empty(&a, &checkpoint, &b, &mut target, budget()).unwrap();
    drop(target);
    let conn = Connection::open(b.join("nodes.sqlite")).unwrap();
    conn.execute(
        "UPDATE nodes SET body=?1 WHERE oid=?2",
        params![b"invalid".as_slice(), objects[0].sha256.as_str()],
    )
    .unwrap();
    drop(conn);
    assert_eq!(
        verify_target(&b, &observation, budget()),
        Err(Error::Corrupt)
    );
    let conn = Connection::open(a.join("nodes.sqlite")).unwrap();
    conn.execute(
        "UPDATE nodes SET body=?1 WHERE oid=?2",
        params![b"invalid".as_slice(), objects[0].sha256.as_str()],
    )
    .unwrap();
    drop(conn);
    let c = root.path().join("c");
    let mut fresh = MetadataStore::create(&c, bounds(10)).unwrap();
    assert_eq!(
        copy_to_empty(&a, &checkpoint, &c, &mut fresh, budget()),
        Err(Error::Corrupt)
    );
}

#[test]
fn empty_source_is_verified_not_confused_with_a_missing_provider() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    let (checkpoint, _) = source(&a, 0);
    let mut target = MetadataStore::create(&b, bounds(10)).unwrap();
    let observation = copy_to_empty(&a, &checkpoint, &b, &mut target, budget()).unwrap();
    assert_eq!((observation.nodes, observation.bytes), (0, 0));
    verify_target(&b, &observation, budget()).unwrap();
}

#[test]
fn old_merkle_root_reopens_and_target_can_grow_without_changing_source() {
    use git_asset_core::{
        LogicalPath,
        tree::{Tree, TreeLimits},
    };
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    let mut original = MetadataStore::create(&a, bounds(100)).unwrap();
    let mut entries = BTreeMap::from([
        (
            LogicalPath::parse("nested/stable").unwrap(),
            Object::from_bytes(b"external-one"),
        ),
        (
            LogicalPath::parse("nested/change").unwrap(),
            Object::from_bytes(b"external-two"),
        ),
    ]);
    let limits = TreeLimits::default();
    let old = Tree::build(&entries, &mut original, limits).unwrap().0;
    let checkpoint = original.finish().unwrap();
    drop(original);
    let mut target = MetadataStore::create(&b, bounds(1000)).unwrap();
    let copy = copy_to_empty(&a, &checkpoint, &b, &mut target, budget()).unwrap();
    assert_eq!(old.entries(&mut target, limits).unwrap().0, entries);
    let old_entries = entries.clone();
    entries.insert(
        LogicalPath::parse("nested/change").unwrap(),
        Object::from_bytes(b"replacement"),
    );
    let new = Tree::build(&entries, &mut target, limits).unwrap().0;
    let grown = target.finish().unwrap();
    drop(target);
    let mut reader = MetadataReader::open(&b, &grown).unwrap();
    assert_eq!(new.entries(&mut reader, limits).unwrap().0, entries);
    assert_eq!(old.entries(&mut reader, limits).unwrap().0, old_entries);
    let mut source = MetadataReader::open(&a, &checkpoint).unwrap();
    assert_eq!(source.checkpoint(), &checkpoint);
    assert_eq!(old.entries(&mut source, limits).unwrap().0, old_entries);
    assert_eq!(verify_target(&b, &copy, budget()), Err(Error::Foreign));
}
