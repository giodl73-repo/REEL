//! Read-only historical effects and separate unverified current presence.
use super::*;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    NotStarted,
    Admitted,
    Intent,
    Released,
    Deleted,
    Cancelled,
    RecoveryRequired,
}
#[derive(Debug, Serialize)]
pub struct Status {
    pub schema: &'static str,
    pub plan: Digest,
    pub phase: Phase,
    pub observed: Observation,
    pub grants_released: bool,
    pub charged_metadata: Option<u64>,
    pub removal_authority: bool,
}
/// Synchronizes the ledger but adds no commands, repairs or provider records.
/// This observation does not authenticate current authority or claim absence.
pub fn inspect(
    store: &LocalStore,
    ledger: &mut CapacityLedger,
    job: &JournalRef,
    plan: &Plan,
) -> Result<Status> {
    plan.validate(store)?;
    if ledger.identity() != &plan.request.capacity {
        return Err(Error::WrongAuthority);
    }
    let (operation, _) = keys(plan)?;
    let allocation = ledger.allocation_reference(&operation, true)?;
    let binding = digest(&("tig.final-allocation.design-1", plan, job))?;
    let charged_metadata = match &allocation {
        Some(a) => ledger.retention_status(a, &binding)?,
        None => None,
    };
    let state = store.load_final_state(&plan.birth)?;
    let phase = if let Some(s) = &state {
        if s.intent.job != *job
            || Some(&s.intent.allocation) != allocation.as_ref()
            || digest(&s.intent.plan)? != plan.id()?
        {
            return Err(Error::WrongAuthority);
        }
        if s.retired() {
            Phase::Deleted
        } else if s.cancelled() {
            Phase::Cancelled
        } else if charged_metadata.is_some() {
            Phase::RecoveryRequired
        } else if s.release.is_some() {
            Phase::Released
        } else {
            Phase::Intent
        }
    } else if charged_metadata.is_some() {
        Phase::RecoveryRequired
    } else if allocation.is_some() {
        Phase::Admitted
    } else {
        Phase::NotStarted
    };
    Ok(Status {
        schema: "tig.final-retirement-status.design-1",
        plan: plan.id()?,
        phase,
        observed: observe(store, &plan.birth.object),
        grants_released: state.is_some_and(|s| s.release.is_some()),
        charged_metadata,
        removal_authority: false,
    })
}
