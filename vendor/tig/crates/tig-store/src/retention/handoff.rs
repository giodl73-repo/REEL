//! Exact owner-authorized claim handoff. Payload bytes are never removed here.
use super::{RetentionRecord, *};
use crate::local::LocalStore;
const SCHEMA: &str = "tig.retention-handoff.design-1";
#[derive(Debug)]
pub enum HandoffError {
    Retention(RetentionError),
    WrongAuthorization,
    InvalidPlan,
    AlreadyReleased,
    Injected,
}
impl From<RetentionError> for HandoffError {
    fn from(e: RetentionError) -> Self {
        Self::Retention(e)
    }
}
impl From<Error> for HandoffError {
    fn from(e: Error) -> Self {
        Self::Retention(e.into())
    }
}
impl From<LedgerError> for HandoffError {
    fn from(e: LedgerError) -> Self {
        Self::Retention(e.into())
    }
}
impl std::fmt::Display for HandoffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for HandoffError {}
pub type Result<T> = std::result::Result<T, HandoffError>;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffRequest {
    pub operation: Key,
    pub owner: Key,
    pub previous: RetentionGrant,
    pub successor: RetentionGrant,
    pub source_domain: Key,
    pub successor_domain: Key,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffPlan {
    store: Digest,
    request: HandoffRequest,
    limit: u64,
    quantum: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffAuthorization {
    reviewed: Digest,
    owner: Key,
}
impl HandoffPlan {
    pub fn required_peak(&self) -> u64 {
        self.limit
    }
    pub fn request(&self) -> &HandoffRequest {
        &self.request
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        encode(self)
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let value = identity::parse_json(bytes).map_err(|_| HandoffError::InvalidPlan)?;
        let p: Self = serde_json::from_value(value).map_err(|_| HandoffError::InvalidPlan)?;
        validate_request(&p.request)?;
        Ok(p)
    }
    /// Explicit trusted owner/application decision; never inferred from age/labels.
    pub fn authorize_handoff(&self) -> Result<HandoffAuthorization> {
        validate_request(&self.request)?;
        Ok(HandoffAuthorization {
            reviewed: Digest::blob(&encode(self)?),
            owner: self.request.owner.clone(),
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HandoffRecord {
    schema: String,
    plan: HandoffPlan,
    job: JournalRef,
    allocation: JournalRef,
}
impl HandoffRecord {
    pub(crate) fn previous(&self) -> &RetentionRecord {
        &self.plan.request.previous.record
    }
    pub(crate) fn matches_previous(&self, record: &RetentionRecord, store: &Digest) -> bool {
        self.schema == SCHEMA
            && &self.plan.store == store
            && validate_request(&self.plan.request).is_ok()
            && matches!((encode(self.previous()),encode(record)),(Ok(a),Ok(b)) if a==b)
    }
    pub(crate) fn same(&self, other: &Self) -> bool {
        match (encode(self), encode(other)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }
}
pub struct HandoffReceipt {
    pub settlement: ledger::Receipt,
    pub marker: Digest,
    pub charged_footprint: u64,
}
fn validate_request(r: &HandoffRequest) -> Result<()> {
    for grant in [&r.previous, &r.successor] {
        if grant.record.schema != super::SCHEMA
            || Digest::blob(&encode(&grant.record)?) != grant.sha256
        {
            return Err(HandoffError::InvalidPlan);
        }
    }
    if r.owner != r.previous.request().owner
        || r.previous.store() == r.successor.store()
        || r.source_domain == r.successor_domain
        || r.previous.request().publication.object != r.successor.request().publication.object
        || r.successor.request().publication.required < r.previous.request().publication.required
    {
        return Err(HandoffError::InvalidPlan);
    }
    Ok(())
}
pub fn plan(source: &LocalStore, request: &HandoffRequest) -> Result<HandoffPlan> {
    validate_request(request)?;
    if source.identity() != request.previous.store() {
        return Err(HandoffError::InvalidPlan);
    }
    source
        .capabilities()
        .check(&request.previous.request().publication)?;
    let quantum = source.allocation_quantum()?;
    let limit = 65_536u64
        .checked_add(quantum - 1)
        .and_then(|n| n.checked_div(quantum))
        .and_then(|n| n.checked_mul(quantum))
        .and_then(|n| n.checked_mul(MAX_SCOPE_FILES as u64 + 2))
        .ok_or(HandoffError::InvalidPlan)?;
    Ok(HandoffPlan {
        store: source.identity().clone(),
        request: request.clone(),
        limit,
        quantum,
    })
}
fn keys(p: &HandoffPlan) -> Result<(Key, Key)> {
    let id = Digest::blob(&encode(&(
        &p.store,
        &p.request.owner,
        &p.request.operation,
    ))?);
    Ok((
        Key::parse(&format!("handoff-{}", id.as_str()))?,
        Key::parse(&format!("handedoff-{}", id.as_str()))?,
    ))
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Admitted,
    Bound,
    SuccessorVerified,
    Marked,
    Retired,
    Settled,
}
pub fn apply(
    source_ledger: &mut CapacityLedger,
    source: &LocalStore,
    successor_ledger: &mut CapacityLedger,
    successor: &LocalStore,
    job: &JournalRef,
    planned: &HandoffPlan,
    authorization: &HandoffAuthorization,
) -> Result<HandoffReceipt> {
    apply_hook(
        source_ledger,
        source,
        successor_ledger,
        successor,
        job,
        planned,
        authorization,
        |_| Ok(()),
    )
}
#[allow(clippy::too_many_arguments)]
fn apply_hook(
    source_ledger: &mut CapacityLedger,
    source: &LocalStore,
    successor_ledger: &mut CapacityLedger,
    successor: &LocalStore,
    job: &JournalRef,
    planned: &HandoffPlan,
    authorization: &HandoffAuthorization,
    mut hook: impl FnMut(Stage) -> Result<()>,
) -> Result<HandoffReceipt> {
    if encode(&plan(source, &planned.request)?)? != encode(planned)? {
        return Err(HandoffError::InvalidPlan);
    }
    if Digest::blob(&encode(planned)?) != authorization.reviewed
        || planned.request.owner != authorization.owner
    {
        return Err(HandoffError::WrongAuthorization);
    }
    if successor.identity() != planned.request.successor.store() {
        return Err(HandoffError::InvalidPlan);
    }
    let previous = &planned.request.previous.record;
    let old_binding = Digest::blob(&encode(&(&previous.plan, &previous.job))?);
    if source_ledger
        .retention_status(&previous.allocation, &old_binding)?
        .is_none()
    {
        return Err(RetentionError::NotSettled.into());
    }
    let existing = source.handoff_record(previous)?;
    if let Some(marker) = &existing
        && (encode(&marker.plan)? != encode(planned)? || &marker.job != job)
    {
        return Err(HandoffError::AlreadyReleased);
    }
    let (op, settle) = keys(planned)?;
    let binding = Digest::blob(&encode(&(planned, job))?);
    let allocation = source_ledger
        .begin_handoff(
            op.clone(),
            job,
            planned.request.owner.clone(),
            binding.clone(),
            planned.limit,
        )?
        .reference()
        .clone();
    let record = HandoffRecord {
        schema: SCHEMA.into(),
        plan: planned.clone(),
        job: job.clone(),
        allocation: allocation.clone(),
    };
    hook(Stage::Admitted)?;
    if let Some(charge) = source_ledger.handoff_status(&allocation, &binding)? {
        if existing.as_ref().is_none_or(|m| !m.same(&record))
            || !source.handoff_claim_absent(previous)?
        {
            return Err(HandoffError::InvalidPlan);
        }
        return Ok(HandoffReceipt {
            marker: Digest::blob(&encode(&record)?),
            settlement: source_ledger.settle_handoff(settle, &allocation, binding, charge)?,
            charged_footprint: charge,
        });
    }
    let scope = Request {
        operation: op,
        owner: planned.request.owner.clone(),
        object: previous.plan.request.publication.object.clone(),
        required: previous.plan.request.publication.required,
    };
    let attempt = source.attempt(&scope)?;
    if source_ledger
        .handoff_status(&allocation, &binding)?
        .is_some()
    {
        return Err(LedgerError::Conflict.into());
    }
    attempt.bind(&record)?;
    hook(Stage::Bound)?;
    let mut keeper = if existing.is_none() {
        Some(super::verify(
            successor_ledger,
            successor,
            &planned.request.successor,
        )?)
    } else {
        None
    };
    if keeper.is_some() {
        hook(Stage::SuccessorVerified)?;
    }
    let fence = source.handoff_fence(&scope.object)?;
    // Recheck after obtaining the destructive metadata scope, never an old view.
    if source.handoff_record(previous)?.is_none() {
        source.verify_handoff_source(previous, &fence)?;
        let Some(keeper) = keeper.as_mut() else {
            return Err(HandoffError::InvalidPlan);
        };
        keeper.revalidate(successor_ledger, successor)?;
        attempt.install_handoff(&record, &fence)?;
    } else if source
        .handoff_record(previous)?
        .is_none_or(|m| !m.same(&record))
    {
        return Err(HandoffError::AlreadyReleased);
    }
    hook(Stage::Marked)?;
    source.retire_handoff_claim(previous, &fence)?;
    hook(Stage::Retired)?;
    let charge = attempt.handoff_footprint(&record)?;
    if charge > planned.limit {
        return Err(Error::Budget.into());
    }
    let settlement = source_ledger.settle_handoff(settle, &allocation, binding, charge)?;
    hook(Stage::Settled)?;
    Ok(HandoffReceipt {
        settlement,
        marker: Digest::blob(&encode(&record)?),
        charged_footprint: charge,
    })
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    Ok(super::encode(value)?)
}
#[cfg(test)]
#[path = "handoff_tests.rs"]
mod tests;
