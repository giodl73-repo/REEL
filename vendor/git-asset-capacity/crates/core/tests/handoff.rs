use git_asset_core::{Claim, Digest, Error, HoldReason, Object, Placement, View};
use std::collections::BTreeSet;

fn p(id: u8, place: &str) -> Placement {
    Placement {
        object: Object {
            sha256: Digest::blob(&[id; 100]),
            bytes: 100,
        },
        place: place.into(),
    }
}

fn claim(id: &str, placements: Vec<Placement>) -> Claim {
    Claim {
        id: id.into(),
        placements: placements.into_iter().collect(),
    }
}

fn draft() -> View {
    View::new(
        7,
        true,
        vec![
            claim("main", vec![p(1, "working"), p(2, "working")]),
            claim("branch-fix", vec![p(1, "working"), p(3, "working")]),
        ],
    )
    .unwrap()
}

fn merged(view: &View) -> View {
    view.transition(
        view.generation,
        vec![
            claim("main", vec![p(1, "working"), p(3, "working")]),
            claim("previous", vec![p(2, "history")]),
        ],
        vec!["branch-fix".into()],
    )
    .unwrap()
}

#[test]
fn branch_scratch_is_a_claim_not_a_duplicate_payload_copy() {
    let plan = draft()
        .plan(&BTreeSet::from([p(1, "working"), p(2, "working")]))
        .unwrap();
    assert_eq!(plan.ensure, BTreeSet::from([p(3, "working")]));
    assert_eq!(plan.required_bytes_by_place["working"], 300);

    // A larger scene/episode-like object set changes only five assigned objects.
    // This checks the set delta, not index throughput or physical IO costs.
    let item = |id: u32, place: &str| {
        let mut bytes = [0; 100];
        bytes[..4].copy_from_slice(&id.to_be_bytes());
        Placement {
            object: Object {
                sha256: Digest::blob(&bytes),
                bytes: 100,
            },
            place: place.into(),
        }
    };
    let original: Vec<_> = (0..1000).map(|i| item(i, "working")).collect();
    let replacement: Vec<_> = (0..995)
        .chain(1000..1005)
        .map(|i| item(i, "working"))
        .collect();
    let large = View::new(
        1,
        true,
        vec![
            claim("main", original.clone()),
            claim("draft", replacement.clone()),
        ],
    )
    .unwrap();
    let before: BTreeSet<_> = original.into_iter().collect();
    assert_eq!(
        large.plan(&before).unwrap().ensure,
        (1000..1005).map(|i| item(i, "working")).collect()
    );
    let after_merge = large
        .transition(
            1,
            vec![
                claim("main", replacement),
                claim("history", (995..1000).map(|i| item(i, "history")).collect()),
            ],
            vec!["draft".into()],
        )
        .unwrap();
    let mut present: BTreeSet<_> = (0..1005).map(|i| item(i, "working")).collect();
    assert_eq!(after_merge.plan(&present).unwrap().ensure.len(), 5);
    assert!(
        after_merge
            .plan(&present)
            .unwrap()
            .release_candidates
            .is_empty()
    );
    present.extend((995..1000).map(|i| item(i, "history")));
    let satisfied = after_merge.plan(&present).unwrap();
    assert!(satisfied.ensure.is_empty());
    assert_eq!(
        satisfied.release_candidates,
        (995..1000).map(|i| item(i, "working")).collect()
    );
    assert_eq!(satisfied.required_bytes_by_place["working"], 100_000);
}

#[test]
fn merge_reuses_new_objects_and_holds_old_until_history_copy_arrives() {
    let view = merged(&draft());
    let plan = view
        .plan(&BTreeSet::from([
            p(1, "working"),
            p(2, "working"),
            p(3, "working"),
        ]))
        .unwrap();
    assert_eq!(plan.ensure, BTreeSet::from([p(2, "history")]));
    assert!(plan.release_candidates.is_empty());
    assert_eq!(plan.held[&p(2, "working")], HoldReason::RequiredCopyMissing);
}

#[test]
fn retained_history_enables_only_the_replaced_working_copy_candidate() {
    let plan = merged(&draft())
        .plan(&BTreeSet::from([
            p(1, "working"),
            p(2, "working"),
            p(3, "working"),
            p(2, "history"),
        ]))
        .unwrap();
    assert!(plan.ensure.is_empty());
    assert_eq!(plan.release_candidates, BTreeSet::from([p(2, "working")]));
}

#[test]
fn another_branch_keeps_old_working_copy_protected() {
    let base = draft()
        .transition(
            7,
            vec![claim("other-branch", vec![p(2, "working")])],
            vec![],
        )
        .unwrap();
    let plan = merged(&base)
        .plan(&BTreeSet::from([
            p(1, "working"),
            p(2, "working"),
            p(3, "working"),
            p(2, "history"),
        ]))
        .unwrap();
    assert!(plan.release_candidates.is_empty());
    assert!(plan.retained.contains(&p(2, "working")));
}

