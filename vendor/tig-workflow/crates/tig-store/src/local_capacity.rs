//! Explicitly enrolled local sampling. No automatic volume/quota discovery.
use crate::{
    Key,
    ledger::{CapacityLedger, LedgerError, Receipt},
    local::LocalStore,
};
use tig_core::Digest;

#[derive(Debug)]
pub enum ProbeError {
    Store(crate::Error),
    Ledger(LedgerError),
    WrongEnrollment,
    InvalidSample,
}
impl From<crate::Error> for ProbeError {
    fn from(value: crate::Error) -> Self {
        Self::Store(value)
    }
}
impl From<LedgerError> for ProbeError {
    fn from(value: LedgerError) -> Self {
        Self::Ledger(value)
    }
}
impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ProbeError {}
pub type Result<T> = std::result::Result<T, ProbeError>;

/// User-available capacity, not privileged free blocks or a physical reservation.
#[derive(Clone, Debug)]
pub struct SpaceSample {
    pub available: u64,
    pub total: u64,
    pub allocation_granularity: u64,
}

/// Trusted explicit assertion of a store-to-accounting-domain mapping. Multiple
/// aliases/places sharing capacity MUST enroll against the same ledger authority.
#[derive(Clone, Debug)]
pub struct LocalCapacityProbe {
    store: Digest,
    ledger: Digest,
}
#[derive(Debug)]
pub struct CapacityObservation {
    store: Digest,
    ledger: Digest,
    sample: SpaceSample,
    receipt: Receipt,
}
impl CapacityObservation {
    pub fn store(&self) -> &Digest {
        &self.store
    }
    pub fn ledger(&self) -> &Digest {
        &self.ledger
    }
    pub fn sample(&self) -> &SpaceSample {
        &self.sample
    }
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
}
impl LocalCapacityProbe {
    /// Caller admits the capacity mapping. Does not prove native volume identity
    /// or create roots/bindings; only validates already enrolled local instances.
    pub fn bind(store: &LocalStore, ledger: &mut CapacityLedger) -> Result<Self> {
        store.check_identity()?;
        ledger.check_history()?;
        Ok(Self {
            store: store.identity().clone(),
            ledger: ledger.identity().clone(),
        })
    }
    pub fn observe(
        &self,
        store: &LocalStore,
        ledger: &mut CapacityLedger,
        probe: Key,
        observation: Key,
    ) -> Result<CapacityObservation> {
        self.observe_with(store, ledger, probe, observation, || {
            Ok(store.sample_space()?)
        })
    }
    fn observe_with(
        &self,
        store: &LocalStore,
        ledger: &mut CapacityLedger,
        probe: Key,
        observation: Key,
        sample: impl FnOnce() -> Result<SpaceSample>,
    ) -> Result<CapacityObservation> {
        if store.identity() != &self.store || ledger.identity() != &self.ledger {
            return Err(ProbeError::WrongEnrollment);
        }
        store.check_identity()?;
        let ticket = ledger.begin_fresh_probe(probe)?;
        let sample = sample()?;
        store.check_identity()?;
        if sample.available > sample.total || sample.allocation_granularity == 0 {
            return Err(ProbeError::InvalidSample);
        }
        let receipt = ledger.observe(observation, ticket.reference(), sample.available)?;
        // A fresh ticket is part of the immutable observe command, so an old
        // observation ID cannot match this new command and silently replay it.
        if receipt.was_replayed() {
            return Err(LedgerError::NeedsProbe.into());
        }
        Ok(CapacityObservation {
            store: self.store.clone(),
            ledger: self.ledger.clone(),
            sample,
            receipt,
        })
    }
}

#[cfg(test)]
#[path = "local_capacity_tests.rs"]
mod tests;
