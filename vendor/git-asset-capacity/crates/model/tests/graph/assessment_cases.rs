use super::*;
use git_asset_model::graph::recipe::assessment::{self, Key, Observation, Request, State};

fn key(id: &str) -> Key {
    Key {
        asset_id: id.into(),
        selector: Selector::Selected {},
    }
}
fn request<'a>(
    id: &str,
    desired: Option<&'a Plan>,
    current: Option<&'a BoundSubject>,
    claim: Option<&'a produced::PlanCheckedClaim<'a>>,
) -> Request<'a> {
    Request {
        key: key(id),
        desired,
        current: current.map_or(Observation::Absent, Observation::Present),
        claim: claim.map_or(Observation::Absent, Observation::Present),
    }
}
fn state(r: Request<'_>, checkpoint: &Digest) -> State {
    assessment::assess(checkpoint, &[r], 1).unwrap().entries()[0].state
}

#[test]
fn assessment_preserves_unbuilt_unproven_matching_and_stale_states() {
    let (mut f, input, old, current) = setup();
    let claim = claim(&old, &current);
    let checked = claim.check_plan(&old).unwrap();
    let id = current.subject().asset_id();
    assert_eq!(
        state(request(id, Some(&old), None, None), old.checkpoint()),
        State::Unbuilt
    );
    assert_eq!(
        state(
            request(id, Some(&old), Some(&current), None),
            old.checkpoint()
        ),
        State::Unproven
    );
    assert_eq!(
        state(
            request(id, Some(&old), Some(&current), Some(&checked)),
            old.checkpoint()
        ),
        State::ClaimedRecipeMatch
    );
    let changed = f.rewrite(&input, |v| {
        v["selected"] = json!(Object::from_bytes(b"fixed input"))
    });
    let desired = plan(&mut f, &changed, operation());
    assert_eq!(
        state(
            request(id, Some(&desired), Some(&current), Some(&checked)),
            desired.checkpoint()
        ),
        State::StaleRecipe
    );
    assert!(!State::Unproven.catch_up_candidate());
    assert!(!State::Blocked.catch_up_candidate());
    assert!(!State::ClaimedRecipeMatch.catch_up_candidate());
    assert!(State::Unbuilt.catch_up_candidate() && State::StaleRecipe.catch_up_candidate());
}

#[test]
fn assessment_blocks_missing_plans_checkpoint_drift_and_wrong_subject_keys() {
    let (_, _, plan, current) = setup();
    let claim = claim(&plan, &current);
    let checked = claim.check_plan(&plan).unwrap();
    let id = current.subject().asset_id();
    assert_eq!(
        state(request(id, None, None, None), plan.checkpoint()),
        State::Blocked
    );
    assert_eq!(
        state(
            request(id, Some(&plan), None, None),
            &Digest::blob(b"new commit")
        ),
        State::Blocked
    );
    assert_eq!(
        state(
            request("foreign", Some(&plan), Some(&current), Some(&checked)),
            plan.checkpoint()
        ),
        State::Blocked
    );
    let mut named = request(id, Some(&plan), Some(&current), Some(&checked));
    named.key.selector = Selector::Stream {
        slot: "version-1".into(),
    };
    assert_eq!(state(named, plan.checkpoint()), State::Blocked);
}

#[test]
fn assessment_foreign_claim_and_claim_without_current_are_blocked_not_unbuilt() {
    let (mut f, _, plan, current) = setup();
    let claim = claim(&plan, &current);
    let checked = claim.check_plan(&plan).unwrap();
    assert_eq!(
        state(
            request(
                current.subject().asset_id(),
                Some(&plan),
                None,
                Some(&checked)
            ),
            plan.checkpoint()
        ),
        State::Blocked
    );
    let foreign = f.node(
        "foreign/output",
        NodeKind::Output,
        &[],
        Some(b"another output"),
    );
    let foreign = subject(&f, &foreign, Selector::Selected {}).unwrap();
    assert_eq!(
        state(
            request(
                foreign.subject().asset_id(),
                Some(&plan),
                Some(&foreign),
                Some(&checked)
            ),
            plan.checkpoint()
        ),
        State::Blocked
    );
}

#[test]
fn assessment_unavailable_observations_never_turn_into_absence_or_rebuild_advice() {
    let (_, _, plan, current) = setup();
    let id = current.subject().asset_id();
    let mut missing = request(id, Some(&plan), None, None);
    missing.current = Observation::Unavailable;
    assert_eq!(state(missing, plan.checkpoint()), State::Blocked);
    let mut unreadable = request(id, Some(&plan), Some(&current), None);
    unreadable.claim = Observation::Unavailable;
    assert_eq!(state(unreadable, plan.checkpoint()), State::Blocked);
    let mut both_missing = request(id, Some(&plan), None, None);
    both_missing.claim = Observation::Unavailable;
    assert_eq!(state(both_missing, plan.checkpoint()), State::Blocked);
}

