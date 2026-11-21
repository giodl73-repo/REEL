use super::*;
use git_asset_core::{
    Digest,
    tree::{MemoryNodes, NodeStore, Tree, TreeError, TreeLimits, TreeResult},
};
use git_asset_store::source_claims::Selection;

fn context(
    source: &mut AssetSnapshot,
    profiles: &[Profile],
    store: &mut impl NodeStore,
) -> git_asset::source::native::context::ProductionContext {
    let q = query(source, "invoice/42");
    source
        .freeze_production_context(
            &q,
            profiles,
            b"opaque private job request",
            store,
            TreeLimits::default(),
        )
        .unwrap()
}
fn replay(
    selection: &Selection,
    profiles: &[Profile],
    store: &mut impl NodeStore,
) -> Result<
    (
        AssetSnapshot,
        git_asset::source::native::context::ProductionContext,
    ),
    Error,
> {
    AssetSnapshot::from_production_context(
        selection,
        profiles,
        store,
        Limits::default(),
        TreeLimits::default(),
        graph::Limits {
            nodes: 200,
            edges: 200,
            depth: 10,
            bytes: 2_000_000,
        },
    )
}
fn fixture() -> (Fixture, AssetSnapshot) {
    let f = Fixture::new();
    f.record("row", "rows/1", true, json!([]), Some(b"invoice rows"));
    f.record(
        "invoice",
        "invoice/42",
        false,
        json!([{"id":"rows/1"}]),
        None,
    );
    let source = capture(&reader(f.root()), commit(f.root()));
    (f, source)
}

