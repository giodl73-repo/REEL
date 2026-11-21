use super::*;
use crate::source_claims::protection::effects::{Admission, Outcome, Request};

fn fixture() -> Fixture {
    Fixture::configured_effects(3, false, false, true)
}
fn request(f: &mut Fixture, op: &str, n: usize) -> Request {
    Request {
        operation: key(op),
        object: object(n),
        store: d("F"),
        context: d(op),
        expected: f.index.effect_frame().unwrap(),
    }
}
fn begin(f: &mut Fixture, r: &Request) -> effects::HeldEffect {
    match f.index.begin_effect(r, &mut f.nodes).unwrap() {
        Admission::New(held) => held,
        _ => panic!("expected new admission"),
    }
}

struct Stopped(Digest);

#[test]
fn interrupted_archive_keeps_selected_outcomes_and_resumes_without_new_effect_permission() {
    let mut f = fixture();
    let mut requests = Vec::new();
    for n in 0..8 {
        let r = request(&mut f, &format!("interrupted-archive-{n}"), 99);
        begin(&mut f, &r)
            .finish(
                &mut f.index,
                &mut f.nodes,
                Outcome::NoEffect {
                    evidence: d(&format!("stopped-{n}")),
                },
            )
            .unwrap();
        requests.push(r);
    }
    let frame = f.index.effect_frame().unwrap();
    let before = f.index.checkpoint();
    assert!(matches!(
        f.index
            .archive_effect_history_with_hook(&object(99), &mut f.nodes, || Err(Error::Injected)),
        Err(Error::Injected)
    ));
    assert_ne!(f.index.checkpoint(), before);
    f.reopen();
    for r in &requests {
        assert!(matches!(
            f.index.begin_effect(r, &mut f.nodes).unwrap(),
            Admission::Historical(Outcome::NoEffect { .. })
        ));
    }
    assert!(
        f.index
            .archive_effect_history(&object(99), &mut f.nodes)
            .unwrap()
            > 0
    );
    assert_eq!(f.index.effect_frame().unwrap(), frame);
    f.reopen();
    for r in &requests {
        assert!(matches!(
            f.index.inspect_effect(r, &f.nodes).unwrap(),
            Some(Admission::Historical(_))
        ));
    }
}

#[test]
fn archived_effect_history_crosses_repeated_effect_limit_without_renewing_old_requests() {
    let mut f = fixture();
    let frame = f.index.effect_frame().unwrap();
    let mut requests = Vec::new();
    for n in 0..70 {
        let mut r = request(&mut f, &format!("archive-op-{n}"), 99);
        if n % 2 == 0 {
            r.store = d("D");
        }
        let outcome = if n % 3 == 0 {
            Outcome::Removed {
                evidence: d(&format!("removed-{n}")),
            }
        } else {
            Outcome::NoEffect {
                evidence: d(&format!("no-effect-{n}")),
            }
        };
        begin(&mut f, &r)
            .finish(&mut f.index, &mut f.nodes, outcome.clone())
            .unwrap();
        requests.push((r, outcome));
        if n % 13 == 0 {
            f.reopen();
        }
    }
    assert_eq!(f.index.effect_frame().unwrap(), frame);
    f.reopen();
    let before = f.index.checkpoint();
    for (r, outcome) in &requests {
        assert!(
            matches!(f.index.begin_effect(r, &mut f.nodes).unwrap(), Admission::Historical(found) if found == *outcome)
        );
    }
    assert_eq!(f.index.checkpoint(), before);
    let mut changed = requests[0].0.clone();
    changed.context = d("changed-old-context");
    assert!(matches!(
        f.index.begin_effect(&changed, &mut f.nodes),
        Err(Error::Conflict)
    ));
    assert!(
        f.index
            .archive_effect_history(&object(99), &mut f.nodes)
            .unwrap()
            > 0
    );
    let before = f.index.checkpoint();
    assert_eq!(
        f.index
            .archive_effect_history(&object(99), &mut f.nodes)
            .unwrap(),
        0
    );
    assert_eq!(f.index.checkpoint(), before);
    let protected = f.request(
        "jobs",
        "new-archive-job",
        [(p("jobs/archived"), Some(object(99)))].into(),
        true,
    );
    f.index.update(&protected, &mut f.nodes).unwrap();
    assert!(matches!(
        f.index.begin_effect(&requests[0].0, &mut f.nodes).unwrap(),
        Admission::Historical(_)
    ));
    let fresh = request(&mut f, "new-protected-effect", 99);
    assert!(matches!(
        f.index.begin_effect(&fresh, &mut f.nodes),
        Err(Error::Protected)
    ));
}

