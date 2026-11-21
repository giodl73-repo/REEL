//! Save one immutable recipe before original native profiled intent admission.
use super::{
    admission, bookkeeping, codec, input_identity,
    model::{self, Handle, Recipe, StageInput},
    session::Session,
};
use tig_core::{Digest, Object};
use tig_store::{Key, retirement_intent as intent};

fn command(operation: &Digest, phase: &str) -> Result<Key, String> {
    Key::parse(&format!("retire-intent-{phase}-{}", operation.as_str()))
        .map_err(|_| "invalid native intent command".into())
}

pub fn stage(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() > model::MAX_INPUT {
        return Err("native stage profile byte budget".into());
    }
    let input: StageInput = codec::decode(bytes)?;
    input.validate()?;
    let fingerprint = input_identity::fingerprint(&input)?;
    let mut session = Session::open(&input.authorities)?;
    let controls = admission::controls(&input, &mut session)?;
    let operation = &controls.decision.approval().operation;
    let prior = bookkeeping::existing(
        &mut session.journal,
        &session.nodes,
        operation,
        &input.enrollment.owner,
    )?;
    let recipe = if let Some((_, prior)) = prior {
        if prior.input != fingerprint {
            return Err("changed input under saved controller operation".into());
        }
        // Native histories validate every old and newly supplied witness. Do not
        // recompute a projection from an actor changed by previous execution.
        session
            .nodes
            .finish()
            .map_err(|_| "control metadata finish uncertain")?;
        session = Session::reconcile(&session.witnesses(&input.authorities), &prior.authorities)?;
        prior
    } else {
        let id = Key::parse(operation.as_str()).map_err(|_| "invalid intent operation")?;
        if session
            .journal
            .source(&id)
            .map_err(|_| "intent authority unavailable")?
            .is_some()
        {
            return Err("cannot attach a controller recipe to an existing foreign intent".into());
        }
        let projections = admission::projections(&input, &controls, &mut session)?;
        let grants = admission::grant_index(&controls, &mut session)?;
        let decision = codec::put(
            &mut session.nodes,
            &controls
                .decision
                .to_json()
                .map_err(|_| "invalid decision encoding")?,
        )?;
        Recipe {
            policy_authority: None,
            controller_operation: None,
            schema: model::RECIPE.into(),
            input: fingerprint,
            mode: input.mode,
            operation: operation.clone(),
            enrollment: input.enrollment.clone(),
            authorities: input.authorities.clone(),
            places: input.places.clone(),
            review: input.review.object.clone(),
            decision,
            context: input.context.object.clone(),
            grants,
            preservations: input.preservations.clone(),
            handoffs: input.handoffs.clone(),
            history: input.history.clone(),
            projections,
            limits: input.limits.clone(),
        }
    };
    let capacity_minimums =
        model::capacity_minimums(recipe.places.values().chain(input.places.values()))?;
    let root = bookkeeping::save(&mut session.journal, &mut session.nodes, &recipe)?;
    admit(&mut session, &recipe, &controls)?;
    let mut handle = Handle {
        policy_minimum: None,
        schema: model::HANDLE.into(),
        operation: recipe.operation.clone(),
        recipe: root,
        enrollment: recipe.enrollment.clone(),
        authorities: session.witnesses(&input.authorities),
        capacity_minimums,
        digest: Digest::blob(b""),
    };
    handle.digest = codec::handle_digest(&handle)?;
    codec::encode(&handle)
}

fn admit(
    session: &mut Session,
    recipe: &Recipe,
    controls: &admission::Controls,
) -> Result<(), String> {
    let operation = &recipe.operation;
    let enrollment = Session::enrollment(&recipe.enrollment);
    let id = Key::parse(operation.as_str()).map_err(|_| "invalid intent operation")?;
    if Object::from_bytes(
        &controls
            .decision
            .to_json()
            .map_err(|_| "invalid decision")?,
    ) != recipe.decision
        || Object::from_bytes(&controls.review.to_json().map_err(|_| "invalid review")?)
            != recipe.review
        || Object::from_bytes(&controls.context.to_json().map_err(|_| "invalid context")?)
            != recipe.context
    {
        return Err("saved recipe controls differ".into());
    }
    match session
        .journal
        .source(&id)
        .map_err(|_| "intent authority unavailable")?
    {
        None => {
            let candidate = intent::stage_profiled(
                &mut session.journal,
                &mut session.nodes,
                &enrollment,
                &controls.review,
                &controls.decision,
                &controls.context,
            )
            .map_err(|_| "native intent staging refused")?;
            let ready = intent::finish(&mut session.journal, &mut session.nodes, candidate)
                .map_err(|_| "native intent metadata finish uncertain")?;
            let prepared = intent::prepare(
                &mut session.journal,
                &session.nodes,
                ready,
                command(operation, "prepare")?,
            )
            .map_err(|_| "native intent prepare uncertain; retry exact input")?;
            intent::adopt(
                &mut session.journal,
                &session.nodes,
                &prepared,
                command(operation, "adopt")?,
            )
            .map_err(|_| "native intent adoption uncertain; retry exact input")?;
        }
        Some(state) if state.pending.is_some() && !state.cancelled => {
            let prepared = intent::resume_pending(
                &mut session.journal,
                &mut session.nodes,
                &enrollment,
                operation,
            )
            .map_err(|_| "native intent pending recovery refused")?;
            intent::adopt(
                &mut session.journal,
                &session.nodes,
                &prepared,
                command(operation, "adopt")?,
            )
            .map_err(|_| "native intent recovery adoption uncertain")?;
        }
        Some(_) => (),
    }
    let observed = intent::observe(&mut session.journal, &session.nodes, &enrollment, operation)
        .map_err(|_| "native intent unavailable/cancelled")?;
    if observed.decision() != &controls.decision
        || observed
            .review()
            .to_json()
            .map_err(|_| "invalid observed review")?
            != controls.review.to_json().map_err(|_| "invalid review")?
        || observed
            .execution_profile()
            .map(|p| p.fingerprint())
            .transpose()
            .map_err(|_| "invalid saved profile")?
            != Some(
                controls
                    .context
                    .fingerprint()
                    .map_err(|_| "invalid expected profile")?,
            )
    {
        return Err("native intent differs from saved controller recipe".into());
    }
    Ok(())
}
