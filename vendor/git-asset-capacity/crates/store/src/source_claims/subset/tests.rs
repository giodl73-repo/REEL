use super::*;
use crate::{
    metadata,
    source_claims::{Checkpoint, Places},
};
use std::{fs, path::PathBuf, process::Command as Process};

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}

#[test]
fn original_pages_prepare_conditional_survivors_and_predict_actual_checkpoints() {
    let mut f = Fixture::new();
    let before = f.ns.checkpoint();
    let requests = vec![f.request(&["delete"]), f.request(&["shared"])];
    let steps = plan_pages(&f.e, &mut f.ns, requests, &mut f.metadata).unwrap();
    assert_eq!(before, f.ns.checkpoint());
    assert_eq!(steps.len(), 2);
    assert_eq!(
        steps[1].plan.request.source,
        steps[0].plan.survivor_source().unwrap().unwrap()
    );
    assert_eq!(steps[1].plan.request.predecessor_revision, 3);
    f.metadata.finish().unwrap();
    for step in &steps {
        assert!(f.apply(&step.plan).settlement_current);
        let status = inspect(&f.e, &mut f.ns, &step.plan).unwrap();
        assert_eq!(
            status.historical_retirement.unwrap().checkpoint,
            step.retirement_checkpoint
        );
    }
    assert!(f.demands(&f.a.clone()).is_empty());
    assert_eq!(f.demands(&f.b.clone()), vec!["D", "M"]);
    // This is a historical plan prediction; the first survivor has now gone.
    assert!(
        !inspect(&f.e, &mut f.ns, &steps[0].plan)
            .unwrap()
            .survivor_protection_current
    );
}

#[test]
fn one_shared_metadata_budget_spans_conditional_page_applications() {
    use crate::protection::budget::Budget;
    let mut measured = Fixture::new();
    let request = measured.request(&["delete"]);
    let steps = plan_pages(
        &measured.e,
        &mut measured.ns,
        vec![request],
        &mut measured.metadata,
    )
    .unwrap();
    measured.metadata.finish().unwrap();
    let generous = Budget::new(&measured.e.limits, 65536, 32 * 1024 * 1024).unwrap();
    apply_metered(
        &measured.e,
        &mut measured.ns,
        &mut measured.metadata,
        &steps[0].plan,
        &generous,
        |_| Ok(()),
    )
    .unwrap();
    let used = generous.usage().0;
    assert!(used > 1);
    let mut f = Fixture::new();
    let requests = vec![f.request(&["delete"]), f.request(&["shared"])];
    let steps = plan_pages(&f.e, &mut f.ns, requests, &mut f.metadata).unwrap();
    f.metadata.finish().unwrap();
    let mut limits = f.e.limits.clone();
    limits.reads = used + 1;
    let budget = Budget::new(&limits, 65536, 32 * 1024 * 1024).unwrap();
    apply_metered(
        &f.e,
        &mut f.ns,
        &mut f.metadata,
        &steps[0].plan,
        &budget,
        |_| Ok(()),
    )
    .unwrap();
    let before = f.ns.checkpoint();
    assert!(matches!(
        apply_metered(
            &f.e,
            &mut f.ns,
            &mut f.metadata,
            &steps[1].plan,
            &budget,
            |_| Ok(())
        ),
        Err(Error::Tree(TreeError::Budget))
    ));
    assert_eq!(before, f.ns.checkpoint());
    assert_eq!(f.demands(&f.b.clone()), vec!["D", "M"]);
}