#[test]
fn effect_history_refuses_pending_work_and_missing_archive_instead_of_readmitting_old_effect() {
    let mut f = fixture();
    let r = request(&mut f, "archive-old", 99);
    begin(&mut f, &r)
        .finish(
            &mut f.index,
            &mut f.nodes,
            Outcome::NoEffect {
                evidence: d("no worker"),
            },
        )
        .unwrap();
    assert!(
        f.index
            .archive_effect_history(&object(99), &mut f.nodes)
            .unwrap()
            > 0
    );
    let fresh = request(&mut f, "archive-unresolved", 99);
    drop(begin(&mut f, &fresh));
    let before = f.index.checkpoint();
    assert!(
        f.index
            .archive_effect_history(&object(99), &mut f.nodes)
            .is_err()
    );
    assert_eq!(f.index.checkpoint(), before);
    let fingerprint = effects::fingerprint(f.index.identity(), &fresh).unwrap();
    f.index
        .reconcile_effect(&fresh, &Stopped(fingerprint), &mut f.nodes, || {
            Ok(Outcome::NoEffect {
                evidence: d("confirmed stopped"),
            })
        })
        .unwrap();
    // Fault an outcome known only through the archive, after redundant live
    // copies have been consolidated. Independent terminal copies remain valid.
    f.index
        .archive_effect_history(&object(99), &mut f.nodes)
        .unwrap();
    let source = key(&format!("effect-{}", object(99).sha256.as_str()));
    let state = f.index.journal.source(&source).unwrap().unwrap();
    let context = d("tig.protection-effect-history.design-1");
    let archived = state
        .predecessors
        .iter()
        .find(|v| v.selection.context == context)
        .unwrap();
    let mut header: serde_json::Value =
        serde_json::from_slice(&f.nodes.get(&archived.selection.root).unwrap().unwrap()).unwrap();
    header["tree"] = serde_json::to_value(Object::from_bytes(b"missing history tree")).unwrap();
    let corrupt = f
        .nodes
        .put(&identity::canonical_json(&header).unwrap())
        .unwrap();
    f.nodes.finish().unwrap();
    let authority = f.index.source().unwrap().unwrap();
    f.index
        .command(
            key("replace-archive-with-missing-tree"),
            Command::RetireGuarded {
                source,
                owner: key("owner"),
                expected_revision: state.revision,
                predecessor_revision: archived.revision,
                replacement: Some(Selection {
                    root: corrupt,
                    context,
                }),
                guards: vec![crate::source_claims::Guard {
                    source: key("authority"),
                    owner: key("owner"),
                    expected_revision: authority.revision,
                }],
            },
        )
        .unwrap();
    let before = f.index.checkpoint();
    assert!(f.index.inspect_effect(&r, &f.nodes).is_err());
    assert!(f.index.begin_effect(&r, &mut f.nodes).is_err());
    assert!(
        f.index
            .archive_effect_history(&object(99), &mut f.nodes)
            .is_err()
    );
    assert_eq!(f.index.checkpoint(), before);
}
#[test]
fn policy_allowance_frame_follows_accepted_source_not_unrelated_job_activity() {
    let mut f = fixture();
    let before = f.index.policy_frame(&f.nodes, &key("main")).unwrap();
    let job = f.request(
        "jobs",
        "job-does-not-reset-policy",
        [(p("jobs/b"), Some(object(99)))].into(),
        true,
    );
    f.index.update(&job, &mut f.nodes).unwrap();
    assert_eq!(
        f.index.policy_frame(&f.nodes, &key("main")).unwrap(),
        before
    );
    let main = f.request(
        "main",
        "accept-next-main",
        [(p("assets/a-00000"), Some(object(98)))].into(),
        true,
    );
    f.index.update(&main, &mut f.nodes).unwrap();
    let after = f.index.policy_frame(&f.nodes, &key("main")).unwrap();
    assert_ne!(after.selection, before.selection);
    assert!(after.generation > before.generation);
    let unknown = f.request("main", "unknown-main", BTreeMap::new(), false);
    f.index.update(&unknown, &mut f.nodes).unwrap();
    assert!(matches!(
        f.index.policy_frame(&f.nodes, &key("main")),
        Err(Error::Unknown)
    ));
    assert!(
        Fixture::new(3)
            .index
            .policy_frame(&f.nodes, &key("main"))
            .is_err()
    );
}
impl effects::Terminated for Stopped {
    fn effect(&self) -> &Digest {
        &self.0
    }
}

