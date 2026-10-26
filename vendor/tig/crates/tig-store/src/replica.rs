//! Live byte verification and declared-domain coverage, not retention/eviction.
use crate::{
    AckLevel, Capabilities, Error, Key, Request,
    local::{LocalStore, MAX_OBJECT_BYTES, ObjectRead},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read},
    time::{Duration, Instant},
};
use tig_core::{Digest, identity};

const FRESH_FOR: Duration = Duration::from_secs(5);
const MAX_REPLICAS: usize = 16;
const SCHEMA: &str = "tig.replica-observation.design-1";
#[derive(Debug)]
pub enum ReplicaError {
    Store(Error),
    MissingCompletion,
    InvalidRecord,
    WrongStore,
    Stale,
    Invalidated,
    Budget,
    DuplicateStore,
    MissingEnrollment,
    ConflictingEnrollment,
    ObjectMismatch,
    InsufficientAcknowledgement,
}
impl From<Error> for ReplicaError {
    fn from(value: Error) -> Self {
        Self::Store(value)
    }
}
impl std::fmt::Display for ReplicaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ReplicaError {}
pub type Result<T> = std::result::Result<T, ReplicaError>;

/// Portable historical evidence only. Deserialization never creates a live guard.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplicaObservation {
    schema: String,
    store: Digest,
    publication: Request,
    level: AckLevel,
    observed_unix_ms: u64,
}
impl ReplicaObservation {
    pub fn store(&self) -> &Digest {
        &self.store
    }
    pub fn publication(&self) -> &Request {
        &self.publication
    }
    pub fn level(&self) -> AckLevel {
        self.level
    }
    /// A saved observation can request a NEW verification of its exact store.
    /// Its wall-clock timestamp never restores process-local freshness.
    pub fn verify_current(&self, store: &LocalStore) -> Result<VerifiedReplica> {
        if self.schema != SCHEMA || self.level != self.publication.required {
            return Err(ReplicaError::InvalidRecord);
        }
        if store.identity() != &self.store {
            return Err(ReplicaError::WrongStore);
        }
        verify(store, &self.publication)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        identity::canonical_json(
            &serde_json::to_value(self).map_err(|_| ReplicaError::InvalidRecord)?,
        )
        .map_err(|_| ReplicaError::InvalidRecord)
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let value = identity::parse_json(bytes).map_err(|_| ReplicaError::InvalidRecord)?;
        let result: Self =
            serde_json::from_value(value).map_err(|_| ReplicaError::InvalidRecord)?;
        if result.schema != SCHEMA || result.level != result.publication.required {
            return Err(ReplicaError::InvalidRecord);
        }
        Capabilities {
            max_object_bytes: MAX_OBJECT_BYTES,
            max_ack: AckLevel::ProcessRestart,
        }
        .check(&result.publication)?;
        Ok(result)
    }
}
/// Not Clone/Serialize: the process-local guard and clock are essential evidence.
pub struct VerifiedReplica {
    observation: ReplicaObservation,
    reader: ObjectRead,
    checked: Instant,
    valid: bool,
}
impl VerifiedReplica {
    pub fn observation(&self) -> &ReplicaObservation {
        &self.observation
    }
    fn fresh(&self) -> Result<()> {
        if !self.valid {
            return Err(ReplicaError::Invalidated);
        }
        if self.checked.elapsed() > FRESH_FOR {
            return Err(ReplicaError::Stale);
        }
        Ok(())
    }
    /// Preserve the previous guard if revalidation fails. No automatic expiry unlock.
    pub fn revalidate(&mut self, store: &LocalStore) -> Result<()> {
        self.valid = false;
        if store.identity() != &self.observation.store {
            return Err(ReplicaError::WrongStore);
        }
        let fresh = verify(store, &self.observation.publication)?;
        *self = fresh;
        Ok(())
    }
}
impl Read for VerifiedReplica {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.reader.read(bytes)
    }
}
/// Exact completed publication and actual byte verification, with a retained guard.
pub fn verify(store: &LocalStore, publication: &Request) -> Result<VerifiedReplica> {
    store.capabilities().check(publication)?;
    let start = Instant::now();
    let observed_unix_ms = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| ReplicaError::InvalidRecord)?
            .as_millis(),
    )
    .map_err(|_| ReplicaError::InvalidRecord)?;
    let (receipt, reader) = store
        .completed_reader(publication)?
        .ok_or(ReplicaError::MissingCompletion)?;
    store.check_identity()?;
    if start.elapsed() > FRESH_FOR {
        return Err(ReplicaError::Stale);
    }
    Ok(VerifiedReplica {
        observation: ReplicaObservation {
            schema: SCHEMA.into(),
            store: store.identity().clone(),
            publication: publication.clone(),
            level: receipt.level(),
            observed_unix_ms,
        },
        reader,
        checked: start,
        valid: true,
    })
}

