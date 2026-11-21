//! Input-bound plans and source-preserving local copy. No removal authority.
use crate::{
    AckLevel, Error as StoreError, Key, Request, accounted,
    ledger::{CapacityLedger, JournalRef, LedgerError},
    local::{LocalStore, MAX_OBJECT_BYTES, ObjectRead},
    retention::{self, RetainedReplica, RetentionRequest},
};
use git_asset_core::{Digest, identity};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, io::Read};

pub const MAX_ACTIONS: usize = 32;
const SCHEMA: &str = "tig.storage-plan.design-1";
const LOCAL_CONTRACT: &[u8] = b"tig.local-store.design-5/process-restart/copy-retain/v1";
const PROTECTED_CONTRACT: &[u8] = b"tig.local-store.design-6/process-restart/copy-retain/v1";
const MANAGED_CONTRACT: &[u8] = b"tig.local-store.design-7/process-restart/managed-incarnation/v1";
const FINAL_CONTRACT: &[u8] = b"tig.local-store.design-8/process-restart/final-incarnation/v1";

#[derive(Debug)]
pub enum Error {
    InvalidPlan,
    Budget,
    StaleAdmission,
    WrongStore,
    MissingSource,
    Store(StoreError),
    Ledger(LedgerError),
    Publication(accounted::AccountedError),
    Retention(retention::RetentionError),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<StoreError> for Error {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}
impl From<LedgerError> for Error {
    fn from(value: LedgerError) -> Self {
        Self::Ledger(value)
    }
}
impl From<accounted::AccountedError> for Error {
    fn from(value: accounted::AccountedError) -> Self {
        Self::Publication(value)
    }
}
impl From<retention::RetentionError> for Error {
    fn from(value: retention::RetentionError) -> Self {
        Self::Retention(value)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

/// Caller-admitted enrollment, not physical-domain or credential proof.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    pub instance: Digest,
    pub generation: u64,
    pub contract: Digest,
}
impl Place {
    pub fn local(store: &LocalStore, generation: u64) -> Result<Self> {
        if generation == 0 {
            return Err(Error::InvalidPlan);
        }
        store.check_identity()?;
        if store
            .protection()
            .is_some_and(|binding| binding.generation() != generation)
        {
            return Err(Error::StaleAdmission);
        }
        Ok(Self {
            instance: store.identity().clone(),
            generation,
            contract: Digest::blob(if store.final_policy().is_some() {
                FINAL_CONTRACT
            } else if store.supports_retirement() {
                MANAGED_CONTRACT
            } else if store.protection().is_some() {
                PROTECTED_CONTRACT
            } else {
                LOCAL_CONTRACT
            }),
        })
    }
    fn validate(&self) -> Result<()> {
        if self.generation == 0
            || (self.contract != Digest::blob(LOCAL_CONTRACT)
                && self.contract != Digest::blob(PROTECTED_CONTRACT)
                && self.contract != Digest::blob(MANAGED_CONTRACT)
                && self.contract != Digest::blob(FINAL_CONTRACT))
        {
            return Err(Error::InvalidPlan);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProtectionScope {
    Unknown {
        gaps: Vec<Key>,
    },
    /// A recorded caller assertion only; never produces removal permission.
    DeclaredComplete {
        registry: Digest,
        generation: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Basis {
    pub source: Digest,
    pub policy: Digest,
    pub claims: Digest,
    pub authority: Digest,
    pub protection: ProtectionScope,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub max_objects: usize,
    pub max_payload_bytes: u64,
    /// Conservative additional publication + retention file-data footprint.
    /// Not measured net growth or complete volume accounting.
    pub max_peak_bytes: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Copy {
    pub source: Request,
    pub operation: Key,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRequest {
    pub source: Place,
    pub destination: Place,
    pub capacity: Digest,
    pub owner: Key,
    pub basis: Basis,
    pub budget: Budget,
    pub copies: Vec<Copy>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    schema: String,
    request: PlanRequest,
    id: Digest,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WirePlan {
    schema: String,
    request: PlanRequest,
    id: Digest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Hold {
    IncompleteProtectionScope,
    RemovalExecutorUnavailable,
}

impl Plan {
    pub fn new(mut request: PlanRequest) -> Result<Self> {
        request.source.validate()?;
        request.destination.validate()?;
        if request.source.instance == request.destination.instance {
            return Err(Error::InvalidPlan);
        }
        let budget = &request.budget;
        if budget.max_objects == 0
            || budget.max_objects > MAX_ACTIONS
            || request.copies.is_empty()
            || request.copies.len() > budget.max_objects
            || budget.max_peak_bytes == 0
        {
            return Err(Error::Budget);
        }
        match &request.basis.protection {
            ProtectionScope::Unknown { gaps } if gaps.len() > MAX_ACTIONS => {
                return Err(Error::Budget);
            }
            ProtectionScope::DeclaredComplete { generation: 0, .. } => {
                return Err(Error::InvalidPlan);
            }
            _ => (),
        }
        request
            .copies
            .sort_by(|a, b| a.operation.as_str().cmp(b.operation.as_str()));
        let mut hashes = BTreeSet::new();
        let mut operations = BTreeSet::new();
        let mut payload = 0u64;
        for copy in &request.copies {
            if !operations.insert(copy.operation.as_str())
                || !hashes.insert(&copy.source.object.sha256)
            {
                return Err(Error::InvalidPlan);
            }
            crate::Capabilities {
                max_object_bytes: MAX_OBJECT_BYTES,
                max_ack: AckLevel::ProcessRestart,
            }
            .check(&copy.source)?;
            payload = payload
                .checked_add(copy.source.object.bytes)
                .ok_or(Error::Budget)?;
        }
        if payload > budget.max_payload_bytes {
            return Err(Error::Budget);
        }
        let id = Digest::blob(&encode(&(SCHEMA, &request))?);
        let plan = Self {
            schema: SCHEMA.into(),
            request,
            id,
        };
        plan.to_json()?;
        Ok(plan)
    }
    pub fn id(&self) -> &Digest {
        &self.id
    }
    pub fn request(&self) -> &PlanRequest {
        &self.request
    }
    pub fn holds(&self) -> Vec<Hold> {
        let mut holds = vec![Hold::RemovalExecutorUnavailable];
        if matches!(
            self.request.basis.protection,
            ProtectionScope::Unknown { .. }
        ) {
            holds.push(Hold::IncompleteProtectionScope);
        }
        holds
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        encode(self)
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let value = identity::parse_json(bytes).map_err(|_| Error::InvalidPlan)?;
        let wire: WirePlan = serde_json::from_value(value).map_err(|_| Error::InvalidPlan)?;
        let plan = Self::new(wire.request)?;
        if wire.schema != SCHEMA || wire.id != plan.id || plan.to_json()? != bytes {
            return Err(Error::InvalidPlan);
        }
        Ok(plan)
    }
    fn publication(&self, index: usize) -> Result<Request> {
        let copy = self.request.copies.get(index).ok_or(Error::InvalidPlan)?;
        Ok(Request {
            operation: copy.operation.clone(),
            owner: self.request.owner.clone(),
            object: copy.source.object.clone(),
            required: AckLevel::ProcessRestart,
        })
    }
    fn plans(
        &self,
        store: &LocalStore,
        index: usize,
    ) -> Result<(accounted::PublicationPlan, retention::RetentionPlan)> {
        let request = self.publication(index)?;
        let publication = accounted::plan_bound(store, &request, self.id.clone())?;
        let retention = retention::plan(
            store,
            &RetentionRequest {
                operation: request.operation.clone(),
                owner: request.owner.clone(),
                publication: request,
                minimum_ack: AckLevel::ProcessRestart,
            },
        )?;
        Ok((publication, retention))
    }
    /// Reads provider allocation granularity, but creates no store or ledger work.
    pub fn local_peak(&self, destination: &LocalStore) -> Result<u64> {
        destination.check_identity()?;
        if destination.identity() != &self.request.destination.instance {
            return Err(Error::WrongStore);
        }
        if Place::local(destination, self.request.destination.generation)?
            != self.request.destination
        {
            return Err(Error::StaleAdmission);
        }
        let mut peak = 0u64;
        for index in 0..self.request.copies.len() {
            let (publication, retention) = self.plans(destination, index)?;
            peak = peak
                .checked_add(publication.required_peak())
                .and_then(|n| n.checked_add(retention.required_peak()))
                .ok_or(Error::Budget)?;
        }
        if peak > self.request.budget.max_peak_bytes {
            return Err(Error::Budget);
        }
        Ok(peak)
    }
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    identity::canonical_json(&serde_json::to_value(value).map_err(|_| Error::InvalidPlan)?)
        .map_err(|_| Error::InvalidPlan)
}

/// A trusted caller supplies its CURRENT admitted configuration, independently
/// of deserialized plan input. Equality checks do not authenticate that caller.
pub struct LocalExecution<'a> {
    pub source: &'a LocalStore,
    pub destination: &'a LocalStore,
    pub ledger: &'a mut CapacityLedger,
    pub admitted_source: &'a Place,
    pub admitted_destination: &'a Place,
    pub admitted_basis: &'a Basis,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    NotStarted,
    Partial,
    Retained,
}
pub struct Status {
    pub phase: Phase,
    /// Live verified protection only, not reconstructible from JSON status.
    pub retained: Option<RetainedReplica>,
}
pub struct CopyReceipt {
    pub plan: Digest,
    pub operation: Key,
    pub publication_footprint: u64,
    pub retention_footprint: u64,
    pub retained: RetainedReplica,
}
impl LocalExecution<'_> {
    fn validate(&mut self, plan: &Plan) -> Result<()> {
        if self.admitted_source != &plan.request.source
            || self.admitted_destination != &plan.request.destination
            || self.admitted_basis != &plan.request.basis
        {
            return Err(Error::StaleAdmission);
        }
        self.source.check_identity()?;
        self.destination.check_identity()?;
        self.ledger.check_history()?;
        if self.source.identity() != &plan.request.source.instance
            || self.destination.identity() != &plan.request.destination.instance
            || self.ledger.identity() != &plan.request.capacity
        {
            return Err(Error::WrongStore);
        }
        if Place::local(self.source, plan.request.source.generation)? != plan.request.source
            || Place::local(self.destination, plan.request.destination.generation)?
                != plan.request.destination
        {
            return Err(Error::StaleAdmission);
        }
        plan.local_peak(self.destination)?;
        Ok(())
    }
    /// Exact-action inspection with fresh retained-byte verification, no repair.
    pub fn status(&mut self, plan: &Plan, index: usize, job: &JournalRef) -> Result<Status> {
        self.validate(plan)?;
        let request = plan.publication(index)?;
        let (publication, retention) = plan.plans(self.destination, index)?;
        let id = Digest::blob(&encode(&(self.destination.identity(), &request.operation))?);
        let op = Key::parse(&format!("publish-{}", id.as_str()))?;
        let Some(allocation) = self.ledger.allocation_reference(&op, false)? else {
            return Ok(Status {
                phase: Phase::NotStarted,
                retained: None,
            });
        };
        let binding = Digest::blob(&encode(&(&publication, job))?);
        let settled = self
            .ledger
            .publication_status(&allocation, &binding)?
            .is_some();
        if !settled {
            return Ok(Status {
                phase: Phase::Partial,
                retained: None,
            });
        }
        // A settled publication may never be interpreted as an ordinary miss.
        self.destination
            .completed_receipt(&request)?
            .ok_or(Error::Store(StoreError::MissingAcknowledgedObject))?;
        let retained = retention::inspect(self.ledger, self.destination, job, &retention)?;
        Ok(Status {
            phase: if retained.is_some() {
                Phase::Retained
            } else {
                Phase::Partial
            },
            retained,
        })
    }
    /// Resume one action. Caller chooses order/cancellation and retains its job.
    /// Publication/retention replay is exact; no deletion or automatic cleanup.
    pub fn apply(&mut self, plan: &Plan, index: usize, job: &JournalRef) -> Result<CopyReceipt> {
        self.apply_hook(plan, index, job, |_| Ok(()))
    }
    fn apply_hook(
        &mut self,
        plan: &Plan,
        index: usize,
        job: &JournalRef,
        mut hook: impl FnMut(Phase) -> Result<()>,
    ) -> Result<CopyReceipt> {
        self.validate(plan)?;
        let (publication, retention_plan) = plan.plans(self.destination, index)?;
        // Guard the exact enrolled completed source through copying/retention.
        let source_request = &plan.request.copies[index].source;
        let mut reader = SourceReader {
            store: self.source,
            request: source_request,
            reader: None,
        };
        let publication = accounted::publish_accounted(
            self.ledger,
            self.destination,
            job,
            &publication,
            &mut reader as &mut dyn Read,
        )?;
        hook(Phase::Partial)?;
        let retention = retention::retain(self.ledger, self.destination, job, &retention_plan)?;
        let retained = retention::verify(self.ledger, self.destination, &retention.grant)?;
        hook(Phase::Retained)?;
        Ok(CopyReceipt {
            plan: plan.id.clone(),
            operation: plan.request.copies[index].operation.clone(),
            publication_footprint: publication.charged_footprint(),
            retention_footprint: retention.charged_footprint,
            retained,
        })
    }
}

// Open/verify source only if publication recovery actually needs bytes. A lost
// completion reply can be recovered from a retained destination without rereading
// a now unavailable source. Once opened, its guard spans publication/retention.
struct SourceReader<'a> {
    store: &'a LocalStore,
    request: &'a Request,
    reader: Option<ObjectRead>,
}
impl Read for SourceReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.reader.is_none() {
            self.reader = Some(
                self.store
                    .completed_reader(self.request)
                    .map_err(std::io::Error::other)?
                    .ok_or_else(|| std::io::Error::other("missing admitted source completion"))?
                    .1,
            );
        }
        self.reader
            .as_mut()
            .expect("initialized source reader")
            .read(buffer)
    }
}

#[cfg(test)]
mod tests;
