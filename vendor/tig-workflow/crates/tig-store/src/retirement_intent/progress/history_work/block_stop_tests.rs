use super::*;

#[test]
fn cancelling_partial_group_preserves_other_groups_committed_outcome() {
    for effected_before_stop in [false, true] {
        let mut f = Fixture::new_block();
        let all = planned(&mut f);
        let second = all.members[1].object.clone();
        let other = plan(
            &mut f.j,
            &mut f.n,
            &f.ctx,
            &key("main"),
            2,
            &[second.clone()].into(),
            &Enrollment {
                owner: key("source-owner"),
                subject: d("subject"),
            },
        )
        .unwrap();
        start(&mut f.j, &mut f.n, &f.ctx, other.clone()).unwrap();
        assert!(start(&mut f.j, &mut f.n, &f.ctx, all.clone()).is_err());
        let member = &other.members[0];
        if effected_before_stop {
            apply(&mut f.j, &f.n, &f.ctx, &member.object, member.attempt).unwrap();
        }
        let (a, attempt) = anchor(&all);
        cancel(&mut f.j, &mut f.n, &f.ctx, a, attempt).unwrap();
        if !effected_before_stop {
            apply(&mut f.j, &f.n, &f.ctx, &member.object, member.attempt).unwrap();
        }
        report(&mut f.j, &mut f.n, &f.ctx, &member.object, member.attempt).unwrap();
        assert!(
            inspect(&mut f.j, &f.n, &f.ctx, &second)
                .unwrap()
                .released
                .contains(&member.claim)
        );
        assert!(
            inspect(&mut f.j, &f.n, &f.ctx, a)
                .unwrap()
                .released
                .is_empty()
        );
    }
}

#[test]
fn exact_plan_stop_before_any_acknowledgement_fences_only_that_plan() {
    let mut f = Fixture::new_block();
    let b = planned(&mut f);
    let original = f.j.source(&key("main")).unwrap().unwrap();
    let mut foreign = b.clone();
    foreign.journal = d("foreign");
    let before = f.j.checkpoint();
    assert_eq!(
        stop_plan(&mut f.j, &f.ctx, &foreign).unwrap_err(),
        Error::Invalid
    );
    assert_eq!(f.j.checkpoint(), before);
    let stopped = stop_plan(&mut f.j, &f.ctx, &b).unwrap();
    assert_eq!(stopped.revision, 1);
    let state = f.j.source(&stopped.source).unwrap().unwrap();
    assert!(state.cancelled && state.current.is_none() && state.pending.is_none());
    let before = f.j.checkpoint();
    stop_plan(&mut f.j, &f.ctx, &b).unwrap();
    assert_eq!(before, f.j.checkpoint());
    assert!(start(&mut f.j, &mut f.n, &f.ctx, b.clone()).is_err());
    for m in &b.members {
        let progress = inspect(&mut f.j, &f.n, &f.ctx, &m.object).unwrap();
        assert!(progress.in_flight.is_none() && progress.released.is_empty());
    }
    assert_eq!(f.j.source(&key("main")).unwrap().unwrap(), original);
}

#[test]
fn saved_plan_stop_needs_no_metadata_and_does_not_assert_an_outcome() {
    let mut f = Fixture::new_block();
    let b = begin(&mut f);
    let minimum = f.n.checkpoint();
    let root = f.temp.path().to_path_buf();
    let mut j = f.j;
    let ctx = f.ctx;
    drop(f.n);
    std::fs::rename(root.join("nodes"), root.join("nodes-offline")).unwrap();
    let stopped = stop_plan(&mut j, &ctx, &b).unwrap();
    assert!(j.source(&stopped.source).unwrap().unwrap().cancelled);
    std::fs::rename(root.join("nodes-offline"), root.join("nodes")).unwrap();
    let mut nodes = MetadataStore::open(&root.join("nodes"), &minimum).unwrap();
    for m in &b.members {
        let progress = inspect(&mut j, &nodes, &ctx, &m.object).unwrap();
        assert!(progress.in_flight.is_some() && progress.released.is_empty());
    }
    cancel_plan(&mut j, &mut nodes, &ctx, &b).unwrap();
    for m in &b.members {
        assert!(
            inspect(&mut j, &nodes, &ctx, &m.object)
                .unwrap()
                .in_flight
                .is_none()
        );
    }
}

#[test]
fn five_actual_child_exit_phases_reconcile_after_parent_cancellation() {
    for phase in [
        "prepared",
        "members",
        "admitted",
        "effected",
        "reported-one",
    ] {
        let mut f = Fixture::new_block();
        let b = planned(&mut f);
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
        crate::retirement_intent::cancel(
            &mut f.j,
            &key("controller"),
            &d("retire"),
            2,
            key("stop-parent"),
        )
        .unwrap();
        let stopped = cancel_plan(&mut f.j, &mut f.n, &f.ctx, &b).unwrap();
        assert!(f.j.source(&stopped.source).unwrap().unwrap().cancelled);
        assert_eq!(stopped.checkpoint, f.j.checkpoint());
        let effected = matches!(phase, "effected" | "reported-one");
        for m in &b.members {
            let progress = inspect(&mut f.j, &f.n, &f.ctx, &m.object).unwrap();
            assert!(progress.in_flight.is_none(), "{phase}");
            assert_eq!(progress.released.contains(&m.claim), effected, "{phase}");
        }
        let retained = f.retained();
        for m in &b.members {
            assert_eq!(
                retained.values().any(|o| *o == m.object),
                !effected,
                "{phase}"
            );
        }
        let before = f.j.checkpoint();
        cancel_plan(&mut f.j, &mut f.n, &f.ctx, &b).unwrap();
        assert_eq!(before, f.j.checkpoint(), "{phase}");
        if !effected {
            let (a, attempt) = anchor(&b);
            assert!(
                apply(&mut f.j, &f.n, &f.ctx, a, attempt).is_err(),
                "{phase}"
            );
        }
    }
}
