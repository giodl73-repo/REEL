//! Real native registry/head/intent fixtures. No payload effect is claimed here.
use super::*;
use crate::retention::routine_sources::Validation;
use crate::retirement_intent::{
    self as intent,
    execution_profile::{Context, Role},
    policy::{self, AuthorityBindings, GrantRef, NativeAuthority, Proof},
};
use std::sync::Arc;
use tig_core::retirement::review::{
    self as review,
    decision::Target,
    eviction::{Desired, Selection as EvictionSelection},
    routine_policy::{Run, RunConfig},
};

fn review(catalogue: &str, value: usize) -> review::Review {
    let object = object(value);
    let selection = EvictionSelection::new(
        d(catalogue),
        d("policy"),
        [(p("old-cue"), object.clone())].into(),
        ["F".into()].into(),
        Desired {
            checkpoint: d("desired"),
            complete: true,
            placements: [Target {
                object: object.clone(),
                store: d("M"),
            }]
            .into(),
        },
    )
    .unwrap();
    review::Review::create_eviction(
        selection,
        d("subject"),
        [("F".into(), d("F"))].into(),
        review::Protection {
            checkpoint: d("protection"),
            complete: true,
            obligations: vec![],
        },
        review::Inventory {
            checkpoint: d("inventory"),
            complete: true,
            stores: [d("F"), d("M")].into(),
            copies: ["F", "M"]
                .map(|s| review::Copy {
                    object: object.clone(),
                    store: d(s),
                    state: review::CopyState::Verified { evidence: d(s) },
                })
                .into(),
        },
        Default::default(),
    )
    .unwrap()
}
pub(super) fn stage(
    c: &mut Case,
    r: &mut Registry,
) -> (intent::ObservedIntent, AuthorityBindings, Context) {
    stage_variant(c, r, "policy-work", "candidates", 99).unwrap()
}
pub(super) fn stage_variant(
    c: &mut Case,
    r: &mut Registry,
    tag: &str,
    catalogue: &str,
    value: usize,
) -> std::result::Result<(intent::ObservedIntent, AuthorityBindings, Context), intent::Error> {
    let held = r.hold(&key("main"), None, &c.f.nodes).unwrap();
    let head = coordinated::accepted_head::hold_current(&mut c.f.index, &mut c.source).unwrap();
    let grant = GrantRef {
        registry: r.identity().clone(),
        slot: key("main"),
        version: held.version().clone(),
    };
    let run = Run::new(RunConfig {
        namespace: c.source.identity().clone(),
        frame: head.frame().clone(),
        catalogue: d(catalogue),
        provider_bindings: held.policy().config().provider_bindings.clone(),
        admission: grant.fingerprint().unwrap(),
        native_sources: Default::default(),
    })
    .unwrap();
    let review = review(catalogue, value);
    let observed = Validation::new(held.policy(), &run, &review, key("owner"), 0)
        .unwrap()
        .finish()
        .unwrap();
    let refs: BTreeMap<_, _> = held
        .providers()
        .iter()
        .map(|(id, p)| (p.binding.place.clone(), id.clone()))
        .collect();
    let base = Context::new(
        d(tag),
        held.providers().clone(),
        [
            (Role::Removal, [(d("F"), refs[&d("F")].clone())].into()),
            (Role::Custody, [(d("M"), refs[&d("M")].clone())].into()),
        ]
        .into(),
        Default::default(),
    )
    .unwrap();
    let enrollment = intent::Enrollment {
        owner: key("owner"),
        subject: d("subject"),
    };
    let (candidate, context, decision) = policy::stage_registered(
        &mut c.source,
        &mut c.f.nodes,
        r,
        &enrollment,
        &held,
        &head,
        &observed,
        &run,
        &review,
        base,
    )?;
    let operation = decision.approval().operation.clone();
    drop(head);
    drop(held);
    let ready = intent::finish(&mut c.source, &mut c.f.nodes, candidate).unwrap();
    let prepared = intent::prepare(
        &mut c.source,
        &c.f.nodes,
        ready,
        key(&format!("prepare-{tag}")),
    )
    .unwrap();
    intent::adopt(
        &mut c.source,
        &c.f.nodes,
        &prepared,
        key(&format!("adopt-{tag}")),
    )
    .unwrap();
    let observed = intent::observe(&mut c.source, &c.f.nodes, &enrollment, &operation).unwrap();
    let root = c.f.temp.path();
    let bindings = AuthorityBindings {
        registry_root: root.join("accounted"),
        registry: r.identity().clone(),
        registry_minimum: r.checkpoint(),
        slot: key("main"),
        index_root: root.join("index"),
        index: c.f.index.identity().clone(),
        index_minimum: c.f.index.checkpoint(),
        source_root: root.join("source"),
        source: c.source.identity().clone(),
        source_minimum: c.source.checkpoint(),
    };
    Ok((observed, bindings, context))
}
pub(super) fn enabled() -> (Case, Registry) {
    let mut c = Case::new();
    let head = coordinated::accepted_head::hold_current(&mut c.f.index, &mut c.source).unwrap();
    let binding = registry::Binding::from_head(&c.f.index, &head, vec![key("main")]).unwrap();
    drop(head);
    let mut r = Registry::create_accounted(
        &c.f.temp.path().join("accounted"),
        key("policy-epoch"),
        binding,
    )
    .unwrap();
    let req = request(&mut c, &r, "main", 1, 0, "enable");
    r.update(&req, &mut c.f.nodes).unwrap();
    (c, r)
}
#[test]
fn saved_provenance_recovers_without_renewing_execution_permission() {
    let (mut c, mut r) = enabled();
    let (observed, bindings, context) = stage(&mut c, &mut r);
    let restored = Context::from_json(&context.to_json().unwrap()).unwrap();
    let proof = Proof::load(
        &c.f.nodes,
        &restored,
        observed.review(),
        observed.decision(),
    )
    .unwrap();
    assert_eq!(proof.grant().registry, *r.identity());
    assert_eq!(proof.owner(), &key("owner"));
    let bytes = observed.decision().to_json().unwrap();
    for mode in 0..3 {
        let mut record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        match mode {
            0 => record["schema"] = serde_json::json!("tig.retirement-decision.design-1"),
            1 => {
                record.as_object_mut().unwrap().remove("execution_context");
            }
            _ => record["execution_context"] = serde_json::Value::Null,
        }
        assert!(
            tig_core::retirement::review::decision::Decision::from_json(
                &identity::canonical_json(&record).unwrap(),
                observed.review()
            )
            .is_err()
        );
    }
    assert!(policy::require_effect_authority(&observed, &c.f.nodes).is_err());
    assert!(
        intent::stage(
            &mut c.source,
            &mut c.f.nodes,
            &intent::Enrollment {
                owner: key("owner"),
                subject: d("subject")
            },
            observed.review(),
            observed.decision()
        )
        .is_err()
    );
    assert!(
        restored
            .authorize(
                observed.review(),
                observed.decision().approval().clone(),
                observed.decision().consent().clone()
            )
            .is_err()
    );
    assert!(
        intent::stage_profiled(
            &mut c.source,
            &mut c.f.nodes,
            &intent::Enrollment {
                owner: key("owner"),
                subject: d("subject")
            },
            observed.review(),
            observed.decision(),
            &restored
        )
        .is_err()
    );
    let observed = observed
        .attach_policy_authority(
            &c.f.nodes,
            Arc::new(NativeAuthority::new(bindings).unwrap()),
        )
        .unwrap();
    assert!(
        policy::require_effect_authority(&observed, &c.f.nodes)
            .unwrap()
            .is_some()
    );
}
#[test]
fn effective_permission_fences_stop_then_revocation_blocks_new_permission() {
    let (mut c, mut r) = enabled();
    let (observed, bindings, _) = stage(&mut c, &mut r);
    let observed = observed
        .attach_policy_authority(
            &c.f.nodes,
            Arc::new(NativeAuthority::new(bindings).unwrap()),
        )
        .unwrap();
    let hold = policy::require_effect_authority(&observed, &c.f.nodes).unwrap();
    assert!(matches!(
        r.revoke(&key("main"), &key("owner"), 2, key("stop")),
        Err(RegistryError::Journal(claims::Error::Busy))
    ));
    drop(hold);
    r.revoke(&key("main"), &key("owner"), 2, key("stop"))
        .unwrap();
    assert!(policy::require_effect_authority(&observed, &c.f.nodes).is_err());
    assert!(
        Proof::load(
            &c.f.nodes,
            observed.execution_profile().unwrap(),
            observed.review(),
            observed.decision()
        )
        .is_ok()
    );
}
#[test]
fn new_accepted_source_generation_refuses_saved_execution() {
    let (mut c, mut r) = enabled();
    let (observed, bindings, _) = stage(&mut c, &mut r);
    let observed = observed
        .attach_policy_authority(
            &c.f.nodes,
            Arc::new(NativeAuthority::new(bindings).unwrap()),
        )
        .unwrap();
    let hold = policy::require_effect_authority(&observed, &c.f.nodes).unwrap();
    drop(hold);
    let batch = c.batch("new-merge", 10);
    let work = c.stage(&batch);
    c.run(&work, |_| Ok(())).unwrap();
    assert!(policy::require_effect_authority(&observed, &c.f.nodes).is_err());
}