#[test]
fn assessment_duplicate_invalid_keys_and_resource_limits_refuse_atomically() {
    let (_, _, plan, _) = setup();
    let requests = [
        request("output", Some(&plan), None, None),
        request("output", None, None, None),
    ];
    assert!(assessment::assess(plan.checkpoint(), &requests, 2).is_err());
    for id in ["", "secret\ntext"] {
        assert!(
            assessment::assess(plan.checkpoint(), &[request(id, None, None, None)], 1).is_err()
        );
    }
    for limit in [0, 4097] {
        assert!(assessment::assess(plan.checkpoint(), &requests[..1], limit).is_err());
    }
    assert!(assessment::assess(plan.checkpoint(), &requests, 1).is_err());
    assert!(assessment::assess(plan.checkpoint(), &[], 1).is_err());
    let mut bad = request("output", None, None, None);
    bad.key.selector = Selector::Stream { slot: "".into() };
    assert!(assessment::assess(plan.checkpoint(), &[bad], 1).is_err());
}

#[test]
fn assessment_orders_independent_variants_deterministically_and_redacts_debug() {
    let (_, _, plan, _) = setup();
    let a = request("private-output", Some(&plan), None, None);
    let mut b = request("private-output", None, None, None);
    b.key.selector = Selector::Stream {
        slot: "private-language".into(),
    };
    let before = assessment::assess(plan.checkpoint(), &[a, b], 2).unwrap();
    let mut a = request("private-output", None, None, None);
    a.key.selector = Selector::Stream {
        slot: "private-language".into(),
    };
    let b = request("private-output", Some(&plan), None, None);
    let after = assessment::assess(plan.checkpoint(), &[a, b], 2).unwrap();
    assert!(before.entries() == after.entries());
    assert_eq!(before.checkpoint(), plan.checkpoint());
    assert!(!before.complete_semantic_coverage());
    let debug = format!("{before:?}");
    assert!(!debug.contains("private-output") && !debug.contains("private-language"));
    assert!(debug.contains("catch_up_candidates: 1"));
}

#[test]
fn assessment_thousand_outputs_stay_bounded_without_implicit_expansion() {
    let (_, _, plan, _) = setup();
    let requests: Vec<_> = (0..1000)
        .map(|i| request(&format!("output/{i:04}"), Some(&plan), None, None))
        .collect();
    let report = assessment::assess(plan.checkpoint(), &requests, 1000).unwrap();
    assert_eq!(report.entries().len(), 1000);
    assert!(report.entries().iter().all(|e| e.state == State::Unbuilt));
    assert!(assessment::assess(plan.checkpoint(), &requests, 999).is_err());
}

#[test]
fn assessment_coalesces_independent_cel_and_cue_fixes_without_rendering_intermediate_versions() {
    let mut f = Fixture::new();
    let cel = f.node("cel/b", NodeKind::Primary, &[], Some(b"old cel"));
    let cue = f.node("cue/1", NodeKind::Primary, &[], Some(b"old cue"));
    let scene = f.node(
        "scene/1",
        NodeKind::Container,
        &[cel.clone(), cue.clone()],
        None,
    );
    let scene = f.rewrite(&scene, |v| {
        v["authored"]["deps"] = json!([{"id":"cel/b"},{"id":"cue/1"}]);
    });
    let old = plan(&mut f, &scene, operation());
    let output = f.node("episode/master", NodeKind::Output, &[], Some(b"old master"));
    let current = subject(&f, &output, Selector::Selected {}).unwrap();
    let claim = claim(&old, &current);
    let checked = claim.check_plan(&old).unwrap();
    f.rewrite(&cel, |v| {
        v["selected"] = json!(Object::from_bytes(b"fixed cel"))
    });
    f.rewrite(&cue, |v| {
        v["selected"] = json!(Object::from_bytes(b"fixed cue"))
    });
    f.checkpoint = Digest::blob(b"merged cel and cue fixes");
    let desired = plan(&mut f, &scene, operation());
    let report = assessment::assess(
        desired.checkpoint(),
        &[
            request(
                "episode/master",
                Some(&desired),
                Some(&current),
                Some(&checked),
            ),
            request("history/unproven", Some(&desired), None, None),
            request("missing/declaration", None, None, None),
        ],
        3,
    )
    .unwrap();
    assert_eq!(report.entries()[0].state, State::StaleRecipe);
    assert_eq!(report.entries()[1].state, State::Unbuilt);
    assert_eq!(report.entries()[2].state, State::Blocked);
    assert_ne!(desired.fingerprint(), old.fingerprint());
    assert_eq!(checked.claim().recipe(), old.fingerprint());
}
