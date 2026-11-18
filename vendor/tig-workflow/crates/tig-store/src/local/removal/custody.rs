//! Retained-custody mode for the existing native removal kernel.
use super::*;
use crate::retention::{self, RetainedReplica, RetentionGrant};
use std::collections::BTreeSet;
use tig_core::retirement::review::Loss;
pub mod portable;
const MAX_SURVIVORS: usize = 16;

/// Failure domains and required coverage are admitted enrollment/policy inputs.
/// They are not inferred from paths and are not native independence evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Survivor {
    pub grant: RetentionGrant,
    pub failure_domain: Key,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub source_domain: Key,
    /// Strictly sorted by native store ID, unique, and exactly reviewed survivors.
    pub retained: Vec<Survivor>,
    pub required_domains: usize,
    pub minimum_ack: AckLevel,
}
pub struct Endpoint<'a> {
    pub store: &'a LocalStore,
    pub ledger: &'a mut CapacityLedger,
}
fn shape(p: &Profile, source: &Digest, object: &Object) -> Result<BTreeSet<Digest>> {
    if p.retained.is_empty()
        || p.retained.len() > MAX_SURVIVORS
        || p.required_domains == 0
        || p.required_domains > MAX_SURVIVORS
        || p.minimum_ack < AckLevel::ProcessRestart
    {
        return Err(RemovalError::InvalidPlan);
    }
    let mut stores = BTreeSet::new();
    let mut domains = BTreeSet::new();
    let mut previous: Option<&Digest> = None;
    for r in &p.retained {
        let store = r.grant.store();
        if store == source
            || previous.is_some_and(|old| old >= store)
            || r.grant.request().publication.object != *object
            || r.grant.request().publication.required < p.minimum_ack
        {
            return Err(RemovalError::InvalidPlan);
        }
        previous = Some(store);
        stores.insert(store.clone());
        // Same-source-domain replicas are protected but never count independently.
        if r.failure_domain != p.source_domain {
            domains.insert(r.failure_domain.as_str());
        }
    }
    if domains.len() < p.required_domains {
        return Err(RemovalError::InvalidPlan);
    }
    Ok(stores)
}
pub(super) fn validate(
    store: &LocalStore,
    intent: &ObservedIntent,
    scope: &Scope,
    p: &Profile,
) -> Result<()> {
    let stores = shape(p, &store.instance, &scope.object)?;
    if scope.preservation.as_ref() != Some(&Digest::blob(&encode(p)?))
        || intent
            .decision()
            .consent()
            .accept_intentional_loss
            .contains(&scope.object)
        || !intent.decision().consent().remove_copies.contains(&Target {
            object: scope.object.clone(),
            store: store.instance.clone(),
        })
    {
        return Err(RemovalError::InvalidPlan);
    }
    let reviewed = intent
        .review()
        .objects()
        .iter()
        .find(|o| o.object() == &scope.object)
        .ok_or(RemovalError::InvalidPlan)?;
    if !matches!(reviewed.loss(), Loss::ObservedSurvivors(survivors) if survivors == &stores) {
        return Err(RemovalError::InvalidPlan);
    }
    Ok(())
}
pub(super) fn fresh(keepers: &[(usize, RetainedReplica)]) -> Result<()> {
    for (_, keeper) in keepers {
        keeper.fresh()?;
    }
    Ok(())
}
pub fn plan(
    store: &LocalStore,
    intent: &ObservedIntent,
    object: &Object,
    attempt: u64,
    profile: &Profile,
) -> Result<Plan> {
    // Bound counts before clone/encoding; combined record must fit provider limits.
    shape(profile, store.identity(), object)?;
    let mut p = plan_inner(store, intent, object, attempt)?;
    p.scope.preservation = Some(Digest::blob(&encode(profile)?));
    p.custody = Some(profile.clone());
    validate(store, intent, &p.scope, profile)?;
    if encode(&p)?.len() > 48 * 1024 {
        return Err(Error::Budget.into());
    }
    Ok(p)
}
pub fn apply<N: NodeStore, P: Protection>(
    execution: Execution<'_, N, P>,
    planned: &Plan,
    endpoints: &mut [Endpoint<'_>],
) -> Result<RemovalReceipt> {
    if planned.custody.is_none() {
        return Err(RemovalError::InvalidPlan);
    }
    run(execution, planned, endpoints, |_| Ok(()))
}
pub(super) fn acquire(
    profile: Option<&Profile>,
    endpoints: &mut [Endpoint<'_>],
    scope: &Scope,
) -> Result<Vec<(usize, RetainedReplica)>> {
    let Some(p) = profile else {
        if !endpoints.is_empty() {
            return Err(RemovalError::InvalidPlan);
        }
        return Ok(Vec::new());
    };
    if endpoints.len() != p.retained.len() || endpoints.len() > MAX_SURVIVORS {
        return Err(RemovalError::InvalidPlan);
    }
    let mut ids = BTreeSet::new();
    for e in endpoints.iter() {
        if !ids.insert(e.store.identity().clone()) {
            return Err(RemovalError::InvalidPlan);
        }
    }
    if ids != shape(p, &scope.store, &scope.object)? {
        return Err(RemovalError::InvalidPlan);
    }
    let mut keepers = Vec::with_capacity(p.retained.len());
    for r in &p.retained {
        let index = endpoints
            .iter()
            .position(|e| e.store.identity() == r.grant.store())
            .ok_or(RemovalError::InvalidPlan)?;
        let e = &mut endpoints[index];
        keepers.push((index, retention::verify(e.ledger, e.store, &r.grant)?));
    }
    Ok(keepers)
}
pub(super) fn revalidate(
    keepers: &mut [(usize, RetainedReplica)],
    endpoints: &mut [Endpoint<'_>],
    scope: &Scope,
) -> Result<()> {
    for (index, keeper) in keepers {
        let e = endpoints.get_mut(*index).ok_or(RemovalError::InvalidPlan)?;
        if keeper.grant().request().publication.object != scope.object {
            return Err(RemovalError::InvalidPlan);
        }
        keeper.revalidate(e.ledger, e.store)?;
    }
    Ok(())
}
#[cfg(test)]
#[path = "custody_tests.rs"]
mod tests;
