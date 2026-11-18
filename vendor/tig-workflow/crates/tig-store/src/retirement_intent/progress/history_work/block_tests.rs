use super::*;
#[path = "block_native_tests.rs"]
mod native;
#[path = "block_pressure_tests.rs"]
mod pressure;
#[path = "block_stop_tests.rs"]
mod stop;
use crate::retirement_intent::{Error, progress::history_work::tests::Fixture};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn d(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn members(f: &Fixture) -> BTreeSet<Object> {
    f.ctx
        .decision()
        .consent()
        .release_owned_claims
        .iter()
        .map(|r| r.object.clone())
        .collect()
}
fn planned(f: &mut Fixture) -> Block {
    let objects = members(f);
    plan(
        &mut f.j,
        &mut f.n,
        &f.ctx,
        &key("main"),
        2,
        &objects,
        &Enrollment {
            owner: key("source-owner"),
            subject: d("subject"),
        },
    )
    .unwrap()
}
fn anchor(b: &Block) -> (&Object, u64) {
    (&b.members[0].object, b.members[0].attempt)
}
fn begin(f: &mut Fixture) -> Block {
    let b = planned(f);
    start(&mut f.j, &mut f.n, &f.ctx, b.clone()).unwrap();
    b
}
#[test]
fn block_retires_both_approved_objects_in_one_projection_and_preserves_other_history() {
    let mut f = Fixture::new_block();
    let original = f.j.source(&key("main")).unwrap().unwrap();
    let b = begin(&mut f);
    assert_eq!(b.members.len(), 2);
    f.reopen();
    let (a, attempt) = anchor(&b);
    let found = current(&mut f.j, &f.n, &f.ctx, a).unwrap().unwrap();
    assert_eq!(found.1, b);
    resume(&mut f.j, &mut f.n, &f.ctx, a, attempt).unwrap();
    let evidence = apply(&mut f.j, &f.n, &f.ctx, a, attempt).unwrap();
    f.reopen();
    assert_eq!(apply(&mut f.j, &f.n, &f.ctx, a, attempt).unwrap(), evidence);
    let state = f.j.source(&key("main")).unwrap().unwrap();
    assert_eq!(state.revision, original.revision + 1);
    assert_eq!(state.current, original.current);
    let expected = f
        .old
        .iter()
        .filter(|(_, o)| !members(&f).contains(o))
        .map(|(p, o)| (p.clone(), o.clone()))
        .collect();
    assert_eq!(f.retained(), expected);
    report(&mut f.j, &mut f.n, &f.ctx, a, attempt).unwrap();
    for m in &b.members {
        assert!(
            inspect(&mut f.j, &f.n, &f.ctx, &m.object)
                .unwrap()
                .released
                .contains(&m.claim)
        );
    }
    assert_eq!(apply(&mut f.j, &f.n, &f.ctx, a, attempt).unwrap(), evidence);
}
#[test]
fn partial_member_admission_resumes_without_retaining_a_block_plan() {
    let mut f = Fixture::new_block();
    let b = planned(&mut f);
    let (a, attempt) = anchor(&b);
    assert!(
        start_hook(&mut f.j, &mut f.n, &f.ctx, b.clone(), |_, _| Err(
            Error::Invalid
        ))
        .is_err()
    );
    let before = f.j.source(&key("main")).unwrap().unwrap();
    assert!(apply(&mut f.j, &f.n, &f.ctx, a, attempt).is_err());
    assert_eq!(before, f.j.source(&key("main")).unwrap().unwrap());
    f.reopen();
    let (_, restored) = current(&mut f.j, &f.n, &f.ctx, a).unwrap().unwrap();
    assert_eq!(b, restored);
    resume(&mut f.j, &mut f.n, &f.ctx, a, attempt).unwrap();
    apply(&mut f.j, &f.n, &f.ctx, a, attempt).unwrap();
    report(&mut f.j, &mut f.n, &f.ctx, a, attempt).unwrap();
}
#[test]
fn cancellation_fences_partial_admission_and_reports_only_real_admitted_no_effect() {
    let mut f = Fixture::new_block();
    let b = planned(&mut f);
    let (a, attempt) = anchor(&b);
    assert!(
        start_hook(&mut f.j, &mut f.n, &f.ctx, b.clone(), |j, _| {
            crate::retirement_intent::cancel(
                j,
                &key("controller"),
                &d("retire"),
                2,
                key("cancel-intent"),
            )?;
            Err(Error::Cancelled)
        })
        .is_err()
    );
    assert!(resume(&mut f.j, &mut f.n, &f.ctx, a, attempt).is_err());
    cancel(&mut f.j, &mut f.n, &f.ctx, a, attempt).unwrap();
    assert!(apply(&mut f.j, &f.n, &f.ctx, a, attempt).is_err());
    assert!(
        inspect(&mut f.j, &f.n, &f.ctx, a)
            .unwrap()
            .in_flight
            .is_none()
    );
    let other = &b.members[1];
    assert!(
        inspect(&mut f.j, &f.n, &f.ctx, &other.object)
            .unwrap()
            .in_flight
            .is_none()
    );
    assert_eq!(f.retained(), f.old);
    cancel(&mut f.j, &mut f.n, &f.ctx, a, attempt).unwrap();
}
#[test]
fn stopped_group_refuses_old_workers_and_allows_a_fresh_same_scope_attempt() {
    let mut f = Fixture::new_block();
    let b = begin(&mut f);
    let (a, attempt) = anchor(&b);
    cancel(&mut f.j, &mut f.n, &f.ctx, a, attempt).unwrap();
    assert!(apply(&mut f.j, &f.n, &f.ctx, a, attempt).is_err());
    f.reopen();
    let next = begin(&mut f);
    assert_eq!(next.members[0].attempt, b.members[0].attempt + 1);
    assert!(apply(&mut f.j, &f.n, &f.ctx, a, attempt).is_err());
    let (a, attempt) = anchor(&next);
    apply(&mut f.j, &f.n, &f.ctx, a, attempt).unwrap();
    report(&mut f.j, &mut f.n, &f.ctx, a, attempt).unwrap();
}
#[test]
fn a_committed_effect_wins_cancellation_and_partial_reports_reconcile_after_parent_cancel() {
    let mut f = Fixture::new_block();
    let b = begin(&mut f);
    let (a, attempt) = anchor(&b);
    apply(&mut f.j, &f.n, &f.ctx, a, attempt).unwrap();
    let (root, b) = locate(&mut f.j, &f.n, &f.ctx, a, attempt).unwrap();
    let receipt = effect_receipt(&mut f.j, &f.ctx, &root, &b)
        .unwrap()
        .unwrap();
    assert!(
        report_hook(
            &mut f.j,
            &mut f.n,
            &f.ctx,
            &root,
            &b,
            Some(&receipt),
            |_| Err(Error::Invalid)
        )
        .is_err()
    );
    let next = super::super::super::start(
        &mut f.j,
        &mut f.n,
        &f.ctx,
        a,
        Action::Remove { store: d("D") },
    )
    .unwrap();
    crate::retirement_intent::cancel(
        &mut f.j,
        &key("controller"),
        &d("retire"),
        2,
        key("cancel-parent"),
    )
    .unwrap();
    f.reopen();
    let (a, attempt) = anchor(&b);
    cancel(&mut f.j, &mut f.n, &f.ctx, a, attempt).unwrap();
    for m in &b.members {
        assert!(
            inspect(&mut f.j, &f.n, &f.ctx, &m.object)
                .unwrap()
                .released
                .contains(&m.claim)
        );
    }
    assert_eq!(
        inspect(&mut f.j, &f.n, &f.ctx, a).unwrap().in_flight,
        Some((next.attempt, Action::Remove { store: d("D") }))
    );
}
#[test]
fn source_progress_or_stop_races_after_validation_cannot_expand_or_commit_the_block() {
    for stop in [false, true] {
        let mut f = Fixture::new_block();
        let b = begin(&mut f);
        let (a, attempt) = anchor(&b);
        let before = f.j.source(&key("main")).unwrap().unwrap();
        let (root, _) = locate(&mut f.j, &f.n, &f.ctx, a, attempt).unwrap();
        let mut nodes =
            MetadataStore::open(&f.temp.path().join("nodes"), &f.n.checkpoint()).unwrap();
        assert!(
            apply_hook(&mut f.j, &f.n, &f.ctx, a, attempt, |j| {
                if stop {
                    j.apply(
                        key("concurrent-stop"),
                        Command::Cancel {
                            source: group(&root)?,
                            owner: key("controller"),
                            expected_revision: 2,
                        },
                    )
                    .map_err(Error::Journal)?;
                } else {
                    finish(
                        j,
                        &mut nodes,
                        &f.ctx,
                        a,
                        attempt,
                        Action::Release {
                            claim: b.members[0].claim.clone(),
                        },
                        Outcome::NoEffect {
                            evidence: d("fenced-undispatched"),
                        },
                    )?;
                }
                Ok(())
            })
            .is_err()
        );
        assert_eq!(before, f.j.source(&key("main")).unwrap().unwrap());
    }
}

#[test]
fn admitted_group_may_reconcile_after_parent_cancellation_without_new_admission() {
    let mut f = Fixture::new_block();
    let b = begin(&mut f);
    let (a, attempt) = anchor(&b);
    crate::retirement_intent::cancel(&mut f.j, &key("controller"), &d("retire"), 2, key("cancel"))
        .unwrap();
    f.reopen();
    apply(&mut f.j, &f.n, &f.ctx, a, attempt).unwrap();
    report(&mut f.j, &mut f.n, &f.ctx, a, attempt).unwrap();
    for m in b.members {
        assert!(
            inspect(&mut f.j, &f.n, &f.ctx, &m.object)
                .unwrap()
                .released
                .contains(&m.claim)
        );
    }
}
#[test]
fn fifteen_members_use_existing_guard_bound_and_sixteen_refuse_before_mutation() {
    let mut f = Fixture::new_members(MAX_MEMBERS);
    let b = begin(&mut f);
    let (a, attempt) = anchor(&b);
    apply(&mut f.j, &f.n, &f.ctx, a, attempt).unwrap();
    report(&mut f.j, &mut f.n, &f.ctx, a, attempt).unwrap();
    assert_eq!(f.retained().len(), 2);
    let mut f = Fixture::new_members(MAX_MEMBERS + 1);
    let objects = members(&f);
    let before = f.j.checkpoint();
    assert!(matches!(
        plan(
            &mut f.j,
            &mut f.n,
            &f.ctx,
            &key("main"),
            2,
            &objects,
            &Enrollment {
                owner: key("source-owner"),
                subject: d("subject")
            }
        ),
        Err(Error::Budget)
    ));
    assert_eq!(before, f.j.checkpoint());
}
#[test]
fn incomplete_projection_or_missing_plan_never_becomes_retirement_permission() {
    let mut f = Fixture::new_block();
    let mut b = planned(&mut f);
    let incomplete = Tree::build(&BTreeMap::new(), &mut f.n, TreeLimits::default())
        .unwrap()
        .0;
    b.replacement.as_mut().unwrap().root = incomplete.root().clone();
    start(&mut f.j, &mut f.n, &f.ctx, b.clone()).unwrap();
    let (a, attempt) = anchor(&b);
    let before = f.j.source(&key("main")).unwrap().unwrap();
    assert_eq!(
        apply(&mut f.j, &f.n, &f.ctx, a, attempt),
        Err(Error::Integrity)
    );
    assert_eq!(before, f.j.source(&key("main")).unwrap().unwrap());
    let (root, _) = locate(&mut f.j, &f.n, &f.ctx, a, attempt).unwrap();
    struct Hidden<'a>(&'a MetadataStore, Object);
    impl NodeStore for Hidden<'_> {
        fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
            if *o == self.1 {
                Ok(None)
            } else {
                self.0.get(o)
            }
        }
        fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
            panic!("read only")
        }
    }
    assert!(apply(&mut f.j, &Hidden(&f.n, root), &f.ctx, a, attempt).is_err());
    assert_eq!(before, f.j.source(&key("main")).unwrap().unwrap());
}
#[derive(Serialize, Deserialize)]
struct Controls {
    journal: source_claims::Checkpoint,
    nodes: crate::metadata::Checkpoint,
}
#[test]
fn block_child() {
    let Some(root) = std::env::var_os("TIG_HISTORY_BLOCK_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let controls: Controls =
        serde_json::from_slice(&std::fs::read(root.join("block-controls.json")).unwrap()).unwrap();
    let mut j = SourceJournal::open(&root.join("journal"), &controls.journal).unwrap();
    let mut nodes = MetadataStore::open(&root.join("nodes"), &controls.nodes).unwrap();
    let ctx = crate::retirement_intent::observe(
        &mut j,
        &nodes,
        &Enrollment {
            owner: key("controller"),
            subject: d("subject"),
        },
        &d("retire"),
    )
    .unwrap();
    let phase = std::env::var("TIG_HISTORY_BLOCK_PHASE").unwrap();
    let objects = ctx
        .decision()
        .consent()
        .release_owned_claims
        .iter()
        .map(|r| r.object.clone())
        .collect();
    let b = plan(
        &mut j,
        &mut nodes,
        &ctx,
        &key("main"),
        2,
        &objects,
        &Enrollment {
            owner: key("source-owner"),
            subject: d("subject"),
        },
    )
    .unwrap();
    if phase == "prepared" {
        let root = put(
            &mut nodes,
            &encode(&BlockRecord {
                schema: BLOCK_SCHEMA.into(),
                block: b.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        prepare_group(&mut j, &mut nodes, &ctx, &root, &b).unwrap();
        std::process::exit(80);
    }
    start_hook(&mut j, &mut nodes, &ctx, b.clone(), |_, index| {
        if phase == "members" && index == 0 {
            std::process::exit(80);
        }
        Ok(())
    })
    .unwrap();
    if phase == "admitted" {
        std::process::exit(80);
    }
    let (a, attempt) = anchor(&b);
    apply(&mut j, &nodes, &ctx, a, attempt).unwrap();
    if phase == "reported-one" {
        let (root, b) = locate(&mut j, &nodes, &ctx, a, attempt).unwrap();
        let receipt = effect_receipt(&mut j, &ctx, &root, &b).unwrap().unwrap();
        report_hook(&mut j, &mut nodes, &ctx, &root, &b, Some(&receipt), |_| {
            std::process::exit(80)
        })
        .unwrap();
    }
    std::process::exit(80);
}
#[test]
fn actual_child_exits_restore_partial_admission_effect_and_partial_reporting_without_plan_roots() {
    for phase in [
        "prepared",
        "members",
        "admitted",
        "effected",
        "reported-one",
    ] {
        let mut f = Fixture::new_block();
        let anchor = members(&f).first().unwrap().clone();
        std::fs::write(
            f.temp.path().join("block-controls.json"),
            serde_json::to_vec(&Controls {
                journal: f.j.checkpoint(),
                nodes: f.n.checkpoint(),
            })
            .unwrap(),
        )
        .unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "retirement_intent::progress::history_work::block::tests::block_child",
                "--nocapture",
            ])
            .env("TIG_HISTORY_BLOCK_ROOT", f.temp.path())
            .env("TIG_HISTORY_BLOCK_PHASE", phase)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(80));
        f.reopen();
        if phase == "prepared" {
            let p = groups(&mut f.j, &key("controller"), 16).unwrap().remove(0);
            resume_group(&mut f.j, &mut f.n, &f.ctx, &p).unwrap();
        }
        let (attempt, b) = current(&mut f.j, &f.n, &f.ctx, &anchor).unwrap().unwrap();
        resume(&mut f.j, &mut f.n, &f.ctx, &anchor, attempt).unwrap();
        apply(&mut f.j, &f.n, &f.ctx, &anchor, attempt).unwrap();
        report(&mut f.j, &mut f.n, &f.ctx, &anchor, attempt).unwrap();
        for m in b.members {
            assert!(
                inspect(&mut f.j, &f.n, &f.ctx, &m.object)
                    .unwrap()
                    .released
                    .contains(&m.claim)
            );
        }
        assert_eq!(f.retained().len(), 2);
    }
}
