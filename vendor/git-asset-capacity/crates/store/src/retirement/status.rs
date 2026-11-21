//! Historical operation evidence and separate fresh byte observation. No admission.
use super::*;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    NotStarted,
    Admitted,
    Pending,
    Retired,
    Cancelled,
    Published,
    RecoveryRequired,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Observation {
    VerifiedPresent,
    AuthorizedAbsence,
    Unavailable,
    Busy,
    Unproven,
}
#[derive(Debug, Serialize)]
pub struct Status {
    pub schema: &'static str,
    pub plan: Digest,
    pub phase: Phase,
    pub observed: Observation,
    pub historical_outcome: bool,
    pub removal_authority: bool,
}
fn observe(store: &LocalStore, object: &git_asset_core::Object) -> Observation {
    match store.read_object(object) {
        Ok(_) => Observation::VerifiedPresent,
        Err(crate::Error::AuthorizedAbsence) => Observation::AuthorizedAbsence,
        Err(crate::Error::Busy) => Observation::Busy,
        Err(crate::Error::Io(..)) => Observation::Unavailable,
        Err(_) => Observation::Unproven,
    }
}
pub fn inspect(
    store: &LocalStore,
    ledger: &mut CapacityLedger,
    job: &JournalRef,
    plan: &Plan,
) -> Result<Status> {
    plan.validate(store)?;
    if ledger.identity() != &plan.request.capacity {
        return Err(Error::InvalidPlan);
    }
    let (operation, _) = keys(store, &plan.request.operation, "evict")?;
    let allocation = ledger.allocation_reference(&operation, true)?;
    let binding = lifecycle::digest(&(plan, job))?;
    let settled = match &allocation {
        Some(a) => ledger.retention_status(a, &binding)?.is_some(),
        None => false,
    };
    let intent = store.retirement_intent(&plan.publication.object, plan.birth.index)?;
    let done = store.retirement_done(&plan.publication.object, plan.birth.index)?;
    let aborted = store.retirement_aborted(&plan.publication.object, plan.birth.index)?;
    let (phase, historical) = if let Some(intent) = intent {
        intent.validate(store, &plan.birth)?;
        if lifecycle::digest(&intent.plan)? != plan.id()?
            || intent.job != *job
            || allocation.as_ref() != Some(&intent.allocation)
        {
            return Err(Error::InvalidPlan);
        }
        match (done, aborted) {
            (Some(done), None) => {
                done.validate(&intent)?;
                (Phase::Retired, true)
            }
            (None, Some(aborted)) => {
                aborted.validate(&intent)?;
                (Phase::Cancelled, true)
            }
            (None, None) if !settled => (Phase::Pending, false),
            _ => (Phase::RecoveryRequired, false),
        }
    } else if settled || done.is_some() || aborted.is_some() {
        (Phase::RecoveryRequired, false)
    } else if allocation.is_some() {
        (Phase::Admitted, false)
    } else {
        (Phase::NotStarted, false)
    };
    Ok(Status {
        schema: "tig.retirement-status.design-1",
        plan: plan.id()?,
        phase,
        observed: observe(store, &plan.publication.object),
        historical_outcome: historical,
        removal_authority: false,
    })
}
pub fn inspect_reacquisition(
    store: &LocalStore,
    ledger: &mut CapacityLedger,
    job: &JournalRef,
    plan: &ReacquisitionPlan,
) -> Result<Status> {
    plan.validate(store)?;
    if ledger.identity() != &plan.capacity {
        return Err(Error::InvalidPlan);
    }
    let id = lifecycle::digest(&(store.identity(), &plan.publication.operation))?;
    let operation = Key::parse(&format!("publish-{}", id.as_str()))?;
    let allocation = ledger.allocation_reference(&operation, false)?;
    let (metadata_operation, _) = keys(store, &plan.publication.operation, "reacquire")?;
    let metadata = ledger.allocation_reference(&metadata_operation, true)?;
    let metadata_settled = match metadata {
        Some(a) => ledger
            .retention_status(&a, &lifecycle::digest(&(plan, job))?)?
            .is_some(),
        None => false,
    };
    let settled = match &allocation {
        Some(a) => ledger
            .publication_status(a, &lifecycle::digest(&(&plan.payload, job))?)?
            .is_some(),
        None => false,
    };
    let current = store.completed_receipt(&plan.publication);
    let phase = match (settled, current) {
        (true, _) if !metadata_settled => Phase::RecoveryRequired,
        (true, Ok(Some(_))) => Phase::Published,
        (true, Err(crate::Error::AuthorizedAbsence)) => Phase::Retired,
        (true, _) => Phase::RecoveryRequired,
        (false, Ok(Some(_))) if allocation.is_none() => Phase::RecoveryRequired,
        _ if allocation.is_some() => Phase::Admitted,
        _ => Phase::NotStarted,
    };
    let historical = matches!(phase, Phase::Published | Phase::Retired);
    Ok(Status {
        schema: "tig.reacquisition-status.design-1",
        plan: plan.id()?,
        phase,
        observed: observe(store, &plan.publication.object),
        historical_outcome: historical,
        removal_authority: false,
    })
}
