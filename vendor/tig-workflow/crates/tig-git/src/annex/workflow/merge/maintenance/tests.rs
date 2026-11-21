use super::*;
use tempfile::TempDir;

struct Fixture {
    _root: TempDir,
    journal: SourceJournal,
    nodes: MetadataStore,
    owner: Key,
    head: Head,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut journal = SourceJournal::create(
            &root.path().join("journal"),
            k("merges").unwrap(),
            k("epoch").unwrap(),
        )
        .unwrap();
        let mut nodes = MetadataStore::create(
            &root.path().join("nodes"),
            tig_store::metadata::Limits {
                nodes: 2048,
                bytes: 4 * 1024 * 1024,
                database_bytes: 16 * 1024 * 1024,
                min_free_bytes: 65_536,
            },
        )
        .unwrap();
        let deferred = Tree::empty(&mut nodes).unwrap().root().clone();
        let owner = k("owner").unwrap();
        save(
            &mut journal,
            &mut nodes,
            &k("configuration").unwrap(),
            &owner,
            &"synthetic immutable control",
            None,
            false,
        )
        .unwrap();
        let head = Head {
            settled: false,
            index_minimum: journal.checkpoint(),
            policy_minimum: journal.checkpoint(),
            cursor: "synthetic".into(),
            deferred,
            active: None,
        };
        let mut f = Self {
            _root: root,
            journal,
            nodes,
            owner,
            head,
        };
        f.save();
        f
    }
    fn save(&mut self) {
        save(
            &mut self.journal,
            &mut self.nodes,
            &k("head").unwrap(),
            &self.owner,
            &self.head,
            None,
            false,
        )
        .unwrap();
    }
    fn state(&mut self) -> source_claims::Source {
        self.journal.source(&k("head").unwrap()).unwrap().unwrap()
    }
    fn refuses(&mut self) {
        let checkpoint = self.journal.checkpoint();
        assert!(trim(&mut self.journal, &mut self.nodes, &self.owner).is_err());
        assert_eq!(self.journal.checkpoint(), checkpoint);
    }
}

#[test]
fn repeated_retirement_preserves_current_and_unrelated_sources_and_metadata_bytes() {
    let mut f = Fixture::new();
    save(
        &mut f.journal,
        &mut f.nodes,
        &k("unrelated").unwrap(),
        &f.owner,
        &"retained",
        None,
        false,
    )
    .unwrap();
    let unrelated = f.journal.source(&k("unrelated").unwrap()).unwrap();
    let mut count = 0;
    for n in 0..90 {
        if f.state().predecessors.len() + 3 > 64 {
            let current = f.state().current;
            let before = f.nodes.checkpoint();
            count += trim(&mut f.journal, &mut f.nodes, &f.owner).unwrap();
            assert_eq!(f.state().current, current);
            assert_eq!(f.nodes.checkpoint(), before);
        }
        f.head.cursor = format!("snapshot-{n}");
        f.save();
    }
    assert!(count > 60);
    assert_eq!(
        f.journal.source(&k("unrelated").unwrap()).unwrap(),
        unrelated
    );
}

#[test]
fn pending_active_or_reserved_progress_refuses_without_mutating_journal() {
    let mut f = Fixture::new();
    f.head.active = Some(Active {
        operation: Digest::blob(b"active"),
        commit: "current".into(),
        recipe: Object::from_bytes(b"unknown"),
    });
    f.save();
    f.refuses();
    let mut f = Fixture::new();
    let state = f.state();
    let budget = progress_budget::reserve(
        &mut f.journal,
        k("held").unwrap(),
        f.owner.clone(),
        k("head").unwrap(),
        state.revision,
        vec![source_claims::Guard {
            source: k("configuration").unwrap(),
            owner: f.owner.clone(),
            expected_revision: 2,
        }],
    )
    .unwrap();
    f.refuses();
    progress_budget::release(&mut f.journal, &budget).unwrap();
    f.journal
        .apply(
            k("pending").unwrap(),
            Command::Prepare {
                source: k("head").unwrap(),
                owner: f.owner.clone(),
                expected_revision: state.revision,
                selection: state.current.unwrap().selection,
            },
        )
        .unwrap();
    f.refuses();
}

#[test]
fn missing_terminal_run_or_current_tree_never_withdraws_unknown_references() {
    let mut f = Fixture::new();
    f.head.active = Some(Active {
        operation: Digest::blob(b"unsettled-old-run"),
        commit: "current".into(),
        recipe: Object::from_bytes(b"missing recipe"),
    });
    f.save();
    f.head.active = None;
    f.save();
    f.refuses();
    let mut f = Fixture::new();
    f.head.deferred = Object::from_bytes(b"unknown deferred tree");
    f.save();
    f.refuses();
}

#[test]
fn all_preflight_reads_share_one_budget_including_missing_nodes_and_refuse_writes() {
    use tig_core::tree::TreeError;
    let f = Fixture::new();
    let checkpoint = f.nodes.checkpoint();
    let mut reader = ReadScope {
        nodes: &f.nodes,
        used: Cell::new((0, 0)),
    };
    let missing = Object {
        sha256: Digest::blob(b"missing metadata"),
        bytes: 65_536,
    };
    for _ in 0..64 {
        assert!(reader.get(&missing).unwrap().is_none());
    }
    assert!(matches!(reader.get(&missing), Err(TreeError::Budget)));
    assert!(reader.put(b"no writes").is_err());
    let reader = ReadScope {
        nodes: &f.nodes,
        used: Cell::new((0, 0)),
    };
    let missing = Object {
        sha256: Digest::blob(b"missing small metadata"),
        bytes: 1,
    };
    for _ in 0..4096 {
        assert!(reader.get(&missing).unwrap().is_none());
    }
    assert!(matches!(reader.get(&missing), Err(TreeError::Budget)));
    assert_eq!(f.nodes.checkpoint(), checkpoint);
}
