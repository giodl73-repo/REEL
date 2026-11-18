//! Frozen policy input and acknowledged output in the original journal.
use super::*;
use tig_core::retirement::review::Inventory;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    budget: BudgetRef,
    input: Object,
    review: Object,
    profiles: Option<Object>,
    cancelled: bool,
}
fn source(r: &Request) -> io::Result<Key> {
    key("stage", &r.operation)
}
fn load(s: &mut Session, r: &Request) -> io::Result<Option<State>> {
    store::load(&mut s.journal, &s.nodes, &source(r)?, &s.enrolled.owner)
}
fn save(s: &mut Session, r: &Request, state: &State, first: bool) -> io::Result<()> {
    store::save(
        &mut s.journal,
        &mut s.nodes,
        &source(r)?,
        &s.enrolled.owner,
        state,
        Some(&state.budget),
        first,
    )
}
pub(super) fn exists(s: &mut Session, r: &Request) -> io::Result<bool> {
    Ok(load(s, r)?.is_some())
}
pub(super) fn input_root(s: &mut Session, r: &Request) -> io::Result<Option<Object>> {
    Ok(load(s, r)?.map(|v| v.input))
}
pub(super) fn cancelled(s: &mut Session, r: &Request) -> io::Result<bool> {
    Ok(load(s, r)?.is_some_and(|v| v.cancelled))
}
/// Frozen stage input is also an acknowledged checkpoint witness. Preserve it
/// even when the run's ordinary Prepared record has not yet been published.
pub(super) fn advance(s: &mut Session, r: &Request) -> io::Result<()> {
    if let Some(state) = load(s, r)? {
        let input: policy::Stage = read(&s.nodes, &state.input)?;
        let c = input.base.target.control.as_ref().ok_or_else(invalid)?;
        if input.operation != r.operation
            || c.index != s.enrolled.config.package.index
            || c.root != s.enrolled.config.package.index_root
        {
            return Err(invalid());
        }
        s.handle.index_minimum = stronger(&s.handle.index_minimum, &c.minimum)?;
        s.handle.policy_minimum = stronger(&s.handle.policy_minimum, &input.authority.minimum)?;
        if let Some(root) = state.profiles {
            let profiles: Vec<retirement::Profile> = read(&s.nodes, &root)?;
            for profile in profiles {
                let authority = profile.policy.as_ref().ok_or_else(invalid)?;
                s.handle.policy_minimum =
                    stronger(&s.handle.policy_minimum, &authority.authority.minimum)?;
            }
        }
    }
    Ok(())
}
pub(super) fn prepare(
    s: &mut Session,
    r: &Request,
    recipe: &Recipe,
    inventory: Option<Inventory>,
    publish: bool,
) -> io::Result<Option<Vec<retirement::Profile>>> {
    let mut state = match load(s, r)? {
        Some(v) => v,
        None => {
            let Some((input, review)) = input(s, r, recipe, inventory.ok_or_else(invalid)?)? else {
                return Ok(Some(vec![]));
            };
            let root = put(&mut s.nodes, &input)?;
            s.nodes.finish().map_err(|_| invalid())?;
            let budget = reserve(
                &mut s.journal,
                &s.enrolled.owner,
                &source(r)?,
                &r.operation,
                "stage-budget",
            )?;
            let state = State {
                budget,
                input: root,
                review,
                profiles: None,
                cancelled: false,
            };
            save(s, r, &state, true)?;
            state
        }
    };
    if state.cancelled {
        return Err(invalid());
    }
    let input: policy::Stage = read(&s.nodes, &state.input)?;
    if input.operation != r.operation || input.catalogue != recipe.cleanup.sha256 {
        return Err(invalid());
    }
    if s.index()?
        .actor(&mut s.nodes, &s.enrolled.config.current)
        .map_err(|_| invalid())?
        .selection
        != recipe.current_after
    {
        return Err(invalid());
    }
    let review_bytes = store::bytes(&s.nodes, &state.review)?;
    if profile_file(&s.handle.root.join("io"), &review_bytes)? != input.review_input {
        return Err(invalid());
    }
    if !publish {
        return Ok(None);
    }
    if state.profiles.is_none() {
        let profiles: Vec<retirement::Profile> = child(s, ControlCommand::StagePolicy, &input)?;
        state.profiles = Some(put(&mut s.nodes, &profiles)?);
        s.nodes.finish().map_err(|_| invalid())?;
        save(s, r, &state, false)?;
    }
    progress_budget::release(&mut s.journal, &state.budget).map_err(|_| invalid())?;
    read(&s.nodes, state.profiles.as_ref().ok_or_else(invalid)?).map(Some)
}
/// No effect can start in this runner before acknowledged profile publication.
/// Cancel a lost-output stage by original intent identity, without fresh grant
/// admission or reconstructing deletion profiles from newer observations.
pub(super) fn cancel(s: &mut Session, r: &Request) -> io::Result<Vec<retirement::Profile>> {
    let Some(mut state) = load(s, r)? else {
        return Ok(vec![]);
    };
    if state.profiles.is_none() {
        // Acknowledge cancellation before touching the intent; a crash must not
        // allow the default run path to reconstruct fresh staging permission.
        state.cancelled = true;
        state.profiles = Some(put(&mut s.nodes, &Vec::<retirement::Profile>::new())?);
        s.nodes.finish().map_err(|_| invalid())?;
        save(s, r, &state, false)?;
    }
    if state.cancelled {
        let input: policy::Stage = read(&s.nodes, &state.input)?;
        if input.operation != r.operation {
            return Err(invalid());
        }
        let mut j = SourceJournal::open(&input.base.intent.root, &input.base.intent.minimum)
            .map_err(|_| invalid())?;
        let id = Key::parse(r.operation.as_str()).map_err(|_| invalid())?;
        let current = j.source(&id).map_err(|_| invalid())?;
        if current
            .as_ref()
            .is_some_and(|v| v.owner != s.enrolled.owner)
        {
            return Err(invalid());
        }
        if current.as_ref().is_none_or(|v| !v.cancelled) {
            // Cancel revision zero too: an absent intent is not permission for
            // a delayed staging process to create it after this acknowledgement.
            tig_store::retirement_intent::cancel(
                &mut j,
                &s.enrolled.owner,
                &r.operation,
                current.map_or(0, |v| v.revision),
                key("cancel-stage", &r.operation)?,
            )
            .map_err(|_| invalid())?;
        }
    }
    progress_budget::release(&mut s.journal, &state.budget).map_err(|_| invalid())?;
    read(&s.nodes, state.profiles.as_ref().ok_or_else(invalid)?)
}
fn input(
    s: &mut Session,
    r: &Request,
    recipe: &Recipe,
    inventory: tig_core::retirement::review::Inventory,
) -> io::Result<Option<(policy::Stage, Object)>> {
    let Some(review) = plan::review(s, recipe, inventory)? else {
        return Ok(None);
    };
    if !review.objects().iter().any(|o| o.eligible()) {
        return Ok(None);
    }
    let c = s.config()?;
    let mut index = s.index()?;
    let mut providers = c
        .destinations
        .iter()
        .map(|d| d.enrollment.clone())
        .collect::<Vec<_>>();
    for p in &mut providers {
        p.control.as_mut().ok_or_else(invalid)?.minimum = index.checkpoint();
    }
    let target = providers.pop().ok_or_else(invalid)?;
    let control = target.control.as_ref().ok_or_else(invalid)?;
    let base = retirement::Profile {
        policy: None,
        target: target.clone(),
        metadata: retirement::MetadataBinding {
            root: s.handle.metadata.root.clone(),
            minimum: s.nodes.checkpoint(),
        },
        intent: retirement::IntentBinding {
            root: c.package.intent_root.clone(),
            minimum: c.package.intent_minimum.clone(),
            owner: s.enrolled.owner.clone(),
            subject: index.enrollment_profile().subject.clone(),
            operation: r.operation.clone(),
        },
        request: source_claims::protection::effects::Request {
            operation: key("placeholder", &r.operation)?,
            object: recipe.cleanup.clone(),
            store: control.store.clone(),
            context: Digest::blob(SCHEMA.as_bytes()),
            expected: index.effect_frame().map_err(|_| invalid())?,
        },
        custody: c.custody.clone(),
        survivors: providers,
        verification_bytes: c.limits.verification_bytes,
    };
    let root = s.handle.root.join("io");
    let bytes = review.to_json().map_err(|_| invalid())?;
    let path = profile_file(&root, &bytes)?;
    let review = s.nodes.put(&bytes).map_err(|_| invalid())?;
    Ok(Some((
        policy::Stage {
            base,
            authority: c.policy,
            review_input: path,
            catalogue: recipe.cleanup.sha256.clone(),
            operation: r.operation.clone(),
        },
        review,
    )))
}
