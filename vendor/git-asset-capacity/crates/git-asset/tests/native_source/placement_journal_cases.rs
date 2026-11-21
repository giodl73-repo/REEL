use super::*;
use git_asset::source::native::placement::journal::{self, Limits as ProjectionLimits, Projection};

fn setup() -> (TempDir, SourceJournal, MemoryNodes, Selection, Selection) {
    let mut f = HistoryFixture::new(Layout::ReelCas, true);
    let first = inspection(&f);
    let a = demand(&first, "main", &mut expander(&first));
    next(&mut f, true);
    let second = inspection(&f);
    let b = demand(&second, "main", &mut expander(&second));
    let mut nodes = MemoryNodes::default();
    let old = a.archive(&mut nodes, TreeLimits::default()).unwrap();
    let current = b.archive(&mut nodes, TreeLimits::default()).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let journal =
        SourceJournal::create(&temp.path().join("journal"), key("project"), key("epoch")).unwrap();
    // f and its Git source are dropped: subsequent projection uses only archives.
    (temp, journal, nodes, old, current)
}
fn projected(j: &mut SourceJournal, nodes: &MemoryNodes) -> Projection {
    journal::project(
        &j.obligations(4096).unwrap(),
        nodes,
        ProjectionLimits::default(),
    )
    .unwrap()
}
fn required(p: &Projection) -> BTreeSet<Placement> {
    p.view().plan(&BTreeSet::new()).unwrap().ensure
}
fn working(p: &Projection, subject: &Object) -> bool {
    required(p).contains(&Placement {
        object: subject.clone(),
        place: "working".into(),
    })
}

#[test]
fn pending_current_and_predecessor_roles_remove_old_hot_demand_without_losing_history_or_peers() {
    let (temp, mut j, nodes, a, b) = setup();
    let old = Object::from_bytes(b"payload not present in store");
    let new = Object::from_bytes(b"new selected output");
    adopt(&mut j, "main", 0, a.clone(), "first");
    assert!(working(&projected(&mut j, &nodes), &old));
    let pending = j
        .apply(
            key("next-prepare"),
            OwnerCommand::Prepare {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 2,
                selection: b.clone(),
            },
        )
        .unwrap();
    let p = projected(&mut j, &nodes);
    assert!(working(&p, &old) && working(&p, &new));
    assert_eq!(p.roots(), 2);
    assert_eq!(p.view().claims().count(), 1);
    let minimum = j.checkpoint();
    drop(j);
    let mut j = SourceJournal::open(&temp.path().join("journal"), &minimum).unwrap();
    let restarted = projected(&mut j, &nodes);
    assert_eq!(required(&p), required(&restarted));
    j.apply(
        key("next-adopt"),
        OwnerCommand::Adopt {
            source: key("main"),
            owner: key("owner"),
            expected_revision: 3,
            prepared: pending.reference,
        },
    )
    .unwrap();
    let p = projected(&mut j, &nodes);
    assert!(!working(&p, &old) && working(&p, &new));
    assert!(required(&p).contains(&Placement {
        object: old.clone(),
        place: "archive".into()
    }));
    assert_eq!(p.view().generation, p.checkpoint().sequence());
    assert_eq!(p.checkpoint().identity(), j.identity());
    assert!(!p.view().complete_claim_coverage);
    adopt(&mut j, "peer", 0, a, "peer");
    let peer = projected(&mut j, &nodes);
    assert!(working(&peer, &old));
    assert_eq!(peer.view().claims().count(), 2);
    let main_id = p.view().claims().next().unwrap().id.clone();
    assert!(peer.view().claims().any(|c| c.id == main_id));
    j.apply(
        key("peer-done"),
        OwnerCommand::Withdraw {
            source: key("peer"),
            owner: key("owner"),
            expected_revision: 2,
        },
    )
    .unwrap();
    let done = projected(&mut j, &nodes);
    assert!(!working(&done, &old));
    assert!(working(&done, &new));
    assert_eq!(done.roots(), 3);
    assert_eq!(done.view().claims().count(), 2);
    assert!(
        done.view()
            .plan(&required(&peer))
            .unwrap()
            .release_candidates
            .is_empty()
    );
}