#[test]
fn reservation_without_begin_requires_recovery_and_never_renews_permission() {
    use crate::source_claims::progress_budget;
    let mut f = fixture();
    let r = request(&mut f, "publication-failed", 99);
    let fingerprint = effects::fingerprint(f.index.identity(), &r).unwrap();
    let id = key(&format!("effect-budget-{}", fingerprint.as_str()));
    let source = key(&format!("effect-{}", r.object.sha256.as_str()));
    let guards = vec![crate::source_claims::Guard {
        source: key("authority"),
        owner: key("owner"),
        expected_revision: f.index.source().unwrap().unwrap().revision,
    }];
    progress_budget::reserve_mode(
        &mut f.index.journal,
        id,
        key("owner"),
        source,
        0,
        guards,
        true,
    )
    .unwrap();
    let held = progress_budget::capacity(&mut f.index.journal)
        .unwrap()
        .held_events;
    assert!(held > 0);
    assert!(f.index.inspect_effect(&r, &f.nodes).unwrap().is_none());
    assert_eq!(
        progress_budget::capacity(&mut f.index.journal)
            .unwrap()
            .held_events,
        held
    );
    f.reopen();
    let result = f
        .index
        .reconcile_effect(&r, &Stopped(fingerprint), &mut f.nodes, || {
            panic!("no worker was admitted; no provider observation needed")
        })
        .unwrap();
    assert!(matches!(result, Outcome::NoEffect { .. }));
    assert_eq!(
        progress_budget::capacity(&mut f.index.journal)
            .unwrap()
            .held_events,
        0
    );
    assert!(matches!(
        f.index.begin_effect(&r, &mut f.nodes).unwrap(),
        Admission::Historical(Outcome::NoEffect { .. })
    ));
}

#[test]
fn ordinary_source_creation_cannot_steal_reserved_effect_source_slot() {
    use crate::source_claims::{MAX_SOURCES, progress_budget};
    let mut f = fixture();
    let source = key("effect-reserved-slot");
    let guards = vec![crate::source_claims::Guard {
        source: key("authority"),
        owner: key("owner"),
        expected_revision: f.index.source().unwrap().unwrap().revision,
    }];
    progress_budget::reserve_mode(
        &mut f.index.journal,
        key("slot-budget"),
        key("owner"),
        source.clone(),
        0,
        guards,
        true,
    )
    .unwrap();
    // Exercise the admission predicate at the exact bounded limit without
    // manufacturing thousands of unrelated durable journal events.
    let mut state = f.index.journal.state.clone();
    let exemplar = state.sources.values().next().unwrap().clone();
    for i in state.sources.len()..MAX_SOURCES - 1 {
        state
            .sources
            .insert(format!("occupied-{i}"), exemplar.clone());
    }
    let reserved = Command::Prepare {
        source,
        owner: key("owner"),
        expected_revision: 0,
        selection: f.index.effect_frame().unwrap(),
    };
    let mut other = reserved.clone();
    if let Command::Prepare { source, .. } = &mut other {
        *source = key("unreserved");
    }
    assert!(matches!(
        effects::admit_capacity(&state, &other),
        Err(crate::source_claims::Error::Budget)
    ));
    effects::admit_capacity(&state, &reserved).unwrap();
}

#[test]
fn held_effect_blocks_conflicting_job_and_allows_another_object_then_completes() {
    let mut f = fixture();
    let r = request(&mut f, "remove-a", 99);
    let held = begin(&mut f, &r);
    let conflict = f.request(
        "jobs",
        "job-a",
        [(p("jobs/a"), Some(object(99)))].into(),
        true,
    );
    assert!(f.index.update(&conflict, &mut f.nodes).is_err());
    let unrelated = f.request(
        "jobs",
        "job-b",
        [(p("jobs/b"), Some(object(98)))].into(),
        true,
    );
    f.index.update(&unrelated, &mut f.nodes).unwrap();
    held.finish(
        &mut f.index,
        &mut f.nodes,
        Outcome::NoEffect {
            evidence: d("worker exited without effect"),
        },
    )
    .unwrap();
    let conflict = f.request(
        "jobs",
        "job-a-after",
        [(p("jobs/a"), Some(object(99)))].into(),
        true,
    );
    f.index.update(&conflict, &mut f.nodes).unwrap();
    assert_eq!(
        f.index
            .inspect(&mut f.nodes, &object(99), &d("F"))
            .unwrap()
            .protected_by,
        vec![key("jobs")]
    );
}

