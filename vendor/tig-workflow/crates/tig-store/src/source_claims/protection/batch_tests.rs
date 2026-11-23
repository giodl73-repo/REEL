use super::*;
use crate::source_claims::protection::batch::Batch;
fn transition(f: &mut Fixture, id: &str) -> Batch {
    let history = f.request(
        "history",
        id,
        [(p("history/old"), Some(object(0)))].into(),
        true,
    );
    let main = f.request(
        "main",
        id,
        [(p("assets/a-00000"), Some(object(10)))].into(),
        true,
    );
    Batch {
        operation: key(id),
        updates: vec![history, main],
    }
}
fn revisions(f: &mut Fixture) -> (u64, u64) {
    (
        f.index
            .actor(&mut f.nodes, &key("history"))
            .unwrap()
            .revision,
        f.index.actor(&mut f.nodes, &key("main")).unwrap().revision,
    )
}
#[test]
fn batch_prepare_preserves_union_and_adopt_selects_both_actors() {
    let mut f = Fixture::new(3);
    let b = transition(&mut f, "atomic");
    assert!(matches!(
        f.index
            .update_batch_hook(&b, &mut f.nodes, |s| if s == Stage::Prepared {
                Err(Error::Injected)
            } else {
                Ok(())
            }),
        Err(Error::Injected)
    ));
    assert_eq!(revisions(&mut f), (1, 1));
    assert_eq!(
        f.index
            .inspect(&mut f.nodes, &object(0), &d("F"))
            .unwrap()
            .protected_by,
        vec![key("main")]
    );
    assert_eq!(
        f.index
            .inspect(&mut f.nodes, &object(0), &d("D"))
            .unwrap()
            .protected_by,
        vec![key("history")]
    );
    assert_eq!(
        f.index
            .inspect(&mut f.nodes, &object(10), &d("F"))
            .unwrap()
            .protected_by,
        vec![key("main")]
    );
    f.reopen();
    f.index.update_batch(&b, &mut f.nodes).unwrap();
    assert_eq!(revisions(&mut f), (2, 2));
    assert!(
        f.index
            .inspect(&mut f.nodes, &object(0), &d("F"))
            .unwrap()
            .protected_by
            .is_empty()
    );
    assert_eq!(
        f.index
            .inspect(&mut f.nodes, &object(0), &d("M"))
            .unwrap()
            .protected_by,
        vec![key("history")]
    );
    assert_eq!(
        f.index.actor(&mut f.nodes, &key("jobs")).unwrap().revision,
        1
    );
    assert!(f.index.update_batch(&b, &mut f.nodes).unwrap().replayed);
    let mut changed = b.clone();
    changed.updates[0].selection.context = d("changed");
    assert!(matches!(
        f.index.update_batch(&changed, &mut f.nodes),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        f.index.update(&b.updates[0], &mut f.nodes),
        Err(Error::Conflict)
    ));
}
#[test]
fn one_stale_member_refuses_the_entire_saved_batch_without_rebase() {
    let mut f = Fixture::new(3);
    let b = transition(&mut f, "stale-batch");
    let reference = work::stage_batch(&mut f.index, &b, None, &mut f.nodes).unwrap();
    let newer = f.request(
        "history",
        "new-history",
        [(p("history/other"), Some(object(20)))].into(),
        true,
    );
    f.index.update(&newer, &mut f.nodes).unwrap();
    assert!(matches!(
        work::execute(&mut f.index, &reference, &mut f.nodes),
        Err(Error::Stale)
    ));
    assert_eq!(revisions(&mut f), (2, 1));
    assert!(!work::complete(&mut f.index, &reference).unwrap());
    assert_eq!(
        f.index
            .inspect(&mut f.nodes, &object(0), &d("F"))
            .unwrap()
            .protected_by,
        vec![key("main")]
    );
}
#[test]
fn batch_bounds_owners_order_and_aggregate_changes_refuse_before_admission() {
    let mut f = Fixture::new(3);
    let b = transition(&mut f, "bounded");
    for mode in 0..7 {
        let mut bad = b.clone();
        match mode {
            0 => bad.updates.reverse(),
            1 => bad.updates[0].owner = key("foreign"),
            2 => bad.updates[0].operation = key("other"),
            3 => bad.updates[1] = bad.updates[0].clone(),
            4 => bad.updates.clear(),
            5 => bad.updates = vec![bad.updates[0].clone(); 17],
            _ => bad.updates[1].selection.root = bad.updates[0].selection.root.clone(),
        }
        assert!(f.index.update_batch(&bad, &mut f.nodes).is_err());
        assert_eq!(revisions(&mut f), (1, 1));
    }
    let mut bad = b;
    for (member, r) in bad.updates.iter_mut().enumerate() {
        r.changes = (0..65)
            .map(|i| (p(&format!("expanded/{member}/{i}")), Some(object(i + 100))))
            .collect();
    }
    assert!(matches!(
        f.index.update_batch(&bad, &mut f.nodes),
        Err(Error::Budget)
    ));
    assert_eq!(revisions(&mut f), (1, 1));
}
#[test]
fn batch_fences_conflicts_and_invalidated_coverage_without_global_object_lock() {
    let mut f = Fixture::new(3);
    let b = transition(&mut f, "fenced");
    let conflict = f.index.lock_name(object(0).sha256.as_str()).unwrap();
    assert!(matches!(
        f.index.update_batch(&b, &mut f.nodes),
        Err(Error::Store(crate::Error::Busy))
    ));
    drop(conflict);
    let unrelated = f.index.lock_name(object(9999).sha256.as_str()).unwrap();
    f.index.update_batch(&b, &mut f.nodes).unwrap();
    drop(unrelated);
    let mut next = transition(&mut f, "unknown");
    next.updates[0].ready = false;
    let coverage = f.index.lock_mode("coverage", false).unwrap();
    assert!(matches!(
        f.index.update_batch(&next, &mut f.nodes),
        Err(Error::Store(crate::Error::Busy))
    ));
    drop(coverage);
    f.index.update_batch(&next, &mut f.nodes).unwrap();
    assert!(matches!(
        f.index.inspect(&mut f.nodes, &object(999), &d("F")),
        Err(Error::Unknown)
    ));
}

