//! Explicit ordinary replica retirement. No final-copy deletion or live adoption.
use crate::{
    AckLevel, Key, Request as Publication,
    accounted::{self, AccountedError, AccountedReceipt, PublicationPlan},
    ledger::{CapacityLedger, JournalRef, LedgerError},
    local::{
        LocalStore, MAX_SCOPE_FILES,
        lifecycle::{self, Birth},
    },
    protection::Limits,
    replica::ReplicaEnrollment,
    retention::{self, RetainedReplica, RetentionError, RetentionGrant},
    source_claims::admission::Namespace,
    store_protection::{self, Checked, Enrollment},
};
use git_asset_core::{Digest, tree::NodeStore};
use serde::{Deserialize, Serialize};

const INTENT: &str = "tig.replica-retirement-intent.design-1";
const DONE: &str = "tig.replica-retirement-completion.design-1";
pub mod status;
#[derive(Debug)]
pub enum Error {
    InvalidPlan,
    WrongAuthorization,
    MissingCustody,
    PendingPartial,
    Injected,
    Store(crate::Error),
    Ledger(LedgerError),
    Retention(RetentionError),
    Publication(AccountedError),
    Protection(store_protection::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<crate::Error> for Error {
    fn from(e: crate::Error) -> Self {
        Self::Store(e)
    }
}
impl From<LedgerError> for Error {
    fn from(e: LedgerError) -> Self {
        Self::Ledger(e)
    }
}
impl From<RetentionError> for Error {
    fn from(e: RetentionError) -> Self {
        Self::Retention(e)
    }
}
impl From<AccountedError> for Error {
    fn from(e: AccountedError) -> Self {
        Self::Publication(e)
    }
}
impl From<store_protection::Error> for Error {
    fn from(e: store_protection::Error) -> Self {
        Self::Protection(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keeper {
    pub grant: RetentionGrant,
    pub enrollment: ReplicaEnrollment,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub operation: Key,
    pub owner: Key,
    pub publication: Publication,
    pub context: Digest,
    pub source: ReplicaEnrollment,
    pub keepers: Vec<Keeper>,
    pub required_domains: usize,
    pub capacity: Digest,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    schema: String,
    pub(crate) enrollment: Enrollment,
    pub(crate) birth: Birth,
    pub(crate) publication: Publication,
    request: Request,
    limit: u64,
}
impl Plan {
    pub fn request(&self) -> &Request {
        &self.request
    }
    pub fn enrollment(&self) -> &Enrollment {
        &self.enrollment
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.id()?;
        Ok(lifecycle::encode(self)?)
    }
    pub fn from_json(raw: &[u8]) -> Result<Self> {
        if raw.len() > 48 * 1024 {
            return Err(Error::InvalidPlan);
        }
        let value = git_asset_core::identity::parse_json(raw).map_err(|_| Error::InvalidPlan)?;
        let plan: Self = serde_json::from_value(value).map_err(|_| Error::InvalidPlan)?;
        if plan.to_json()? != raw {
            return Err(Error::InvalidPlan);
        }
        Ok(plan)
    }
    pub fn id(&self) -> Result<Digest> {
        if self.schema != "tig.replica-retirement-plan.design-1"
            || self.request.keepers.len() > 16
            || self.birth.index >= lifecycle::MAX_BIRTHS - 1
        {
            return Err(Error::InvalidPlan);
        }
        Ok(lifecycle::digest(self)?)
    }
    pub fn required_peak(&self) -> u64 {
        self.limit
    }
    pub fn authorize(&self) -> Result<Authorization> {
        Ok(Authorization { plan: self.id()? })
    }
    fn validate(&self, store: &LocalStore) -> Result<()> {
        self.enrollment.check(store)?;
        validate_request(store, &self.request)?;
        if self.publication != self.request.publication
            || self.schema != "tig.replica-retirement-plan.design-1"
            || self
                .request
                .keepers
                .windows(2)
                .any(|k| k[0].enrollment.store >= k[1].enrollment.store)
            || self.limit != metadata_peak(store)?
            || self.birth.index >= lifecycle::MAX_BIRTHS - 1
            || lifecycle::encode(self)?.len() > 48 * 1024
        {
            return Err(Error::InvalidPlan);
        }
        Ok(())
    }
}
pub struct Authorization {
    plan: Digest,
}
fn metadata_peak(store: &LocalStore) -> Result<u64> {
    if !store.supports_retirement() {
        return Err(Error::InvalidPlan);
    }
    let quantum = store.allocation_quantum()?;
    65536u64
        .checked_add(quantum - 1)
        .and_then(|n| n.checked_div(quantum))
        .and_then(|n| n.checked_mul(quantum))
        .and_then(|n| {
            n.checked_mul(
                (MAX_SCOPE_FILES
                    + if store.final_policy().is_some() {
                        lifecycle::MAX_FINAL_FILES
                    } else {
                        lifecycle::MAX_FILES
                    }
                    + 3) as u64,
            )
        })
        .ok_or(Error::InvalidPlan)
}
fn validate_request(store: &LocalStore, request: &Request) -> Result<()> {
    if request.owner != request.publication.owner
        || request.source.store != *store.identity()
        || request.keepers.is_empty()
        || request.keepers.len() > 16
        || request.required_domains == 0
        || request.required_domains > request.keepers.len()
        || request.publication.required != AckLevel::ProcessRestart
    {
        return Err(Error::InvalidPlan);
    }
    for keeper in &request.keepers {
        if keeper.grant.store() != &keeper.enrollment.store
            || keeper.grant.request().publication.object != request.publication.object
        {
            return Err(Error::InvalidPlan);
        }
    }
    Ok(())
}
pub fn plan(store: &LocalStore, mut request: Request) -> Result<Plan> {
    validate_request(store, &request)?;
    request
        .keepers
        .sort_by(|a, b| a.enrollment.store.cmp(&b.enrollment.store));
    let planned = Plan {
        schema: "tig.replica-retirement-plan.design-1".into(),
        enrollment: Enrollment::bind(store)?,
        birth: store.current_birth(&request.publication)?,
        publication: request.publication.clone(),
        request,
        limit: metadata_peak(store)?,
    };
    planned.validate(store)?;
    Ok(planned)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Intent {
    schema: String,
    pub plan: Plan,
    job: JournalRef,
    allocation: JournalRef,
    checked: Vec<Checked>,
    allocated: u64,
}
impl Intent {
    pub(crate) fn validate(&self, store: &LocalStore, birth: &Birth) -> crate::Result<()> {
        if self.schema != INTENT
            || lifecycle::digest(&self.plan.birth)? != lifecycle::digest(birth)?
            || self.plan.validate(store).is_err()
        {
            return Err(crate::Error::CorruptRecord);
        }
        let members = store
            .protection()
            .ok_or(crate::Error::CorruptRecord)?
            .members();
        if self.checked.len() != members.len()
            || self
                .checked
                .iter()
                .zip(members)
                .any(|(c, m)| c.namespace != m.namespace || c.checkpoint.identity() != &m.namespace)
        {
            return Err(crate::Error::CorruptRecord);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Completion {
    schema: String,
    intent: Digest,
    pub(crate) cancelled: bool,
}
impl Completion {
    pub(crate) fn aborted(intent: &Intent) -> crate::Result<Self> {
        Ok(Self {
            schema: "tig.replica-retirement-abort.design-1".into(),
            intent: lifecycle::digest(intent)?,
            cancelled: true,
        })
    }
    pub(crate) fn new(intent: &Intent) -> crate::Result<Self> {
        Ok(Self {
            schema: DONE.into(),
            intent: lifecycle::digest(intent)?,
            cancelled: false,
        })
    }
    pub(crate) fn validate(&self, intent: &Intent) -> crate::Result<()> {
        if self.schema
            != (if self.cancelled {
                "tig.replica-retirement-abort.design-1"
            } else {
                DONE
            })
            || self.intent != lifecycle::digest(intent)?
        {
            return Err(crate::Error::CorruptRecord);
        }
        Ok(())
    }
}
#[derive(Debug, Serialize)]
pub struct Receipt {
    pub schema: &'static str,
    pub plan: Digest,
    pub logical_bytes: u64,
    pub file_allocation_before: u64,
    pub historical_replay: bool,
    pub charged_footprint: u64,
    pub cancelled: bool,
}
/// Private profiles supply actual existing roots/ledgers; grants are reverified.
pub struct Backup<'a> {
    pub store: &'a LocalStore,
    pub ledger: &'a mut CapacityLedger,
    pub enrollment: &'a ReplicaEnrollment,
}
pub struct Execution<'a, 'b, S> {
    pub store: &'a LocalStore,
    pub ledger: &'a mut CapacityLedger,
    pub job: &'a JournalRef,
    pub namespaces: &'a mut [Namespace],
    pub nodes: &'a S,
    pub limits: &'a Limits,
    pub backups: &'a mut [Backup<'b>],
    pub admitted_context: &'a Digest,
    pub source_enrollment: &'a ReplicaEnrollment,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Admitted,
    Bound,
    Assessed,
    KeepersVerified,
    Intent,
    Unlinked,
    Completed,
    Settled,
    ReacquisitionBorn,
    Aborted,
    CancellationBorn,
}
fn keys(store: &LocalStore, operation: &Key, prefix: &str) -> crate::Result<(Key, Key)> {
    let id = lifecycle::digest(&(store.identity(), operation))?;
    Ok((
        Key::parse(&format!("{prefix}-{}", id.as_str()))?,
        Key::parse(&format!("settle-{prefix}-{}", id.as_str()))?,
    ))
}
impl<S: NodeStore> Execution<'_, '_, S> {
    pub fn cancel(&mut self, plan: &Plan, authorization: &Authorization) -> Result<Receipt> {
        self.cancel_hook(plan, authorization, |_| Ok(()))
    }
    fn cancel_hook(
        &mut self,
        plan: &Plan,
        authorization: &Authorization,
        mut hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        plan.validate(self.store)?;
        if self.ledger.identity() != &plan.request.capacity {
            return Err(Error::InvalidPlan);
        }
        if authorization.plan != plan.id()? {
            return Err(Error::WrongAuthorization);
        }
        let (operation, settle) = keys(self.store, &plan.request.operation, "evict")?;
        let binding = lifecycle::digest(&(plan, self.job))?;
        let allocation = self
            .ledger
            .begin_retention(
                operation.clone(),
                self.job,
                plan.request.owner.clone(),
                binding.clone(),
                plan.limit,
            )?
            .reference()
            .clone();
        let scope = Publication {
            operation,
            owner: plan.request.owner.clone(),
            object: plan.publication.object.clone(),
            required: AckLevel::ProcessRestart,
        };
        let attempt = self.store.attempt(&scope)?;
        attempt.bind(&(plan, self.job, &allocation))?;
        if attempt.has_partials()? {
            return Err(Error::PendingPartial);
        }
        let intent = self
            .store
            .retirement_intent(&scope.object, plan.birth.index)?
            .ok_or(Error::InvalidPlan)?;
        intent.validate(self.store, &plan.birth)?;
        if lifecycle::encode(&intent.plan)? != lifecycle::encode(plan)?
            || intent.job != *self.job
            || intent.allocation != allocation
            || self
                .store
                .retirement_done(&scope.object, plan.birth.index)?
                .is_some()
        {
            return Err(Error::InvalidPlan);
        }
        let already_settled = self
            .ledger
            .retention_status(&allocation, &binding)?
            .is_some();
        if already_settled
            && self
                .store
                .retirement_aborted(&scope.object, plan.birth.index)?
                .is_none()
        {
            return Err(crate::Error::NeedsRecovery.into());
        }
        if !already_settled {
            let head = self
                .store
                .life_head(&scope.object)?
                .ok_or(Error::InvalidPlan)?;
            if lifecycle::digest(&head.birth)? == lifecycle::digest(&plan.birth)? {
                let fence = self.store.handoff_fence(&scope.object)?;
                if self
                    .store
                    .retirement_aborted(&scope.object, plan.birth.index)?
                    .is_none()
                {
                    attempt.managed_store().abort_retirement(&intent, &fence)?;
                }
                hook(Stage::Aborted)?;
                attempt
                    .managed_store()
                    .continue_cancelled_retirement(&intent, &fence)?;
                hook(Stage::CancellationBorn)?;
            }
        }
        finish(
            self.ledger,
            self.store,
            self.job,
            plan,
            &attempt,
            &intent,
            &allocation,
            &binding,
            settle,
            true,
        )
    }
    pub fn apply(&mut self, plan: &Plan, authorization: &Authorization) -> Result<Receipt> {
        self.apply_hook(plan, authorization, |_| Ok(()))
    }
    #[cfg(test)]
    pub(crate) fn fixture_pending_intent(&mut self, plan: &Plan) -> Result<Receipt> {
        self.apply_hook(plan, &plan.authorize()?, |stage| {
            if stage == Stage::Intent {
                Err(Error::Injected)
            } else {
                Ok(())
            }
        })
    }
    fn apply_hook(
        &mut self,
        plan: &Plan,
        authorization: &Authorization,
        mut hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        plan.validate(self.store)?;
        if self.ledger.identity() != &plan.request.capacity {
            return Err(Error::InvalidPlan);
        }
        if self.admitted_context != &plan.request.context
            || self.source_enrollment != &plan.request.source
        {
            return Err(Error::InvalidPlan);
        }
        if authorization.plan != plan.id()? {
            return Err(Error::WrongAuthorization);
        }
        let (operation, settle) = keys(self.store, &plan.request.operation, "evict")?;
        let binding = lifecycle::digest(&(plan, self.job))?;
        let allocation = self
            .ledger
            .begin_retention(
                operation.clone(),
                self.job,
                plan.request.owner.clone(),
                binding.clone(),
                plan.limit,
            )?
            .reference()
            .clone();
        hook(Stage::Admitted)?;
        let scope = Publication {
            operation,
            owner: plan.request.owner.clone(),
            object: plan.publication.object.clone(),
            required: AckLevel::ProcessRestart,
        };
        let attempt = self.store.attempt(&scope)?;
        attempt.bind(&(plan, self.job, &allocation))?;
        if attempt.has_partials()? {
            return Err(Error::PendingPartial);
        }
        hook(Stage::Bound)?;
        let existing = self
            .store
            .retirement_intent(&scope.object, plan.birth.index)?;
        let settled = self
            .ledger
            .retention_status(&allocation, &binding)?
            .is_some();
        if settled
            && (existing.is_none()
                || (self
                    .store
                    .retirement_done(&scope.object, plan.birth.index)?
                    .is_none()
                    && self
                        .store
                        .retirement_aborted(&scope.object, plan.birth.index)?
                        .is_none()))
        {
            return Err(crate::Error::NeedsRecovery.into());
        }
        if let Some(intent) = &existing {
            intent.validate(self.store, &plan.birth)?;
            if lifecycle::encode(&intent.plan)? != lifecycle::encode(plan)?
                || intent.job != *self.job
                || intent.allocation != allocation
            {
                return Err(Error::InvalidPlan);
            }
            if let Some(aborted) = self
                .store
                .retirement_aborted(&scope.object, plan.birth.index)?
            {
                aborted.validate(intent)?;
                if self
                    .store
                    .retirement_done(&scope.object, plan.birth.index)?
                    .is_some()
                {
                    return Err(Error::InvalidPlan);
                }
                if self
                    .ledger
                    .retention_status(&allocation, &binding)?
                    .is_none()
                {
                    let head = self
                        .store
                        .life_head(&scope.object)?
                        .ok_or(Error::InvalidPlan)?;
                    if lifecycle::digest(&head.birth)? == lifecycle::digest(&plan.birth)? {
                        let fence = self.store.handoff_fence(&scope.object)?;
                        attempt
                            .managed_store()
                            .continue_cancelled_retirement(intent, &fence)?;
                    }
                }
                return finish(
                    self.ledger,
                    self.store,
                    self.job,
                    plan,
                    &attempt,
                    intent,
                    &allocation,
                    &binding,
                    settle,
                    true,
                );
            }
            if let Some(done) = self
                .store
                .retirement_done(&scope.object, plan.birth.index)?
            {
                done.validate(intent)?;
                return finish(
                    self.ledger,
                    self.store,
                    self.job,
                    plan,
                    &attempt,
                    intent,
                    &allocation,
                    &binding,
                    settle,
                    true,
                );
            }
            let fence = self.store.existing_exclusive_object(&scope.object)?;
            if !self.store.object_present(&scope.object)? {
                attempt
                    .managed_store()
                    .complete_retirement(intent, &fence)?;
                hook(Stage::Completed)?;
                return finish(
                    self.ledger,
                    self.store,
                    self.job,
                    plan,
                    &attempt,
                    intent,
                    &allocation,
                    &binding,
                    settle,
                    true,
                );
            }
        }
        let assessment = store_protection::assess(
            self.store,
            &plan.enrollment,
            &scope.object,
            self.namespaces,
            self.nodes,
            self.limits,
        )?;
        hook(Stage::Assessed)?;
        let allocated = self.store.verify_retirement_source(
            &plan.publication,
            &plan.birth,
            assessment.provider(),
        )?;
        if self.backups.len() != plan.request.keepers.len() {
            return Err(Error::MissingCustody);
        }
        let mut retained = Vec::<RetainedReplica>::new();
        let mut enrollments = vec![plan.request.source.clone()];
        for keeper in &plan.request.keepers {
            let mut matching = self
                .backups
                .iter_mut()
                .filter(|b| b.store.identity() == keeper.grant.store());
            let backup = matching.next().ok_or(Error::MissingCustody)?;
            if backup.enrollment != &keeper.enrollment {
                return Err(Error::MissingCustody);
            }
            if matching.next().is_some() {
                return Err(Error::MissingCustody);
            }
            retained.push(retention::verify(
                backup.ledger,
                backup.store,
                &keeper.grant,
            )?);
            enrollments.push(keeper.enrollment.clone());
        }
        if !retention::coverage_subject(
            &scope.object,
            self.store.identity(),
            &retained,
            &enrollments,
            plan.request.required_domains,
            plan.publication.required,
        )? {
            return Err(Error::MissingCustody);
        }
        hook(Stage::KeepersVerified)?;
        let intent = existing.unwrap_or(Intent {
            schema: INTENT.into(),
            plan: plan.clone(),
            job: self.job.clone(),
            allocation: allocation.clone(),
            checked: assessment.summary().checked.to_vec(),
            allocated,
        });
        attempt
            .managed_store()
            .install_retirement(&intent, assessment.provider())?;
        hook(Stage::Intent)?;
        assessment.check_current()?;
        for (keeper, live) in plan.request.keepers.iter().zip(&mut retained) {
            let backup = self
                .backups
                .iter_mut()
                .find(|b| b.store.identity() == keeper.grant.store())
                .ok_or(Error::MissingCustody)?;
            live.revalidate(backup.ledger, backup.store)?;
        }
        if !retention::coverage_subject(
            &scope.object,
            self.store.identity(),
            &retained,
            &enrollments,
            plan.request.required_domains,
            plan.publication.required,
        )? {
            return Err(Error::MissingCustody);
        }
        self.store
            .unlink_retirement(&intent, assessment.provider())?;
        hook(Stage::Unlinked)?;
        attempt
            .managed_store()
            .complete_retirement(&intent, assessment.provider())?;
        hook(Stage::Completed)?;
        let receipt = finish(
            self.ledger,
            self.store,
            self.job,
            plan,
            &attempt,
            &intent,
            &allocation,
            &binding,
            settle,
            false,
        )?;
        hook(Stage::Settled)?;
        Ok(receipt)
    }
}
#[allow(clippy::too_many_arguments)]
fn finish(
    ledger: &mut CapacityLedger,
    store: &LocalStore,
    _job: &JournalRef,
    plan: &Plan,
    attempt: &crate::local::LocalAttempt,
    intent: &Intent,
    allocation: &JournalRef,
    binding: &Digest,
    settle: Key,
    replay: bool,
) -> Result<Receipt> {
    let charge = match ledger.retention_status(allocation, binding)? {
        Some(n) => n,
        None => attempt.lifecycle_footprint(&plan.publication.object)?,
    };
    if charge > plan.limit {
        return Err(Error::InvalidPlan);
    }
    ledger.settle_retention(settle, allocation, binding.clone(), charge)?;
    let cancelled = store
        .retirement_aborted(&plan.publication.object, plan.birth.index)?
        .is_some();
    Ok(Receipt {
        schema: "tig.replica-retirement-receipt.design-1",
        plan: plan.id()?,
        logical_bytes: if cancelled {
            0
        } else {
            plan.publication.object.bytes
        },
        file_allocation_before: intent.allocated,
        historical_replay: replay,
        charged_footprint: charge,
        cancelled,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReacquisitionPlan {
    schema: String,
    enrollment: Enrollment,
    previous: Completion,
    birth: Birth,
    publication: Publication,
    context: Digest,
    payload: PublicationPlan,
    limit: u64,
    capacity: Digest,
}
impl ReacquisitionPlan {
    pub(crate) fn payload_plan(&self) -> &PublicationPlan {
        &self.payload
    }
    pub fn publication(&self) -> &Publication {
        &self.publication
    }
    pub fn enrollment(&self) -> &Enrollment {
        &self.enrollment
    }
    pub fn context(&self) -> &Digest {
        &self.context
    }
    pub fn capacity(&self) -> &Digest {
        &self.capacity
    }
    fn validate(&self, store: &LocalStore) -> Result<()> {
        self.enrollment.check(store)?;
        self.id()?;
        if self.limit != metadata_peak(store)? {
            return Err(Error::InvalidPlan);
        }
        let expected = accounted::plan_bound(
            store,
            &self.publication,
            lifecycle::digest(&("reacquire", &self.birth, &self.context))?,
        )?;
        if lifecycle::encode(&expected)? != lifecycle::encode(&self.payload)? {
            return Err(Error::InvalidPlan);
        }
        Ok(())
    }
    pub fn id(&self) -> Result<Digest> {
        if self.schema != "tig.replica-reacquisition-plan.design-1"
            || self.birth.index == 0
            || self.birth.index >= lifecycle::MAX_BIRTHS
        {
            return Err(Error::InvalidPlan);
        }
        Ok(lifecycle::digest(self)?)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.id()?;
        Ok(lifecycle::encode(self)?)
    }
    pub fn from_json(raw: &[u8]) -> Result<Self> {
        if raw.len() > 65536 {
            return Err(Error::InvalidPlan);
        }
        let value = git_asset_core::identity::parse_json(raw).map_err(|_| Error::InvalidPlan)?;
        let plan: Self = serde_json::from_value(value).map_err(|_| Error::InvalidPlan)?;
        if plan.to_json()? != raw {
            return Err(Error::InvalidPlan);
        }
        Ok(plan)
    }
    pub fn required_peak(&self) -> Result<u64> {
        self.limit
            .checked_add(self.payload.required_peak())
            .ok_or(Error::InvalidPlan)
    }
    pub fn authorize(&self) -> Result<Authorization> {
        Ok(Authorization {
            plan: lifecycle::digest(self)?,
        })
    }
}
pub fn plan_reacquisition(
    store: &LocalStore,
    retired: &Plan,
    publication: Publication,
    context: Digest,
) -> Result<ReacquisitionPlan> {
    retired.validate(store)?;
    let intent = store
        .retirement_intent(&retired.publication.object, retired.birth.index)?
        .ok_or(Error::InvalidPlan)?;
    intent.validate(store, &retired.birth)?;
    if lifecycle::encode(&intent.plan)? != lifecycle::encode(retired)? {
        return Err(Error::InvalidPlan);
    }
    let previous = store
        .retirement_done(&publication.object, retired.birth.index)?
        .ok_or(Error::InvalidPlan)?;
    previous.validate(&intent)?;
    let birth = store.next_birth(&retired.publication.object, &previous, &publication)?;
    let payload = accounted::plan_bound(
        store,
        &publication,
        lifecycle::digest(&("reacquire", &birth, &context))?,
    )?;
    Ok(ReacquisitionPlan {
        schema: "tig.replica-reacquisition-plan.design-1".into(),
        enrollment: Enrollment::bind(store)?,
        previous,
        birth,
        publication,
        context,
        payload,
        limit: metadata_peak(store)?,
        capacity: retired.request.capacity.clone(),
    })
}
pub struct Reacquisition<'a> {
    pub store: &'a LocalStore,
    pub ledger: &'a mut CapacityLedger,
    pub job: &'a JournalRef,
}
/// Reconstruct the original typed plan after its new birth was persisted. The
/// per-object chain and original context/ledger bindings still verify on apply.
pub(crate) fn resume_reacquisition(
    store: &LocalStore,
    birth: Birth,
    context: Digest,
) -> Result<ReacquisitionPlan> {
    let publication = birth.reacquire.clone().ok_or(Error::InvalidPlan)?;
    let old_index = birth.index.checked_sub(1).ok_or(Error::InvalidPlan)?;
    let intent = store
        .retirement_intent(&publication.object, old_index)?
        .ok_or(Error::InvalidPlan)?;
    let previous = store
        .retirement_done(&publication.object, old_index)?
        .ok_or(Error::InvalidPlan)?;
    previous.validate(&intent)?;
    let payload = accounted::plan_bound(
        store,
        &publication,
        lifecycle::digest(&("reacquire", &birth, &context))?,
    )?;
    let plan = ReacquisitionPlan {
        schema: "tig.replica-reacquisition-plan.design-1".into(),
        enrollment: Enrollment::bind(store)?,
        previous,
        birth,
        publication,
        context,
        payload,
        limit: metadata_peak(store)?,
        capacity: intent.plan.request.capacity,
    };
    plan.validate(store)?;
    Ok(plan)
}
impl Reacquisition<'_> {
    pub fn apply(
        &mut self,
        plan: &ReacquisitionPlan,
        authorization: &Authorization,
        source: &mut dyn std::io::Read,
    ) -> Result<AccountedReceipt> {
        self.apply_hook(plan, authorization, source, |_| Ok(()))
    }
    fn apply_hook(
        &mut self,
        plan: &ReacquisitionPlan,
        authorization: &Authorization,
        source: &mut dyn std::io::Read,
        mut hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<AccountedReceipt> {
        plan.validate(self.store)?;
        if self.ledger.identity() != &plan.capacity {
            return Err(Error::InvalidPlan);
        }
        if authorization.plan != lifecycle::digest(plan)?
            || plan.limit != metadata_peak(self.store)?
            || plan.birth.index == 0
            || plan.birth.index >= lifecycle::MAX_BIRTHS
        {
            return Err(Error::WrongAuthorization);
        }
        let (operation, settle) = keys(self.store, &plan.publication.operation, "reacquire")?;
        let binding = lifecycle::digest(&(plan, self.job))?;
        let allocation = self
            .ledger
            .begin_retention(
                operation.clone(),
                self.job,
                plan.publication.owner.clone(),
                binding.clone(),
                plan.limit,
            )?
            .reference()
            .clone();
        let payload_allocation =
            accounted::admit(self.ledger, self.store, self.job, &plan.payload)?;
        if self
            .ledger
            .retention_status(&allocation, &binding)?
            .is_some()
            && self
                .ledger
                .publication_status(
                    &payload_allocation,
                    &lifecycle::digest(&(&plan.payload, self.job))?,
                )?
                .is_some()
        {
            return Ok(accounted::publish_accounted(
                self.ledger,
                self.store,
                self.job,
                &plan.payload,
                source,
            )?);
        }
        let payload_attempt = accounted::prepare(
            self.ledger,
            self.store,
            self.job,
            &plan.payload,
            &payload_allocation,
        )?;
        let scope = Publication {
            operation,
            owner: plan.publication.owner.clone(),
            object: plan.publication.object.clone(),
            required: plan.publication.required,
        };
        let attempt = self.store.attempt(&scope)?;
        attempt.bind(&(plan, self.job, &allocation))?;
        if attempt.has_partials()? {
            return Err(Error::PendingPartial);
        }
        let fence = self.store.existing_exclusive_object(&scope.object)?;
        let head = self
            .store
            .life_head(&scope.object)?
            .ok_or(Error::InvalidPlan)?;
        if lifecycle::digest(&head.birth)? != lifecycle::digest(&plan.birth)? {
            if payload_attempt.has_partials()? {
                return Err(Error::PendingPartial);
            }
            let expected =
                self.store
                    .next_birth(&scope.object, &plan.previous, &plan.publication)?;
            if lifecycle::digest(&expected)? != lifecycle::digest(&plan.birth)? {
                return Err(Error::InvalidPlan);
            }
            attempt
                .managed_store()
                .begin_reacquisition(&plan.birth, &fence)?;
        }
        hook(Stage::ReacquisitionBorn)?;
        let charge = match self.ledger.retention_status(&allocation, &binding)? {
            Some(n) => n,
            None => attempt.lifecycle_footprint(&scope.object)?,
        };
        if charge > plan.limit {
            return Err(Error::InvalidPlan);
        }
        self.ledger
            .settle_retention(settle, &allocation, binding, charge)?;
        drop(fence);
        drop(payload_attempt);
        Ok(accounted::publish_accounted(
            self.ledger,
            self.store,
            self.job,
            &plan.payload,
            source,
        )?)
    }
}

#[cfg(test)]
mod tests;
