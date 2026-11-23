use super::*;

fn request(f: &mut Fixture, id: &str, value: usize) -> Update {
    f.request(
        "main",
        id,
        [(p("assets/a-00000"), Some(object(value)))].into(),
        true,
    )
}

#[test]
fn work_staging_discovery_and_replay_preserve_exact_scope() {
    let mut f = Fixture::new(3);
    let r = request(&mut f, "saved", 10);
    let reference = work::stage(&mut f.index, &r, None, &mut f.nodes).unwrap();
    assert_eq!(
        f.index.actor(&mut f.nodes, &key("main")).unwrap().revision,
        1
    );
    assert_eq!(
        f.index
            .inspect(&mut f.nodes, &object(0), &d("F"))
            .unwrap()
            .protected_by,
        vec![key("main")]
    );
    assert!(
        f.index
            .inspect(&mut f.nodes, &object(10), &d("F"))
            .unwrap()
            .protected_by
            .is_empty()
    );
    let checkpoint = f.nodes.checkpoint();
    f.reopen();
    f.nodes = metadata::MetadataStore::open(&f.temp.path().join("nodes"), &checkpoint).unwrap();
    let found = work::discover(&mut f.index, 1).unwrap();
    assert_eq!(found, vec![reference.clone()]);
    assert_eq!(
        encode(
            &work::read(&mut f.index, &found[0], &f.nodes)
                .unwrap()
                .update
        )
        .unwrap(),
        encode(&r).unwrap()
    );
    work::execute(&mut f.index, &found[0], &mut f.nodes).unwrap();
    assert!(work::complete(&mut f.index, &reference).unwrap());
    assert_eq!(
        f.index.actor(&mut f.nodes, &key("main")).unwrap().revision,
        2
    );
    assert_eq!(
        work::stage(&mut f.index, &r, None, &mut f.nodes).unwrap(),
        reference
    );
    assert!(
        work::execute(&mut f.index, &reference, &mut f.nodes)
            .unwrap()
            .replayed
    );
    let mut changed = r.clone();
    changed.ready = false;
    assert!(matches!(
        work::stage(&mut f.index, &changed, None, &mut f.nodes),
        Err(Error::Conflict)
    ));
    let evidence = f.nodes.put(b"changed evidence").unwrap();
    assert!(matches!(
        work::stage(&mut f.index, &r, Some(evidence), &mut f.nodes),
        Err(Error::Conflict)
    ));
    let mut foreign = Fixture::new(3);
    assert!(matches!(
        work::read(&mut foreign.index, &reference, &foreign.nodes),
        Err(Error::Invalid)
    ));
    assert!(
        f.index
            .inspect(&mut f.nodes, &object(0), &d("F"))
            .unwrap()
            .protected_by
            .is_empty()
    );
}

#[test]
fn staged_work_cannot_rebase_over_a_new_actor_revision() {
    let mut f = Fixture::new(3);
    let r = request(&mut f, "old", 10);
    let reference = work::stage(&mut f.index, &r, None, &mut f.nodes).unwrap();
    let newer = request(&mut f, "new", 20);
    f.index.update(&newer, &mut f.nodes).unwrap();
    assert!(matches!(
        work::execute(&mut f.index, &reference, &mut f.nodes),
        Err(Error::Stale)
    ));
    assert!(!work::complete(&mut f.index, &reference).unwrap());
    assert_eq!(
        f.index.actor(&mut f.nodes, &key("main")).unwrap().selection,
        newer.selection
    );
}

#[test]
fn source_validation_repeats_after_io_and_never_admitted_cannot_resume() {
    let mut f = Fixture::new(3);
    let r = request(&mut f, "checked", 10);
    let reference = work::stage(&mut f.index, &r, None, &mut f.nodes).unwrap();
    let mut calls = 0;
    assert!(matches!(
        work::execute_validated(&mut f.index, &reference, &mut f.nodes, |_| {
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
    assert!(matches!(
        work::resume_admitted(&mut f.index, &reference, &mut f.nodes),
        Err(Error::Pending)
    ));
    assert_eq!(
        f.index.actor(&mut f.nodes, &key("main")).unwrap().revision,
        1
    );
    assert_eq!(work::discover(&mut f.index, 1).unwrap(), vec![reference]);
}

#[derive(Serialize, Deserialize)]
struct Start {
    journal: Checkpoint,
    metadata: metadata::Checkpoint,
    phase: String,
}

#[test]
fn work_child() {
    let Some(root) = std::env::var_os("TIG_PUBLIC_PROTECTION_WORK_CHILD") else {
        return;
    };
    let root = PathBuf::from(root);
    let start: Start =
        serde_json::from_slice(&std::fs::read(root.join("work-start.json")).unwrap()).unwrap();
    let mut index = Index::open(&root.join("index"), &start.journal).unwrap();
    let mut nodes = metadata::MetadataStore::open(&root.join("nodes"), &start.metadata).unwrap();
    let reference = work::discover(&mut index, 1).unwrap().pop().unwrap();
    let r = work::read(&mut index, &reference, &nodes).unwrap().update;
    match start.phase.as_str() {
        "staged" => {}
        "prepared" => {
            let _ = index.update_hook(&r, &mut nodes, |s| {
                if s == Stage::Prepared {
                    std::process::exit(71);
                }
                Ok(())
            });
            panic!("must exit after native admission");
        }
        "adopted" => {
            index.update(&r, &mut nodes).unwrap();
        }
        "complete" => {
            work::execute(&mut index, &reference, &mut nodes).unwrap();
        }
        _ => panic!("unknown boundary"),
    }
    std::process::exit(71);
}

#[test]
fn actual_work_exits_recover_from_only_trusted_minima() {
    for phase in ["staged", "prepared", "adopted", "complete"] {
        let mut f = Fixture::new(3);
        let r = request(&mut f, "process-work", 10);
        work::stage(&mut f.index, &r, None, &mut f.nodes).unwrap();
        let start = Start {
            journal: f.index.checkpoint(),
            metadata: f.nodes.checkpoint(),
            phase: phase.into(),
        };
        std::fs::write(
            f.temp.path().join("work-start.json"),
            encode(&start).unwrap(),
        )
        .unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "source_claims::protection::tests::work_tests::work_child",
                "--nocapture",
            ])
            .env("TIG_PUBLIC_PROTECTION_WORK_CHILD", f.temp.path())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(71));
        f.index = Index::open(&f.temp.path().join("index"), &start.journal).unwrap();
        f.nodes =
            metadata::MetadataStore::open(&f.temp.path().join("nodes"), &start.metadata).unwrap();
        let reference = work::discover(&mut f.index, 1).unwrap().pop().unwrap();
        if phase == "staged" {
            work::execute(&mut f.index, &reference, &mut f.nodes).unwrap();
        } else {
            work::resume_admitted(&mut f.index, &reference, &mut f.nodes).unwrap();
        }
        assert!(work::complete(&mut f.index, &reference).unwrap());
        assert_eq!(
            f.index.actor(&mut f.nodes, &key("main")).unwrap().revision,
            2
        );
        assert_eq!(
            f.index
                .inspect(&mut f.nodes, &object(10), &d("F"))
                .unwrap()
                .protected_by,
            vec![key("main")]
        );
    }
}
