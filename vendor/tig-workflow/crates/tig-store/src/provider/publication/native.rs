//! Adapter over the existing native accounted publication kernel.
use super::*;
use crate::{
    PublicationBackend, accounted,
    ledger::{CapacityLedger, JournalRef, LedgerError},
    local::LocalStore,
};
pub struct Accounted<'a> {
    store: &'a LocalStore,
    ledger: &'a mut CapacityLedger,
    job: &'a JournalRef,
    plan: &'a accounted::PublicationPlan,
}
impl<'a> Accounted<'a> {
    pub fn new(
        store: &'a LocalStore,
        ledger: &'a mut CapacityLedger,
        job: &'a JournalRef,
        plan: &'a accounted::PublicationPlan,
    ) -> Self {
        Self {
            store,
            ledger,
            job,
            plan,
        }
    }
}
/// IO-free native publication descriptor; shared by preflight and execution.
pub fn capabilities(store: &LocalStore) -> Capabilities {
    let caps = store.capabilities();
    Capabilities {
        place: store.identity().clone(),
        max_object_bytes: caps.max_object_bytes,
        max_ack: caps.max_ack,
        replay: Replay::ExactOperation,
    }
}
impl ExactPublisher for Accounted<'_> {
    type Evidence = accounted::AccountedReceipt;
    fn describe(&self) -> Capabilities {
        capabilities(self.store)
    }
    fn publish(
        &mut self,
        request: &Request,
        source: &mut dyn Read,
    ) -> Result<Published<Self::Evidence>, Error> {
        if request != self.plan.request() {
            return Err(Error::Invalid);
        }
        let evidence =
            accounted::publish_accounted(self.ledger, self.store, self.job, self.plan, source)
                .map_err(accounted_error)?;
        let version = self
            .store
            .completed_version(request)
            .map_err(store_error)?
            .ok_or(Error::Unknown)?;
        Ok(Published {
            place: self.store.identity().clone(),
            request: evidence.publication().request().clone(),
            version: Version::new(version.as_str().as_bytes()).map_err(|_| Error::Integrity)?,
            level: evidence.publication().level(),
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
        crate::Error::Io(kind, _) => match kind {
            std::io::ErrorKind::PermissionDenied => Error::Denied,
            std::io::ErrorKind::NotFound => Error::Unavailable,
            _ => Error::Unknown,
        },
    }
}
fn accounted_error(e: accounted::AccountedError) -> Error {
    match e {
        accounted::AccountedError::Store(e) => store_error(e),
        accounted::AccountedError::Ledger(LedgerError::Store(e)) => store_error(e),
        accounted::AccountedError::Ledger(LedgerError::Busy) => Error::Busy,
        accounted::AccountedError::Ledger(LedgerError::Capacity(_) | LedgerError::Budget) => {
            Error::Budget
        }
        accounted::AccountedError::Ledger(LedgerError::Conflict | LedgerError::Cancelled) => {
            Error::Conflict
        }
        accounted::AccountedError::InvalidPlan => Error::Invalid,
        accounted::AccountedError::MissingSettledPublication => Error::Integrity,
        accounted::AccountedError::FootprintExceedsLimit => Error::Budget,
        _ => Error::Unknown,
    }
}
