//! Portable survivor custody through the original native source-removal kernel.
use super::super::*;
use crate::provider::custody::{self as provider, Profile, Retainer};
use tig_core::retirement::review::Loss;
pub(in crate::local::removal) fn validate(
    store: &LocalStore,
    intent: &ObservedIntent,
    scope: &Scope,
    profile: &Profile,
) -> Result<()> {
    let places = profile.places(store.identity(), &scope.object)?;
    if scope.preservation.as_ref() != Some(&profile.digest()?)
        || intent
            .decision()
            .consent()
            .accept_intentional_loss
            .contains(&scope.object)
        || !intent.decision().consent().remove_copies.contains(&Target {
            object: scope.object.clone(),
            store: store.identity().clone(),
        })
    {
        return Err(RemovalError::InvalidPlan);
    }
    let reviewed = intent
        .review()
        .objects()
        .iter()
        .find(|r| r.object() == &scope.object)
        .ok_or(RemovalError::InvalidPlan)?;
    if !matches!(reviewed.loss(), Loss::ObservedSurvivors(found) if found == &places) {
        return Err(RemovalError::InvalidPlan);
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
    profile.places(store.identity(), object)?;
    let mut p = plan_inner(store, intent, object, attempt)?;
    p.scope.preservation = Some(profile.digest()?);
    p.provider_custody = Some(profile.clone());
    validate(store, intent, &p.scope, profile)?;
    if encode(&p)?.len() > 48 * 1024 {
        return Err(Error::Budget.into());
    }
    Ok(p)
}
struct HeldGroup<'a>(provider::Group<'a>);
impl Keepers for HeldGroup<'_> {
    fn revalidate(&mut self) -> Result<()> {
        self.0.revalidate().map_err(Into::into)
    }
    fn fresh(&self) -> Result<()> {
        self.0.fresh().map_err(Into::into)
    }
}
pub fn apply<N: NodeStore, P: Protection>(
    execution: Execution<'_, N, P>,
    plan: &Plan,
    endpoints: &mut [&mut dyn Retainer],
) -> Result<RemovalReceipt> {
    run(execution, plan, endpoints, |_| Ok(()))
}
pub(in crate::local::removal) fn run<N: NodeStore, P: Protection>(
    execution: Execution<'_, N, P>,
    plan: &Plan,
    endpoints: &mut [&mut dyn Retainer],
    hook: impl FnMut(Stage) -> Result<()>,
) -> Result<RemovalReceipt> {
    let profile = plan
        .provider_custody
        .as_ref()
        .ok_or(RemovalError::InvalidPlan)?;
    if plan.custody.is_some() {
        return Err(RemovalError::InvalidPlan);
    }
    run_with(
        execution,
        plan,
        |scope| {
            Ok(Box::new(HeldGroup(provider::acquire(
                profile,
                &scope.store,
                &scope.object,
                endpoints,
            )?)))
        },
        hook,
    )
}
