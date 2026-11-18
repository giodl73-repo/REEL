//! Optional one-publication reservation; legacy general-purpose jobs stay unchanged.
use super::*;

/// Reserve peak for this exact plan under a stable job operation. Requires the
/// original fresh-observation admission unless replaying identical saved work.
/// This is not an owner authentication, new observation or publication receipt.
pub fn reserve_publication(
    ledger: &mut CapacityLedger,
    operation: Key,
    plan: &PublicationPlan,
) -> Result<ledger::Receipt> {
    let reference = ledger.proposed_reference(operation.clone());
    let binding = publication_binding(plan, &reference)?;
    Ok(ledger.reserve_publication(operation, plan.request.owner.clone(), plan.limit, binding)?)
}

#[cfg(test)]
mod tests;