/// Explicit trusted mapping, not automatic proof of native failure independence.
#[derive(Clone, Debug)]
pub struct ReplicaEnrollment {
    pub store: Digest,
    pub failure_domain: Key,
}
/// Historical summary only; carries no retention commitment or removal authority.
#[derive(Debug, Serialize)]
pub struct ReplicaCoverage {
    basis: &'static str,
    retention_established: bool,
    eviction_authority: bool,
    retained_stores: usize,
    declared_independent_domains: usize,
    required_domains: usize,
}
impl ReplicaCoverage {
    pub fn retained_stores(&self) -> usize {
        self.retained_stores
    }
    pub fn declared_independent_domains(&self) -> usize {
        self.declared_independent_domains
    }
    pub fn meets_declared_coverage(&self) -> bool {
        self.declared_independent_domains >= self.required_domains
    }
    pub fn retention_established(&self) -> bool {
        self.retention_established
    }
    pub fn authorizes_eviction(&self) -> bool {
        self.eviction_authority
    }
}
pub fn coverage(
    source: &VerifiedReplica,
    retained: &[&VerifiedReplica],
    enrollments: &[ReplicaEnrollment],
    required_domains: usize,
    required_ack: AckLevel,
) -> Result<ReplicaCoverage> {
    if retained.len() > MAX_REPLICAS
        || enrollments.len() > MAX_REPLICAS + 1
        || required_domains == 0
        || required_domains > MAX_REPLICAS
    {
        return Err(ReplicaError::Budget);
    }
    source.fresh()?;
    let mut mapping = BTreeMap::new();
    for enrollment in enrollments {
        if mapping
            .insert(&enrollment.store, &enrollment.failure_domain)
            .is_some()
        {
            return Err(ReplicaError::ConflictingEnrollment);
        }
    }
    let source_domain = mapping
        .get(&source.observation.store)
        .ok_or(ReplicaError::MissingEnrollment)?;
    let mut stores = BTreeSet::from([&source.observation.store]);
    let mut domains = BTreeSet::new();
    for replica in retained {
        replica.fresh()?;
        if replica.observation.publication.object != source.observation.publication.object {
            return Err(ReplicaError::ObjectMismatch);
        }
        if replica.observation.level < required_ack {
            return Err(ReplicaError::InsufficientAcknowledgement);
        }
        if !stores.insert(&replica.observation.store) {
            return Err(ReplicaError::DuplicateStore);
        }
        let domain = mapping
            .get(&replica.observation.store)
            .ok_or(ReplicaError::MissingEnrollment)?;
        if domain != source_domain {
            domains.insert(domain.as_str());
        }
    }
    Ok(ReplicaCoverage {
        basis: "live-integrity-and-declared-domain-coverage",
        retention_established: false,
        eviction_authority: false,
        retained_stores: retained.len(),
        declared_independent_domains: domains.len(),
        required_domains,
    })
}

// Required to call the explicit provider capability port; no provider setup.
use crate::PublicationBackend;
#[cfg(test)]
#[path = "replica_tests.rs"]
mod tests;
