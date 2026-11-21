//! Admitted local publication. The charged basis is a conservative file-data
//! footprint; not total filesystem overhead, net growth, or eviction authority.
use crate::{
    ledger::{CapacityLedger, JournalRef, LedgerError},
    local::{LocalStore, MAX_SCOPE_FILES},
    *,
};
use serde::{Deserialize, Serialize};
pub mod cleanup;
use git_asset_core::{Digest, identity};

#[derive(Debug)]
pub enum AccountedError {
    Store(Error),
    Ledger(LedgerError),
    InvalidPlan,
    PendingPartial,
    MissingSettledPublication,
    FootprintExceedsLimit,
    Injected,
}
impl From<Error> for AccountedError {
    fn from(e: Error) -> Self {
        Self::Store(e)
    }
}
impl From<LedgerError> for AccountedError {
    fn from(e: LedgerError) -> Self {
        Self::Ledger(e)
    }
}
impl std::fmt::Display for AccountedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for AccountedError {}
pub type Result<T> = std::result::Result<T, AccountedError>;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationPlan {
    store: Digest,
    request: Request,
    quantum: u64,
    limit: u64,
    basis: String,
    /// Optional admitted orchestration identity. Omitted legacy plans retain
    /// their exact encoding and replay binding; older readers reject this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context: Option<Digest>,
}
impl PublicationPlan {
    pub fn required_peak(&self) -> u64 {
        self.limit
    }
    pub fn basis(&self) -> &str {
        &self.basis
    }
}
#[derive(Debug)]
pub struct AccountedReceipt {
    publication: Receipt,
    allocation: JournalRef,
    settlement: ledger::Receipt,
    charged_footprint: u64,
}
impl AccountedReceipt {
    pub fn publication(&self) -> &Receipt {
        &self.publication
    }
    pub fn allocation(&self) -> &JournalRef {
        &self.allocation
    }
    pub fn settlement(&self) -> &ledger::Receipt {
        &self.settlement
    }
    pub fn charged_footprint(&self) -> u64 {
        self.charged_footprint
    }
    pub fn basis(&self) -> &str {
        "conservative-allocated-file-data-footprint"
    }
}
#[derive(Serialize)]
struct Binding<'a> {
    schema: &'static str,
    plan: &'a PublicationPlan,
    job: &'a JournalRef,
    allocation: &'a JournalRef,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Admitted,
    Checked,
    Bound,
    Published,
    Settled,
}