#[test]
fn abandoned_pending_survives_reopen_and_new_plan_cannot_clear_it() {
    let mut f = fixture();
    let r = request(&mut f, "abandoned", 99);
    drop(begin(&mut f, &r));
    f.reopen();
    let newer = request(&mut f, "new-plan", 99);
    assert!(
        matches!(f.index.begin_effect(&newer, &mut f.nodes).unwrap(), Admission::Uncertain(old) if old == r)
    );
    let conflict = f.request(
        "jobs",
        "late-job",
        [(p("jobs/a"), Some(object(99)))].into(),
        true,
    );
    assert!(matches!(
        f.index.update(&conflict, &mut f.nodes),
        Err(Error::Pending)
    ));
    // Coverage invalidation with no changed leaves must also honor the durable fence.
    let unknown = f.request("jobs", "unknown-coverage", BTreeMap::new(), false);
    assert!(matches!(
        f.index.update(&unknown, &mut f.nodes),
        Err(Error::Pending)
    ));
    let unrelated = f.request(
        "jobs",
        "unrelated",
        [(p("jobs/b"), Some(object(98)))].into(),
        true,
    );
    f.index.update(&unrelated, &mut f.nodes).unwrap();
}

#[test]
fn historical_completion_does_not_revalidate_or_remove_reingested_same_key() {
    let mut f = fixture();
    let r = request(&mut f, "completed", 99);
    begin(&mut f, &r)
        .finish(
            &mut f.index,
            &mut f.nodes,
            Outcome::Removed {
                evidence: d("actual completed provider result"),
            },
        )
        .unwrap();
    let demand = f.request(
        "jobs",
        "reingested",
        [(p("jobs/a"), Some(object(99)))].into(),
        true,
    );
    f.index.update(&demand, &mut f.nodes).unwrap();
    assert!(matches!(
        f.index.begin_effect(&r, &mut f.nodes).unwrap(),
        Admission::Historical(Outcome::Removed { .. })
    ));
    let mut changed = r.clone();
    changed.context = d("changed consent");
    assert!(matches!(
        f.index.begin_effect(&changed, &mut f.nodes),
        Err(Error::Conflict)
    ));
    let fresh = request(&mut f, "fresh-operation", 99);
    assert!(matches!(
        f.index.begin_effect(&fresh, &mut f.nodes),
        Err(Error::Protected)
    ));
}

#[test]
fn changed_frame_existing_protection_unknown_metadata_and_legacy_profile_refuse_before_effect() {
    let mut f = fixture();
    let protected = request(&mut f, "protected", 0);
    assert!(matches!(
        f.index.begin_effect(&protected, &mut f.nodes),
        Err(Error::Protected)
    ));
    let mut stale = request(&mut f, "stale", 99);
    stale.expected.context = d("different frame");
    assert!(matches!(
        f.index.begin_effect(&stale, &mut f.nodes),
        Err(Error::Stale)
    ));
    let fresh = request(&mut f, "missing-metadata", 99);
    let mut empty = nodes(&f.temp.path().join("offline"));
    assert!(f.index.begin_effect(&fresh, &mut empty).is_err());
    assert!(
        f.index
            .journal
            .control_sources(&key("owner"), "effect-", 1024)
            .unwrap()
            .is_empty()
    );
    let mut old = Fixture::new(3);
    assert!(old.index.begin_effect(&fresh, &mut old.nodes).is_err());
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(f.temp.path().join("index/claims.json")).unwrap())
            .unwrap();
    assert_eq!(config["schema"], "tig.source-claims.design-9");
    assert_eq!(config["effect_fences"], true);
    let old_config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(old.temp.path().join("index/claims.json")).unwrap())
            .unwrap();
    assert!(old_config.get("effect_fences").is_none());
}

#[test]
fn completion_metadata_failure_preserves_pending_after_live_guard_is_lost() {
    let mut f = fixture();
    let r = request(&mut f, "outcome-failure", 99);
    let held = begin(&mut f, &r);
    let mut unavailable = nodes(&f.temp.path().join("unavailable"));
    assert!(
        held.finish(
            &mut f.index,
            &mut unavailable,
            Outcome::NoEffect {
                evidence: d("worker terminated")
            }
        )
        .is_err()
    );
    f.reopen();
    assert!(matches!(
        f.index.begin_effect(&r, &mut f.nodes).unwrap(),
        Admission::Uncertain(_)
    ));
}

