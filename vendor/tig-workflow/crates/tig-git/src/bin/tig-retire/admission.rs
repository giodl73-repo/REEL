//! Bounded control admission. No payload provider is opened by these helpers.
use super::{
    codec,
    model::{Projection, StageInput},
    session::Session,
};
use std::collections::{BTreeMap, BTreeSet};
use tig_core::{
    Digest, LogicalPath, Object,
    retirement::review::{Review, decision::Decision},
    tree::{Tree, TreeLimits},
};
use tig_store::{
    retention::RetentionGrant,
    retirement_intent::{
        execution_profile::{Context, Role},
        progress::history_work,
    },
};

pub struct Controls {
    pub review: Review,
    pub context: Context,
    pub decision: Decision,
    pub grants: BTreeMap<Digest, RetentionGrant>,
}

pub fn controls(input: &StageInput, session: &mut Session) -> Result<Controls, String> {
    input.validate()?;
    if input.approval.namespace != input.authorities.journal.identity
        || input.approval.owner != input.enrollment.subject
    {
        return Err("foreign retirement approval enrollment".into());
    }
    let mut budget = codec::ArtifactBudget::new();
    let review = Review::from_json(&budget.load(&mut session.nodes, &input.review)?)
        .map_err(|_| "invalid exact retirement review")?;
    review_mode(input.mode, &review)?;
    let context = Context::from_json(&budget.load(&mut session.nodes, &input.context)?)
        .map_err(|_| "invalid exact execution context")?;
    let decision = context
        .authorize(&review, input.approval.clone(), input.consent.clone())
        .map_err(|_| "exact retirement consent/profile refused")?;
    let mut grants = BTreeMap::new();
    for artifact in &input.grants {
        let grant = RetentionGrant::from_json(&budget.load(&mut session.nodes, artifact)?)
            .map_err(|_| "invalid exact native retention grant")?;
        if grants.insert(grant.claim_id().clone(), grant).is_some() {
            return Err("duplicate native claim grant".into());
        }
    }
    let result = Controls {
        review,
        context,
        decision,
        grants,
    };
    validate_scope(input, &result)?;
    Ok(result)
}

