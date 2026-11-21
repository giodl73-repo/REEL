//! Existing native retention/accounting kernel, without a new claim store.
use super::*;
use crate::{
    ledger::{CapacityLedger, JournalRef, LedgerError},
    local::LocalStore,
    retention::{self, RetentionError, RetentionPlan},
};
fn bound(plan: &RetentionPlan, version: Version) -> Request {
    Request {
        operation: plan.request.operation.clone(),
        owner: plan.request.owner.clone(),
        place: plan.store.clone(),
        object: plan.request.publication.object.clone(),
        version,
        required: plan.request.minimum_ack,
    }
}
/// Explicit bounded completion-metadata lookup, with no payload hash/read or census.
pub fn request(store: &LocalStore, plan: &RetentionPlan) -> Result<Request, Error> {
    if store.identity() != &plan.store {
        return Err(Error::Stale);
    }
    let version = store
        .completed_version(&plan.request.publication)
        .map_err(store_error)?
        .ok_or(Error::Unknown)?;
    Ok(bound(
        plan,
        Version::new(version.as_str().as_bytes()).map_err(|_| Error::Integrity)?,
    ))
}
pub struct Native<'a> {
    pub store: &'a LocalStore,
    pub ledger: &'a mut CapacityLedger,
    pub job: &'a JournalRef,
    pub plan: &'a RetentionPlan,
}
/// Same IO-free descriptor used by native issuance and preflight.
pub fn capabilities(store: &LocalStore) -> Capabilities {
    Capabilities {
        place: store.identity().clone(),
        max_object_bytes: store.max_object_bytes(),
        max_ack: AckLevel::ProcessRestart,
        exact_replay: true,
        enforcement: Some(Enforcement::Cooperative),
    }
}
impl Issuer for Native<'_> {
    type Evidence = retention::RetentionReceipt;
    fn describe(&self) -> Capabilities {
        capabilities(self.store)
    }
    fn issue(&mut self, r: &Request) -> Result<Issued<Self::Evidence>, Error> {
        if *r != bound(self.plan, r.version.clone()) {
            return Err(Error::Invalid);
        }
        if *r != request(self.store, self.plan)? {
            return Err(Error::Stale);
        }
        let evidence = retention::retain(self.ledger, self.store, self.job, self.plan)
            .map_err(retention_error)?;
        // Postcheck failure may follow installed retention. Keep original retry identity.
        if *r != request(self.store, self.plan)? {
            return Err(Error::Stale);
        }
        Ok(Issued {
            request: r.clone(),
            commitment: super::super::native::commitment(&evidence.grant),
            evidence,
        })
    }
}
fn store_error(e: crate::Error) -> Error {
    match e {
        crate::Error::InvalidRequest => Error::Invalid,
        crate::Error::InvalidRoot => Error::Stale,
        crate::Error::Unsupported => Error::Unsupported,
        crate::Error::Budget => Error::Budget,
        crate::Error::OperationConflict => Error::Conflict,
        crate::Error::Integrity
        | crate::Error::CorruptRecord
        | crate::Error::MissingAcknowledgedObject => Error::Integrity,
        crate::Error::Busy | crate::Error::Retained => Error::Busy,
        crate::Error::Io(std::io::ErrorKind::PermissionDenied, _) => Error::Denied,
        crate::Error::Io(std::io::ErrorKind::NotFound, _) => Error::Unavailable,
        _ => Error::Unknown,
    }
}
fn retention_error(e: RetentionError) -> Error {
    match e {
        RetentionError::Store(e) => store_error(e),
        RetentionError::Replica(crate::replica::ReplicaError::Store(e)) => store_error(e),
        RetentionError::Ledger(LedgerError::Store(e)) => store_error(e),
        RetentionError::Ledger(LedgerError::Busy) => Error::Busy,
        RetentionError::Ledger(LedgerError::Capacity(_) | LedgerError::Budget) => Error::Budget,
        RetentionError::Ledger(LedgerError::Conflict | LedgerError::Cancelled) => Error::Conflict,
        RetentionError::Released | RetentionError::Invalidated => Error::Stale,
        RetentionError::InvalidPlan => Error::Invalid,
        _ => Error::Unknown,
    }
}