#[test]
fn completion_uses_reserved_headroom_after_ordinary_event_exhaustion() {
    let mut f = fixture();
    let r = request(&mut f, "reserved", 99);
    let held = begin(&mut f, &r);
    let selection = f.index.effect_frame().unwrap();
    let mut revision = 0;
    let mut i = 0;
    loop {
        let prepared = f.index.command(
            key(&format!("pressure-{i}-prepare")),
            Command::Prepare {
                source: key("pressure"),
                owner: key("owner"),
                expected_revision: revision,
                selection: selection.clone(),
            },
        );
        let receipt = match prepared {
            Ok(r) => r,
            Err(Error::Journal(super::super::super::Error::Budget)) => break,
            Err(e) => panic!("{e:?}"),
        };
        revision += 1;
        match f.index.command(
            key(&format!("pressure-{i}-abort")),
            Command::Abort {
                source: key("pressure"),
                owner: key("owner"),
                expected_revision: revision,
                prepared: receipt.reference,
            },
        ) {
            Ok(_) => revision += 1,
            Err(Error::Journal(super::super::super::Error::Budget)) => break,
            Err(e) => panic!("{e:?}"),
        }
        i += 1;
    }
    assert!(i > 1000);
    held.finish(
        &mut f.index,
        &mut f.nodes,
        Outcome::NoEffect {
            evidence: d("provider never acted"),
        },
    )
    .unwrap();
    let next = request(&mut f, "must-refuse", 98);
    assert!(matches!(
        f.index.begin_effect(&next, &mut f.nodes),
        Err(Error::Journal(super::super::super::Error::Budget))
    ));
}

#[test]
#[ignore = "invoked only by actual process-loss fixture"]
fn pending_effect_child() {
    let marker = std::env::var_os("TIG_TEST_EFFECT_MARKER").unwrap();
    let mut f = fixture();
    let r = request(&mut f, "crashed-worker", 99);
    let _held = begin(&mut f, &r);
    let value = serde_json::json!({"root":f.temp.path(),"index":f.index.checkpoint(),"metadata":f.nodes.checkpoint(),"request":r});
    std::fs::write(marker, serde_json::to_vec(&value).unwrap()).unwrap();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
fn actual_controller_process_exit_preserves_pending_admission_fence() {
    let parent = tempfile::tempdir().unwrap();
    let marker = parent.path().join("child.json");
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "source_claims::protection::tests::effects_tests::pending_effect_child",
            "--ignored",
        ])
        .env("TIG_TEST_EFFECT_MARKER", &marker)
        .env("TEMP", parent.path())
        .env("TMP", parent.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while !marker.exists() && start.elapsed() < std::time::Duration::from_secs(15) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let ready = marker.exists();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(ready);
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(marker).unwrap()).unwrap();
    let root: PathBuf = serde_json::from_value(value["root"].clone()).unwrap();
    assert!(root.starts_with(parent.path()));
    let minimum = serde_json::from_value(value["index"].clone()).unwrap();
    let mut index = Index::open(&root.join("index"), &minimum).unwrap();
    let mut nodes = metadata::MetadataStore::open(
        &root.join("nodes"),
        &serde_json::from_value(value["metadata"].clone()).unwrap(),
    )
    .unwrap();
    let r = serde_json::from_value(value["request"].clone()).unwrap();
    assert!(matches!(
        index.begin_effect(&r, &mut nodes).unwrap(),
        Admission::Uncertain(_)
    ));
    let state = index.actor(&mut nodes, &key("jobs")).unwrap();
    let old = Tree::from_root(state.selection.root.clone());
    let changes = [(p("jobs/late"), Some(object(99)))].into();
    let (next, _) = old
        .edit(old.root(), &changes, &mut nodes, limits())
        .unwrap();
    let update = Update {
        operation: key("late-job"),
        actor: key("jobs"),
        owner: key("source-owner"),
        expected_revision: state.revision,
        selection: Selection {
            root: next.root().clone(),
            context: d("new-job"),
        },
        changes,
        ready: true,
    };
    assert!(matches!(
        index.update(&update, &mut nodes),
        Err(Error::Pending)
    ));
}
