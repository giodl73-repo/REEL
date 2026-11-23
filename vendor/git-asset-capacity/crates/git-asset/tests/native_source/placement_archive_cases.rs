use super::*;
use git_asset_core::tree::{MemoryNodes, NodeStore, Tree, TreeLimits};
use git_asset_store::{
    Key,
    metadata::{Limits as StoreLimits, MetadataReader, MetadataStore},
    source_claims::{Command as OwnerCommand, Selection, SourceJournal},
};
#[path = "placement_handoff_cases.rs"]
mod handoff_cases;
#[path = "placement_journal_cases.rs"]
mod journal_cases;

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn adopt(j: &mut SourceJournal, source: &str, version: u64, selection: Selection, op: &str) {
    let prepared = j
        .apply(
            key(&format!("{op}-prepare")),
            OwnerCommand::Prepare {
                source: key(source),
                owner: key("owner"),
                expected_revision: version,
                selection,
            },
        )
        .unwrap();
    j.apply(
        key(&format!("{op}-adopt")),
        OwnerCommand::Adopt {
            source: key(source),
            owner: key("owner"),
            expected_revision: version + 1,
            prepared: prepared.reference,
        },
    )
    .unwrap();
}
fn same(a: &Demand, b: &Demand) {
    assert_eq!(a.source_checkpoint(), b.source_checkpoint());
    assert_eq!(a.record(), b.record());
    assert_eq!(a.policy(), b.policy());
    assert_eq!(a.claim(), b.claim());
    assert_eq!(a.working(), b.working());
    assert_eq!(a.retained(), b.retained());
    assert!(!b.complete_claim_coverage() && !b.authorizes_eviction());
}

