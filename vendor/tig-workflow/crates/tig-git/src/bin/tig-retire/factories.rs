//! Lazy original native plans/reservations. No payload effect runs here.
use super::{
    codec,
    model::{Handoff, Recipe},
    session::Session,
};
use std::collections::BTreeMap;
use tig_core::{Digest, Object};
use tig_store::{
    Key,
    ledger::{self, CapacityLedger, JournalRef, LedgerError},
    local::{LocalStore, removal},
    local_capacity::LocalCapacityProbe,
    provider::{self, profile::Qualification},
    retention::{RetentionGrant, handoff, release},
    retirement_intent::{
        ObservedIntent,
        execution_profile::Role,
        progress::{
            handoff_work::{self, HandoffWork},
            local_work::LocalWork,
            release_work::ReleaseWork,
        },
    },
};

// Pure admission preflight over the saved public descriptor. Actual calls still
// use the original profile guards inside tig-release/tig-remove execution.
fn qualified<T: PartialEq>(expected: &Qualification<T>, actual: &T) -> Result<(), String> {
    match expected {
        Qualification::Qualified { capabilities, .. } if capabilities == actual => Ok(()),
        Qualification::Qualified { .. } => {
            Err("native capability changed from saved profile".into())
        }
        Qualification::Unsupported => Err("required native capability unsupported".into()),
        Qualification::Unqualified => Err("required native capability unqualified".into()),
    }
}
fn store(recipe: &Recipe, place: &Digest) -> Result<LocalStore, String> {
    let binding = recipe
        .places
        .get(place)
        .ok_or("missing native place binding")?;
    let store = LocalStore::open(&binding.root).map_err(|_| "native place unavailable")?;
    if store.identity() != place {
        return Err("native place identity changed".into());
    }
    Ok(store)
}
fn capacity(
    recipe: &Recipe,
    place: &Digest,
    minimums: &BTreeMap<Digest, ledger::Checkpoint>,
) -> Result<CapacityLedger, String> {
    let binding = &recipe
        .places
        .get(place)
        .ok_or("missing native capacity binding")?
        .capacity;
    // Validate all retained witnesses, including lower-sequence forks, against
    // the original journal rather than trusting sequence comparison as ancestry.
    for alias in recipe
        .places
        .values()
        .filter(|p| p.capacity.ledger == binding.ledger)
    {
        if alias.capacity.root != binding.root {
            return Err("conflicting shared capacity root".into());
        }
        drop(
            CapacityLedger::open(&binding.root, &alias.capacity.minimum_checkpoint)
                .map_err(|_| "saved capacity witness unavailable/rolled back")?,
        );
    }
    let minimum = minimums
        .get(&binding.ledger)
        .ok_or("missing current capacity witness")?;
    let ledger = CapacityLedger::open(&binding.root, minimum)
        .map_err(|_| "current capacity witness unavailable/rolled back")?;
    if ledger.identity() != &binding.ledger {
        return Err("native capacity identity changed".into());
    }
    Ok(ledger)
}
fn key(
    recipe: &Recipe,
    object: &Object,
    action: &str,
    attempt: u64,
    phase: &str,
    sequence: u64,
) -> Result<Key, String> {
    let digest = Digest::blob(&codec::encode(&(
        "tig.native-retirement-budget.experimental-1",
        &recipe.operation,
        object,
        action,
        attempt,
        phase,
        sequence,
    ))?);
    Key::parse(&format!("retire-budget-{}", digest.as_str()))
        .map_err(|_| "invalid native budget key".into())
}
fn reserve(
    recipe: &Recipe,
    object: &Object,
    action: &str,
    attempt: u64,
    store: &LocalStore,
    ledger: &mut CapacityLedger,
    peak: u64,
) -> Result<JournalRef, String> {
    if peak == 0 || peak > recipe.limits.maximum_peak_per_action {
        return Err("native action peak exceeds admitted budget".into());
    }
    let job = key(recipe, object, action, attempt, "job", 0)?;
    let request = ledger.reserve(job.clone(), recipe.enrollment.owner.clone(), peak);
    let result = match request {
        Err(LedgerError::NeedsProbe) => {
            let sequence = ledger.checkpoint().sequence();
            LocalCapacityProbe::bind(store, ledger)
                .map_err(|_| "native capacity probe binding refused")?
                .observe(
                    store,
                    ledger,
                    key(recipe, object, action, attempt, "probe", sequence)?,
                    key(recipe, object, action, attempt, "sample", sequence)?,
                )
                .map_err(|_| "native capacity observation unavailable")?;
            ledger.reserve(job, recipe.enrollment.owner.clone(), peak)
        }
        other => other,
    };
    result
        .map(|r| r.reference().clone())
        .map_err(|_| "native resource reservation refused/uncertain".into())
}
fn release_preflight(
    ctx: &ObservedIntent,
    place: &LocalStore,
    claim: &Digest,
    role: Role,
    replay: provider::custody::release::Replay,
) -> Result<(), String> {
    let context = ctx
        .execution_profile()
        .ok_or("missing saved execution profile")?;
    let (_, profile) = context
        .binding(role, claim)
        .map_err(|_| "missing exact claim execution binding")?;
    qualified(
        &profile.release,
        &provider::custody::release::native::capabilities(place, replay),
    )
}
pub fn release(
    recipe: &Recipe,
    ctx: &ObservedIntent,
    grant: &RetentionGrant,
    attempt: u64,
    minimums: &mut BTreeMap<Digest, ledger::Checkpoint>,
) -> Result<ReleaseWork, String> {
    let store = store(recipe, grant.store())?;
    release_preflight(
        ctx,
        &store,
        grant.claim_id(),
        Role::RetirementRelease,
        provider::custody::release::Replay::Historical,
    )?;
    let plan = release::plan(
        &store,
        grant,
        ctx.decision(),
        &Session::enrollment(&recipe.enrollment),
    )
    .map_err(|_| "original native release planning refused")?;
    let mut ledger = capacity(recipe, grant.store(), minimums)?;
    let result = reserve(
        recipe,
        &grant.request().publication.object,
        &format!("release-{}", grant.claim_id().as_str()),
        attempt,
        &store,
        &mut ledger,
        plan.required_peak(),
    );
    minimums.insert(ledger.identity().clone(), ledger.checkpoint());
    Ok(ReleaseWork::new(plan, result?))
}
pub fn handoff(
    recipe: &Recipe,
    ctx: &ObservedIntent,
    grant: &RetentionGrant,
    successor: &RetentionGrant,
    binding: &Handoff,
    attempt: u64,
    minimums: &mut BTreeMap<Digest, ledger::Checkpoint>,
) -> Result<HandoffWork, String> {
    let source = store(recipe, grant.store())?;
    release_preflight(
        ctx,
        &source,
        grant.claim_id(),
        Role::HandoffRelease,
        provider::custody::release::Replay::Admitted,
    )?;
    let peer = store(recipe, successor.store())?;
    let context = ctx.execution_profile().ok_or("missing execution context")?;
    let (_, profile) = context
        .binding(Role::Custody, successor.store())
        .map_err(|_| "missing successor custody binding")?;
    let guard = profile
        .admit(
            &profile
                .fingerprint()
                .map_err(|_| "invalid successor profile")?,
            &profile.binding,
        )
        .map_err(|_| "successor custody profile refused")?;
    guard
        .check_custody(&provider::custody::native::capabilities(&peer))
        .map_err(|_| "successor custody capability refused")?;
    let request = handoff::HandoffRequest {
        operation: handoff_work::operation(ctx, &grant.request().publication.object, attempt)
            .map_err(|_| "invalid native handoff operation")?,
        owner: recipe.enrollment.owner.clone(),
        previous: grant.clone(),
        successor: successor.clone(),
        source_domain: binding.source_domain.clone(),
        successor_domain: binding.successor_domain.clone(),
    };
    let plan =
        handoff::plan(&source, &request).map_err(|_| "original native handoff planning refused")?;
    let authorization = plan
        .authorize_handoff()
        .map_err(|_| "native exact handoff authorization refused")?;
    let mut ledger = capacity(recipe, grant.store(), minimums)?;
    let result = reserve(
        recipe,
        &grant.request().publication.object,
        &format!("handoff-{}", grant.claim_id().as_str()),
        attempt,
        &source,
        &mut ledger,
        plan.required_peak(),
    );
    minimums.insert(ledger.identity().clone(), ledger.checkpoint());
    Ok(HandoffWork::new(plan, authorization, result?))
}
pub fn removal(
    recipe: &Recipe,
    ctx: &ObservedIntent,
    object: &Object,
    place: &Digest,
    attempt: u64,
    minimums: &mut BTreeMap<Digest, ledger::Checkpoint>,
) -> Result<LocalWork, String> {
    let store = store(recipe, place)?;
    let context = ctx.execution_profile().ok_or("missing execution context")?;
    let (_, profile) = context
        .binding(Role::Removal, place)
        .map_err(|_| "missing removal binding")?;
    qualified(
        &profile.remove,
        &provider::removal::native::capabilities(&store),
    )?;
    let preservation = recipe
        .preservations
        .iter()
        .find(|p| p.source == *place && p.object == *object);
    let plan = if let Some(preservation) = preservation {
        for survivor in &preservation.profile.retained {
            let peer = self::store(recipe, &survivor.commitment.place)?;
            let (_, profile) = context
                .binding(Role::Custody, &survivor.commitment.place)
                .map_err(|_| "missing survivor custody binding")?;
            let guard = profile
                .admit(
                    &profile
                        .fingerprint()
                        .map_err(|_| "invalid survivor profile")?,
                    &profile.binding,
                )
                .map_err(|_| "survivor profile refused")?;
            guard
                .check_custody(&provider::custody::native::capabilities(&peer))
                .map_err(|_| "survivor custody capability refused")?;
        }
        removal::custody::portable::plan(&store, ctx, object, attempt, &preservation.profile)
    } else {
        removal::plan(&store, ctx, object, attempt)
    }
    .map_err(|_| "original native removal planning refused")?;
    let mut ledger = capacity(recipe, place, minimums)?;
    let result = reserve(
        recipe,
        object,
        &format!("remove-{}", place.as_str()),
        attempt,
        &store,
        &mut ledger,
        plan.required_peak(),
    );
    minimums.insert(ledger.identity().clone(), ledger.checkpoint());
    Ok(LocalWork::new(plan, result?))
}
