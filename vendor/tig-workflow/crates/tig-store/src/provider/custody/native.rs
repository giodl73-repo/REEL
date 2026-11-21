//! Existing native retention/ledger verification behind the portable hold port.
use super::*;
use crate::{
    ledger::CapacityLedger,
    local::LocalStore,
    retention::{self, RetainedReplica, RetentionError, RetentionGrant},
};
pub fn commitment(grant: &RetentionGrant) -> Commitment {
    Commitment {
        place: grant.store().clone(),
        object: grant.request().publication.object.clone(),
        claim: grant.claim_id().clone(),
        owner: grant.request().owner.clone(),
        level: grant.request().publication.required,
    }
}
pub struct Native<'a> {
    pub store: &'a LocalStore,
    pub ledger: &'a mut CapacityLedger,
    pub grant: &'a RetentionGrant,
}
struct Guard<'a> {
    store: &'a LocalStore,
    ledger: &'a mut CapacityLedger,
    replica: RetainedReplica,
    commitment: Commitment,
}
impl Retainer for Native<'_> {
    fn describe(&self) -> Capabilities {
        capabilities(self.store)
    }
    fn acquire(&mut self, wanted: &Commitment) -> Result<Box<dyn Held + '_>, SourceError> {
        hold(self.store, self.ledger, self.grant, wanted)
    }
}
/// IO-free descriptor shared by native bindings and their profile preflight.
pub fn capabilities(store: &LocalStore) -> Capabilities {
    Capabilities {
        place: store.identity().clone(),
        max_object_bytes: store.max_object_bytes(),
        max_ack: AckLevel::ProcessRestart,
        enforcement: Some(Enforcement::Cooperative),
    }
}
/// Same native verification and effective guard for lazily opened native bindings.
/// The caller owns the store/ledger lifetimes; no new retention or authority is
/// established by constructing a binding or by calling this helper.
pub fn hold<'a>(
    store: &'a LocalStore,
    ledger: &'a mut CapacityLedger,
    grant: &RetentionGrant,
    wanted: &Commitment,
) -> Result<Box<dyn Held + 'a>, SourceError> {
    if commitment(grant) != *wanted || store.identity() != &wanted.place {
        return Err(SourceError::Stale);
    }
    let replica = retention::verify(ledger, store, grant).map_err(error)?;
    Ok(Box::new(Guard {
        store,
        ledger,
        replica,
        commitment: wanted.clone(),
    }))
}
impl Held for Guard<'_> {
    fn commitment(&self) -> &Commitment {
        &self.commitment
    }
    fn revalidate(&mut self) -> Result<(), SourceError> {
        self.replica
            .revalidate(self.ledger, self.store)
            .map_err(error)
    }
    fn fresh(&self) -> Result<(), SourceError> {
        self.replica.fresh().map_err(error)
    }
}
fn error(e: RetentionError) -> SourceError {
    match e {
        RetentionError::Store(e) => crate::acquire::native_error(e),
        RetentionError::Replica(crate::replica::ReplicaError::Store(e)) => {
            crate::acquire::native_error(e)
        }
        RetentionError::InvalidPlan => SourceError::Invalid,
        RetentionError::Invalidated | RetentionError::Released => SourceError::Stale,
        _ => SourceError::Unknown,
    }
}