#[test]
fn placement_roots_finish_reopen_and_survive_independent_source_handoff_without_git() {
    let mut f = HistoryFixture::new(Layout::ReelCas, true);
    let first = inspection(&f);
    let a = demand(&first, "main", &mut expander(&first));
    next(&mut f, true);
    let second = inspection(&f);
    let b = demand(&second, "main", &mut expander(&second));
    let temp = tempfile::tempdir().unwrap();
    let metadata = temp.path().join("metadata");
    let mut store = MetadataStore::create(
        &metadata,
        StoreLimits {
            nodes: 65536,
            bytes: 32 * 1024 * 1024,
            database_bytes: 64 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let old = a.archive(&mut store, TreeLimits::default()).unwrap();
    let old_checkpoint = store.finish().unwrap();
    let current = b.archive(&mut store, TreeLimits::default()).unwrap();
    let checkpoint = store.finish().unwrap();
    drop(store);
    fs::rename(
        f.source.root().join(".git"),
        f.source.root().join("detached-git"),
    )
    .unwrap();
    let mut reader = MetadataReader::open(&metadata, &checkpoint).unwrap();
    same(
        &a,
        &Demand::from_archive(&old, &mut reader, TreeLimits::default()).unwrap(),
    );
    same(
        &b,
        &Demand::from_archive(&current, &mut reader, TreeLimits::default()).unwrap(),
    );
    // An older trusted minimum can observe later immutable additions without losing A.
    let mut earlier = MetadataReader::open(&metadata, &old_checkpoint).unwrap();
    same(
        &a,
        &Demand::from_archive(&old, &mut earlier, TreeLimits::default()).unwrap(),
    );
    let journal_path = temp.path().join("journal");
    let mut journal = SourceJournal::create(&journal_path, key("project"), key("epoch")).unwrap();
    adopt(&mut journal, "main", 0, old.clone(), "first");
    adopt(&mut journal, "peer", 0, old.clone(), "peer");
    let prepare = OwnerCommand::Prepare {
        source: key("main"),
        owner: key("owner"),
        expected_revision: 2,
        selection: current.clone(),
    };
    let pending = journal
        .apply(key("second-prepare"), prepare.clone())
        .unwrap();
    let witness = journal.checkpoint();
    drop(journal);
    let mut journal = SourceJournal::open(&journal_path, &witness).unwrap();
    let state = journal.source(&key("main")).unwrap().unwrap();
    assert_eq!(state.current.unwrap().selection, old);
    assert_eq!(state.pending.unwrap().selection, current);
    assert_eq!(journal.obligations(10).unwrap().len(), 3);
    journal
        .apply(
            key("second-adopt"),
            OwnerCommand::Adopt {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 3,
                prepared: pending.reference.clone(),
            },
        )
        .unwrap();
    let recovered = journal.apply(key("second-prepare"), prepare).unwrap();
    assert!(recovered.replayed);
    assert_eq!(recovered.reference, pending.reference);
    assert_eq!(
        journal
            .source(&key("main"))
            .unwrap()
            .unwrap()
            .current
            .unwrap()
            .selection,
        current
    );
    // Explicit owner journal transition only: this is NOT payload/claim-release authority.
    journal
        .apply(
            key("retire-owned-predecessor"),
            OwnerCommand::Retire {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 4,
                predecessor_revision: 2,
            },
        )
        .unwrap();
    assert_eq!(
        journal
            .source(&key("peer"))
            .unwrap()
            .unwrap()
            .current
            .unwrap()
            .selection,
        old
    );
    assert_eq!(journal.obligations(10).unwrap().len(), 2);
    same(
        &a,
        &Demand::from_archive(&old, &mut reader, TreeLimits::default()).unwrap(),
    );
}

#[test]
fn archive_is_deterministic_sparse_shared_metadata_and_never_copies_payload_bytes() {
    let mut f = HistoryFixture::new(Layout::GitLfs, false);
    let first = inspection(&f);
    let a = demand(&first, "branch", &mut expander(&first));
    let mut nodes = MemoryNodes::default();
    let old = a.archive(&mut nodes, TreeLimits::default()).unwrap();
    let count = nodes.objects().count();
    assert_eq!(a.archive(&mut nodes, TreeLimits::default()).unwrap(), old);
    assert_eq!(nodes.objects().count(), count);
    next(&mut f, true);
    let second = inspection(&f);
    let b = demand(&second, "branch", &mut expander(&second));
    let current = b.archive(&mut nodes, TreeLimits::default()).unwrap();
    assert_ne!(old, current);
    let entries = Tree::from_root(current.root.clone())
        .entries(&mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    assert_eq!(entries.len(), b.retained().len() + 1);
    assert!(
        nodes
            .get(&Object::from_bytes(b"new selected output"))
            .unwrap()
            .is_none()
    );
    same(
        &a,
        &Demand::from_archive(&old, &mut nodes, TreeLimits::default()).unwrap(),
    );
    same(
        &b,
        &Demand::from_archive(&current, &mut nodes, TreeLimits::default()).unwrap(),
    );
}

#[test]
fn missing_corrupt_foreign_context_unknown_paths_and_invalid_headers_refuse() {
    let f = HistoryFixture::new(Layout::ReelCas, true);
    let i = inspection(&f);
    let d = demand(&i, "main", &mut expander(&i));
    let mut nodes = MemoryNodes::default();
    let selection = d.archive(&mut nodes, TreeLimits::default()).unwrap();
    let entries = Tree::from_root(selection.root.clone())
        .entries(&mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    let identity_path = LogicalPath::parse("identity").unwrap();
    let header_pin = entries[&identity_path].clone();
    let raw = nodes.get(&header_pin).unwrap().unwrap();
    let mut wrong_context = selection.clone();
    wrong_context.context = Digest::blob(b"foreign context");
    assert!(Demand::from_archive(&wrong_context, &mut nodes, TreeLimits::default()).is_err());
    assert!(
        Demand::from_archive(
            &selection,
            &mut MemoryNodes::default(),
            TreeLimits::default()
        )
        .is_err()
    );
    for mode in 0..7 {
        let mut changed = entries.clone();
        let mut context = selection.context.clone();
        if mode < 4 {
            let mut header: Value = serde_json::from_slice(&raw).unwrap();
            match mode {
                0 => header["schema"] = json!("unsupported"),
                1 => header["retained"] = json!(0),
                2 => header["record"] = json!(Object::from_bytes(b"foreign record")),
                _ => header["unknown"] = json!(true),
            }
            let pin = nodes
                .put(&identity::canonical_json(&header).unwrap())
                .unwrap();
            context = pin.sha256.clone();
            changed.insert(identity_path.clone(), pin);
        } else if mode == 4 {
            let mut noncanonical = raw.clone();
            noncanonical.push(b' ');
            let pin = nodes.put(&noncanonical).unwrap();
            context = pin.sha256.clone();
            changed.insert(identity_path.clone(), pin);
        } else if mode == 5 {
            changed.insert(
                LogicalPath::parse("unknown/path").unwrap(),
                Object::from_bytes(b"foreign"),
            );
        } else {
            let path = changed
                .keys()
                .find(|p| p.as_str().starts_with("working/"))
                .unwrap()
                .clone();
            changed.remove(&path);
        }
        let root = Tree::build(&changed, &mut nodes, TreeLimits::default())
            .unwrap()
            .0
            .root()
            .clone();
        assert!(
            Demand::from_archive(
                &Selection { root, context },
                &mut nodes,
                TreeLimits::default()
            )
            .is_err()
        );
    }
    same(
        &d,
        &Demand::from_archive(&selection, &mut nodes, TreeLimits::default()).unwrap(),
    );
}

#[test]
fn archive_header_io_shares_tree_budgets_and_zero_write_budget_writes_nothing() {
    let f = HistoryFixture::new(Layout::ReelCas, true);
    let i = inspection(&f);
    let d = demand(&i, "main", &mut expander(&i));
    let mut nodes = MemoryNodes::default();
    assert!(
        d.archive(
            &mut nodes,
            TreeLimits {
                max_writes: 0,
                ..TreeLimits::default()
            }
        )
        .is_err()
    );
    assert_eq!(nodes.objects().count(), 0);
    let selection = d.archive(&mut nodes, TreeLimits::default()).unwrap();
    let (_, metrics) = Tree::from_root(selection.root.clone())
        .entries(&mut nodes, TreeLimits::default())
        .unwrap();
    for limits in [
        TreeLimits {
            max_read_bytes: metrics.read_bytes,
            ..TreeLimits::default()
        },
        TreeLimits {
            max_reads: metrics.reads,
            ..TreeLimits::default()
        },
    ] {
        assert!(matches!(
            Demand::from_archive(&selection, &mut nodes, limits),
            Err(crate::Error::Budget)
        ));
    }
}

#[test]
fn interrupted_archive_preserves_old_root_and_exact_retry_recovers_without_rewriting_it() {
    struct Fault<'a> {
        store: &'a mut MemoryNodes,
        writes: usize,
    }
    impl NodeStore for Fault<'_> {
        fn get(&self, o: &Object) -> git_asset_core::tree::TreeResult<Option<Vec<u8>>> {
            self.store.get(o)
        }
        fn put(&mut self, raw: &[u8]) -> git_asset_core::tree::TreeResult<Object> {
            if self.writes == 0 {
                return Err(git_asset_core::tree::TreeError::Store(
                    "private synthetic failure".into(),
                ));
            }
            self.writes -= 1;
            self.store.put(raw)
        }
    }
    let mut f = HistoryFixture::new(Layout::ReelCas, true);
    let first = inspection(&f);
    let a = demand(&first, "main", &mut expander(&first));
    let mut nodes = MemoryNodes::default();
    let old = a.archive(&mut nodes, TreeLimits::default()).unwrap();
    next(&mut f, true);
    let second = inspection(&f);
    let b = demand(&second, "main", &mut expander(&second));
    for writes in [0, 1, 3] {
        let mut fault = Fault {
            store: &mut nodes,
            writes,
        };
        assert!(b.archive(&mut fault, TreeLimits::default()).is_err());
        same(
            &a,
            &Demand::from_archive(&old, &mut nodes, TreeLimits::default()).unwrap(),
        );
    }
    let retried = b.archive(&mut nodes, TreeLimits::default()).unwrap();
    let expected = b
        .archive(&mut MemoryNodes::default(), TreeLimits::default())
        .unwrap();
    assert_eq!(retried, expected);
    same(
        &b,
        &Demand::from_archive(&retried, &mut nodes, TreeLimits::default()).unwrap(),
    );
}