pub(super) fn validate_scope(input: &StageInput, controls: &Controls) -> Result<(), String> {
    let consent = controls.decision.consent();
    let mut used_grants = BTreeSet::new();
    let mut history_claims = BTreeSet::new();
    let selected: BTreeSet<_> = consent
        .remove_copies
        .iter()
        .map(|t| t.object.clone())
        .collect();
    let actions = consent
        .remove_copies
        .len()
        .checked_add(consent.release_owned_claims.len())
        .ok_or("native controller action budget")?;
    if actions > input.limits.maximum_actions as usize
        || selected
            .iter()
            .any(|o| o.bytes > input.limits.maximum_object_bytes)
        || consent
            .remove_copies
            .iter()
            .any(|t| !input.places.contains_key(&t.store))
    {
        return Err("native controller target/action budget".into());
    }
    let mut actors = BTreeMap::new();
    let mut source_versions = BTreeSet::new();
    for history in &input.history {
        if history.objects.is_empty()
            || history.objects.len() > history_work::block::MAX_MEMBERS
            || !history.objects.is_subset(&selected)
            || history.enrollment.subject != input.enrollment.subject
            || !source_versions.insert((
                history.source.as_str().to_owned(),
                history.predecessor.revision,
            ))
        {
            return Err("invalid/duplicate history group".into());
        }
        if let Some((owner, revision)) = actors.insert(
            history.actor.as_str(),
            (&history.actor_owner, history.expected_actor_revision),
        ) && (owner != &history.actor_owner || revision != history.expected_actor_revision)
        {
            return Err("conflicting shared history projection actor".into());
        }
        for object in &history.objects {
            let claim = history_work::claim_id(
                &input.authorities.journal.identity,
                &history.source,
                &history.enrollment.owner,
                &history.predecessor,
                object,
            )
            .map_err(|_| "invalid exact history claim")?;
            if !consent
                .release_owned_claims
                .iter()
                .any(|r| r.claim == claim && r.object == *object)
                || !history_claims.insert(claim)
            {
                return Err("history claim outside exact consent".into());
            }
        }
    }
    if &history_claims != controls.context.history_releases() {
        return Err("history groups do not match admitted execution context".into());
    }
    let mut handoffs = BTreeMap::new();
    for handoff in &input.handoffs {
        if handoffs.insert(handoff.claim.clone(), handoff).is_some()
            || !consent
                .release_owned_claims
                .iter()
                .any(|r| r.claim == handoff.claim)
        {
            return Err("duplicate/unselected handoff".into());
        }
    }
    for release in &consent.release_owned_claims {
        if history_claims.contains(&release.claim) {
            continue;
        }
        let grant = controls
            .grants
            .get(&release.claim)
            .ok_or("missing released native grant")?;
        if grant.request().owner != input.enrollment.owner
            || grant.request().publication.object != release.object
            || !input.places.contains_key(grant.store())
        {
            return Err("foreign native claim release".into());
        }
        used_grants.insert(release.claim.clone());
        let role = if let Some(handoff) = handoffs.get(&release.claim) {
            let successor = controls
                .grants
                .get(&handoff.successor)
                .ok_or("missing successor grant")?;
            if successor.request().publication.object != release.object
                || successor.store() == grant.store()
                || !input.places.contains_key(successor.store())
                || successor.request().publication.required < grant.request().publication.required
                || handoff.source_domain == handoff.successor_domain
                || consent
                    .release_owned_claims
                    .iter()
                    .any(|r| r.claim == handoff.successor)
            {
                return Err("invalid retained handoff successor".into());
            }
            controls
                .context
                .verify_handoff_binding(&tig_store::retention::handoff::HandoffRequest {
                    // The exact specification excludes the derived typed attempt.
                    operation: grant.request().operation.clone(),
                    owner: input.enrollment.owner.clone(),
                    previous: grant.clone(),
                    successor: successor.clone(),
                    source_domain: handoff.source_domain.clone(),
                    successor_domain: handoff.successor_domain.clone(),
                })
                .map_err(|_| "handoff differs from exact saved execution context")?;
            used_grants.insert(handoff.successor.clone());
            Role::HandoffRelease
        } else {
            Role::RetirementRelease
        };
        let (_, profile) = controls
            .context
            .binding(role, &release.claim)
            .map_err(|_| "claim protocol differs from saved context")?;
        if &profile.binding.place != grant.store() {
            return Err("claim provider differs from saved context".into());
        }
    }
    let mut preserved = BTreeSet::new();
    for preservation in &input.preservations {
        let target = tig_core::retirement::review::decision::Target {
            object: preservation.object.clone(),
            store: preservation.source.clone(),
        };
        if !consent.remove_copies.contains(&target)
            || consent.accept_intentional_loss.contains(&target.object)
            || !preserved.insert(target)
        {
            return Err("duplicate/unselected/loss preservation".into());
        }
        let places = preservation
            .profile
            .places(&preservation.source, &preservation.object)
            .map_err(|_| "invalid exact survivor profile")?;
        let entry = controls
            .review
            .objects()
            .iter()
            .find(|o| o.object() == &preservation.object)
            .ok_or("missing preserved review object")?;
        let tig_core::retirement::review::Loss::ObservedSurvivors(reviewed) = entry.loss() else {
            return Err("preservation lacks reviewed survivor scope".into());
        };
        if &places != reviewed {
            return Err("preservation differs from reviewed survivors".into());
        }
        for survivor in &preservation.profile.retained {
            let commitment = &survivor.commitment;
            let grant = controls
                .grants
                .get(&commitment.claim)
                .ok_or("missing surviving native grant")?;
            if grant.store() != &commitment.place
                || grant.request().publication.object != commitment.object
                || grant.request().owner != commitment.owner
                || grant.request().publication.required != commitment.level
                || !input.places.contains_key(&commitment.place)
                || consent
                    .release_owned_claims
                    .iter()
                    .any(|r| r.claim == commitment.claim)
            {
                return Err("surviving commitment differs from native grant".into());
            }
            used_grants.insert(commitment.claim.clone());
        }
    }
    let required: BTreeSet<_> = consent
        .remove_copies
        .iter()
        .filter(|t| !consent.accept_intentional_loss.contains(&t.object))
        .cloned()
        .collect();
    if preserved != required || used_grants != controls.grants.keys().cloned().collect() {
        return Err("missing preservation or unused native grant".into());
    }
    Ok(())
}

