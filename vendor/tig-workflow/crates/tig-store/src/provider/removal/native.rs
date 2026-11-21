//! Bound adapters reusing all original native removal/authority/custody checks.
use super::*;
use crate::{
    local::{LocalStore, removal as local},
    provider::custody::Retainer,
};
use tig_core::tree::NodeStore;
pub fn request(plan: &local::Plan) -> Result<Request, Error> {
    let s = plan.scope();
    Ok(Request {
        operation: s.operation.clone(),
        decision: s.decision.clone(),
        owner: s.owner.clone(),
        attempt: s.attempt,
        target: Target {
            place: s.store.clone(),
            object: s.object.clone(),
            version: CopyVersion::new(s.incarnation.as_str().as_bytes())?,
        },
        preservation: s.preservation.clone(),
        required: AckLevel::ProcessRestart,
    })
}
/// IO-free native descriptor; original kernel enforcement is unchanged.
pub fn capabilities(store: &LocalStore) -> Capabilities {
    Capabilities {
        place: store.identity().clone(),
        max_object_bytes: store.max_object_bytes(),
        max_ack: AckLevel::ProcessRestart,
        enforcement: if store.removal_enabled() {
            Enforcement::CooperativeIncarnation
        } else {
            Enforcement::Unsupported
        },
    }
}
fn reborrow<'a, N: NodeStore, P: local::Protection>(
    e: &'a mut local::Execution<'_, N, P>,
) -> local::Execution<'a, N, P> {
    local::Execution {
        store: e.store,
        ledger: &mut *e.ledger,
        job: e.job,
        journal: &mut *e.journal,
        nodes: e.nodes,
        intent: e.intent,
        protection: &mut *e.protection,
    }
}
struct Bound<F> {
    profile: Capabilities,
    wanted: Request,
    run: F,
}
impl<F: FnMut() -> local::Result<local::RemovalReceipt>> ExactRemover for Bound<F> {
    type Evidence = local::RemovalReceipt;
    fn describe(&self) -> Capabilities {
        self.profile.clone()
    }
    fn remove(&mut self, r: &Request) -> Result<Removed<Self::Evidence>, Error> {
        if *r != self.wanted {
            return Err(Error::Invalid);
        }
        let evidence = (self.run)().map_err(native_error)?;
        Ok(Removed {
            request: r.clone(),
            level: AckLevel::ProcessRestart,
            evidence,
        })
    }
}
pub fn final_loss<'a, N: NodeStore + 'a, P: local::Protection + 'a>(
    mut e: local::Execution<'a, N, P>,
    plan: &'a local::Plan,
) -> Result<impl ExactRemover<Evidence = local::RemovalReceipt> + 'a, Error> {
    let profile = capabilities(e.store);
    Ok(Bound {
        profile,
        wanted: request(plan)?,
        run: move || local::apply(reborrow(&mut e), plan),
    })
}
pub fn retained<'a, 'b: 'a, N: NodeStore + 'a, P: local::Protection + 'a>(
    mut e: local::Execution<'a, N, P>,
    plan: &'a local::Plan,
    endpoints: &'a mut [&'b mut dyn Retainer],
) -> Result<impl ExactRemover<Evidence = local::RemovalReceipt> + 'a, Error> {
    let profile = capabilities(e.store);
    Ok(Bound {
        profile,
        wanted: request(plan)?,
        run: move || local::custody::portable::apply(reborrow(&mut e), plan, endpoints),
    })
}
pub fn retained_native<'a, 'b: 'a, N: NodeStore + 'a, P: local::Protection + 'a>(
    mut e: local::Execution<'a, N, P>,
    plan: &'a local::Plan,
    endpoints: &'a mut [local::custody::Endpoint<'b>],
) -> Result<impl ExactRemover<Evidence = local::RemovalReceipt> + 'a, Error> {
    let profile = capabilities(e.store);
    Ok(Bound {
        profile,
        wanted: request(plan)?,
        run: move || local::custody::apply(reborrow(&mut e), plan, endpoints),
    })
}
fn native_error(e: local::RemovalError) -> Error {
    match e {
        local::RemovalError::Store(crate::Error::Busy) => Error::Busy,
        local::RemovalError::Store(crate::Error::Retained) => Error::Protected,
        local::RemovalError::Store(crate::Error::Budget) => Error::Budget,
        local::RemovalError::Store(crate::Error::OperationConflict) => Error::Conflict,
        local::RemovalError::Store(crate::Error::InvalidRoot) => Error::Stale,
        local::RemovalError::Store(
            crate::Error::Integrity
            | crate::Error::CorruptRecord
            | crate::Error::MissingAcknowledgedObject,
        ) => Error::Integrity,
        local::RemovalError::Store(crate::Error::Unsupported) => Error::Unsupported,
        local::RemovalError::Ledger(crate::ledger::LedgerError::Busy) => Error::Busy,
        local::RemovalError::Ledger(
            crate::ledger::LedgerError::Budget | crate::ledger::LedgerError::Capacity(_),
        ) => Error::Budget,
        local::RemovalError::InvalidPlan => Error::Invalid,
        local::RemovalError::NotAdmitted => Error::Protected,
        _ => Error::Unknown,
    }
}
