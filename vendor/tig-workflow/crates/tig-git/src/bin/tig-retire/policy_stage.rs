//! Native standing-policy admission. The input contains no Approval or Consent.
use super::{
    admission::{self, Controls},
    bookkeeping, codec, input_identity,
    model::{self, Artifact, Authority, Handle, Recipe, StageInput},
    session::Session,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tig_core::{
    Digest,
    retirement::review::{Review, decision::Decision, routine_policy::Run},
};
use tig_store::{
    Key,
    ledger::CapacityLedger,
    local::LocalStore,
    retention::{RetentionGrant, routine_sources::Validation},
    retirement_intent::{
        self as intent,
        execution_profile::Context,
        policy::{self, AuthorityBindings, Proof},
    },
    source_claims::{Version, coordinated::accepted_head::hold_current, policy_registry::Registry},
};
const SCHEMA: &str = "tig.native-policy-retirement-stage.experimental-1";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema: String,
    operation: Digest,
    registry: Authority,
    slot: Key,
    version: Version,
    run: Artifact,
    request: Value,
}
fn key(operation: &Digest, phase: &str) -> Result<Key, String> {
    Key::parse(&format!("retire-policy-{phase}-{}", operation.as_str()))
        .map_err(|_| "invalid policy command".into())
}

pub fn stage(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() > model::MAX_INPUT {
        return Err("policy stage byte budget".into());
    }
    let mut p: Input = codec::decode(bytes)?;
    if p.schema != SCHEMA
        || !p.registry.root.is_absolute()
        || !p.run.file.is_absolute()
        || p.run.object.bytes == 0
        || p.run.object.bytes > model::MAX_ARTIFACT
    {
        return Err("invalid policy stage schema/bindings".into());
    }
    let request = p.request.as_object_mut().ok_or("invalid policy request")?;
    for field in ["schema", "mode", "approval", "consent"] {
        if request.contains_key(field) {
            return Err("policy request cannot supply mode, approval or consent".into());
        }
    }
    let namespace = request
        .get("authorities")
        .and_then(|v| v.get("journal"))
        .and_then(|v| v.get("identity"))
        .ok_or("missing policy journal")?
        .clone();
    let subject = request
        .get("enrollment")
        .and_then(|v| v.get("subject"))
        .ok_or("missing policy enrollment")?
        .clone();
    request.insert("schema".into(), json!(model::STAGE_INPUT));
    request.insert("mode".into(), json!("retained-eviction"));
    request.insert("approval".into(),json!({"namespace":namespace,"owner":subject,"operation":p.operation,"evidence":Digest::blob(b"policy-input-not-approval")}));
    request.insert(
        "consent".into(),
        json!({"release_owned_claims":[],"remove_copies":[],"accept_intentional_loss":[]}),
    );
    let mut input: StageInput = codec::decode(&codec::encode(&p.request)?)?;
    input.validate()?;
    if !input.history.is_empty() {
        return Err("standing routine policy cannot release source history".into());
    }
    let fingerprint = Digest::blob(&codec::encode(&(
        SCHEMA,
        input_identity::fingerprint(&input)?,
        &p.operation,
        &p.registry,
        &p.slot,
        &p.version,
        &p.run.object,
    ))?);
    let identity = Digest::blob(&codec::encode(&(
        SCHEMA,
        &input.authorities.journal.identity,
        &input.enrollment.owner,
        &p.operation,
    ))?);
    let mut session = Session::open(&input.authorities)?;
    if let Some((_, prior)) = bookkeeping::existing(
        &mut session.journal,
        &session.nodes,
        &identity,
        &input.enrollment.owner,
    )? {
        if prior.input != fingerprint {
            return Err("changed policy input under saved controller operation".into());
        }
        session
            .nodes
            .finish()
            .map_err(|_| "policy metadata finish uncertain")?;
        session = Session::reconcile(&session.witnesses(&input.authorities), &prior.authorities)?;
        finish_intent(&mut session, &prior, None)?;
        return result(&mut session, &input, &prior);
    }
    let mut budget = codec::ArtifactBudget::new();
    let review = Review::from_json(&budget.load(&mut session.nodes, &input.review)?)
        .map_err(|_| "invalid policy review")?;
    let base = Context::from_json(&budget.load(&mut session.nodes, &input.context)?)
        .map_err(|_| "invalid policy base context")?;
    if base.logical_operation() != &p.operation
        || base.policy_proof().is_some()
        || !review.is_routine_eviction()
    {
        return Err("policy base/review scope mismatch".into());
    }
    let run = Run::from_json(&budget.load(&mut session.nodes, &p.run)?)
        .map_err(|_| "invalid exact policy run")?;
    let mut grants = BTreeMap::new();
    for artifact in &input.grants {
        let grant = RetentionGrant::from_json(&budget.load(&mut session.nodes, artifact)?)
            .map_err(|_| "invalid policy grant")?;
        if grants.insert(grant.claim_id().clone(), grant).is_some() {
            return Err("duplicate policy grant".into());
        }
    }
    let mut registry = Registry::open(&p.registry.root, &p.registry.minimum_checkpoint)
        .map_err(|_| "policy registry unavailable/rolled back")?;
    if registry.identity() != &p.registry.identity {
        return Err("foreign policy registry".into());
    }
    let held = registry
        .hold(&p.slot, Some(&p.version), &session.nodes)
        .map_err(|_| "policy grant not current")?;
    let head = hold_current(&mut session.index, &mut session.journal)
        .map_err(|_| "accepted source unavailable/pending")?;
    let mut selected = Validation::new(
        held.policy(),
        &run,
        &review,
        input.enrollment.owner.clone(),
        256,
    )
    .map_err(|_| "policy proposal refused")?;
    let declarations = selected.pending().clone();
    for (claim, declared) in declarations {
        let binding = input
            .places
            .get(&declared.source)
            .ok_or("missing native policy source")?;
        let store =
            LocalStore::open(&binding.root).map_err(|_| "native policy source unavailable")?;
        let mut ledger =
            CapacityLedger::open(&binding.capacity.root, &binding.capacity.minimum_checkpoint)
                .map_err(|_| "native policy capacity unavailable")?;
        if ledger.identity() != &binding.capacity.ledger
            || held
                .catalogue()
                .places()
                .get(&declared.source)
                .is_none_or(|p| &p.capacity_domain != ledger.identity())
        {
            return Err("policy capacity binding changed".into());
        }
        selected
            .observe(
                &claim,
                grants.get(&claim).ok_or("missing selected native grant")?,
                &store,
                &mut ledger,
            )
            .map_err(|_| "selected native grant no longer active")?;
    }
    let observed = selected
        .finish()
        .map_err(|_| "incomplete policy native observations")?;
    let (context, decision) = policy::stage_proof(
        &mut session.nodes,
        &held,
        &head,
        &observed,
        &run,
        &review,
        base.clone(),
    )
    .map_err(|_| "registered policy proposal refused")?;
    input.approval = decision.approval().clone();
    input.consent = decision.consent().clone();
    let controls = Controls {
        review,
        context,
        decision,
        grants,
    };
    admission::validate_scope(&input, &controls)?;
    let projections = admission::projections(&input, &controls, &mut session)?;
    let grants = admission::grant_index(&controls, &mut session)?;
    let (candidate, admitted_context, admitted_decision) = policy::stage_registered(
        &mut session.journal,
        &mut session.nodes,
        &mut registry,
        &Session::enrollment(&input.enrollment),
        &held,
        &head,
        &observed,
        &run,
        &controls.review,
        base,
    )
    .map_err(|_| "registered policy admission/budget refused")?;
    if admitted_context != controls.context || admitted_decision != controls.decision {
        return Err("policy controls changed during admission".into());
    }
    let authority = AuthorityBindings {
        registry_root: p.registry.root,
        registry: registry.identity().clone(),
        registry_minimum: registry.checkpoint(),
        slot: p.slot,
        index_root: input.authorities.index.root.clone(),
        index: session.index.identity().clone(),
        index_minimum: session.index.checkpoint(),
        source_root: input.authorities.journal.root.clone(),
        source: session.journal.identity().clone(),
        source_minimum: session.journal.checkpoint(),
    };
    drop(head);
    drop(held);
    let decision = codec::put(
        &mut session.nodes,
        &controls
            .decision
            .to_json()
            .map_err(|_| "invalid policy decision")?,
    )?;
    let context = codec::put(
        &mut session.nodes,
        &controls
            .context
            .to_json()
            .map_err(|_| "invalid policy context")?,
    )?;
    let recipe = Recipe {
        schema: model::POLICY_RECIPE.into(),
        policy_authority: Some(authority),
        controller_operation: Some(identity),
        input: fingerprint,
        mode: input.mode,
        operation: controls.decision.approval().operation.clone(),
        enrollment: input.enrollment.clone(),
        authorities: input.authorities.clone(),
        places: input.places.clone(),
        review: input.review.object.clone(),
        decision,
        context,
        grants,
        preservations: input.preservations.clone(),
        handoffs: input.handoffs.clone(),
        history: vec![],
        projections,
        limits: input.limits.clone(),
    };
    bookkeeping::save(&mut session.journal, &mut session.nodes, &recipe)?;
    finish_intent(&mut session, &recipe, Some(candidate))?;
    result(&mut session, &input, &recipe)
}
fn result(session: &mut Session, input: &StageInput, recipe: &Recipe) -> Result<Vec<u8>, String> {
    let root = bookkeeping::save(&mut session.journal, &mut session.nodes, recipe)?;
    let mut handle = Handle {
        schema: model::POLICY_HANDLE.into(),
        policy_minimum: Some(
            recipe
                .policy_authority
                .as_ref()
                .ok_or("missing policy authority")?
                .registry_minimum
                .clone(),
        ),
        operation: recipe.operation.clone(),
        recipe: root,
        enrollment: recipe.enrollment.clone(),
        authorities: session.witnesses(&input.authorities),
        capacity_minimums: model::capacity_minimums(
            recipe.places.values().chain(input.places.values()),
        )?,
        digest: Digest::blob(b""),
    };
    handle.digest = codec::handle_digest(&handle)?;
    codec::encode(&handle)
}
fn finish_intent(
    session: &mut Session,
    recipe: &Recipe,
    candidate: Option<intent::Candidate>,
) -> Result<(), String> {
    let enrollment = Session::enrollment(&recipe.enrollment);
    let state = session
        .journal
        .source(&Key::parse(recipe.operation.as_str()).map_err(|_| "invalid policy operation")?)
        .map_err(|_| "intent unavailable")?;
    let review = Review::from_json(&codec::get(&session.nodes, &recipe.review)?)
        .map_err(|_| "saved review invalid")?;
    let decision = Decision::from_json(&codec::get(&session.nodes, &recipe.decision)?, &review)
        .map_err(|_| "saved decision invalid")?;
    let context = Context::from_json(&codec::get(&session.nodes, &recipe.context)?)
        .map_err(|_| "saved context invalid")?;
    if state.is_none() {
        let candidate = if let Some(candidate) = candidate {
            candidate
        } else {
            let binding = recipe
                .policy_authority
                .as_ref()
                .ok_or("missing saved policy authority")?;
            let mut registry = Registry::open(&binding.registry_root, &binding.registry_minimum)
                .map_err(|_| "saved registry unavailable")?;
            let proof = Proof::load(&session.nodes, &context, &review, &decision)
                .map_err(|_| "invalid saved policy proof")?;
            let held = registry
                .hold(&binding.slot, Some(&proof.grant().version), &session.nodes)
                .map_err(|_| "saved policy no longer current")?;
            let head = hold_current(&mut session.index, &mut session.journal)
                .map_err(|_| "saved source no longer current")?;
            policy::stage_saved_registered(
                &mut session.journal,
                &mut session.nodes,
                &enrollment,
                &mut registry,
                &held,
                &head,
                &review,
                &decision,
                &context,
            )
            .map_err(|_| "saved policy admission refused")?
        };
        let ready = intent::finish(&mut session.journal, &mut session.nodes, candidate)
            .map_err(|_| "policy intent finish uncertain")?;
        let prepared = intent::prepare(
            &mut session.journal,
            &session.nodes,
            ready,
            key(&recipe.operation, "prepare")?,
        )
        .map_err(|_| "policy intent prepare uncertain")?;
        intent::adopt(
            &mut session.journal,
            &session.nodes,
            &prepared,
            key(&recipe.operation, "adopt")?,
        )
        .map_err(|_| "policy intent adopt uncertain")?;
    } else if state.is_some_and(|s| s.pending.is_some() && !s.cancelled) {
        let prepared = intent::resume_pending(
            &mut session.journal,
            &mut session.nodes,
            &enrollment,
            &recipe.operation,
        )
        .map_err(|_| "pending policy intent recovery refused")?;
        intent::adopt(
            &mut session.journal,
            &session.nodes,
            &prepared,
            key(&recipe.operation, "adopt")?,
        )
        .map_err(|_| "policy intent recovery uncertain")?;
    }
    let observed = intent::observe(
        &mut session.journal,
        &session.nodes,
        &enrollment,
        &recipe.operation,
    )
    .map_err(|_| "policy intent unavailable/cancelled")?;
    if observed.decision() != &decision || observed.execution_profile() != Some(&context) {
        return Err("saved policy intent differs from recipe".into());
    }
    Ok(())
}