#[test]
fn all_archives_share_one_io_budget_including_repeated_roots() {
    let (_temp, mut j, nodes, a, _) = setup();
    adopt(&mut j, "first", 0, a.clone(), "first");
    let roots = j.obligations(10).unwrap();
    // Determine the exact header+tree read count/bytes for one demand archive.
    let (_, metrics) = Tree::from_root(a.root.clone())
        .entries(&mut MemoryCopy(&nodes), TreeLimits::default())
        .unwrap();
    let entries = Tree::from_root(a.root.clone())
        .entries(&mut MemoryCopy(&nodes), TreeLimits::default())
        .unwrap()
        .0;
    let header = &entries[&LogicalPath::parse("identity").unwrap()];
    let tree = TreeLimits {
        max_reads: metrics.reads + 1,
        max_read_bytes: metrics.read_bytes + header.bytes as usize,
        ..TreeLimits::default()
    };
    let limits = ProjectionLimits {
        tree,
        ..ProjectionLimits::default()
    };
    assert!(journal::project(&roots, &nodes, limits).is_ok());
    adopt(&mut j, "second", 0, a, "second");
    let roots = j.obligations(10).unwrap();
    assert!(matches!(
        journal::project(&roots, &nodes, limits),
        Err(crate::Error::Budget)
    ));
    assert!(journal::project(&roots, &nodes, ProjectionLimits::default()).is_ok());
    assert!(matches!(
        journal::project(
            &roots,
            &nodes,
            ProjectionLimits {
                roots: 1,
                ..ProjectionLimits::default()
            }
        ),
        Err(crate::Error::Budget)
    ));
}
struct MemoryCopy<'a>(&'a MemoryNodes);
impl NodeStore for MemoryCopy<'_> {
    fn get(&self, o: &Object) -> git_asset_core::tree::TreeResult<Option<Vec<u8>>> {
        self.0.get(o)
    }
    fn put(&mut self, _: &[u8]) -> git_asset_core::tree::TreeResult<Object> {
        panic!("projection must not write")
    }
}

#[test]
fn missing_or_foreign_root_refuses_without_partial_claims_and_empty_journal_is_not_complete() {
    let (_temp, mut j, nodes, a, _) = setup();
    let empty = projected(&mut j, &nodes);
    assert_eq!(empty.roots(), 0);
    assert!(!empty.view().complete_claim_coverage);
    adopt(&mut j, "valid", 0, a.clone(), "valid");
    let foreign = Selection {
        root: Object::from_bytes(b"unknown metadata root"),
        context: a.context,
    };
    adopt(&mut j, "unknown", 0, foreign, "unknown");
    let roots = j.obligations(10).unwrap();
    let before = nodes.objects().count();
    assert!(journal::project(&roots, &MemoryCopy(&nodes), ProjectionLimits::default()).is_err());
    assert_eq!(nodes.objects().count(), before);
    assert_eq!(j.obligations(10).unwrap().len(), 2);
}

#[test]
fn different_journals_owners_and_sources_have_distinct_stable_claim_ids() {
    let (temp, mut first, nodes, a, _) = setup();
    adopt(&mut first, "source", 0, a.clone(), "first");
    let p = projected(&mut first, &nodes);
    let id = p.view().claims().next().unwrap().id.clone();
    assert_eq!(
        projected(&mut first, &nodes)
            .view()
            .claims()
            .next()
            .unwrap()
            .id,
        id
    );
    let mut second =
        SourceJournal::create(&temp.path().join("second"), key("project"), key("epoch")).unwrap();
    adopt(&mut second, "source", 0, a.clone(), "second");
    let q = projected(&mut second, &nodes);
    assert_ne!(q.view().claims().next().unwrap().id, id);
    let combined = View::new(
        0,
        false,
        p.view()
            .claims()
            .chain(q.view().claims())
            .cloned()
            .collect(),
    )
    .unwrap();
    assert_eq!(combined.claims().count(), 2);
    adopt(&mut first, "another-source", 0, a, "another");
    assert_eq!(projected(&mut first, &nodes).view().claims().count(), 2);
}
