//! Explicit final claim release; payload bytes and other claims are untouched.
use super::*;
use crate::{
    retirement_intent::{self, Enrollment, ObservedIntent, progress},
    source_claims::SourceJournal,
};
use tig_core::{
    retirement::review::{Release, decision::Decision},
    tree::NodeStore,
};
const SCHEMA: &str = "tig.retention-final-release.design-1";

#[derive(Debug)]
pub enum ReleaseError {
    Retention(RetentionError),
    Progress(retirement_intent::Error),
    InvalidPlan,
    NotAdmitted,
    AlreadyReleased,
    Injected,
}
impl From<RetentionError> for ReleaseError {
    fn from(e: RetentionError) -> Self {
        Self::Retention(e)
    }
}
impl From<Error> for ReleaseError {
    fn from(e: Error) -> Self {
        Self::Retention(e.into())
    }
}
impl From<LedgerError> for ReleaseError {
    fn from(e: LedgerError) -> Self {
        Self::Retention(e.into())
    }
}
impl std::fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ReleaseError {}
pub type Result<T> = std::result::Result<T, ReleaseError>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleasePlan {
    grant: RetentionGrant,
    decision: Digest,
    operation: Digest,
    owner: Key,
    subject: Digest,
    quantum: u64,
    limit: u64,
}
impl ReleasePlan {
    /// Exact existing native allocation identity, shared by execution and adapters.
    pub fn operation_id(&self, attempt: u64) -> Result<Digest> {
        Ok(Digest::blob(&encode(&(
            SCHEMA,
            &self.grant.sha256,
            &self.decision,
            attempt,
        ))?))
    }
    pub fn grant(&self) -> &RetentionGrant {
        &self.grant
    }
    pub fn decision(&self) -> &Digest {
        &self.decision
    }
    pub fn operation(&self) -> &Digest {
        &self.operation
    }
    pub fn owner(&self) -> &Key {
        &self.owner
    }
    pub fn subject(&self) -> &Digest {
        &self.subject
    }
    pub fn required_peak(&self) -> u64 {
        self.limit
    }
    pub(crate) fn object(&self) -> &tig_core::Object {
        &self.grant.request().publication.object
    }
    pub(crate) fn matches_work(
        &self,
        intent: &ObservedIntent,
        object: &tig_core::Object,
        claim: &Digest,
    ) -> bool {
        validate(self).is_ok()
            && self.owner == *intent.owner()
            && self.subject == intent.decision().approval().owner
            && intent.decision().digest().is_ok_and(|d| d == self.decision)
            && self.operation == intent.decision().approval().operation
            && self.grant.claim_id() == claim
            && self.grant.request().publication.object == *object
    }
}
fn validate(p: &ReleasePlan) -> Result<()> {
    let g = &p.grant;
    if g.record.schema != super::SCHEMA
        || Digest::blob(&super::encode(&g.record)?) != g.sha256
        || p.owner != g.request().owner
    {
        return Err(ReleaseError::InvalidPlan);
    }
    Ok(())
}
/// The enrollment is a trusted adapter mapping, not native owner authentication.
pub fn plan(
    store: &LocalStore,
    grant: &RetentionGrant,
    decision: &Decision,
    enrollment: &Enrollment,
) -> Result<ReleasePlan> {
    if decision.execution_context().is_some()
        || enrollment.subject != decision.approval().owner
        || !decision.consent().release_owned_claims.contains(&Release {
            claim: grant.claim_id().clone(),
            object: grant.request().publication.object.clone(),
        })
    {
        return Err(ReleaseError::InvalidPlan);
    }
    let budget = super::plan(store, grant.request())?;
    let p = ReleasePlan {
        grant: grant.clone(),
        decision: decision.digest().map_err(|_| ReleaseError::InvalidPlan)?,
        operation: decision.approval().operation.clone(),
        owner: enrollment.owner.clone(),
        subject: enrollment.subject.clone(),
        quantum: budget.quantum,
        limit: budget.limit,
    };
    validate(&p)?;
    if store.identity() != grant.store() {
        return Err(ReleaseError::InvalidPlan);
    }
    // The decision and grant are bounded individually; their combined record must
    // also fit the provider's immutable metadata record before any admission.
    if encode(&p)?.len() > 48 * 1024 {
        return Err(Error::Budget.into());
    }
    Ok(p)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReleaseRecord {
    schema: String,
    plan: ReleasePlan,
    job: JournalRef,
    allocation: JournalRef,
    attempt: u64,
}
impl ReleaseRecord {
    pub(crate) fn previous(&self) -> &RetentionRecord {
        &self.plan.grant.record
    }
    pub(crate) fn matches_previous(&self, previous: &RetentionRecord, store: &Digest) -> bool {
        self.schema == SCHEMA
            && self.attempt > 0
            && self.plan.grant.store() == store
            && validate(&self.plan).is_ok()
            && matches!((encode(self.previous()), encode(previous)), (Ok(a), Ok(b)) if a == b)
    }
    pub(crate) fn same(&self, other: &Self) -> bool {
        matches!((encode(self), encode(other)), (Ok(a), Ok(b)) if a == b)
    }
}
pub struct ReleaseReceipt {
    pub evidence: Digest,
    pub settlement: ledger::Receipt,
    pub charged_footprint: u64,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Bound,
    Marked,
    Retired,
    Settled,
}

/// Reconcile an already admitted exact release. A cancel after admission cannot
/// undo its effect; it can still be reconciled/reported, never expanded in scope.
#[allow(clippy::too_many_arguments)]
pub fn apply(
    ledger: &mut CapacityLedger,
    store: &LocalStore,
    job: &JournalRef,
    planned: &ReleasePlan,
    journal: &mut SourceJournal,
    nodes: &impl NodeStore,
    intent: &ObservedIntent,
    attempt: u64,
) -> Result<ReleaseReceipt> {
    apply_hook(
        ledger,
        store,
        job,
        planned,
        journal,
        nodes,
        intent,
        attempt,
        |_| Ok(()),
    )
}
#[allow(clippy::too_many_arguments)]
fn apply_hook(
    ledger: &mut CapacityLedger,
    store: &LocalStore,
    job: &JournalRef,
    planned: &ReleasePlan,
    journal: &mut SourceJournal,
    nodes: &impl NodeStore,
    intent: &ObservedIntent,
    attempt: u64,
    mut hook: impl FnMut(Stage) -> Result<()>,
) -> Result<ReleaseReceipt> {
    let enrollment = Enrollment {
        owner: planned.owner.clone(),
        subject: planned.subject.clone(),
    };
    if encode(&plan(
        store,
        &planned.grant,
        intent.decision(),
        &enrollment,
    )?)? != encode(planned)?
        || intent.owner() != &planned.owner
        || intent
            .decision()
            .digest()
            .map_err(|_| ReleaseError::InvalidPlan)?
            != planned.decision
    {
        return Err(ReleaseError::InvalidPlan);
    }
    let previous = &planned.grant.record;
    let object = &previous.plan.request.publication.object;
    let action = progress::Action::Release {
        claim: planned.grant.claim_id().clone(),
    };
    let snapshot =
        progress::inspect(journal, nodes, intent, object).map_err(ReleaseError::Progress)?;
    let existing = store.final_release_record(previous)?;
    if let Some(marker) = &existing
        && (encode(&marker.plan)? != encode(planned)?
            || &marker.job != job
            || marker.attempt != attempt)
    {
        return Err(ReleaseError::AlreadyReleased);
    }
    if snapshot.pending
        || !(snapshot.in_flight == Some((attempt, action.clone()))
            || (existing.is_some() && snapshot.released.contains(planned.grant.claim_id())))
    {
        return Err(ReleaseError::NotAdmitted);
    }
    progress::release_work::check(journal, nodes, intent, planned, job, attempt)
        .map_err(ReleaseError::Progress)?;
    let old_binding = Digest::blob(&encode(&(&previous.plan, &previous.job))?);
    if ledger
        .retention_status(&previous.allocation, &old_binding)?
        .is_none()
    {
        return Err(RetentionError::NotSettled.into());
    }
    let id = planned.operation_id(attempt)?;
    let op = Key::parse(&format!("release-{}", id.as_str()))?;
    let settle = Key::parse(&format!("released-{}", id.as_str()))?;
    let binding = Digest::blob(&encode(&(planned, job, attempt))?);
    // Reuse the existing budgeted claim-release (handoff) allocation machinery.
    // No payload allocation is credited or modified by this metadata operation.
    let allocation = ledger
        .begin_handoff(
            op.clone(),
            job,
            planned.owner.clone(),
            binding.clone(),
            planned.limit,
        )?
        .reference()
        .clone();
    let record = ReleaseRecord {
        schema: SCHEMA.into(),
        plan: planned.clone(),
        job: job.clone(),
        allocation: allocation.clone(),
        attempt,
    };
    if let Some(charge) = ledger.handoff_status(&allocation, &binding)? {
        if existing.as_ref().is_none_or(|m| !m.same(&record))
            || !store.handoff_claim_absent(previous)?
        {
            return Err(ReleaseError::InvalidPlan);
        }
        return Ok(ReleaseReceipt {
            evidence: Digest::blob(&encode(&record)?),
            settlement: ledger.settle_handoff(settle, &allocation, binding, charge)?,
            charged_footprint: charge,
        });
    }
    let scope = Request {
        operation: op,
        owner: planned.owner.clone(),
        object: object.clone(),
        required: previous.plan.request.publication.required,
    };
    let local = store.attempt(&scope)?;
    if ledger.handoff_status(&allocation, &binding)?.is_some() {
        return Err(LedgerError::Conflict.into());
    }
    local.bind(&record)?;
    hook(Stage::Bound)?;
    let fence = store.handoff_fence(object)?;
    // Admission observed before waiting for the provider fence is not a lease.
    // Reconcile a peer's no-effect/completion before changing claim metadata.
    let snapshot =
        progress::inspect(journal, nodes, intent, object).map_err(ReleaseError::Progress)?;
    if snapshot.pending
        || !(snapshot.in_flight == Some((attempt, action))
            || (existing.is_some() && snapshot.released.contains(planned.grant.claim_id())))
    {
        return Err(ReleaseError::NotAdmitted);
    }
    progress::release_work::check(journal, nodes, intent, planned, job, attempt)
        .map_err(ReleaseError::Progress)?;
    if store.final_release_record(previous)?.is_none() {
        store.verify_handoff_source(previous, &fence)?;
        local.install_final_release(&record, &fence)?;
    } else if store
        .final_release_record(previous)?
        .is_none_or(|m| !m.same(&record))
    {
        return Err(ReleaseError::AlreadyReleased);
    }
    hook(Stage::Marked)?;
    store.retire_handoff_claim(previous, &fence)?;
    hook(Stage::Retired)?;
    let charge = local.final_release_footprint(&record)?;
    if charge > planned.limit {
        return Err(Error::Budget.into());
    }
    let settlement = ledger.settle_handoff(settle, &allocation, binding, charge)?;
    hook(Stage::Settled)?;
    Ok(ReleaseReceipt {
        evidence: Digest::blob(&encode(&record)?),
        settlement,
        charged_footprint: charge,
    })
}
fn encode(v: &impl Serialize) -> Result<Vec<u8>> {
    Ok(super::encode(v)?)
}
#[cfg(test)]
#[path = "release_tests.rs"]
pub(crate) mod tests;