#[test]
fn context_freeze_preserves_policy_descriptor_and_exact_replay_without_payload_storage() {
    let (f, mut source) = fixture();
    let mut store = MemoryNodes::default();
    let first = context(&mut source, &f.profiles, &mut store);
    assert_eq!(
        git_asset::source::native::context::read_descriptor(
            first.selection(),
            &mut store,
            TreeLimits::default()
        )
        .unwrap(),
        first.descriptor()
    );
    let initial = store.objects().count();
    let again = context(&mut source, &f.profiles, &mut store);
    assert_eq!(first.selection(), again.selection());
    assert_eq!(store.objects().count(), initial);
    let (mut restored, second) = replay(first.selection(), &f.profiles, &mut store).unwrap();
    assert_eq!(first.requirements(), second.requirements());
    assert_eq!(first.descriptor(), second.descriptor());
    assert_eq!(first.roots(), second.roots());
    assert_eq!(first.archive(), second.archive());
    assert_eq!(first.policy(), second.policy());
    let q = query(&restored, "invoice/42");
    let different = restored
        .freeze_production_context(
            &q,
            &f.profiles,
            b"another job request",
            &mut store,
            TreeLimits::default(),
        )
        .unwrap();
    assert_ne!(first.selection(), different.selection());
    assert_eq!(first.requirements(), different.requirements());
    assert!(
        store
            .get(&Object::from_bytes(b"invoice rows"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn context_restore_refuses_missing_source_policy_descriptor_and_wrong_witness_or_policy() {
    struct Missing {
        inner: MemoryNodes,
        target: Digest,
        corrupt: bool,
    }
    impl NodeStore for Missing {
        fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
            if object.sha256 == self.target {
                Ok(if self.corrupt {
                    Some(b"corrupt provider bytes".to_vec())
                } else {
                    None
                })
            } else {
                self.inner.get(object)
            }
        }
        fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
            self.inner.put(bytes)
        }
    }
    let (f, mut source) = fixture();
    let mut store = Missing {
        inner: MemoryNodes::default(),
        target: Digest::blob(b"unused"),
        corrupt: false,
    };
    let value = context(&mut source, &f.profiles, &mut store);
    for missing in [
        value.archive().sha256.clone(),
        value.policy().sha256.clone(),
        Object::from_bytes(value.descriptor()).sha256,
        value.selection().context.clone(),
    ] {
        store.target = missing;
        store.corrupt = false;
        assert!(replay(value.selection(), &f.profiles, &mut store).is_err());
        store.corrupt = true;
        assert!(replay(value.selection(), &f.profiles, &mut store).is_err());
    }
    store.target = Digest::blob(b"unused");
    store.corrupt = false;
    let wrong = Selection {
        context: Digest::blob(b"wrong witness"),
        ..value.selection().clone()
    };
    assert!(replay(&wrong, &f.profiles, &mut store).is_err());
    let mut profiles = f.profiles.clone();
    profiles[0].rules[0].optional = true;
    assert!(replay(value.selection(), &profiles, &mut store).is_err());
}

#[test]
fn context_restore_rejects_extra_entries_wrong_roles_roots_and_rebound_headers() {
    let (f, mut source) = fixture();
    let mut store = MemoryNodes::default();
    let value = context(&mut source, &f.profiles, &mut store);
    let original = Tree::from_root(value.selection().root.clone())
        .entries(&mut store, TreeLimits::default())
        .unwrap()
        .0;
    for mutation in [
        "extra",
        "role",
        "role-value",
        "root",
        "root-target",
        "checkpoint",
        "binding",
        "count",
        "archive",
        "policy",
        "descriptor",
        "unknown",
    ] {
        let mut entries = original.clone();
        let mut witness = value.selection().context.clone();
        match mutation {
            "extra" => {
                entries.insert(
                    LogicalPath::parse("unknown/item").unwrap(),
                    Object::from_bytes(b"unknown"),
                );
            }
            "role" => {
                let key = entries
                    .keys()
                    .find(|k| k.components().next() == Some("primary"))
                    .unwrap()
                    .clone();
                let object = entries.remove(&key).unwrap();
                entries.insert(
                    LogicalPath::parse(&format!("controls/{}", object.sha256.as_str())).unwrap(),
                    object,
                );
            }
            "role-value" => {
                let key = entries
                    .keys()
                    .find(|k| k.components().next() == Some("primary"))
                    .unwrap()
                    .clone();
                entries.remove(&key);
                let object = Object::from_bytes(b"substituted primary bytes");
                entries.insert(
                    LogicalPath::parse(&format!("primary/{}", object.sha256.as_str())).unwrap(),
                    object,
                );
            }
            "root" => {
                let key = entries
                    .keys()
                    .find(|k| k.components().next() == Some("roots"))
                    .unwrap()
                    .clone();
                let object = entries.remove(&key).unwrap();
                entries.insert(
                    LogicalPath::parse(&format!(
                        "roots/{}",
                        Digest::blob(b"wrong root key").as_str()
                    ))
                    .unwrap(),
                    object,
                );
            }
            "root-target" => {
                let key = entries
                    .keys()
                    .find(|k| k.components().next() == Some("roots"))
                    .unwrap()
                    .clone();
                entries.remove(&key);
                let object = source
                    .resolve_id("rows/1")
                    .unwrap()
                    .reference()
                    .object
                    .clone();
                entries.insert(
                    LogicalPath::parse(&format!("roots/{}", object.sha256.as_str())).unwrap(),
                    object,
                );
            }
            _ => {
                let key = LogicalPath::parse("context/identity").unwrap();
                let mut header: Value =
                    serde_json::from_slice(&store.get(&entries[&key]).unwrap().unwrap()).unwrap();
                match mutation {
                    "checkpoint" | "binding" => {
                        header[mutation] = json!(Digest::blob(b"substituted"))
                    }
                    "count" => header["primary"] = json!(0),
                    "archive" | "policy" | "descriptor" => {
                        header[mutation] = json!(Object::from_bytes(b"missing"))
                    }
                    _ => header["unexpected"] = json!(true),
                }
                let object = store.put(&serde_json::to_vec(&header).unwrap()).unwrap();
                witness = object.sha256.clone();
                entries.insert(key, object);
            }
        }
        let root = Tree::build(&entries, &mut store, TreeLimits::default())
            .unwrap()
            .0
            .root()
            .clone();
        assert!(
            replay(
                &Selection {
                    root,
                    context: witness
                },
                &f.profiles,
                &mut store
            )
            .is_err(),
            "{mutation}"
        );
    }
}

#[test]
fn context_freeze_checks_bounds_checkpoint_and_coverage_before_metadata_writes() {
    let (f, mut source) = fixture();
    let mut store = MemoryNodes::default();
    let mut q = query(&source, "invoice/42");
    q.checkpoint = Digest::blob(b"foreign checkpoint");
    assert!(
        source
            .freeze_production_context(&q, &f.profiles, b"job", &mut store, TreeLimits::default())
            .is_err()
    );
    q = query(&source, "invoice/42");
    assert!(
        source
            .freeze_production_context(
                &q,
                &f.profiles,
                &vec![b'x'; 65537],
                &mut store,
                TreeLimits::default()
            )
            .is_err()
    );
    let tiny = TreeLimits {
        max_entries: 1,
        ..TreeLimits::default()
    };
    assert!(
        source
            .freeze_production_context(&q, &f.profiles, b"job", &mut store, tiny)
            .is_err()
    );
    q.limits.bytes = 1;
    assert!(
        source
            .freeze_production_context(&q, &f.profiles, b"job", &mut store, TreeLimits::default())
            .is_err()
    );
    assert_eq!(store.objects().count(), 0);
    let value = context(&mut source, &f.profiles, &mut store);
    assert!(
        AssetSnapshot::from_production_context(
            value.selection(),
            &f.profiles,
            &mut store,
            Limits::default(),
            tiny,
            query(&source, "invoice/42").limits
        )
        .is_err()
    );
}

#[test]
fn failed_context_construction_preserves_an_existing_context_for_readonly_replay() {
    struct Fail {
        inner: MemoryNodes,
        writes: usize,
    }
    impl NodeStore for Fail {
        fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
            self.inner.get(object)
        }
        fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
            if self.writes == 0 {
                return Err(TreeError::Store(
                    "synthetic interrupted construction".into(),
                ));
            }
            self.writes -= 1;
            self.inner.put(bytes)
        }
    }
    let (f, mut source) = fixture();
    let mut store = Fail {
        inner: MemoryNodes::default(),
        writes: 1000,
    };
    let old = context(&mut source, &f.profiles, &mut store);
    f.record("row", "rows/1", true, json!([]), Some(b"new invoice rows"));
    let mut new = capture(&reader(f.root()), commit(f.root()));
    store.writes = 2;
    let q = query(&new, "invoice/42");
    assert!(
        new.freeze_production_context(
            &q,
            &f.profiles,
            b"new job",
            &mut store,
            TreeLimits::default()
        )
        .is_err()
    );
    store.writes = 0;
    let (_, reopened) = replay(old.selection(), &f.profiles, &mut store).unwrap();
    assert_eq!(old.requirements(), reopened.requirements());
}

