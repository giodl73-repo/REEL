//! Explicit bounded local final retirement. No live enrollment or automatic apply.
pub mod reacquire;
pub mod status;
use super::claims;
use crate::{
    AckLevel, Key, Request as Publication,
    ledger::{CapacityLedger, JournalRef, LedgerError},
    local::{
        LocalStore,
        lifecycle::{self, Birth},
    },
    retention::RetentionGrant,
    source_claims::admission::Namespace,
    store_protection::{Checked, Enrollment},
};
use git_asset_core::{Digest, Object, tree::NodeStore};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum Error {
    Invalid,
    Unsupported,
    WrongAuthority,
    Clock,
    ProtectedGrant,
    Partial,
    Recovery,
    Injected,
    Store(crate::Error),
    Ledger(LedgerError),
    Claims(claims::Error),
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
impl From<claims::Error> for Error {
    fn from(e: claims::Error) -> Self {
        Self::Claims(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;
fn encode(v: &impl Serialize) -> Result<Vec<u8>> {
    Ok(lifecycle::encode(v)?)
}
fn digest(v: &impl Serialize) -> Result<Digest> {
    Ok(lifecycle::digest(v)?)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    schema: String,
    pub authority: Digest,
    pub epoch: Digest,
    pub scope: Digest,
    pub clock: Digest,
    pub owners: Vec<Key>,
    pub max_window: u64,
}
impl Policy {
    pub fn new(
        authority: Digest,
        epoch: Digest,
        scope: Digest,
        clock: Digest,
        mut owners: Vec<Key>,
        max_window: u64,
    ) -> Result<Self> {
        owners.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        let p = Self {
            schema: "tig.final-policy.design-1".into(),
            authority,
            epoch,
            scope,
            clock,
            owners,
            max_window,
        };
        p.id()?;
        Ok(p)
    }
    pub fn id(&self) -> Result<Digest> {
        if self.schema != "tig.final-policy.design-1"
            || self.owners.is_empty()
            || self.owners.len() > 16
            || self
                .owners
                .windows(2)
                .any(|p| p[0].as_str() >= p[1].as_str())
            || self.max_window == 0
            || self.max_window > 604_800_000
        {
            return Err(Error::Invalid);
        }
        digest(self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub not_before: u64,
    pub expires: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub operation: Key,
    pub owner: Key,
    pub publication: Publication,
    pub capacity: Digest,
    pub context: Digest,
    pub window: Window,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    schema: String,
    pub(crate) enrollment: Enrollment,
    pub(crate) policy: Policy,
    pub(crate) birth: Birth,
    pub(crate) request: Request,
    pub(crate) grants: Vec<RetentionGrant>,
    limit: u64,
}
impl Plan {
    pub fn request(&self) -> &Request {
        &self.request
    }
    pub fn grants(&self) -> &[RetentionGrant] {
        &self.grants
    }
    pub fn required_peak(&self) -> u64 {
        self.limit
    }
    pub fn id(&self) -> Result<Digest> {
        self.policy.id()?;
        let r = &self.request;
        if self.schema != "tig.final-retirement-plan.design-1"
            || r.owner != r.publication.owner
            || !self.policy.owners.contains(&r.owner)
            || r.publication.required != AckLevel::ProcessRestart
            || r.publication.object.bytes > crate::local::MAX_OBJECT_BYTES
            || r.window.expires <= r.window.not_before
            || r.window.expires - r.window.not_before > self.policy.max_window
            || self.birth.object != r.publication.object
            || &self.birth.store != self.enrollment.store()
            || self
                .birth
                .index
                .checked_add(1)
                .is_none_or(|i| i >= lifecycle::MAX_BIRTHS)
            || self.grants.len() > 16
            || self.limit == 0
            || self.grants.windows(2).any(|p| p[0].sha256 >= p[1].sha256)
        {
            return Err(Error::Invalid);
        }
        let mut pins = std::collections::BTreeSet::new();
        for grant in &self.grants {
            if grant.record.schema != crate::retention::SCHEMA
                || digest(&grant.record)? != grant.sha256
                || grant.store() != self.enrollment.store()
                || grant.request().owner != r.owner
                || grant.request().publication.object != r.publication.object
                || !pins.insert(grant.request().pin_id()?)
            {
                return Err(Error::Invalid);
            }
        }
        digest(self)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.id()?;
        encode(self)
    }
    pub fn from_json(raw: &[u8]) -> Result<Self> {
        if raw.len() > 65536 {
            return Err(Error::Invalid);
        }
        let p: Self = serde_json::from_value(
            git_asset_core::identity::parse_json(raw).map_err(|_| Error::Invalid)?,
        )
        .map_err(|_| Error::Invalid)?;
        if p.to_json()? != raw {
            return Err(Error::Invalid);
        }
        Ok(p)
    }
    /// Exact trusted owner/application decision, separate from clock/fence checks.
    pub fn authorize(&self) -> Result<Authorization> {
        Ok(Authorization { plan: self.id()? })
    }
    pub(crate) fn validate(&self, store: &LocalStore) -> crate::Result<()> {
        self.id().map_err(|_| crate::Error::CorruptRecord)?;
        self.enrollment
            .check(store)
            .map_err(|_| crate::Error::OperationConflict)?;
        if store.final_policy() != Some(&self.policy) || self.limit != peak(store)? {
            return Err(crate::Error::OperationConflict);
        }
        Ok(())
    }
}
pub struct Authorization {
    plan: Digest,
}
fn peak(store: &LocalStore) -> crate::Result<u64> {
    if store.final_policy().is_none() {
        return Err(crate::Error::Unsupported);
    }
    let q = store.allocation_quantum()?;
    65536u64
        .checked_add(q - 1)
        .and_then(|n| n.checked_div(q))
        .and_then(|n| n.checked_mul(q))
        .and_then(|n| {
            n.checked_mul((crate::local::MAX_SCOPE_FILES + lifecycle::MAX_FINAL_FILES + 20) as u64)
        })
        .ok_or(crate::Error::Budget)
}
pub fn plan(store: &LocalStore, request: Request) -> Result<Plan> {
    let policy = store.final_policy().ok_or(Error::Unsupported)?.clone();
    let mut grants = store.active_final_grants(&request.publication.object)?;
    if grants.iter().any(|g| g.request().owner != request.owner) {
        return Err(Error::ProtectedGrant);
    }
    grants.sort_by(|a, b| a.sha256.cmp(&b.sha256));
    let p = Plan {
        schema: "tig.final-retirement-plan.design-1".into(),
        enrollment: Enrollment::bind(store).map_err(|_| Error::Invalid)?,
        policy,
        birth: store.current_birth(&request.publication)?,
        request,
        grants,
        limit: peak(store)?,
    };
    p.id()?;
    Ok(p)
}

/// Private caller's enrolled current clock authority, including anti-rollback.
/// No SystemTime fallback or request-supplied timestamp is accepted by execution.
pub trait Clock {
    fn now(&self, policy: &Digest) -> Result<u64>;
    fn bounds(&self, policy: &Digest) -> Result<TimeBounds> {
        let time = self.now(policy)?;
        Ok(TimeBounds {
            earliest: time,
            latest: time,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeBounds {
    pub earliest: u64,
    pub latest: u64,
}
impl Window {
    pub(crate) fn check_clock(
        &self,
        clock: &dyn Clock,
        policy: &Digest,
        minimum: u64,
    ) -> Result<u64> {
        let bounds = clock.bounds(policy)?;
        if bounds.earliest > bounds.latest
            || bounds.earliest < minimum
            || bounds.earliest < self.not_before
            || bounds.latest >= self.expires
        {
            return Err(Error::Clock);
        }
        Ok(bounds.earliest)
    }
}
/// Additional trusted consent/authority predicate under live content fences.
/// It cannot weaken mandatory provider or complete-membership checks.
pub trait Condition {
    fn check(&self, clearance: &claims::Clearance<'_>) -> Result<()>;
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Mode {
    Delete,
    CancelOnly,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Intent {
    schema: String,
    pub plan: Plan,
    pub job: JournalRef,
    pub allocation: JournalRef,
    pub roster: claims::Roster,
    pub checked: Vec<Checked>,
    pub mode: Mode,
    pub time: Option<u64>,
    pub allocated: u64,
}
impl Intent {
    pub(crate) fn validate(&self, store: &LocalStore, birth: &Birth) -> crate::Result<()> {
        self.plan.validate(store)?;
        if self.schema != "tig.final-retirement-intent.design-1"
            || self.roster.coverage.is_none()
            || !self
                .roster
                .covers_binding(store.protection().ok_or(crate::Error::CorruptRecord)?)
                .map_err(|_| crate::Error::CorruptRecord)?
            || lifecycle::digest(birth)? != lifecycle::digest(&self.plan.birth)?
            || self.roster.scope().ok().as_ref() != Some(&self.plan.policy.scope)
        {
            return Err(crate::Error::CorruptRecord);
        }
        match self.mode {
            Mode::Delete => {
                let t = self.time.ok_or(crate::Error::CorruptRecord)?;
                if t < self.plan.request.window.not_before
                    || t >= self.plan.request.window.expires
                    || self.checked.len() != self.roster.members.len()
                {
                    return Err(crate::Error::CorruptRecord);
                }
                for (checked, pin) in self.checked.iter().zip(&self.roster.members) {
                    if checked.namespace != pin.namespace
                        || checked.checkpoint.identity() != &pin.namespace
                        || checked.checkpoint.sequence() < pin.minimum.sequence()
                        || checked.checkpoint.sequence() == pin.minimum.sequence()
                            && checked.checkpoint != pin.minimum
                    {
                        return Err(crate::Error::CorruptRecord);
                    }
                }
            }
            Mode::CancelOnly if self.time.is_none() && self.checked.is_empty() => {}
            _ => return Err(crate::Error::CorruptRecord),
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Release {
    schema: String,
    pub intent: Digest,
    pub time: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Deleted,
    Cancelled,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Terminal {
    schema: String,
    pub intent: Digest,
    pub release: Option<Digest>,
    pub outcome: Outcome,
}
#[derive(Clone, Debug)]
pub(crate) struct State {
    pub intent: Intent,
    pub release: Option<Release>,
    pub terminal: Option<Terminal>,
}
impl State {
    pub fn retired(&self) -> bool {
        self.terminal
            .as_ref()
            .is_some_and(|t| t.outcome == Outcome::Deleted)
    }
    pub fn cancelled(&self) -> bool {
        self.terminal
            .as_ref()
            .is_some_and(|t| t.outcome == Outcome::Cancelled)
    }
    pub(crate) fn validate(&self, store: &LocalStore, birth: &Birth) -> crate::Result<()> {
        self.intent.validate(store, birth)?;
        let id = lifecycle::digest(&self.intent)?;
        if let Some(r) = &self.release
            && (self.intent.mode != Mode::Delete
                || r.schema != "tig.final-retirement-release.design-1"
                || r.intent != id
                || r.time < self.intent.time.ok_or(crate::Error::CorruptRecord)?
                || r.time >= self.intent.plan.request.window.expires)
        {
            return Err(crate::Error::CorruptRecord);
        }
        if let Some(t) = &self.terminal
            && (t.schema != "tig.final-retirement-terminal.design-1"
                || t.intent != id
                || t.release != self.release.as_ref().map(lifecycle::digest).transpose()?
                || t.outcome == Outcome::Deleted && self.release.is_none())
        {
            return Err(crate::Error::CorruptRecord);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Observation {
    PresentUnverified,
    Absent,
    Unknown,
}
#[derive(Debug, Serialize)]
pub struct Receipt {
    pub schema: &'static str,
    pub plan: Digest,
    pub outcome: Option<Outcome>,
    pub grants_released: bool,
    pub historical: bool,
    pub observed: Observation,
    pub file_allocation_before: u64,
    pub charged_metadata: u64,
    pub logical_bytes_removed: u64,
}

pub struct Execution<'a, S> {
    pub store: &'a LocalStore,
    pub ledger: &'a mut CapacityLedger,
    pub job: &'a JournalRef,
    pub roster: &'a claims::Roster,
    pub namespaces: &'a mut [Namespace],
    pub nodes: &'a S,
    pub clock: &'a dyn Clock,
    pub admitted_context: &'a Digest,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Admitted,
    Bound,
    Intent,
    Released,
    GrantsRemoved,
    BeforeUnlink,
    Unlinked,
    Terminal,
    Settled,
    Cancelled,
}

fn keys(p: &Plan) -> Result<(Key, Key)> {
    let id = digest(&(p.enrollment.store(), &p.request.operation))?;
    Ok((
        Key::parse(&format!("final-{}", id.as_str()))?,
        Key::parse(&format!("final-settle-{}", id.as_str()))?,
    ))
}
/// Reserve this exact provider's recovery metadata before a bulk source change.
pub(crate) fn admit(
    store: &LocalStore,
    ledger: &mut CapacityLedger,
    job: &JournalRef,
    p: &Plan,
) -> Result<JournalRef> {
    p.validate(store)?;
    if ledger.identity() != &p.request.capacity {
        return Err(Error::WrongAuthority);
    }
    Ok(ledger
        .begin_retention(
            keys(p)?.0,
            job,
            p.request.owner.clone(),
            digest(&("tig.final-allocation.design-1", p, job))?,
            p.limit,
        )?
        .reference()
        .clone())
}
/// Read-only exact admission lookup; cancellation must not create new work.
pub(crate) fn admitted(ledger: &mut CapacityLedger, job: &JournalRef, p: &Plan) -> Result<bool> {
    p.id()?;
    if ledger.identity() != &p.request.capacity {
        return Err(Error::WrongAuthority);
    }
    let Some(a) = ledger.allocation_reference(&keys(p)?.0, true)? else {
        return Ok(false);
    };
    ledger.retention_status(&a, &digest(&("tig.final-allocation.design-1", p, job))?)?;
    Ok(true)
}
fn observe(store: &LocalStore, object: &Object) -> Observation {
    match store.object_present(object) {
        Ok(true) => Observation::PresentUnverified,
        Ok(false) => Observation::Absent,
        Err(_) => Observation::Unknown,
    }
}
impl<S: NodeStore> Execution<'_, S> {
    fn common(&self, p: &Plan, a: &Authorization) -> Result<()> {
        p.validate(self.store)?;
        if a.plan != p.id()?
            || self.ledger.identity() != &p.request.capacity
            || self.admitted_context != &p.request.context
            || self.roster.scope()? != p.policy.scope
            || !self
                .roster
                .covers_binding(self.store.protection().ok_or(Error::Invalid)?)?
        {
            return Err(Error::WrongAuthority);
        }
        Ok(())
    }
    fn time(&self, p: &Plan, minimum: u64) -> Result<u64> {
        p.request
            .window
            .check_clock(self.clock, &p.policy.clock, minimum)
    }
    pub fn apply(&mut self, p: &Plan, a: &Authorization) -> Result<Receipt> {
        self.apply_hook(p, a, |_| Ok(()))
    }
    pub fn apply_hook(
        &mut self,
        p: &Plan,
        a: &Authorization,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        self.run(p, a, false, hook, None)
    }
    pub fn apply_guarded(
        &mut self,
        p: &Plan,
        a: &Authorization,
        condition: &dyn Condition,
    ) -> Result<Receipt> {
        self.apply_guarded_hook(p, a, condition, |_| Ok(()))
    }
    pub(crate) fn apply_guarded_hook(
        &mut self,
        p: &Plan,
        a: &Authorization,
        condition: &dyn Condition,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        self.run(p, a, false, hook, Some(condition))
    }
    pub fn cancel(&mut self, p: &Plan, a: &Authorization) -> Result<Receipt> {
        self.run(p, a, true, |_| Ok(()), None)
    }
    pub fn cancel_hook(
        &mut self,
        p: &Plan,
        a: &Authorization,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        self.run(p, a, true, hook, None)
    }
    fn run(
        &mut self,
        p: &Plan,
        a: &Authorization,
        cancel: bool,
        mut hook: impl FnMut(Stage) -> Result<()>,
        condition: Option<&dyn Condition>,
    ) -> Result<Receipt> {
        self.common(p, a)?;
        let (operation, settle) = keys(p)?;
        let binding = digest(&("tig.final-allocation.design-1", p, self.job))?;
        let allocation = admit(self.store, self.ledger, self.job, p)?;
        hook(Stage::Admitted)?;
        let scope = Publication {
            operation,
            owner: p.request.owner.clone(),
            object: p.birth.object.clone(),
            required: AckLevel::ProcessRestart,
        };
        let attempt = self.store.attempt(&scope)?;
        attempt.bind(&(p, self.job, &allocation))?;
        if attempt.has_partials()? {
            return Err(Error::Partial);
        }
        hook(Stage::Bound)?;
        let old = self.store.load_final_state(&p.birth)?;
        let settled = self
            .ledger
            .retention_status(&allocation, &binding)?
            .is_some();
        if settled && old.as_ref().is_none_or(|s| s.terminal.is_none()) {
            return Err(Error::Recovery);
        }
        if let Some(s) = &old {
            if digest(&s.intent.plan)? != p.id()?
                || s.intent.job != *self.job
                || s.intent.allocation != allocation
            {
                return Err(Error::WrongAuthority);
            }
            if s.terminal.is_some() {
                if s.cancelled() && !settled {
                    let fence = self.store.handoff_fence(&p.birth.object)?;
                    attempt
                        .managed_store()
                        .continue_final_cancel(&s.intent, &fence)?;
                }
                return finish(
                    self.ledger,
                    self.store,
                    p,
                    &attempt,
                    s,
                    &allocation,
                    &binding,
                    settle,
                    true,
                );
            }
            let fence = self.store.handoff_fence(&p.birth.object)?;
            if !self.store.object_present(&p.birth.object)? {
                if s.release.is_none() {
                    return Err(Error::Recovery);
                }
                let terminal = Terminal {
                    schema: "tig.final-retirement-terminal.design-1".into(),
                    intent: digest(&s.intent)?,
                    release: s.release.as_ref().map(digest).transpose()?,
                    outcome: Outcome::Deleted,
                };
                attempt
                    .managed_store()
                    .write_final_terminal(&s.intent, &terminal, &fence)?;
                hook(Stage::Terminal)?;
                let state = self
                    .store
                    .load_final_state(&p.birth)?
                    .ok_or(Error::Recovery)?;
                return finish(
                    self.ledger,
                    self.store,
                    p,
                    &attempt,
                    &state,
                    &allocation,
                    &binding,
                    settle,
                    true,
                );
            }
        }
        if cancel
            || old
                .as_ref()
                .is_some_and(|s| s.intent.mode == Mode::CancelOnly)
        {
            let fence = self.store.handoff_fence(&p.birth.object)?;
            let intent = if let Some(s) = old {
                s.intent
            } else {
                let allocated = self.store.verify_final_plan_source(
                    &p.request.publication,
                    &p.birth,
                    &fence,
                )?;
                let intent = Intent {
                    schema: "tig.final-retirement-intent.design-1".into(),
                    plan: p.clone(),
                    job: self.job.clone(),
                    allocation: allocation.clone(),
                    roster: self.roster.clone(),
                    checked: Vec::new(),
                    mode: Mode::CancelOnly,
                    time: None,
                    allocated,
                };
                attempt
                    .managed_store()
                    .install_final_intent(&intent, &fence)?;
                hook(Stage::Intent)?;
                intent
            };
            self.store.verify_final_source(&intent, &fence)?;
            let state = self
                .store
                .load_final_state(&p.birth)?
                .ok_or(Error::Recovery)?;
            let terminal = Terminal {
                schema: "tig.final-retirement-terminal.design-1".into(),
                intent: digest(&intent)?,
                release: state.release.as_ref().map(digest).transpose()?,
                outcome: Outcome::Cancelled,
            };
            attempt
                .managed_store()
                .write_final_terminal(&intent, &terminal, &fence)?;
            hook(Stage::Cancelled)?;
            attempt
                .managed_store()
                .continue_final_cancel(&intent, &fence)?;
            let state = self
                .store
                .load_final_state(&p.birth)?
                .ok_or(Error::Recovery)?;
            return finish(
                self.ledger,
                self.store,
                p,
                &attempt,
                &state,
                &allocation,
                &binding,
                settle,
                false,
            );
        }
        let first = self.time(
            p,
            old.as_ref()
                .map(|s| {
                    s.release
                        .as_ref()
                        .map(|r| r.time)
                        .unwrap_or(s.intent.time.unwrap_or(0))
                })
                .unwrap_or(0),
        )?;
        let clearance = claims::acquire(self.roster, &p.birth.object, self.namespaces, self.nodes)?;
        if let Some(condition) = condition {
            condition.check(&clearance)?;
        }
        let fence = self.store.handoff_fence(&p.birth.object)?;
        let allocated =
            self.store
                .verify_final_plan_source(&p.request.publication, &p.birth, &fence)?;
        let intent = if let Some(s) = old {
            s.intent
        } else {
            let actual = self.store.active_final_grants(&p.birth.object)?;
            if encode(&actual)? != encode(&p.grants)? {
                return Err(Error::ProtectedGrant);
            }
            for g in &p.grants {
                if self
                    .ledger
                    .retention_status(
                        &g.record.allocation,
                        &digest(&(&g.record.plan, &g.record.job))?,
                    )?
                    .is_none()
                {
                    return Err(Error::Recovery);
                }
            }
            let intent = Intent {
                schema: "tig.final-retirement-intent.design-1".into(),
                plan: p.clone(),
                job: self.job.clone(),
                allocation: allocation.clone(),
                roster: self.roster.clone(),
                checked: clearance.summary().checked.to_vec(),
                mode: Mode::Delete,
                time: Some(first),
                allocated,
            };
            attempt
                .managed_store()
                .install_final_intent(&intent, &fence)?;
            hook(Stage::Intent)?;
            intent
        };
        let state = self
            .store
            .load_final_state(&p.birth)?
            .ok_or(Error::Recovery)?;
        let release = if let Some(r) = state.release {
            r
        } else {
            let actual = self.store.active_final_grants(&p.birth.object)?;
            if encode(&actual)? != encode(&p.grants)? {
                return Err(Error::ProtectedGrant);
            }
            for g in &p.grants {
                if self
                    .ledger
                    .retention_status(
                        &g.record.allocation,
                        &digest(&(&g.record.plan, &g.record.job))?,
                    )?
                    .is_none()
                {
                    return Err(Error::Recovery);
                }
            }
            clearance.check_current()?;
            let release = Release {
                schema: "tig.final-retirement-release.design-1".into(),
                intent: digest(&intent)?,
                time: p.request.window.check_clock(
                    self.clock,
                    &p.policy.clock,
                    first.max(intent.time.ok_or(Error::Recovery)?),
                )?,
            };
            if release.time < first
                || release.time < intent.time.ok_or(Error::Recovery)?
                || release.time < p.request.window.not_before
                || release.time >= p.request.window.expires
            {
                return Err(Error::Clock);
            }
            attempt
                .managed_store()
                .write_final_release(&intent, &release, &fence)?;
            hook(Stage::Released)?;
            release
        };
        attempt
            .managed_store()
            .cleanup_final_grants(&intent, &fence)?;
        hook(Stage::GrantsRemoved)?;
        self.store.verify_final_source(&intent, &fence)?;
        clearance.check_current()?;
        hook(Stage::BeforeUnlink)?;
        if let Some(condition) = condition {
            condition.check(&clearance)?;
        }
        let now = p
            .request
            .window
            .check_clock(self.clock, &p.policy.clock, release.time)?;
        if now < release.time
            || now < p.request.window.not_before
            || now >= p.request.window.expires
        {
            return Err(Error::Clock);
        }
        attempt
            .managed_store()
            .unlink_final(&intent, &fence, &clearance)?;
        hook(Stage::Unlinked)?;
        let terminal = Terminal {
            schema: "tig.final-retirement-terminal.design-1".into(),
            intent: digest(&intent)?,
            release: Some(digest(&release)?),
            outcome: Outcome::Deleted,
        };
        attempt
            .managed_store()
            .write_final_terminal(&intent, &terminal, &fence)?;
        hook(Stage::Terminal)?;
        let state = self
            .store
            .load_final_state(&p.birth)?
            .ok_or(Error::Recovery)?;
        let receipt = finish(
            self.ledger,
            self.store,
            p,
            &attempt,
            &state,
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
    p: &Plan,
    attempt: &crate::local::LocalAttempt,
    state: &State,
    allocation: &JournalRef,
    binding: &Digest,
    settle: Key,
    historical: bool,
) -> Result<Receipt> {
    let charged = match ledger.retention_status(allocation, binding)? {
        Some(n) => n,
        None => attempt.lifecycle_footprint(&p.birth.object)?,
    };
    if charged > p.limit || state.terminal.is_none() {
        return Err(Error::Recovery);
    }
    ledger.settle_retention(settle, allocation, binding.clone(), charged)?;
    Ok(Receipt {
        schema: "tig.final-retirement-receipt.design-1",
        plan: p.id()?,
        outcome: state.terminal.as_ref().map(|t| t.outcome.clone()),
        grants_released: state.release.is_some(),
        historical,
        observed: observe(store, &p.birth.object),
        file_allocation_before: state.intent.allocated,
        charged_metadata: charged,
        logical_bytes_removed: if state.retired() {
            p.birth.object.bytes
        } else {
            0
        },
    })
}

#[cfg(test)]
mod tests;