pub fn plan(store: &LocalStore, request: &Request) -> Result<PublicationPlan> {
    plan_with_context(store, request, None)
}
/// Bind a higher-level immutable plan into the existing allocation/replay fence.
/// The context is an identity, not authentication or removal authority.
pub fn plan_bound(
    store: &LocalStore,
    request: &Request,
    context: Digest,
) -> Result<PublicationPlan> {
    plan_with_context(store, request, Some(context))
}
fn plan_with_context(
    store: &LocalStore,
    request: &Request,
    context: Option<Digest>,
) -> Result<PublicationPlan> {
    store.capabilities().check(request)?;
    let quantum = store.allocation_quantum()?;
    let round = |n: u64| {
        n.checked_add(quantum - 1)
            .and_then(|v| v.checked_div(quantum))
            .and_then(|v| v.checked_mul(quantum))
            .ok_or(AccountedError::InvalidPlan)
    };
    let payload = round(request.object.bytes)?;
    let lifecycle_files = if store.final_policy().is_some() {
        crate::local::lifecycle::MAX_FINAL_FILES as u64
    } else if store.supports_retirement() {
        crate::local::lifecycle::MAX_FILES as u64
    } else {
        0
    };
    let metadata = round(65_536)?
        .checked_mul(MAX_SCOPE_FILES as u64 + 4 + lifecycle_files)
        .ok_or(AccountedError::InvalidPlan)?;
    let limit = payload
        .checked_mul(2)
        .and_then(|n| n.checked_add(metadata))
        .ok_or(AccountedError::InvalidPlan)?;
    Ok(PublicationPlan {
        store: store.identity().clone(),
        request: request.clone(),
        quantum,
        limit,
        basis: "conservative-allocated-file-data-footprint".into(),
        context,
    })
}
pub fn publish_accounted(
    ledger: &mut CapacityLedger,
    store: &LocalStore,
    job: &JournalRef,
    plan: &PublicationPlan,
    source: &mut dyn Read,
) -> Result<AccountedReceipt> {
    publish_hook(ledger, store, job, plan, source, |_| Ok(()))
}
pub(crate) fn admit(
    ledger: &mut CapacityLedger,
    store: &LocalStore,
    job: &JournalRef,
    planned: &PublicationPlan,
) -> Result<JournalRef> {
    let current = plan_with_context(store, &planned.request, planned.context.clone())?;
    if encode(&current)? != encode(planned)? {
        return Err(AccountedError::InvalidPlan);
    }
    let binding = Digest::blob(&encode(&(planned, job))?);
    let identity = Digest::blob(&encode(&(store.identity(), &planned.request.operation))?);
    Ok(ledger
        .begin_publication(
            Key::parse(&format!("publish-{}", identity.as_str()))?,
            job,
            planned.request.owner.clone(),
            binding,
            planned.limit,
        )?
        .reference()
        .clone())
}
pub(crate) fn prepare(
    ledger: &mut CapacityLedger,
    store: &LocalStore,
    job: &JournalRef,
    planned: &PublicationPlan,
    allocation: &JournalRef,
) -> Result<crate::local::LocalAttempt> {
    let binding = Digest::blob(&encode(&(planned, job))?);
    ledger.publication_status(allocation, &binding)?;
    let attempt = store.attempt(&planned.request)?;
    attempt.bind(&Binding {
        schema: "tig.accounted-publication.design-1",
        plan: planned,
        job,
        allocation,
    })?;
    Ok(attempt)
}
fn publish_hook(
    ledger: &mut CapacityLedger,
    store: &LocalStore,
    job: &JournalRef,
    planned: &PublicationPlan,
    source: &mut dyn Read,
    mut hook: impl FnMut(Stage) -> Result<()>,
) -> Result<AccountedReceipt> {
    let current = plan_with_context(store, &planned.request, planned.context.clone())?;
    if encode(&current)? != encode(planned)? {
        return Err(AccountedError::InvalidPlan);
    }
    let binding = Digest::blob(&encode(&(planned, job))?);
    let identity = Digest::blob(&encode(&(store.identity(), &planned.request.operation))?);
    let allocation_id = Key::parse(&format!("publish-{}", identity.as_str()))?;
    let settle_id = Key::parse(&format!("settle-{}", identity.as_str()))?;
    let allocation = ledger.begin_publication(
        allocation_id,
        job,
        planned.request.owner.clone(),
        binding.clone(),
        planned.limit,
    )?;
    let allocation = allocation.reference().clone();
    hook(Stage::Admitted)?;
    if let Some(charge) = ledger.publication_status(&allocation, &binding)? {
        let publication = store
            .completed_receipt(&planned.request)?
            .ok_or(AccountedError::MissingSettledPublication)?;
        let settlement = ledger.settle_footprint(settle_id, &allocation, binding, charge)?;
        return Ok(AccountedReceipt {
            publication,
            allocation,
            settlement,
            charged_footprint: charge,
        });
    }
    hook(Stage::Checked)?;
    let mut attempt = store.attempt(&planned.request)?;
    // Cleanup may have cancelled while this caller was between status and fence.
    // Never act on the earlier snapshot once we own the attempt.
    if ledger.publication_status(&allocation, &binding)?.is_some() {
        return Err(LedgerError::Conflict.into());
    }
    attempt.bind(&Binding {
        schema: "tig.accounted-publication.design-1",
        plan: planned,
        job,
        allocation: &allocation,
    })?;
    hook(Stage::Bound)?;
    // Keep one object guard through destination measurement and ledger settlement.
    let _object = attempt.store.publication_guard(&planned.request.object)?;
    let recovered = recover(&mut attempt.store, &planned.request)?;
    let publication = match recovered {
        Some(receipt) => receipt,
        None => {
            if attempt.has_partials()? {
                return Err(AccountedError::PendingPartial);
            }
            publish(&mut attempt.store, &planned.request, source)?
        }
    };
    hook(Stage::Published)?;
    let footprint = attempt.footprint(&planned.request)?;
    if footprint > planned.limit {
        return Err(AccountedError::FootprintExceedsLimit);
    }
    let settlement = ledger.settle_footprint(settle_id, &allocation, binding, footprint)?;
    hook(Stage::Settled)?;
    Ok(AccountedReceipt {
        publication,
        allocation,
        settlement,
        charged_footprint: footprint,
    })
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    identity::canonical_json(&serde_json::to_value(value).map_err(|_| AccountedError::InvalidPlan)?)
        .map_err(|_| AccountedError::InvalidPlan)
}

#[cfg(test)]
#[path = "accounted_tests.rs"]
mod tests;