#[derive(Serialize, Deserialize)]
struct Start {
    journal: Checkpoint,
    metadata: metadata::Checkpoint,
    phase: String,
}
#[test]
fn full_aggregate_budget_and_repeated_validation_apply_to_the_whole_batch() {
    let mut f = Fixture::new(3);
    let history = f.request(
        "history",
        "maximum",
        (0..64)
            .map(|i| (p(&format!("history/{i}")), Some(object(i + 100))))
            .collect(),
        true,
    );
    let main = f.request(
        "main",
        "maximum",
        (0..64)
            .map(|i| (p(&format!("assets/new-{i}")), Some(object(i + 100))))
            .collect(),
        true,
    );
    let batch = Batch {
        operation: key("maximum"),
        updates: vec![history, main],
    };
    let reference = work::stage_batch(&mut f.index, &batch, None, &mut f.nodes).unwrap();
    let mut calls = 0;
    assert!(matches!(
        work::execute_validated(&mut f.index, &reference, &mut f.nodes, |record| {
            assert_eq!(record.updates().count(), 2);
            calls += 1;
            if calls == 2 {
                Err(Error::Stale)
            } else {
                Ok(())
            }
        }),
        Err(Error::Stale)
    ));
    assert_eq!(calls, 2);
    assert_eq!(revisions(&mut f), (1, 1));
    assert!(matches!(
        work::resume_admitted(&mut f.index, &reference, &mut f.nodes),
        Err(Error::Pending)
    ));
    work::execute(&mut f.index, &reference, &mut f.nodes).unwrap();
    assert_eq!(revisions(&mut f), (2, 2));
    assert_eq!(
        f.index
            .inspect(&mut f.nodes, &object(100), &d("M"))
            .unwrap()
            .protected_by,
        vec![key("history"), key("main")]
    );
}
#[test]
fn batch_child() {
    let Some(root) = std::env::var_os("TIG_PUBLIC_BATCH_CHILD") else {
        return;
    };
    let root = PathBuf::from(root);
    let start: Start =
        serde_json::from_slice(&std::fs::read(root.join("batch-start.json")).unwrap()).unwrap();
    let mut index = Index::open(&root.join("index"), &start.journal).unwrap();
    let mut nodes = metadata::MetadataStore::open(&root.join("nodes"), &start.metadata).unwrap();
    let reference = work::discover(&mut index, 1).unwrap().pop().unwrap();
    let batch = work::read(&mut index, &reference, &nodes)
        .unwrap()
        .batch()
        .unwrap();
    if start.phase == "Staged" {
        std::process::exit(72);
    }
    if start.phase == "Complete" {
        work::execute(&mut index, &reference, &mut nodes).unwrap();
        std::process::exit(72);
    }
    index
        .update_batch_hook(&batch, &mut nodes, |s| {
            if format!("{s:?}") == start.phase {
                std::process::exit(72);
            }
            Ok(())
        })
        .unwrap();
    panic!("child must exit at boundary");
}
#[test]
fn actual_batch_exits_recover_all_members_from_only_trusted_minima() {
    for phase in ["Staged", "MetadataReady", "Prepared", "Adopted", "Complete"] {
        let mut f = Fixture::new(3);
        let b = transition(&mut f, "process-batch");
        let reference = work::stage_batch(&mut f.index, &b, None, &mut f.nodes).unwrap();
        assert_eq!(revisions(&mut f), (1, 1));
        assert!(matches!(
            work::resume_admitted(&mut f.index, &reference, &mut f.nodes),
            Err(Error::Pending)
        ));
        let start = Start {
            journal: f.index.checkpoint(),
            metadata: f.nodes.checkpoint(),
            phase: phase.into(),
        };
        std::fs::write(
            f.temp.path().join("batch-start.json"),
            encode(&start).unwrap(),
        )
        .unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "source_claims::protection::tests::batch_tests::batch_child",
                "--nocapture",
            ])
            .env("TIG_PUBLIC_BATCH_CHILD", f.temp.path())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(72));
        f.index = Index::open(&f.temp.path().join("index"), &start.journal).unwrap();
        f.nodes =
            metadata::MetadataStore::open(&f.temp.path().join("nodes"), &start.metadata).unwrap();
        let found = work::discover(&mut f.index, 1).unwrap().pop().unwrap();
        assert_eq!(reference, found);
        if phase == "Staged" || phase == "MetadataReady" {
            assert_eq!(revisions(&mut f), (1, 1));
            work::execute(&mut f.index, &found, &mut f.nodes).unwrap();
        } else {
            work::resume_admitted(&mut f.index, &found, &mut f.nodes).unwrap();
        }
        assert_eq!(revisions(&mut f), (2, 2));
        assert!(work::complete(&mut f.index, &found).unwrap());
        assert_eq!(
            work::stage_batch(&mut f.index, &b, None, &mut f.nodes).unwrap(),
            reference
        );
        let mut changed = b.clone();
        changed.updates[0].ready = false;
        assert!(matches!(
            work::stage_batch(&mut f.index, &changed, None, &mut f.nodes),
            Err(Error::Conflict)
        ));
    }
}