#[test]
fn catalogue_and_nonce_changes_share_one_cumulative_action_budget() {
    let (mut c, mut r) = enabled();
    let (observed, _, context) = stage(&mut c, &mut r);
    let before = r.checkpoint();
    let proof = Proof::load(&c.f.nodes, &context, observed.review(), observed.decision()).unwrap();
    let held = r.hold(&key("main"), None, &c.f.nodes).unwrap();
    let head = coordinated::accepted_head::hold_current(&mut c.f.index, &mut c.source).unwrap();
    r.reserve_run(
        &held,
        &head,
        &proof,
        &context,
        observed.review(),
        observed.decision(),
        &mut c.f.nodes,
    )
    .unwrap();
    drop(head);
    drop(held);
    assert_eq!(r.checkpoint(), before, "exact retry must not reserve again");
    stage_variant(&mut c, &mut r, "second", "different-catalogue", 99).unwrap();
    let before = r.checkpoint();
    assert_eq!(
        stage_variant(&mut c, &mut r, "third", "yet-another-catalogue", 99).unwrap_err(),
        intent::Error::Budget
    );
    assert_eq!(
        r.checkpoint(),
        before,
        "over-budget proposal must not update registry"
    );
}
#[test]
fn disjoint_small_proposals_cannot_reset_object_budget_and_new_head_can_begin_a_run() {
    let (mut c, mut r) = enabled();
    let (old, bindings, _) = stage(&mut c, &mut r);
    assert_eq!(
        stage_variant(&mut c, &mut r, "different-object", "split-catalogue", 100).unwrap_err(),
        intent::Error::Budget
    );
    let batch = c.batch("new-merge", 10);
    let work = c.stage(&batch);
    c.run(&work, |_| Ok(())).unwrap();
    stage_variant(&mut c, &mut r, "next-head", "next-catalogue", 100).unwrap();
    let old = old
        .attach_policy_authority(
            &c.f.nodes,
            Arc::new(NativeAuthority::new(bindings).unwrap()),
        )
        .unwrap();
    assert!(policy::require_effect_authority(&old, &c.f.nodes).is_err());
}

#[test]
fn unaccounted_registry_cannot_admit_automatic_policy_work() {
    let mut c = Case::new();
    let mut r = create(&mut c);
    let req = request(&mut c, &r, "main", 1, 0, "enable");
    r.update(&req, &mut c.f.nodes).unwrap();
    let before = r.checkpoint();
    let source = c.source.checkpoint();
    assert_eq!(
        stage_variant(&mut c, &mut r, "work", "candidates", 99).unwrap_err(),
        intent::Error::Budget
    );
    assert_eq!(r.checkpoint(), before);
    assert_eq!(c.source.checkpoint(), source);
}