#[test]
fn active_job_survives_branch_retirement() {
    let base = draft()
        .transition(7, vec![claim("job", vec![p(2, "working")])], vec![])
        .unwrap();
    let next = merged(&base);
    let present = BTreeSet::from([
        p(1, "working"),
        p(2, "working"),
        p(3, "working"),
        p(2, "history"),
    ]);
    assert!(next.plan(&present).unwrap().release_candidates.is_empty());
    let finished = next.transition(9, vec![], vec!["job".into()]).unwrap();
    assert_eq!(
        finished.plan(&present).unwrap().release_candidates,
        BTreeSet::from([p(2, "working")])
    );
}

#[test]
fn a_separate_scratch_place_is_explicit_and_can_remain_until_promotion() {
    let draft = View::new(1, true, vec![claim("draft", vec![p(3, "scratch")])]).unwrap();
    let adopted = draft
        .transition(
            1,
            vec![claim("main", vec![p(3, "working")])],
            vec!["draft".into()],
        )
        .unwrap();
    let pending = adopted.plan(&BTreeSet::from([p(3, "scratch")])).unwrap();
    assert_eq!(pending.ensure, BTreeSet::from([p(3, "working")]));
    assert!(pending.release_candidates.is_empty());
    let completed = adopted
        .plan(&BTreeSet::from([p(3, "scratch"), p(3, "working")]))
        .unwrap();
    assert_eq!(
        completed.release_candidates,
        BTreeSet::from([p(3, "scratch")])
    );
}

#[test]
fn incomplete_coverage_allows_positive_acquisition_but_holds_surplus() {
    let view = View::new(1, false, vec![claim("archive", vec![p(2, "history")])]).unwrap();
    let plan = view
        .plan(&BTreeSet::from([p(2, "working"), p(2, "history")]))
        .unwrap();
    assert_eq!(
        plan.held[&p(2, "working")],
        HoldReason::IncompleteClaimCoverage
    );
    let missing = view.plan(&BTreeSet::new()).unwrap();
    assert_eq!(missing.ensure, BTreeSet::from([p(2, "history")]));
}

#[test]
fn unregistered_objects_are_held_until_a_retention_policy_is_admitted() {
    let empty = View::new(0, true, vec![]).unwrap();
    let present = BTreeSet::from([p(9, "working")]);
    let plan = empty.plan(&present).unwrap();
    assert_eq!(
        plan.held[&p(9, "working")],
        HoldReason::NoRetainedObligation
    );
}

#[test]
fn stale_or_ambiguous_handoffs_do_not_mutate_the_old_view() {
    let view = draft();
    let before = view.clone();
    assert_eq!(
        view.transition(6, vec![], vec![]),
        Err(Error::StaleGeneration)
    );
    assert_eq!(
        view.transition(7, vec![claim("main", vec![])], vec!["main".into()]),
        Err(Error::AmbiguousTransition)
    );
    assert_eq!(
        view.transition(7, vec![], vec!["absent".into()]),
        Err(Error::UnknownRetirement)
    );
    assert_eq!(view, before);
}

#[test]
fn multiple_required_retained_places_must_all_be_present() {
    let view = View::new(
        1,
        true,
        vec![claim("retained", vec![p(2, "history"), p(2, "archive")])],
    )
    .unwrap();
    let plan = view
        .plan(&BTreeSet::from([p(2, "working"), p(2, "history")]))
        .unwrap();
    assert_eq!(plan.ensure, BTreeSet::from([p(2, "archive")]));
    assert!(plan.release_candidates.is_empty());
}

#[test]
fn identity_size_conflicts_and_logical_size_overflow_are_rejected() {
    let mut bad = p(1, "archive");
    bad.object.bytes = 200;
    assert_eq!(
        draft().plan(&BTreeSet::from([bad])),
        Err(Error::ConflictingSize)
    );
    let mut a = p(1, "working");
    a.object.bytes = u64::MAX;
    let view = View::new(0, true, vec![claim("huge", vec![a, p(2, "working")])]).unwrap();
    assert_eq!(view.plan(&BTreeSet::new()), Err(Error::Overflow));
}

#[test]
fn duplicate_claims_and_oversized_views_are_rejected() {
    assert_eq!(
        View::new(0, true, vec![claim("x", vec![]), claim("x", vec![])]),
        Err(Error::DuplicateClaim)
    );
    assert_eq!(
        View::new(0, true, vec![claim("x", vec![]); 1025]),
        Err(Error::Budget)
    );
}

#[test]
fn plans_are_repeatable_and_revert_can_request_old_bytes_without_losing_new() {
    let view = merged(&draft());
    let present = BTreeSet::from([p(1, "working"), p(3, "working"), p(2, "history")]);
    assert_eq!(view.plan(&present), view.plan(&present));
    let reverted = view
        .transition(
            8,
            vec![
                claim("main", vec![p(1, "working"), p(2, "working")]),
                claim("new-retained", vec![p(3, "history")]),
            ],
            vec![],
        )
        .unwrap();
    let plan = reverted.plan(&present).unwrap();
    assert_eq!(
        plan.ensure,
        BTreeSet::from([p(2, "working"), p(3, "history")])
    );
    assert!(plan.release_candidates.is_empty());
}