#[test]
fn sparse_context_replacement_shares_metadata_and_replays_both_roots() {
    let f = Fixture::new();
    for n in 0..99 {
        f.record(
            &format!("row-{n}"),
            &format!("rows/{n}"),
            true,
            json!([]),
            Some(format!("row-v1-{n}").as_bytes()),
        );
    }
    f.record(
        "invoice",
        "invoice/42",
        false,
        json!(
            (0..99)
                .map(|n| json!({"id":format!("rows/{n}")}))
                .collect::<Vec<_>>()
        ),
        None,
    );
    let repository = reader(f.root());
    let mut source = capture(&repository, commit(f.root()));
    let mut store = MemoryNodes::default();
    let before = context(&mut source, &f.profiles, &mut store);
    let initial_nodes = store.objects().count();
    let initial_bytes = store.objects().map(|(_, b)| b.len()).sum::<usize>();
    f.record("row-50", "rows/50", true, json!([]), Some(b"row-v2-50"));
    let mut source = capture(&repository, commit(f.root()));
    let after = context(&mut source, &f.profiles, &mut store);
    assert_ne!(before.selection(), after.selection());
    let added_nodes = store.objects().count() - initial_nodes;
    let added_bytes = store.objects().map(|(_, b)| b.len()).sum::<usize>() - initial_bytes;
    assert!(
        added_nodes < initial_nodes / 2,
        "{added_nodes}/{initial_nodes}"
    );
    assert!(
        added_bytes < initial_bytes / 2,
        "{added_bytes}/{initial_bytes}"
    );
    for expected in [&before, &after] {
        let (_, restored) = replay(expected.selection(), &f.profiles, &mut store).unwrap();
        assert_eq!(expected.requirements(), restored.requirements());
    }
}

#[test]
fn context_preserves_current_and_frozen_revisions_of_one_logical_asset() {
    let f = Fixture::new();
    let first = f.record("row", "rows/1", true, json!([]), Some(b"v1"));
    f.record(
        "invoice",
        "invoice/42",
        false,
        json!([
            {"id":"rows/1"}, {"id":"rows/1","sha256":first.sha256,"bytes":first.bytes}
        ]),
        None,
    );
    let repository = reader(f.root());
    let old = capture(&repository, commit(f.root()));
    f.record("row", "rows/1", true, json!([]), Some(b"v2"));
    let mut current = capture(&repository, commit(f.root()));
    current.retain(old).unwrap();
    let mut store = MemoryNodes::default();
    let frozen = context(&mut current, &f.profiles, &mut store);
    let expected = BTreeSet::from([Object::from_bytes(b"v1"), Object::from_bytes(b"v2")]);
    assert_eq!(frozen.requirements().primary(), &expected);
    let (restored, replay) = replay(frozen.selection(), &f.profiles, &mut store).unwrap();
    assert_eq!(replay.requirements(), frozen.requirements());
    assert_eq!(
        restored.resolve_id("rows/1").unwrap().selected().unwrap(),
        &Object::from_bytes(b"v2")
    );
}
