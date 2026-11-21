//! Explicit discard of reviewed, unacknowledged private attempt files only.
use super::*;
use crate::local::PrivatePartial;

#[derive(Debug)]
pub enum CleanupError {
    Accounted(AccountedError),
    StalePlan,
    WrongAuthorization,
    RecoverPublication,
    AlreadyCancelling,
    Injected,
}
impl From<AccountedError> for CleanupError {
    fn from(e: AccountedError) -> Self {
        Self::Accounted(e)
    }
}
impl From<Error> for CleanupError {
    fn from(e: Error) -> Self {
        Self::Accounted(e.into())
    }
}
impl From<LedgerError> for CleanupError {
    fn from(e: LedgerError) -> Self {
        Self::Accounted(e.into())
    }
}
impl std::fmt::Display for CleanupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CleanupError {}
type Result<T> = std::result::Result<T, CleanupError>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupPlan {
    schema: String,
    publication: PublicationPlan,
    job: JournalRef,
    allocation: JournalRef,
    files: Vec<PrivatePartial>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscardAuthorization {
    reviewed: Digest,
}
#[derive(Debug)]
pub struct CleanupReceipt {
    pub disposition: ledger::Receipt,
    pub remaining_footprint: u64,
    pub reviewed_partial_count: usize,
}
impl CleanupPlan {
    pub fn files(&self) -> &[PrivatePartial] {
        &self.files
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        Ok(encode(self)?)
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let value = identity::parse_json(bytes).map_err(|_| CleanupError::StalePlan)?;
        let plan: Self = serde_json::from_value(value).map_err(|_| CleanupError::StalePlan)?;
        if plan.files.len() > MAX_SCOPE_FILES {
            return Err(CleanupError::StalePlan);
        }
        Ok(plan)
    }
    /// Explicit application/user decision to discard these unacknowledged bytes.
    /// Never inferred from age, a missing label or a role review.
    pub fn authorize_discard(&self) -> Result<DiscardAuthorization> {
        Ok(DiscardAuthorization {
            reviewed: Digest::blob(&self.to_json()?),
        })
    }
}
fn allocation_key(store: &LocalStore, publication: &PublicationPlan) -> Result<Key> {
    let id = Digest::blob(&encode(&(
        store.identity(),
        &publication.request.operation,
    ))?);
    Ok(Key::parse(&format!("publish-{}", id.as_str()))?)
}
fn validate(store: &LocalStore, publication: &PublicationPlan) -> Result<()> {
    if encode(&plan(store, &publication.request)?)? != encode(publication)? {
        return Err(CleanupError::StalePlan);
    }
    Ok(())
}
/// Read-only existing scope preview; no anchor, directory or journal command added.
pub fn inspect(
    ledger: &mut CapacityLedger,
    store: &LocalStore,
    job: &JournalRef,
    publication: &PublicationPlan,
) -> Result<CleanupPlan> {
    validate(store, publication)?;
    let allocation = ledger.publication_reference(allocation_key(store, publication)?)?;
    let binding = Digest::blob(&encode(&(publication, job))?);
    let attempt = store.existing_attempt(&publication.request)?;
    if ledger.cleanup_status(&allocation, &binding)?.is_some() {
        return Err(CleanupError::AlreadyCancelling);
    }
    attempt.check_binding(&Binding {
        schema: "tig.accounted-publication.design-1",
        plan: publication,
        job,
        allocation: &allocation,
    })?;
    if attempt.has_final_or_completion(&publication.request)? {
        return Err(CleanupError::RecoverPublication);
    }
    let files = attempt.partial_evidence(publication.limit)?;
    let result = CleanupPlan {
        schema: "tig.private-cleanup.design-1".into(),
        publication: publication.clone(),
        job: job.clone(),
        allocation,
        files,
    };
    result.to_json()?;
    Ok(result)
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Cancelled,
    Removed,
    Disposed,
}

pub fn apply(
    ledger: &mut CapacityLedger,
    store: &LocalStore,
    reviewed: &CleanupPlan,
    authorization: &DiscardAuthorization,
) -> Result<CleanupReceipt> {
    apply_hook(ledger, store, reviewed, authorization, |_| Ok(()))
}
fn apply_hook(
    ledger: &mut CapacityLedger,
    store: &LocalStore,
    reviewed: &CleanupPlan,
    authorization: &DiscardAuthorization,
    mut hook: impl FnMut(Stage) -> Result<()>,
) -> Result<CleanupReceipt> {
    validate(store, &reviewed.publication)?;
    let digest = Digest::blob(&reviewed.to_json()?);
    if digest != authorization.reviewed {
        return Err(CleanupError::WrongAuthorization);
    }
    if reviewed.schema != "tig.private-cleanup.design-1" || reviewed.files.len() > MAX_SCOPE_FILES {
        return Err(CleanupError::StalePlan);
    }
    let allocation = ledger.publication_reference(allocation_key(store, &reviewed.publication)?)?;
    if allocation != reviewed.allocation {
        return Err(CleanupError::StalePlan);
    }
    let binding = Digest::blob(&encode(&(&reviewed.publication, &reviewed.job))?);
    let attempt = store.existing_attempt(&reviewed.publication.request)?;
    attempt.check_binding(&Binding {
        schema: "tig.accounted-publication.design-1",
        plan: &reviewed.publication,
        job: &reviewed.job,
        allocation: &allocation,
    })?;
    if attempt.has_final_or_completion(&reviewed.publication.request)? {
        return Err(CleanupError::RecoverPublication);
    }
    let cancellation = ledger.cleanup_status(&allocation, &binding)?;
    let current = attempt.partial_evidence(reviewed.publication.limit)?;
    if let Some((approved, _)) = &cancellation {
        if approved != &digest || current.iter().any(|file| !reviewed.files.contains(file)) {
            return Err(CleanupError::StalePlan);
        }
    } else if current != reviewed.files {
        return Err(CleanupError::StalePlan);
    }
    if let Some((_, Some(charge))) = cancellation {
        if !current.is_empty() {
            return Err(CleanupError::StalePlan);
        }
        return Ok(CleanupReceipt {
            disposition: ledger.complete_cleanup(&allocation, binding, digest, charge)?,
            remaining_footprint: charge,
            reviewed_partial_count: reviewed.files.len(),
        });
    }
    ledger.begin_cleanup(&allocation, binding.clone(), digest.clone())?;
    hook(Stage::Cancelled)?;
    for file in &current {
        attempt.remove_partial(&file.name)?;
        hook(Stage::Removed)?;
    }
    if attempt.has_final_or_completion(&reviewed.publication.request)? {
        return Err(CleanupError::RecoverPublication);
    }
    let footprint = attempt.retained_metadata(&reviewed.publication.request)?;
    if footprint > reviewed.publication.limit {
        return Err(CleanupError::StalePlan);
    }
    let disposition = ledger.complete_cleanup(&allocation, binding, digest, footprint)?;
    hook(Stage::Disposed)?;
    Ok(CleanupReceipt {
        disposition,
        remaining_footprint: footprint,
        reviewed_partial_count: reviewed.files.len(),
    })
}

#[cfg(test)]
#[path = "cleanup_tests.rs"]
mod tests;