pub fn grant_index(controls: &Controls, session: &mut Session) -> Result<Object, String> {
    let mut refs = BTreeMap::new();
    for (id, grant) in &controls.grants {
        let bytes = grant.to_json().map_err(|_| "cannot encode grant")?;
        refs.insert(
            LogicalPath::parse(&format!("claims/{}", id.as_str()))
                .map_err(|_| "invalid grant index key")?,
            codec::put(&mut session.nodes, &bytes)?,
        );
    }
    let (tree, _) = Tree::build(&refs, &mut session.nodes, TreeLimits::default())
        .map_err(|_| "bounded native grant index refused")?;
    Ok(tree.root().clone())
}

/// Snapshot only declared actors and exact reviewed paths. No history inventory.
pub fn projections(
    input: &StageInput,
    controls: &Controls,
    session: &mut Session,
) -> Result<Vec<Projection>, String> {
    let mut result = Vec::new();
    let mut reads = super::read_scope::ReadScope::new(&session.nodes, TreeLimits::default());
    let mut groups: BTreeMap<&str, (&super::model::History, BTreeSet<Object>)> = BTreeMap::new();
    for history in &input.history {
        let (prior, objects) = groups
            .entry(history.actor.as_str())
            .or_insert_with(|| (history, BTreeSet::new()));
        if prior.actor_owner != history.actor_owner
            || prior.expected_actor_revision != history.expected_actor_revision
        {
            return Err("conflicting shared history projection actor".into());
        }
        objects.extend(history.objects.iter().cloned());
    }
    for (_, (history, objects)) in groups {
        let state = session
            .index
            .actor(&mut reads, &history.actor)
            .map_err(|_| "history projection actor unavailable")?;
        if !state.ready || state.revision != history.expected_actor_revision {
            return Err("history projection actor unknown/stale".into());
        }
        let tree = Tree::from_root(state.selection.root.clone());
        let mut remove = BTreeMap::new();
        for entry in controls
            .review
            .objects()
            .iter()
            .filter(|o| objects.contains(o.object()))
        {
            for path in entry.paths() {
                let (existing, _) = tree
                    .get(path, &mut reads, TreeLimits::default())
                    .map_err(|_| "history projection exact path unavailable")?;
                if existing.as_ref() == Some(entry.object()) {
                    remove.insert(path.clone(), entry.object().clone());
                }
            }
        }
        if remove.is_empty() || remove.len() > tig_store::source_claims::protection::MAX_CHANGES {
            return Err("history projection empty/over native change bound".into());
        }
        result.push(Projection {
            actor: history.actor.clone(),
            owner: history.actor_owner.clone(),
            expected_revision: state.revision,
            prior: state.selection,
            remove,
        });
    }
    Ok(result)
}

/// A clockless placement review cannot be recast as age-qualified final retirement.
pub fn review_mode(mode: super::model::Mode, review: &Review) -> Result<(), String> {
    if mode == super::model::Mode::FinalRetirement && review.is_routine_eviction() {
        return Err("routine review cannot authorize final-retirement mode".into());
    }
    Ok(())
}
