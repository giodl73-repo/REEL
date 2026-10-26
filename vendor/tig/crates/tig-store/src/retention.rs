//! Budgeted, immutable local retention. No automatic expiry/release or eviction.
use crate::{
    ledger::{CapacityLedger, JournalRef, LedgerError},
    local::{LocalStore, MAX_SCOPE_FILES},
    replica::{self, VerifiedReplica},
    *,
};
use serde::{Deserialize, Serialize};
use tig_core::{Digest, identity};
pub mod handoff;
pub(crate) const MAX_RETAINERS: usize = 16;
pub(crate) const SCHEMA: &str = "tig.retention.design-1";
#[derive(Debug)]
pub enum RetentionError {
    Store(Error),
    Ledger(LedgerError),
    Replica(replica::ReplicaError),
    InvalidPlan,
    NotSettled,
    Invalidated,
    Released,
    Injected,
}
impl From<Error> for RetentionError {
    fn from(e: Error) -> Self {
        Self::Store(e)
    }
}
impl From<LedgerError> for RetentionError {
    fn from(e: LedgerError) -> Self {
        Self::Ledger(e)
    }
}
impl From<replica::ReplicaError> for RetentionError {
    fn from(e: replica::ReplicaError) -> Self {
        Self::Replica(e)
    }
}
impl std::fmt::Display for RetentionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for RetentionError {}
pub type Result<T> = std::result::Result<T, RetentionError>;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionRequest {
    pub operation: Key,
    pub owner: Key,
    pub publication: Request,
    pub minimum_ack: AckLevel,
}
impl RetentionRequest {
    pub(crate) fn pin_id(&self) -> crate::Result<Digest> {
        let value = serde_json::to_value((&self.owner, &self.operation))
            .map_err(|_| Error::InvalidRequest)?;
        Ok(Digest::blob(
            &identity::canonical_json(&value).map_err(|_| Error::InvalidRequest)?,
        ))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionPlan {
    pub(crate) store: Digest,
    pub(crate) request: RetentionRequest,
    quantum: u64,
    limit: u64,
    basis: String,
}
impl RetentionPlan {
    pub fn required_peak(&self) -> u64 {
        self.limit
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RetentionRecord {
    pub schema: String,
    pub plan: RetentionPlan,
    pub job: JournalRef,
    pub allocation: JournalRef,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionGrant {
    record: RetentionRecord,
    sha256: Digest,
}
impl RetentionGrant {
    pub fn store(&self) -> &Digest {
        &self.record.plan.store
    }
    pub fn request(&self) -> &RetentionRequest {
        &self.record.plan.request
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        encode(self)
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let value = identity::parse_json(bytes).map_err(|_| RetentionError::InvalidPlan)?;
        let grant: Self = serde_json::from_value(value).map_err(|_| RetentionError::InvalidPlan)?;
        if grant.record.schema != SCHEMA || Digest::blob(&encode(&grant.record)?) != grant.sha256 {
            return Err(RetentionError::InvalidPlan);
        }
        Ok(grant)
    }
}
pub struct RetentionReceipt {
    pub grant: RetentionGrant,
    pub settlement: ledger::Receipt,
    pub charged_footprint: u64,
}
pub struct RetainedReplica {
    replica: VerifiedReplica,
    grant: RetentionGrant,
    valid: bool,
}
impl RetainedReplica {
    pub fn grant(&self) -> &RetentionGrant {
        &self.grant
    }
    pub fn revalidate(&mut self, ledger: &mut CapacityLedger, store: &LocalStore) -> Result<()> {
        self.valid = false;
        let new = verify(ledger, store, &self.grant)?;
        *self = new;
        Ok(())
    }
}
#[derive(Serialize)]
pub struct RetentionCoverage {
    basis: &'static str,
    retained_stores: usize,
    declared_independent_domains: usize,
    required_domains: usize,
    retention_established: bool,
    eviction_authority: bool,
}
impl RetentionCoverage {
    pub fn retention_established(&self) -> bool {
        self.retention_established
    }
    pub fn authorizes_eviction(&self) -> bool {
        self.eviction_authority
    }
}
pub fn coverage(
    source: &VerifiedReplica,
    retained: &[&RetainedReplica],
    enrollments: &[replica::ReplicaEnrollment],
    required_domains: usize,
    required_ack: AckLevel,
) -> Result<RetentionCoverage> {
    if retained.len() > MAX_RETAINERS {
        return Err(Error::Budget.into());
    }
    let mut live = Vec::new();
    for replica in retained {
        if !replica.valid {
            return Err(RetentionError::Invalidated);
        }
        live.push(&replica.replica);
    }
    let integrity = replica::coverage(source, &live, enrollments, required_domains, required_ack)?;
    Ok(RetentionCoverage {
        basis: "live-integrity-declared-domains-and-local-retention",
        retained_stores: integrity.retained_stores(),
        declared_independent_domains: integrity.declared_independent_domains(),
        required_domains,
        retention_established: integrity.meets_declared_coverage(),
        eviction_authority: false,
    })
}
pub fn plan(store: &LocalStore, request: &RetentionRequest) -> Result<RetentionPlan> {
    store.capabilities().check(&request.publication)?;
    if request.minimum_ack > request.publication.required {
        return Err(Error::Unsupported.into());
    }
    let quantum = store.allocation_quantum()?;
    let limit = 65_536u64
        .checked_add(quantum - 1)
        .and_then(|n| n.checked_div(quantum))
        .and_then(|n| n.checked_mul(quantum))
        .and_then(|n| n.checked_mul(MAX_SCOPE_FILES as u64 + 2))
        .ok_or(RetentionError::InvalidPlan)?;
    Ok(RetentionPlan {
        store: store.identity().clone(),
        request: request.clone(),
        quantum,
        limit,
        basis: "conservative-allocated-file-data-footprint".into(),
    })
}
fn validate(store: &LocalStore, planned: &RetentionPlan) -> Result<()> {
    if encode(&plan(store, &planned.request)?)? != encode(planned)? {
        return Err(RetentionError::InvalidPlan);
    }
    Ok(())
}
fn keys(planned: &RetentionPlan) -> Result<(Key, Key)> {
    let id = Digest::blob(&encode(&(
        &planned.store,
        &planned.request.owner,
        &planned.request.operation,
    ))?);
    Ok((
        Key::parse(&format!("retain-{}", id.as_str()))?,
        Key::parse(&format!("retained-{}", id.as_str()))?,
    ))
}
pub fn verify(
    ledger: &mut CapacityLedger,
    store: &LocalStore,
    grant: &RetentionGrant,
) -> Result<RetainedReplica> {
    let record = &grant.record;
    validate(store, &record.plan)?;
    if record.schema != SCHEMA || Digest::blob(&encode(record)?) != grant.sha256 {
        return Err(RetentionError::InvalidPlan);
    }
    let binding = Digest::blob(&encode(&(&record.plan, &record.job))?);
    if store.retention_released(record)? {
        return Err(RetentionError::Released);
    }
    if ledger
        .retention_status(&record.allocation, &binding)?
        .is_none()
    {
        return Err(RetentionError::NotSettled);
    }
    if store.read_retention(record)? != grant.sha256 {
        return Err(RetentionError::InvalidPlan);
    }
    let replica = replica::verify(store, &record.plan.request.publication)?;
    if replica.observation().level() < record.plan.request.minimum_ack {
        return Err(Error::Unsupported.into());
    }
    store.read_retention(record)?;
    if store.retention_released(record)? {
        return Err(RetentionError::Released);
    }
    Ok(RetainedReplica {
        replica,
        grant: grant.clone(),
        valid: true,
    })
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Admitted,
    Bound,
    Installed,
    Settled,
}
pub fn retain(
    ledger: &mut CapacityLedger,
    store: &LocalStore,
    job: &JournalRef,
    planned: &RetentionPlan,
) -> Result<RetentionReceipt> {
    retain_hook(ledger, store, job, planned, |_| Ok(()))
}
fn retain_hook(
    ledger: &mut CapacityLedger,
    store: &LocalStore,
    job: &JournalRef,
    planned: &RetentionPlan,
    mut hook: impl FnMut(Stage) -> Result<()>,
) -> Result<RetentionReceipt> {
    validate(store, planned)?;
    let (operation, settlement) = keys(planned)?;
    let binding = Digest::blob(&encode(&(planned, job))?);
    let allocation = ledger
        .begin_retention(
            operation.clone(),
            job,
            planned.request.owner.clone(),
            binding.clone(),
            planned.limit,
        )?
        .reference()
        .clone();
    let record = RetentionRecord {
        schema: SCHEMA.into(),
        plan: planned.clone(),
        job: job.clone(),
        allocation: allocation.clone(),
    };
    let grant = RetentionGrant {
        sha256: Digest::blob(&encode(&record)?),
        record,
    };
    hook(Stage::Admitted)?;
    if let Some(charge) = ledger.retention_status(&allocation, &binding)? {
        let _verified = verify(ledger, store, &grant)?;
        return Ok(RetentionReceipt {
            grant,
            settlement: ledger.settle_retention(settlement, &allocation, binding, charge)?,
            charged_footprint: charge,
        });
    }
    let scope = Request {
        operation,
        owner: planned.request.owner.clone(),
        object: planned.request.publication.object.clone(),
        required: planned.request.publication.required,
    };
    let attempt = store.attempt(&scope)?;
    if ledger.retention_status(&allocation, &binding)?.is_some() {
        return Err(LedgerError::Conflict.into());
    }
    attempt.bind(&grant.record)?;
    hook(Stage::Bound)?;
    let _verified = replica::verify(store, &planned.request.publication)?;
    attempt.install_retention(&grant.record)?;
    hook(Stage::Installed)?;
    let charge = attempt.retention_footprint(&grant.record)?;
    if charge > planned.limit {
        return Err(Error::Budget.into());
    }
    let receipt = ledger.settle_retention(settlement, &allocation, binding, charge)?;
    hook(Stage::Settled)?;
    Ok(RetentionReceipt {
        grant,
        settlement: receipt,
        charged_footprint: charge,
    })
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    identity::canonical_json(&serde_json::to_value(value).map_err(|_| RetentionError::InvalidPlan)?)
        .map_err(|_| RetentionError::InvalidPlan)
}
#[cfg(test)]
#[path = "retention_tests.rs"]
mod tests;
