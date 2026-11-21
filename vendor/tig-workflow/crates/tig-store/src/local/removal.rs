//! Scoped native unlink: final loss or held retained custody. Protection is an admitted port.
use super::*;
use crate::{
    ledger::{CapacityLedger, JournalRef, LedgerError},
    retirement_intent::{self, ObservedIntent, progress},
    source_claims::SourceJournal,
};
use tig_core::{retirement::review::decision::Target, tree::NodeStore};
pub mod custody;
const BEGIN: &str = "tig.local-final-removal.design-1";
const DONE: &str = "tig.local-final-disposition.design-1";
pub const MAX_REMOVALS: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub store: Digest,
    pub object: Object,
    pub incarnation: Digest,
    pub decision: Digest,
    pub operation: Digest,
    pub owner: Key,
    pub attempt: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preservation: Option<Digest>,
}
/// Trusted nonblocking, object-scoped admission port. Unknown/incomplete coverage
/// must refuse. A lease must exclude new conflicting claims/jobs until drop.
/// No globally complete implementation over SourceJournal alone is provided.
pub trait Protection {
    type Lease: ProtectionLease;
    fn acquire(&mut self, scope: &Scope) -> crate::Result<Self::Lease>;
}
pub trait ProtectionLease {
    /// Fresh current authority/policy/enrollment check for the entire scope,
    /// including its preservation profile digest; not cached coverage.
    fn revalidate(&mut self, scope: &Scope) -> crate::Result<Digest>;
}
#[derive(Debug)]
pub enum RemovalError {
    Store(Error),
    Ledger(LedgerError),
    Progress(retirement_intent::Error),
    Custody(crate::retention::RetentionError),
    ProviderCustody(crate::acquire::SourceError),
    InvalidPlan,
    NotAdmitted,
    Injected,
}
impl From<Error> for RemovalError {
    fn from(e: Error) -> Self {
        Self::Store(e)
    }
}
impl From<LedgerError> for RemovalError {
    fn from(e: LedgerError) -> Self {
        Self::Ledger(e)
    }
}
impl From<crate::retention::RetentionError> for RemovalError {
    fn from(e: crate::retention::RetentionError) -> Self {
        Self::Custody(e)
    }
}
impl From<crate::acquire::SourceError> for RemovalError {
    fn from(e: crate::acquire::SourceError) -> Self {
        Self::ProviderCustody(e)
    }
}
impl std::fmt::Display for RemovalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for RemovalError {}
pub type Result<T> = std::result::Result<T, RemovalError>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    scope: Scope,
    quantum: u64,
    limit: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    custody: Option<custody::Profile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider_custody: Option<crate::provider::custody::Profile>,
}
impl Plan {
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
    pub fn required_peak(&self) -> u64 {
        self.limit
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Begin {
    schema: String,
    plan: Plan,
    job: JournalRef,
    allocation: JournalRef,
    file_data_before: u64,
    protection: Digest,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Disposition {
    schema: String,
    begin: Digest,
    next: Digest,
}
pub(super) struct ObjectState {
    pub incarnation: Digest,
    depth: usize,
    pub pending: Option<Begin>,
}
fn encode(v: &impl Serialize) -> crate::Result<Vec<u8>> {
    identity::canonical_json(&serde_json::to_value(v).map_err(|_| Error::CorruptRecord)?)
        .map_err(|_| Error::CorruptRecord)
}
fn path(store: &LocalStore, object: &Object, era: &Digest, suffix: &str) -> PathBuf {
    store
        .root
        .join("removals")
        .join(object.sha256.as_str())
        .join(format!("{}.{suffix}", era.as_str()))
}
fn disposition(begin: &Begin) -> crate::Result<Disposition> {
    let begin = Digest::blob(&encode(begin)?);
    Ok(Disposition {
        schema: DONE.into(),
        next: Digest::blob(&encode(&(DONE, &begin))?),
        begin,
    })
}
fn check_disposition(begin: &Begin, done: &Disposition) -> crate::Result<()> {
    if encode(done)? != encode(&disposition(begin)?)? {
        return Err(Error::CorruptRecord);
    }
    Ok(())
}
/// Bounded exact-object chain, never a store census. Pending eras block readers
/// and publication until the matching provider operation is reconciled.
pub(super) fn state(store: &LocalStore, object: &Object) -> crate::Result<ObjectState> {
    store.guard()?;
    if !store.removable {
        return Err(Error::Unsupported);
    }
    check_directory(store, object)?;
    let mut era = Digest::blob(&encode(&(
        "tig.local-incarnation.design-1",
        &store.instance,
        &object.sha256,
    ))?);
    for depth in 0..=MAX_REMOVALS {
        let p = path(store, object, &era, "begin");
        let d = path(store, object, &era, "done");
        if !path_present(&p)? {
            if path_present(&d)? {
                return Err(Error::CorruptRecord);
            }
            return Ok(ObjectState {
                incarnation: era,
                depth,
                pending: None,
            });
        }
        if depth == MAX_REMOVALS {
            return Err(Error::Budget);
        }
        let begin: Begin = read_record(&p)?;
        let scope = &begin.plan.scope;
        if begin.schema != BEGIN
            || scope.store != store.instance
            || scope.object != *object
            || scope.incarnation != era
            || scope.attempt == 0
        {
            return Err(Error::CorruptRecord);
        }
        if !path_present(&d)? {
            return Ok(ObjectState {
                incarnation: era,
                depth,
                pending: Some(begin),
            });
        }
        let done: Disposition = read_record(&d)?;
        check_disposition(&begin, &done)?;
        era = done.next;
    }
    Err(Error::Budget)
}
fn check_directory(store: &LocalStore, object: &Object) -> crate::Result<()> {
    let dir = store.root.join("removals").join(object.sha256.as_str());
    if path_present(&dir)? {
        directory(&dir)?;
    }
    Ok(())
}
fn budget(store: &LocalStore) -> crate::Result<(u64, u64)> {
    if !store.removable {
        return Err(Error::Unsupported);
    }
    let quantum = store.allocation_quantum()?;
    let limit = 65_536u64
        .checked_add(quantum - 1)
        .and_then(|n| n.checked_div(quantum))
        .and_then(|n| n.checked_mul(quantum))
        .and_then(|n| n.checked_mul(MAX_SCOPE_FILES as u64 + 3))
        .ok_or(Error::Budget)?;
    Ok((quantum, limit))
}
fn consent(store: &LocalStore, intent: &ObservedIntent, object: &Object) -> Result<()> {
    if !intent
        .decision()
        .consent()
        .accept_intentional_loss
        .contains(object)
        || !intent.decision().consent().remove_copies.contains(&Target {
            object: object.clone(),
            store: store.instance.clone(),
        })
    {
        return Err(RemovalError::InvalidPlan);
    }
    Ok(())
}
pub fn plan(
    store: &LocalStore,
    intent: &ObservedIntent,
    object: &Object,
    attempt: u64,
) -> Result<Plan> {
    consent(store, intent, object)?;
    plan_inner(store, intent, object, attempt)
}
fn plan_inner(
    store: &LocalStore,
    intent: &ObservedIntent,
    object: &Object,
    attempt: u64,
) -> Result<Plan> {
    store.capabilities().check(&Request {
        operation: Key::parse("plan-removal")?,
        owner: intent.owner().clone(),
        object: object.clone(),
        required: AckLevel::ProcessRestart,
    })?;
    let s = state(store, object)?;
    if attempt == 0 || s.pending.is_some() || s.depth == MAX_REMOVALS {
        return Err(RemovalError::InvalidPlan);
    }
    let (quantum, limit) = budget(store)?;
    Ok(Plan {
        scope: Scope {
            store: store.instance.clone(),
            object: object.clone(),
            incarnation: s.incarnation,
            decision: intent
                .decision()
                .digest()
                .map_err(|_| RemovalError::InvalidPlan)?,
            operation: intent.decision().approval().operation.clone(),
            owner: intent.owner().clone(),
            attempt,
            preservation: None,
        },
        quantum,
        limit,
        custody: None,
        provider_custody: None,
    })
}
pub struct Execution<'a, N: NodeStore, P: Protection> {
    pub store: &'a LocalStore,
    pub ledger: &'a mut CapacityLedger,
    pub job: &'a JournalRef,
    pub journal: &'a mut SourceJournal,
    pub nodes: &'a N,
    pub intent: &'a ObservedIntent,
    pub protection: &'a mut P,
}
pub struct RemovalReceipt {
    pub evidence: Digest,
    /// Allocated data attributed to the removed file link, not net reclaim.
    pub file_data_before: u64,
    pub charged_metadata: u64,
    pub settlement: crate::ledger::Receipt,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Bound,
    Marked,
    Unlinked,
    Completed,
    Settled,
}
pub fn apply<N: NodeStore, P: Protection>(
    execution: Execution<'_, N, P>,
    planned: &Plan,
) -> Result<RemovalReceipt> {
    if planned.custody.is_some() || planned.provider_custody.is_some() {
        return Err(RemovalError::InvalidPlan);
    }
    apply_hook(execution, planned, |_| Ok(()))
}
fn admitted<N: NodeStore, P: Protection>(
    e: &mut Execution<'_, N, P>,
    p: &Plan,
    completed: bool,
) -> Result<()> {
    let snapshot = progress::inspect(e.journal, e.nodes, e.intent, &p.scope.object)
        .map_err(RemovalError::Progress)?;
    if snapshot.pending
        || !(snapshot.in_flight
            == Some((
                p.scope.attempt,
                progress::Action::Remove {
                    store: p.scope.store.clone(),
                },
            ))
            || (completed && snapshot.removed.contains_key(&p.scope.store)))
    {
        return Err(RemovalError::NotAdmitted);
    }
    Ok(())
}
fn apply_hook<N: NodeStore, P: Protection>(
    e: Execution<'_, N, P>,
    p: &Plan,
    hook: impl FnMut(Stage) -> Result<()>,
) -> Result<RemovalReceipt> {
    if p.custody.is_some() || p.provider_custody.is_some() {
        return Err(RemovalError::InvalidPlan);
    }
    run(e, p, &mut [], hook)
}
fn run<N: NodeStore, P: Protection>(
    e: Execution<'_, N, P>,
    p: &Plan,
    endpoints: &mut [custody::Endpoint<'_>],
    hook: impl FnMut(Stage) -> Result<()>,
) -> Result<RemovalReceipt> {
    if p.provider_custody.is_some() {
        return Err(RemovalError::InvalidPlan);
    }
    run_with(
        e,
        p,
        |scope| {
            let keepers = custody::acquire(p.custody.as_ref(), endpoints, scope)?;
            Ok(Box::new(NativeKeepers {
                keepers,
                endpoints,
                scope: scope.clone(),
            }))
        },
        hook,
    )
}
trait Keepers {
    fn revalidate(&mut self) -> Result<()>;
    fn fresh(&self) -> Result<()>;
}
struct NativeKeepers<'a, 'b> {
    keepers: Vec<(usize, crate::retention::RetainedReplica)>,
    endpoints: &'a mut [custody::Endpoint<'b>],
    scope: Scope,
}
impl Keepers for NativeKeepers<'_, '_> {
    fn revalidate(&mut self) -> Result<()> {
        custody::revalidate(&mut self.keepers, self.endpoints, &self.scope)
    }
    fn fresh(&self) -> Result<()> {
        custody::fresh(&self.keepers)
    }
}
fn run_with<'a, N: NodeStore, P: Protection>(
    mut e: Execution<'_, N, P>,
    p: &Plan,
    acquire: impl FnOnce(&Scope) -> Result<Box<dyn Keepers + 'a>>,
    mut hook: impl FnMut(Stage) -> Result<()>,
) -> Result<RemovalReceipt> {
    let scope = &p.scope;
    if let Some(profile) = e
        .intent
        .execution_profile()
        .filter(|v| v.policy_proof().is_some())
    {
        let proof = retirement_intent::policy::Proof::load(
            e.nodes,
            profile,
            e.intent.review(),
            e.intent.decision(),
        )
        .map_err(RemovalError::Progress)?;
        proof
            .check_removal(
                &scope.store,
                &scope.object,
                e.ledger.identity(),
                p.provider_custody.as_ref(),
            )
            .map_err(RemovalError::Progress)?;
    }
    check_directory(e.store, &scope.object)?;
    match (&p.custody, &p.provider_custody) {
        (Some(profile), None) => custody::validate(e.store, e.intent, scope, profile)?,
        (None, Some(profile)) => custody::portable::validate(e.store, e.intent, scope, profile)?,
        (None, None) => {
            if scope.preservation.is_some() {
                return Err(RemovalError::InvalidPlan);
            }
            consent(e.store, e.intent, &scope.object)?;
        }
        _ => return Err(RemovalError::InvalidPlan),
    }
    if encode(p)?.len() > 48 * 1024 {
        return Err(Error::Budget.into());
    }
    let (quantum, limit) = budget(e.store)?;
    if scope.store != e.store.instance
        || scope.owner != *e.intent.owner()
        || scope.attempt == 0
        || scope.decision
            != e.intent
                .decision()
                .digest()
                .map_err(|_| RemovalError::InvalidPlan)?
        || scope.operation != e.intent.decision().approval().operation
        || p.quantum != quantum
        || p.limit != limit
    {
        return Err(RemovalError::InvalidPlan);
    }
    let bp = path(e.store, &scope.object, &scope.incarnation, "begin");
    let dp = path(e.store, &scope.object, &scope.incarnation, "done");
    let completed = path_present(&dp)?;
    admitted(&mut e, p, completed)?;
    progress::local_work::check(e.journal, e.nodes, e.intent, p, e.job)
        .map_err(RemovalError::Progress)?;
    let id = Digest::blob(&encode(&(BEGIN, p))?);
    let op = Key::parse(&format!("remove-{}", id.as_str()))?;
    let settle = Key::parse(&format!("removed-{}", id.as_str()))?;
    let binding = Digest::blob(&encode(&(p, e.job))?);
    let allocation = e
        .ledger
        .begin_handoff(
            op.clone(),
            e.job,
            scope.owner.clone(),
            binding.clone(),
            limit,
        )?
        .reference()
        .clone();
    let existing = if path_present(&bp)? {
        Some(read_record::<Begin>(&bp)?)
    } else {
        None
    };
    if let Some(b) = &existing
        && (b.schema != BEGIN || b.plan != *p || &b.job != e.job || b.allocation != allocation)
    {
        return Err(RemovalError::InvalidPlan);
    }
    let finish = |e: &mut Execution<'_, N, P>, b: &Begin, charge| -> Result<RemovalReceipt> {
        let done: Disposition = read_record(&dp)?;
        check_disposition(b, &done)?;
        Ok(RemovalReceipt {
            evidence: Digest::blob(&encode(&done)?),
            file_data_before: b.file_data_before,
            charged_metadata: charge,
            settlement: e.ledger.settle_handoff(
                settle.clone(),
                &allocation,
                binding.clone(),
                charge,
            )?,
        })
    };
    if completed {
        let b = existing.as_ref().ok_or(RemovalError::InvalidPlan)?;
        // A completed old incarnation is historical. Never inspect/unlink a new
        // incarnation's payload, even when settlement/reporting was interrupted.
        let local = e.store.existing_attempt(&Request {
            operation: op,
            owner: scope.owner.clone(),
            object: scope.object.clone(),
            required: AckLevel::ProcessRestart,
        })?;
        local.check_binding(&(p, e.job, &allocation))?;
        let charge = footprint(&local, &bp, &dp)?;
        if charge > limit {
            return Err(Error::Budget.into());
        }
        return finish(&mut e, b, charge);
    }
    if e.ledger.handoff_status(&allocation, &binding)?.is_some() {
        return Err(RemovalError::InvalidPlan);
    }
    let local = e.store.attempt(&Request {
        operation: op,
        owner: scope.owner.clone(),
        object: scope.object.clone(),
        required: AckLevel::ProcessRestart,
    })?;
    local.bind(&(p, e.job, &allocation))?;
    hook(Stage::Bound)?;
    let _fence = e.store.try_exclusive_object(&scope.object)?;
    admitted(&mut e, p, false)?;
    progress::local_work::check(e.journal, e.nodes, e.intent, p, e.job)
        .map_err(RemovalError::Progress)?;
    let current = state(e.store, &scope.object)?;
    if current.incarnation != scope.incarnation
        || current.depth == MAX_REMOVALS
        || current.pending.as_ref().is_some_and(|b| b.plan != *p)
    {
        return Err(RemovalError::InvalidPlan);
    }
    let mut protection = e.protection.acquire(scope)?;
    let proof = protection.revalidate(scope)?;
    let mut keepers = acquire(scope)?;
    let begin = if let Some(b) = existing {
        b
    } else {
        let file_data_before = verify_payload(e.store, &scope.object)?;
        let b = Begin {
            schema: BEGIN.into(),
            plan: p.clone(),
            job: e.job.clone(),
            allocation: allocation.clone(),
            file_data_before,
            protection: proof,
        };
        let dir = bp.parent().ok_or(Error::InvalidRoot)?;
        match fs::create_dir(dir) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(Error::from(e).into()),
        }
        directory(dir)?;
        local.store.write_record(&bp, &b)?;
        b
    };
    hook(Stage::Marked)?;
    let payload = e.store.object_path(&scope.object);
    let present = path_present(&payload)?;
    if present {
        verify_payload(e.store, &scope.object)?;
    }
    protection.revalidate(scope)?;
    keepers.revalidate()?;
    keepers.fresh()?;
    if present {
        let _policy = retirement_intent::policy::require_effect_authority(e.intent, e.nodes)
            .map_err(RemovalError::Progress)?;
        fs::remove_file(&payload).map_err(Error::from)?;
    }
    hook(Stage::Unlinked)?;
    local.store.write_record(&dp, &disposition(&begin)?)?;
    hook(Stage::Completed)?;
    let charge = footprint(&local, &bp, &dp)?;
    if charge > limit {
        return Err(Error::Budget.into());
    }
    let result = finish(&mut e, &begin, charge)?;
    hook(Stage::Settled)?;
    Ok(result)
}
fn verify_payload(store: &LocalStore, object: &Object) -> crate::Result<u64> {
    let p = store.object_path(object);
    regular(&p)?;
    let mut file = File::open(p)?;
    if file.metadata()?.len() != object.bytes {
        return Err(Error::Integrity);
    }
    copy_verified(&mut file, &mut io::sink(), object)?;
    Ok(fs2::FileExt::allocated_size(&file)?)
}
fn footprint(local: &LocalAttempt, bp: &Path, dp: &Path) -> crate::Result<u64> {
    let mut files = local.files()?;
    files.extend([bp.to_path_buf(), dp.to_path_buf()]);
    let mut charge = 0u64;
    for path in files {
        regular(&path)?;
        charge = charge
            .checked_add(fs2::FileExt::allocated_size(&File::open(path)?)?)
            .ok_or(Error::Budget)?;
    }
    Ok(charge)
}
#[cfg(test)]
#[path = "removal_tests.rs"]
pub(crate) mod tests;