#[test]
fn paged_preparation_rejects_duplicate_bindings_and_existing_survivor_slots() {
    let mut f = Fixture::new();
    let request = f.request(&["delete"]);
    let before = f.ns.checkpoint();
    assert!(matches!(
        plan_pages(
            &f.e,
            &mut f.ns,
            vec![request.clone(), request.clone()],
            &mut f.metadata
        ),
        Err(Error::Invalid)
    ));
    assert_eq!(before, f.ns.checkpoint());
    let p = f.plan(&["delete"]);
    let source = p.survivor_source().unwrap().unwrap();
    enroll(
        &mut f.ns,
        &f.metadata,
        source.as_str(),
        "owner",
        Kind::Branch,
    );
    assert!(plan_pages(&f.e, &mut f.ns, vec![request], &mut f.metadata).is_err());
}
fn path(s: &str) -> LogicalPath {
    LogicalPath::parse(s).unwrap()
}
fn limits() -> Limits {
    Limits {
        millis: 60_000,
        roots: 256,
        bindings: 10000,
        associations: 100000,
        reads: 100000,
        read_bytes: 32 * 1024 * 1024,
    }
}
fn policy() -> admission::Policy {
    let places = Places {
        current: vec![key("F"), key("M")],
        pending: vec![key("F"), key("M")],
        predecessor: vec![key("D"), key("M")],
    };
    let retention = Places {
        current: vec![key("M")],
        pending: vec![key("M")],
        predecessor: vec![key("M")],
    };
    admission::Policy {
        prefer_active: true,
        classes: [
            (Kind::Branch, places.clone()),
            (Kind::Draft, places.clone()),
            (Kind::Job, places),
            (Kind::Retention, retention),
        ]
        .into_iter()
        .collect(),
    }
}
struct Fixture {
    temp: tempfile::TempDir,
    ns: Namespace,
    metadata: MetadataStore,
    e: Enrollment,
    a: Object,
    b: Object,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut ns = Namespace::create(
            &temp.path().join("ns"),
            key("project"),
            key("epoch"),
            policy(),
        )
        .unwrap();
        let mut metadata = MetadataStore::create(
            &temp.path().join("metadata"),
            metadata::Limits {
                nodes: 100000,
                bytes: 64 * 1024 * 1024,
                database_bytes: 128 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let a = Object::from_bytes(b"old-cue");
        let b = Object::from_bytes(b"survivor");
        let original = tree(
            &mut metadata,
            &[
                ("delete", a.clone()),
                ("held", b.clone()),
                ("shared", a.clone()),
            ],
        );
        metadata.finish().unwrap();
        let e = Enrollment {
            authority: Digest::blob(b"authority"),
            namespace: ns.identity().clone(),
            policy: digest(ns.policy()).unwrap(),
            owner: key("owner"),
            classes: vec![Kind::Branch, Kind::Draft],
            limits: limits(),
            writes: 10000,
            write_bytes: 8 * 1024 * 1024,
        };
        enroll(&mut ns, &metadata, "main", "owner", Kind::Branch);
        select(&mut ns, &metadata, "main", original.clone(), "first");
        ns.apply(
            key("end-first"),
            Command::Withdraw {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 3,
            },
            &metadata,
            &limits(),
        )
        .unwrap();
        Self {
            temp,
            ns,
            metadata,
            e,
            a,
            b,
        }
    }
    fn request(&self, paths: &[&str]) -> Request {
        Request {
            source: key("main"),
            predecessor_revision: 3,
            paths: paths.iter().map(|s| path(s)).collect(),
        }
    }
    fn plan(&mut self, paths: &[&str]) -> Plan {
        let request = self.request(paths);
        plan(
            &self.e,
            &mut self.ns,
            request,
            &self.metadata.finished_reader().unwrap(),
        )
        .unwrap()
    }
    fn apply(&mut self, p: &Plan) -> Status {
        apply(&self.e, &mut self.ns, &mut self.metadata, p).unwrap()
    }
    fn demands(&mut self, object: &Object) -> Vec<String> {
        self.ns
            .requirements(&self.metadata.finished_reader().unwrap(), &limits())
            .unwrap()
            .entries
            .iter()
            .filter(|a| &a.object == object)
            .flat_map(|a| a.places.iter().map(|p| p.as_str().to_owned()))
            .collect()
    }
}
fn tree(nodes: &mut impl NodeStore, entries: &[(&str, Object)]) -> Selection {
    let bindings = entries.iter().map(|(p, o)| (path(p), o.clone())).collect();
    let (tree, _) = Tree::build(&bindings, nodes, TreeLimits::default()).unwrap();
    Selection {
        root: tree.root().clone(),
        context: Digest::blob(b"context"),
    }
}
fn enroll(ns: &mut Namespace, nodes: &impl NodeStore, source: &str, owner: &str, class: Kind) {
    ns.apply(
        key(&format!("enroll-{source}")),
        Command::Enroll {
            source: key(source),
            owner: key(owner),
            class,
        },
        nodes,
        &limits(),
    )
    .unwrap();
}
fn select(
    ns: &mut Namespace,
    nodes: &impl NodeStore,
    source: &str,
    selection: Selection,
    label: &str,
) {
    let before = ns.source(&key(source)).unwrap().unwrap();
    ns.select(
        &Select {
            prepare: key(&format!("prepare-{label}")),
            adopt: key(&format!("adopt-{label}")),
            source: key(source),
            owner: before.owner,
            expected_revision: before.revision,
            selection,
        },
        nodes,
        &limits(),
    )
    .unwrap();
}

#[test]
fn mixed_root_preserves_aliases_class_and_other_owners() {
    let mut f = Fixture::new();
    enroll(
        &mut f.ns,
        &f.metadata,
        "foreign-job",
        "other-owner",
        Kind::Job,
    );
    let job = tree(&mut f.metadata, &[("input", f.a.clone())]);
    f.metadata.finish().unwrap();
    select(&mut f.ns, &f.metadata, "foreign-job", job, "job");
    let foreign_before = f.ns.source(&key("foreign-job")).unwrap();
    let p = f.plan(&["delete"]);
    assert_eq!(
        p.summary,
        Summary {
            selected_bindings: 1,
            selected_unique_objects: 1,
            survivor_bindings: 2,
            selected_objects_shared_with_survivors: 1
        }
    );
    let status = f.apply(&p);
    assert!(status.settlement_current);
    assert!(!status.authorizes_payload_deletion);
    assert!(
        f.ns.source(&key("main"))
            .unwrap()
            .unwrap()
            .predecessors
            .is_empty()
    );
    let survivor =
        f.ns.source(&p.survivor_source().unwrap().unwrap())
            .unwrap()
            .unwrap();
    assert!(p.survivor_matches(&f.e, &survivor));
    assert_eq!(f.ns.source(&key("foreign-job")).unwrap(), foreign_before);
    assert_eq!(f.demands(&f.a.clone()), vec!["F", "M"]);
    assert_eq!(f.demands(&f.b.clone()), vec!["D", "M"]);
    let mut reader = f.metadata.finished_reader().unwrap();
    let (entries, _) = Tree::from_root(p.survivor.root.clone())
        .entries(&mut reader, TreeLimits::default())
        .unwrap();
    assert_eq!(
        entries,
        [(path("held"), f.b.clone()), (path("shared"), f.a.clone())]
            .into_iter()
            .collect()
    );
}

#[test]
fn full_root_settlement_uses_no_survivor_source_and_keeps_current() {
    let mut f = Fixture::new();
    let current = tree(&mut f.metadata, &[("current", f.b.clone())]);
    f.metadata.finish().unwrap();
    select(&mut f.ns, &f.metadata, "main", current.clone(), "new-main");
    let p = f.plan(&["delete", "held", "shared"]);
    assert_eq!(p.survivor_source().unwrap(), None);
    let before = f.ns.checkpoint().sequence();
    let s = f.apply(&p);
    assert!(s.settlement_current);
    assert_eq!(f.ns.checkpoint().sequence(), before + 1);
    assert_eq!(
        f.ns.source(&key("main"))
            .unwrap()
            .unwrap()
            .current
            .unwrap()
            .selection,
        current
    );
    assert!(f.demands(&f.a.clone()).is_empty());
    assert_eq!(f.demands(&f.b.clone()), vec!["F", "M"]);
}

#[test]
fn canonical_plan_owner_kind_path_and_budget_refusals_are_read_only() {
    let mut f = Fixture::new();
    let p = f.plan(&["delete"]);
    let bytes = p.to_json().unwrap();
    assert_eq!(Plan::from_json(&bytes).unwrap(), p);
    let mut extra = bytes.clone();
    extra.push(b'\n');
    assert!(Plan::from_json(&extra).is_err());
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["unknown"] = true.into();
    assert!(Plan::from_json(&identity::canonical_json(&value).unwrap()).is_err());
    let before = f.ns.checkpoint();
    for paths in [
        vec!["missing"],
        vec!["delete", "delete"],
        vec!["shared", "delete"],
        vec![],
    ] {
        let request = f.request(&paths);
        assert!(plan(&f.e, &mut f.ns, request, &f.metadata).is_err());
    }
    for kind in [Kind::Job, Kind::Retention] {
        let mut e = f.e.clone();
        e.classes = vec![kind];
        assert!(e.id().is_err());
    }
    let mut e = f.e.clone();
    let request = f.request(&["delete"]);
    e.owner = key("foreign");
    assert!(plan(&e, &mut f.ns, request.clone(), &f.metadata).is_err());
    e = f.e.clone();
    e.limits.reads = 1;
    assert!(plan(&e, &mut f.ns, request.clone(), &f.metadata).is_err());
    e = f.e.clone();
    e.writes = 1;
    assert!(plan(&e, &mut f.ns, request, &f.metadata).is_err());
    assert_eq!(f.ns.checkpoint(), before);
}

#[test]
fn every_checkpoint_replays_without_dropping_survivors() {
    for stop in [
        Stage::MetadataFinished,
        Stage::Enrolled,
        Stage::SurvivorProtected,
        Stage::BeforeRetire,
        Stage::Retired,
    ] {
        let mut f = Fixture::new();
        let p = f.plan(&["delete", "shared"]);
        assert_eq!(
            apply_hook(&f.e, &mut f.ns, &mut f.metadata, &p, |stage| {
                if stage == stop {
                    Err(Error::Injected)
                } else {
                    Ok(())
                }
            })
            .unwrap_err(),
            Error::Injected
        );
        assert_eq!(f.demands(&f.b.clone()), vec!["D", "M"]);
        let status = f.apply(&p);
        assert!(status.settlement_current);
        let checkpoint = f.ns.checkpoint();
        let status = f.apply(&p);
        assert!(status.historical_retirement.unwrap().replayed);
        assert_eq!(f.ns.checkpoint(), checkpoint);
        assert_eq!(f.demands(&f.b.clone()), vec!["D", "M"]);
        assert!(f.demands(&f.a.clone()).is_empty());
    }
}

#[test]
fn original_changes_hold_but_replanning_reuses_survivor_effects() {
    let mut f = Fixture::new();
    let p = f.plan(&["delete", "shared"]);
    apply_hook(&f.e, &mut f.ns, &mut f.metadata, &p, |stage| {
        if stage == Stage::SurvivorProtected {
            Err(Error::Injected)
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    let fresh = tree(&mut f.metadata, &[("active", f.a.clone())]);
    f.metadata.finish().unwrap();
    select(&mut f.ns, &f.metadata, "main", fresh, "advance");
    assert_eq!(
        apply(&f.e, &mut f.ns, &mut f.metadata, &p).unwrap_err(),
        Error::Stale
    );
    let p2 = f.plan(&["delete", "shared"]);
    assert_eq!(p.survivor_source().unwrap(), p2.survivor_source().unwrap());
    assert_ne!(p.id().unwrap(), p2.id().unwrap());
    let before = f.ns.checkpoint().sequence();
    assert!(f.apply(&p2).settlement_current);
    assert_eq!(f.ns.checkpoint().sequence(), before + 1);
    assert_eq!(f.demands(&f.a.clone()), vec!["F", "M"]);
}

#[test]
fn survivor_change_at_final_boundary_holds_original_retirement() {
    let mut f = Fixture::new();
    let p = f.plan(&["delete"]);
    let minimum = f.ns.checkpoint();
    let root = f.temp.path().join("ns");
    let reader = f.metadata.finished_reader().unwrap();
    let outcome = apply_hook(&f.e, &mut f.ns, &mut f.metadata, &p, |stage| {
        if stage == Stage::BeforeRetire {
            let mut peer = Namespace::open(&root, &minimum).unwrap();
            peer.apply(
                key("retire-survivor"),
                Command::Retire {
                    source: p.survivor_source().unwrap().unwrap(),
                    owner: key("owner"),
                    expected_revision: 4,
                    predecessor_revision: 3,
                },
                &reader,
                &limits(),
            )
            .unwrap();
        }
        Ok(())
    });
    assert!(outcome.is_err());
    assert!(
        !f.ns
            .source(&key("main"))
            .unwrap()
            .unwrap()
            .predecessors
            .is_empty()
    );
    assert!(!inspect(&f.e, &mut f.ns, &p).unwrap().settlement_current);
    assert!(apply(&f.e, &mut f.ns, &mut f.metadata, &p).is_err());
}

#[test]
fn historical_retry_does_not_recreate_later_retired_survivor() {
    let mut f = Fixture::new();
    let p = f.plan(&["delete"]);
    f.apply(&p);
    f.ns.apply(
        key("later-authorized-retire"),
        Command::Retire {
            source: p.survivor_source().unwrap().unwrap(),
            owner: key("owner"),
            expected_revision: 4,
            predecessor_revision: 3,
        },
        &f.metadata,
        &limits(),
    )
    .unwrap();
    let before = f.ns.checkpoint();
    let s = f.apply(&p);
    assert!(s.historical_retirement.is_some());
    assert!(!s.settlement_current);
    assert!(!s.survivor_protection_current);
    assert_eq!(f.ns.checkpoint(), before);
}

#[test]
fn matching_foreign_enrollment_without_controller_receipt_is_held() {
    let mut f = Fixture::new();
    let p = f.plan(&["delete"]);
    let source = p.survivor_source().unwrap().unwrap();
    enroll(
        &mut f.ns,
        &f.metadata,
        source.as_str(),
        "owner",
        Kind::Branch,
    );
    assert_eq!(
        apply(&f.e, &mut f.ns, &mut f.metadata, &p).unwrap_err(),
        Error::Stale
    );
    assert!(
        f.ns.source(&key("main"))
            .unwrap()
            .unwrap()
            .predecessors
            .len()
            == 1
    );
}

#[test]
fn missing_corrupt_closure_pending_metadata_and_tampered_plan_hold() {
    struct Missing;
    impl NodeStore for Missing {
        fn get(&self, _: &Object) -> TreeResult<Option<Vec<u8>>> {
            Ok(None)
        }
        fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
            unreachable!()
        }
    }
    struct Corrupt;
    impl NodeStore for Corrupt {
        fn get(&self, _: &Object) -> TreeResult<Option<Vec<u8>>> {
            Ok(Some(b"wrong".to_vec()))
        }
        fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
            unreachable!()
        }
    }
    let mut f = Fixture::new();
    let request = f.request(&["delete"]);
    let before = f.ns.checkpoint();
    assert!(plan(&f.e, &mut f.ns, request.clone(), &Missing).is_err());
    assert!(plan(&f.e, &mut f.ns, request, &Corrupt).is_err());
    let p = f.plan(&["delete"]);
    let mut changed = p.clone();
    changed.summary.survivor_bindings = 1;
    assert_eq!(
        apply(&f.e, &mut f.ns, &mut f.metadata, &changed).unwrap_err(),
        Error::Stale
    );
    f.metadata.put(b"unrelated-unfinished-node").unwrap();
    assert_eq!(
        apply(&f.e, &mut f.ns, &mut f.metadata, &p).unwrap_err(),
        Error::PendingMetadata
    );
    assert_eq!(f.ns.checkpoint(), before);
}

#[test]
fn unrelated_admission_and_changed_run_budget_reuse_exact_survivor() {
    let mut f = Fixture::new();
    let p = f.plan(&["delete"]);
    apply_hook(&f.e, &mut f.ns, &mut f.metadata, &p, |stage| {
        if stage == Stage::SurvivorProtected {
            Err(Error::Injected)
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    f.e.limits.millis = 30_000;
    let p2 = f.plan(&["delete"]);
    assert_eq!(p.survivor_source().unwrap(), p2.survivor_source().unwrap());
    let root = f.temp.path().join("ns");
    let minimum = f.ns.checkpoint();
    let reader = f.metadata.finished_reader().unwrap();
    assert!(
        apply_hook(&f.e, &mut f.ns, &mut f.metadata, &p2, |stage| {
            if stage == Stage::BeforeRetire {
                let mut peer = Namespace::open(&root, &minimum).unwrap();
                enroll(&mut peer, &reader, "unrelated", "other", Kind::Job);
            }
            Ok(())
        })
        .unwrap()
        .settlement_current
    );
    assert!(f.ns.source(&key("unrelated")).unwrap().is_some());
}

#[test]
fn sparse_replacement_reuses_large_root_metadata() {
    let mut f = Fixture::new();
    let bindings = (0..4096)
        .map(|i| {
            (
                path(&format!("cues/{i:05}")),
                Object::from_bytes(format!("cue-{i}").as_bytes()),
            )
        })
        .collect();
    let (large, _) = Tree::build(&bindings, &mut f.metadata, TreeLimits::default()).unwrap();
    f.metadata.finish().unwrap();
    select(
        &mut f.ns,
        &f.metadata,
        "main",
        Selection {
            root: large.root().clone(),
            context: Digest::blob(b"large"),
        },
        "large",
    );
    let rev = f.ns.source(&key("main")).unwrap().unwrap().revision;
    f.ns.apply(
        key("end-large"),
        Command::Withdraw {
            source: key("main"),
            owner: key("owner"),
            expected_revision: rev,
        },
        &f.metadata,
        &limits(),
    )
    .unwrap();
    let request = Request {
        source: key("main"),
        predecessor_revision: rev,
        paths: vec![path("cues/00042")],
    };
    let p = plan(
        &f.e,
        &mut f.ns,
        request,
        &f.metadata.finished_reader().unwrap(),
    )
    .unwrap();
    let before = f.metadata.checkpoint().nodes();
    assert!(f.apply(&p).settlement_current);
    let written = f.metadata.checkpoint().nodes() - before;
    assert!(written > 0 && written < 128, "sparse nodes: {written}");
    assert_eq!(p.summary.survivor_bindings, 4095);
}

#[derive(Serialize, Deserialize)]
struct Saved {
    e: Enrollment,
    plan: Plan,
    ns: Checkpoint,
    metadata: metadata::Checkpoint,
}

#[test]
#[ignore = "requires GIT_ASSET_BASELINE_SOURCE_CLI built from pinned pre-pair source"]
fn baseline_reader_preserves_subset_states_and_retirement_receipt() {
    let executable = std::env::var_os("GIT_ASSET_BASELINE_SOURCE_CLI")
        .expect("explicit old-reader artifact required");
    assert!(PathBuf::from(&executable).is_absolute());
    let mut f = Fixture::new();
    let minimum = f.ns.checkpoint();
    let p = f.plan(&["delete"]);
    f.apply(&p);
    for (index, source) in [key("main"), p.survivor_source().unwrap().unwrap()]
        .into_iter()
        .enumerate()
    {
        let profile = f.temp.path().join(format!("old-profile-{index}.json"));
        let report = f.temp.path().join(format!("old-report-{index}.json"));
        fs::write(&profile, serde_json::to_vec(&serde_json::json!({"schema":"tig.source-observation-profile.experimental-1","journal_root":f.temp.path().join("ns"),"minimum":minimum,"source":source})).unwrap()).unwrap();
        let output = Process::new(&executable)
            .arg("inspect")
            .arg(&profile)
            .arg(&report)
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        let observed: Source = serde_json::from_value(value["source"].clone()).unwrap();
        assert_eq!(Some(observed), f.ns.source(&source).unwrap());
    }
    let (operation, command) = p.retire(&f.e).unwrap();
    let profile = f.temp.path().join("old-ack-profile.json");
    let report = f.temp.path().join("old-ack-report.json");
    fs::write(&profile, serde_json::to_vec(&serde_json::json!({"schema":"tig.source-acknowledgment-profile.experimental-1","journal_root":f.temp.path().join("ns"),"minimum":minimum,"operation":operation,"command":command})).unwrap()).unwrap();
    let output = Process::new(&executable)
        .arg("acknowledge")
        .arg(profile)
        .arg(&report)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(value["replayed"], true);
    assert_eq!(value["source_revision"], 5);
}
#[test]
fn abrupt_exit_helper() {
    let Some(root) = std::env::var_os("GIT_ASSET_SUBSET_EXIT_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let saved: Saved = serde_json::from_slice(&fs::read(root.join("saved.json")).unwrap()).unwrap();
    let mut ns = Namespace::open(&root.join("ns"), &saved.ns).unwrap();
    let mut metadata = MetadataStore::open(&root.join("metadata"), &saved.metadata).unwrap();
    let stop = std::env::var("GIT_ASSET_SUBSET_EXIT_STAGE").unwrap();
    apply_hook(&saved.e, &mut ns, &mut metadata, &saved.plan, |stage| {
        if format!("{stage:?}") == stop {
            std::process::exit(79);
        }
        Ok(())
    })
    .unwrap();
    panic!("requested exit was not reached");
}
#[test]
fn real_process_exit_before_and_after_original_retirement_recovers() {
    for stage in [Stage::Enrolled, Stage::SurvivorProtected, Stage::Retired] {
        let mut f = Fixture::new();
        let p = f.plan(&["delete", "shared"]);
        let saved = Saved {
            e: f.e.clone(),
            plan: p.clone(),
            ns: f.ns.checkpoint(),
            metadata: f.metadata.checkpoint(),
        };
        fs::write(
            f.temp.path().join("saved.json"),
            serde_json::to_vec(&saved).unwrap(),
        )
        .unwrap();
        let output = Process::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "source_claims::subset::tests::abrupt_exit_helper",
                "--nocapture",
            ])
            .env("GIT_ASSET_SUBSET_EXIT_ROOT", f.temp.path())
            .env("GIT_ASSET_SUBSET_EXIT_STAGE", format!("{stage:?}"))
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(79),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        drop(f.ns);
        drop(f.metadata);
        f.ns = Namespace::open(&f.temp.path().join("ns"), &saved.ns).unwrap();
        f.metadata = MetadataStore::open(&f.temp.path().join("metadata"), &saved.metadata).unwrap();
        assert!(!f.demands(&f.b.clone()).is_empty());
        assert!(f.apply(&p).settlement_current);
        assert_eq!(f.demands(&f.b.clone()), vec!["D", "M"]);
        assert!(f.demands(&f.a.clone()).is_empty());
    }
}
