use super::*;
use crate::final_retirement::bulk::witness::{Layout, Witness};
use crate::{
    ledger::JournalRef,
    metadata::{MetadataReader, MetadataStore},
    placement::Local,
    protection::budget::{Budget, Nodes},
    source_claims::Command,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreJob {
    pub store: Digest,
    pub job: JournalRef,
}
#[derive(Serialize)]
struct Bindings<'a> {
    witness: &'a JournalRef,
    jobs: &'a [StoreJob],
    metadata: &'a Digest,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    NotStarted,
    Admitted,
    Intent,
    Completed,
    Cancelled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Admitted,
    Intent,
    PageAdmitted(usize),
    PageIntent(usize),
    Source(usize, subset::Stage),
    AgeAcknowledged(usize),
    PageCompleted(usize),
    Provider(usize, provider::Stage),
    Removed(usize),
    Cancelled,
    Completed,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RunLimits {
    pub pages: usize,
    pub actions: usize,
}
impl Default for RunLimits {
    fn default() -> Self {
        Self {
            pages: 16,
            actions: 16,
        }
    }
}
impl RunLimits {
    fn check(&self) -> Result<()> {
        if self.pages == 0 || self.pages > 64 || self.actions == 0 || self.actions > 32 {
            Err(Error::Budget)
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Held {
    pub kind: &'static str,
    pub index: usize,
}
#[derive(Debug, Serialize)]
pub struct Effect {
    pub action: usize,
    pub receipt: provider::Receipt,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub decision: Digest,
    pub phase: Phase,
    pub completed_pages: Option<usize>,
    pub cancelled_pages: Option<usize>,
    pub historical_retired_pages: Option<usize>,
    pub total_pages: usize,
    pub historical_deleted_actions: Option<usize>,
    pub pending_cancellations: Option<usize>,
    pub total_actions: usize,
    pub excluded_objects: Option<usize>,
    pub metadata_available: bool,
    pub cleanup_unknown: bool,
    /// An aged source retirement lacks a page completion witness. Inheritance
    /// may already exist, but this clock-free status has no registry port.
    pub age_inheritance_unknown: bool,
    pub unreported_holds: usize,
    pub holds: Vec<Held>,
    pub effects: Vec<Effect>,
    pub authorizes_removal: bool,
}
pub struct Execution<'a, 'b> {
    pub enrollment: &'a Enrollment,
    pub roster: &'a claims::Roster,
    pub namespaces: &'a mut [Namespace],
    pub metadata: &'a mut MetadataStore,
    pub places: &'a mut [Local<'b>],
    pub jobs: &'a [StoreJob],
    pub witness: Local<'b>,
    pub clock: &'a dyn provider::Clock,
    pub admitted_context: &'a Digest,
    pub limits: &'a Limits,
    pub run_limits: RunLimits,
}
/// Historical/current observation without a metadata writer or clock port.
pub struct Inspection<'a, 'b> {
    pub enrollment: &'a Enrollment,
    pub roster: &'a claims::Roster,
    pub namespaces: &'a mut [Namespace],
    pub metadata: Option<MetadataReader>,
    pub places: &'a mut [Local<'b>],
    pub jobs: &'a [StoreJob],
    pub witness: &'a mut Local<'b>,
    pub admitted_context: &'a Digest,
    pub limits: &'a Limits,
}
/// Explicit stop authority. Unavailable metadata yields unknown cleanup debt,
/// while the durable global cancel still prevents further source/byte effects.
pub struct Cancellation<'a, 'b> {
    pub inspection: Inspection<'a, 'b>,
}
struct Consent<'a> {
    age: Option<(&'a dyn provider::Clock, &'a Digest, u64)>,
    budget: &'a Budget,
    basis: &'a claims::Roster,
    allowed: BTreeMap<Digest, Vec<(Key, Command)>>,
}
impl<'a> Consent<'a> {
    fn new(
        e: &'a Enrollment,
        l: &'a Loaded,
        budget: &'a Budget,
        age: Option<(&'a dyn provider::Clock, u64)>,
    ) -> Result<Self> {
        let mut allowed = BTreeMap::<_, Vec<_>>::new();
        for (s, p) in &l.steps {
            allowed
                .entry(s.origin.namespace.clone())
                .or_default()
                .extend(p.commands(e.source(&s.origin.namespace)?)?);
        }
        Ok(Self {
            age: age.map(|(clock, deadline)| (clock, &e.clock, deadline)),
            budget,
            basis: &l.prepared.basis,
            allowed,
        })
    }
    fn current(&self, trusted: &claims::Roster, namespaces: &mut [Namespace]) -> Result<()> {
        self.budget.timed()?;
        self.time()?;
        trusted.id()?;
        if trusted.scope()? != self.basis.scope()? || namespaces.len() != self.basis.members.len() {
            return Err(Error::Authority);
        }
        for pin in &self.basis.members {
            let private = trusted
                .members
                .iter()
                .find(|p| p.namespace == pin.namespace && p.policy == pin.policy)
                .ok_or(Error::Authority)?;
            let n = namespaces
                .iter_mut()
                .find(|n| n.identity() == &pin.namespace)
                .ok_or(Error::Authority)?;
            n.refresh_minimum(&private.minimum)?;
            admission::bulk::snapshot(n)?;
            if claims::NamespacePin::current(n)?.policy != pin.policy
                || !admission::bulk::only_owned_since(
                    n,
                    &pin.minimum,
                    self.allowed
                        .get(&pin.namespace)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]),
                )?
            {
                return Err(Error::Stale);
            }
        }
        self.budget.timed()?;
        Ok(())
    }
    fn time(&self) -> Result<()> {
        if let Some((clock, id, deadline)) = self.age {
            let now = clock.bounds(id)?;
            if now.earliest < deadline || now.latest < now.earliest || now.latest > MAX_NUMBER {
                return Err(Error::Authority);
            }
        }
        Ok(())
    }
}
impl provider::Condition for Consent<'_> {
    fn check(&self, c: &claims::Clearance<'_>) -> provider::Result<()> {
        self.time().map_err(|_| provider::Error::Clock)?;
        self.budget
            .timed()
            .map_err(|_| provider::Error::Claims(claims::Error::Budget))?;
        if c.only_owned_commands(self.basis, &self.allowed)? {
            self.budget
                .timed()
                .map_err(|_| provider::Error::Claims(claims::Error::Budget))?;
            Ok(())
        } else {
            Err(provider::Error::WrongAuthority)
        }
    }
}
fn bound(
    e: &Enrollment,
    d: &Decision,
    roster: &claims::Roster,
    places: &[Local<'_>],
    jobs: &[StoreJob],
    witness: &Local<'_>,
    context: &Digest,
) -> Result<()> {
    e.id()?;
    d.id()?;
    d.request.check(e)?;
    roster.id()?;
    if d.enrollment != e.id()?
        || context != &e.context
        || roster.scope()? != e.selection.scope
        || witness.ledger.identity() != &e.capacity
        || witness.store.supports_retirement()
        || Place::local(witness.store, e.witness.generation).map_err(|_| Error::Authority)?
            != e.witness
        || jobs.len() > 4
        || jobs.windows(2).any(|j| j[0].store >= j[1].store)
        || places
            .iter()
            .map(|p| p.store.identity())
            .collect::<BTreeSet<_>>()
            .len()
            != places.len()
    {
        return Err(Error::Authority);
    }
    for p in places {
        if !jobs
            .iter()
            .any(|j| &j.store == p.store.identity() && &j.job == p.job)
        {
            return Err(Error::Authority);
        }
    }
    Ok(())
}
fn witness(
    e: &Enrollment,
    d: &Decision,
    jobs: &[StoreJob],
    local: &Local<'_>,
    step: Option<(&preparation::Step, &subset::Plan)>,
) -> Result<Witness> {
    let bindings = Bindings {
        witness: local.job,
        jobs,
        metadata: &e.selection.metadata,
    };
    Ok(Witness::from_records(
        record_layout(e, d, bindings, step.map(|(s, _)| s))?,
        local.store,
    )?)
}
fn record_layout(
    e: &Enrollment,
    d: &Decision,
    bindings: Bindings<'_>,
    step: Option<&preparation::Step>,
) -> Result<Layout> {
    let b = hash(&bindings)?;
    let id = d.id()?;
    let operation = hash(
        &serde_json::json!({"schema":"tig.paged-control-key.design-1",
        "authority":e.selection.authority,"epoch":e.selection.epoch,"owner":e.selection.owner,"operation":d.request.operation}),
    )?;
    let (key, intent, prefix) = match step {
        None => (
            operation,
            encode(
                &serde_json::json!({"schema":"tig.paged-control-intent.design-1","decision":d,"bindings":bindings}),
            )?,
            "paged",
        ),
        Some(s) => (
            hash(
                &serde_json::json!({"schema":"tig.paged-control-page-key.design-1","operation":operation,"index":s.index}),
            )?,
            encode(
                &serde_json::json!({"schema":"tig.paged-control-page-intent.design-1","decision":id,"bindings":b,
                "step":s,"plan":s.plan}),
            )?,
            "paged-page",
        ),
    };
    let terminal = |cancelled: bool| {
        encode(
            &serde_json::json!({"schema":"tig.paged-control-terminal.design-1",
        "decision":id,"bindings":b,"page":step.map(|s|s.index),"cancelled":cancelled}),
        )
    };
    let docs = [intent, terminal(true)?, terminal(false)?];
    Ok(Layout {
        owner: e.selection.owner.clone(),
        key,
        plan: id.clone(),
        control_binding: hash(
            &serde_json::json!({"schema":"tig.paged-control-binding.design-1","decision":id,"bindings":b,
            "page":step.map(|s|s.index)}),
        )?,
        control_object: Object::from_bytes(&docs[0]),
        documents: docs,
        prefix,
        max_metadata_bytes: e.max_metadata_bytes,
    })
}
fn metadata_peak(
    e: &Enrollment,
    d: &Decision,
    jobs: &[StoreJob],
    local: &Local<'_>,
    l: &Loaded,
) -> Result<u64> {
    let mut peak = witness(e, d, jobs, local, None)?.peak;
    for (s, p) in &l.steps {
        peak = peak
            .checked_add(witness(e, d, jobs, local, Some((s, p)))?.peak)
            .ok_or(Error::Budget)?;
    }
    Ok(peak)
}
struct Observation<'a, 'b> {
    e: &'a Enrollment,
    jobs: &'a [StoreJob],
    local: &'a mut Local<'b>,
    places: &'a mut [Local<'b>],
    namespaces: &'a mut [Namespace],
}
impl Observation<'_, '_> {
    fn report(
        &mut self,
        d: &Decision,
        loaded: Option<&Loaded>,
        effects: Vec<Effect>,
        mut holds: Vec<Held>,
    ) -> Result<Report> {
        let e = self.e;
        let jobs = self.jobs;
        let local = &mut *self.local;
        let places = &mut *self.places;
        let namespaces = &mut *self.namespaces;
        let w = witness(e, d, jobs, local, None)?;
        let state = w.state(local.store, local.ledger, local.job)?;
        let mut r = Report {
            schema: "tig.paged-control-status.design-1",
            decision: d.id()?,
            phase: match state.terminal {
                Some(true) => Phase::Cancelled,
                Some(false) => Phase::Completed,
                None if state.intent => Phase::Intent,
                None if state.admitted => Phase::Admitted,
                None => Phase::NotStarted,
            },
            completed_pages: None,
            cancelled_pages: None,
            historical_retired_pages: None,
            total_pages: d.pages,
            historical_deleted_actions: None,
            pending_cancellations: None,
            total_actions: d.action_count,
            excluded_objects: None,
            metadata_available: loaded.is_some(),
            cleanup_unknown: loaded.is_none(),
            age_inheritance_unknown: e.age.is_some() && loaded.is_none(),
            unreported_holds: 0,
            holds: Vec::new(),
            effects,
            authorizes_removal: false,
        };
        if let Some(l) = loaded {
            r.excluded_objects = Some(l.subjects.iter().filter(|s| s.hold.is_some()).count());
            let (mut completed, mut cancelled, mut retired) = (0, 0, 0);
            let mut source_unknown = false;
            for (s, p) in &l.steps {
                let pw = witness(e, d, jobs, local, Some((s, p)))?;
                let ps = pw.state(local.store, local.ledger, local.job)?;
                if ps.terminal == Some(true) {
                    cancelled += 1;
                }
                if ps.terminal == Some(false) {
                    completed += 1;
                }
                let Some(n) = namespaces
                    .iter_mut()
                    .find(|n| n.identity() == &s.origin.namespace)
                else {
                    source_unknown = true;
                    holds.push(Held {
                        kind: "source_status_unknown",
                        index: s.index,
                    });
                    continue;
                };
                if n.source(&p.request().source)?.is_none() {
                    if ps.admitted {
                        return Err(Error::Recovery);
                    }
                    continue;
                }
                let actual = subset::inspect(e.source(&s.origin.namespace)?, n, p)?;
                if let Some(receipt) = &actual.historical_retirement {
                    if receipt.checkpoint != s.retirement_checkpoint {
                        return Err(Error::Recovery);
                    }
                    retired += 1;
                } else if ps.terminal == Some(false) {
                    return Err(Error::Recovery);
                }
            }
            r.completed_pages = Some(completed);
            r.cancelled_pages = Some(cancelled);
            r.historical_retired_pages = (!source_unknown).then_some(retired);
            r.cleanup_unknown |= source_unknown;
            r.age_inheritance_unknown = e.age.is_some() && (source_unknown || retired > completed);
            r.cleanup_unknown |= r.age_inheritance_unknown;
            let (mut deleted, mut pending) = (0, 0);
            let mut provider_unknown = false;
            for (i, p) in l.actions.iter().enumerate() {
                let Some(local) = places
                    .iter_mut()
                    .find(|v| v.store.identity() == p.enrollment.store())
                else {
                    holds.push(Held {
                        kind: "provider_unavailable",
                        index: i,
                    });
                    r.cleanup_unknown = true;
                    provider_unknown = true;
                    continue;
                };
                let job = jobs
                    .iter()
                    .find(|j| &j.store == p.enrollment.store())
                    .ok_or(Error::Authority)?;
                match provider::status::inspect(local.store, local.ledger, &job.job, p) {
                    Ok(s) if matches!(s.phase, provider::status::Phase::Deleted) => deleted += 1,
                    Ok(s) => {
                        if !matches!(
                            s.phase,
                            provider::status::Phase::NotStarted
                                | provider::status::Phase::Cancelled
                        ) {
                            pending += 1;
                        }
                    }
                    Err(_) => {
                        holds.push(Held {
                            kind: "provider_status_unknown",
                            index: i,
                        });
                        r.cleanup_unknown = true;
                        provider_unknown = true;
                    }
                }
            }
            r.historical_deleted_actions = (!provider_unknown).then_some(deleted);
            r.pending_cancellations = (!provider_unknown).then_some(pending);
        }
        r.unreported_holds = holds.len().saturating_sub(64);
        holds.truncate(64);
        r.holds = holds;
        Ok(r)
    }
}
impl Inspection<'_, '_> {
    fn metadata_identity(&self) -> Result<()> {
        if self
            .metadata
            .as_ref()
            .is_some_and(|m| m.checkpoint().identity() != &self.enrollment.selection.metadata)
        {
            return Err(Error::Authority);
        }
        Ok(())
    }
    pub fn required_witness_peak(&mut self, d: &Decision) -> Result<u64> {
        self.metadata_identity()?;
        bound(
            self.enrollment,
            d,
            self.roster,
            self.places,
            self.jobs,
            self.witness,
            self.admitted_context,
        )?;
        let budget = Budget::new(
            &self.limits.source,
            self.limits.writes,
            self.limits.write_bytes,
        )?;
        let l = decision::load(
            self.enrollment,
            d,
            &mut Nodes {
                inner: self.metadata.as_mut().ok_or(Error::Authority)?,
                budget: Some(&budget),
            },
            self.limits,
        )?;
        metadata_peak(self.enrollment, d, self.jobs, self.witness, &l)
    }
    pub fn status(&mut self, d: &Decision) -> Result<Report> {
        let budget = Budget::new(
            &self.limits.source,
            self.limits.writes,
            self.limits.write_bytes,
        )?;
        self.status_metered(d, &budget)
    }
    pub(super) fn status_metered(&mut self, d: &Decision, budget: &Budget) -> Result<Report> {
        self.metadata_identity()?;
        bound(
            self.enrollment,
            d,
            self.roster,
            self.places,
            self.jobs,
            self.witness,
            self.admitted_context,
        )?;
        let loaded = self
            .metadata
            .as_mut()
            .map(|m| {
                decision::load(
                    self.enrollment,
                    d,
                    &mut Nodes {
                        inner: m,
                        budget: Some(budget),
                    },
                    self.limits,
                )
            })
            .transpose()?;
        let report = Observation {
            e: self.enrollment,
            jobs: self.jobs,
            local: self.witness,
            places: self.places,
            namespaces: self.namespaces,
        }
        .report(d, loaded.as_ref(), Vec::new(), Vec::new())?;
        budget.timed()?;
        Ok(report)
    }
}
struct NoTime;
impl provider::Clock for NoTime {
    fn now(&self, _: &Digest) -> provider::Result<u64> {
        Err(provider::Error::Clock)
    }
}
struct NoNodes;
impl NodeStore for NoNodes {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        Err(TreeError::MissingNode(o.clone()))
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        Err(TreeError::InvalidNode)
    }
}
struct Runtime<'a, 'b> {
    age: Option<aged::Context<'a>>,
    age_deadline: Option<u64>,
    budget: Budget,
    started: Instant,
    e: &'a Enrollment,
    roster: &'a claims::Roster,
    namespaces: &'a mut [Namespace],
    metadata: Option<&'a mut MetadataStore>,
    reader: Option<&'a mut MetadataReader>,
    places: &'a mut [Local<'b>],
    jobs: &'a [StoreJob],
    witness: &'a mut Local<'b>,
    clock: &'a dyn provider::Clock,
    context: &'a Digest,
    limits: &'a Limits,
    run_limits: RunLimits,
}
impl Execution<'_, '_> {
    pub fn required_witness_peak(&mut self, d: &Decision) -> Result<u64> {
        Inspection {
            enrollment: self.enrollment,
            roster: self.roster,
            namespaces: self.namespaces,
            metadata: Some(self.metadata.finished_reader()?),
            places: self.places,
            jobs: self.jobs,
            witness: &mut self.witness,
            admitted_context: self.admitted_context,
            limits: self.limits,
        }
        .required_witness_peak(d)
    }
    pub fn status(&mut self, d: &Decision) -> Result<Report> {
        let reader = self.metadata.finished_reader()?;
        Inspection {
            enrollment: self.enrollment,
            roster: self.roster,
            namespaces: self.namespaces,
            metadata: Some(reader),
            places: self.places,
            jobs: self.jobs,
            witness: &mut self.witness,
            admitted_context: self.admitted_context,
            limits: self.limits,
        }
        .status(d)
    }
    pub fn apply(
        &mut self,
        d: &Decision,
        claims: &ClaimAuthorization,
        finals: &FinalAuthorization,
    ) -> Result<Report> {
        self.apply_hook(d, claims, finals, |_| Ok(()))
    }
    pub fn apply_hook(
        &mut self,
        d: &Decision,
        claims: &ClaimAuthorization,
        finals: &FinalAuthorization,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Report> {
        self.apply_with_age(d, claims, finals, None, hook)
    }
    /// Execute an explicitly aged decision through the same durable controller.
    pub fn apply_aged(
        &mut self,
        d: &Decision,
        claims: &ClaimAuthorization,
        finals: &FinalAuthorization,
        age: aged::Context<'_>,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Report> {
        self.apply_with_age(d, claims, finals, Some(age), hook)
    }
    fn apply_with_age(
        &mut self,
        d: &Decision,
        claims: &ClaimAuthorization,
        finals: &FinalAuthorization,
        age: Option<aged::Context<'_>>,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Report> {
        Runtime {
            age,
            age_deadline: None,
            budget: Budget::new(
                &self.limits.source,
                self.limits.writes,
                self.limits.write_bytes,
            )?,
            started: Instant::now(),
            e: self.enrollment,
            roster: self.roster,
            namespaces: self.namespaces,
            metadata: Some(self.metadata),
            reader: None,
            places: self.places,
            jobs: self.jobs,
            witness: &mut self.witness,
            clock: self.clock,
            context: self.admitted_context,
            limits: self.limits,
            run_limits: self.run_limits.clone(),
        }
        .run(d, claims, finals, false, hook)
    }
    pub fn cancel(
        &mut self,
        d: &Decision,
        claims: &ClaimAuthorization,
        finals: &FinalAuthorization,
    ) -> Result<Report> {
        Runtime {
            age: None,
            age_deadline: None,
            budget: Budget::new(
                &self.limits.source,
                self.limits.writes,
                self.limits.write_bytes,
            )?,
            started: Instant::now(),
            e: self.enrollment,
            roster: self.roster,
            namespaces: self.namespaces,
            metadata: Some(self.metadata),
            reader: None,
            places: self.places,
            jobs: self.jobs,
            witness: &mut self.witness,
            clock: &NoTime,
            context: self.admitted_context,
            limits: self.limits,
            run_limits: self.run_limits.clone(),
        }
        .run(d, claims, finals, true, |_| Ok(()))
    }
}
impl Cancellation<'_, '_> {
    pub fn cancel(
        &mut self,
        d: &Decision,
        claims: &ClaimAuthorization,
        finals: &FinalAuthorization,
    ) -> Result<Report> {
        let i = &mut self.inspection;
        Runtime {
            age: None,
            age_deadline: None,
            budget: Budget::new(&i.limits.source, i.limits.writes, i.limits.write_bytes)?,
            started: Instant::now(),
            e: i.enrollment,
            roster: i.roster,
            namespaces: i.namespaces,
            metadata: None,
            reader: i.metadata.as_mut(),
            places: i.places,
            jobs: i.jobs,
            witness: i.witness,
            clock: &NoTime,
            context: i.admitted_context,
            limits: i.limits,
            run_limits: RunLimits::default(),
        }
        .run(d, claims, finals, true, |_| Ok(()))
    }
}
impl Runtime<'_, '_> {
    fn timed(&self) -> Result<()> {
        if self.started.elapsed() > Duration::from_millis(self.limits.source.millis) {
            Err(Error::Budget)
        } else {
            Ok(())
        }
    }
    fn loaded(&mut self, d: &Decision) -> Result<Option<Loaded>> {
        self.timed()?;
        if let Some(m) = self.metadata.as_deref_mut() {
            let mut reader = m.finished_reader()?;
            Ok(Some(decision::load(
                self.e,
                d,
                &mut Nodes {
                    inner: &mut reader,
                    budget: Some(&self.budget),
                },
                self.limits,
            )?))
        } else {
            self.reader
                .as_deref_mut()
                .map(|m| {
                    decision::load(
                        self.e,
                        d,
                        &mut Nodes {
                            inner: m,
                            budget: Some(&self.budget),
                        },
                        self.limits,
                    )
                })
                .transpose()
        }
    }
    fn report(
        &mut self,
        d: &Decision,
        l: Option<&Loaded>,
        effects: Vec<Effect>,
        holds: Vec<Held>,
    ) -> Result<Report> {
        self.timed()?;
        let report = Observation {
            e: self.e,
            jobs: self.jobs,
            local: self.witness,
            places: self.places,
            namespaces: self.namespaces,
        }
        .report(d, l, effects, holds)?;
        self.timed()?;
        Ok(report)
    }
    fn time(&self, d: &Decision) -> Result<()> {
        self.timed()?;
        d.request.window.check_clock(self.clock, &self.e.clock, 0)?;
        Ok(())
    }
    fn run(
        &mut self,
        d: &Decision,
        claims: &ClaimAuthorization,
        finals: &FinalAuthorization,
        cancel: bool,
        mut hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Report> {
        if self
            .metadata
            .as_ref()
            .is_some_and(|m| m.checkpoint().identity() != &self.e.selection.metadata)
            || self
                .reader
                .as_ref()
                .is_some_and(|m| m.checkpoint().identity() != &self.e.selection.metadata)
        {
            return Err(Error::Authority);
        }
        bound(
            self.e,
            d,
            self.roster,
            self.places,
            self.jobs,
            self.witness,
            self.context,
        )?;
        self.run_limits.check()?;
        if claims.decision != d.id()? || finals.decision != d.id()? {
            return Err(Error::Authority);
        }
        if !cancel {
            if self.e.age.is_some() != self.age.is_some() {
                return Err(Error::Authority);
            }
            if let Some(ctx) = &self.age {
                aged::boundary(self.e, self.roster, ctx)?;
            }
        }
        let w = witness(self.e, d, self.jobs, self.witness, None)?;
        let _guard = w.admit(self.witness.store, self.witness.ledger, self.witness.job)?;
        hook(Stage::Admitted)?;
        let state = w.state(self.witness.store, self.witness.ledger, self.witness.job)?;
        if state.terminal == Some(false) {
            w.close_unused(
                self.witness.store,
                self.witness.ledger,
                self.witness.job,
                false,
            )?;
            let l = self.loaded(d)?;
            return self.report(d, l.as_ref(), Vec::new(), Vec::new());
        }
        let cancelling = cancel || state.terminal == Some(true);
        if cancelling {
            w.publish(0, self.witness.store, self.witness.ledger, self.witness.job)?;
            w.publish(1, self.witness.store, self.witness.ledger, self.witness.job)?;
            hook(Stage::Cancelled)?;
            // Stop is acknowledged before attempting metadata/provider cleanup.
            let loaded = self.loaded(d).unwrap_or_default();
            if let Some(l) = &loaded {
                self.cancel_pages(d, l)?;
            }
            let (effects, holds) = if let Some(l) = &loaded {
                self.actions(d, l, true, &mut hook)?
            } else {
                (Vec::new(), Vec::new())
            };
            w.close_unused(
                self.witness.store,
                self.witness.ledger,
                self.witness.job,
                true,
            )?;
            return self.report(d, loaded.as_ref(), effects, holds);
        }
        self.time(d)?;
        let loaded = self.loaded(d)?.ok_or(Error::Authority)?;
        if self.e.age.is_some() != self.age.is_some() {
            return Err(Error::Authority);
        }
        let proofs = if let Some(ctx) = self.age.as_mut() {
            let metadata = self.metadata.as_deref_mut().ok_or(Error::Authority)?;
            let mut reader = metadata.finished_reader()?;
            let proofs = aged::read(
                self.e,
                d,
                &loaded,
                &mut Nodes {
                    inner: &mut reader,
                    budget: Some(&self.budget),
                },
                self.limits,
            )?;
            proofs.verify(
                self.e,
                self.roster,
                self.namespaces,
                &mut reader,
                ctx,
                &self.budget,
                Some(self.clock),
            )?;
            Some(proofs)
        } else {
            None
        };
        if metadata_peak(self.e, d, self.jobs, self.witness, &loaded)? > self.e.max_metadata_bytes {
            return Err(Error::Budget);
        }
        if !state.intent {
            let m = self.metadata.as_deref_mut().ok_or(Error::Authority)?;
            m.finished_reader()?;
            let stores = self.places.iter().map(|l| l.store).collect::<Vec<_>>();
            let mut fresh = decision::plan_inner(
                self.e,
                d.request.clone(),
                self.roster,
                self.namespaces,
                &mut Nodes {
                    inner: &mut *m,
                    budget: Some(&self.budget),
                },
                &stores,
                self.limits,
            )?;
            // The retained pinned age proofs above are reverified independently;
            // new stronger observations cannot rebind the original approval.
            fresh.age = d.age.clone();
            if fresh != *d {
                return Err(Error::Stale);
            }
            m.finished_reader()?;
        }
        self.age_deadline = proofs.as_ref().map(|p| p.deadline(self.e)).transpose()?;
        let consent = Consent::new(
            self.e,
            &loaded,
            &self.budget,
            self.age_deadline.map(|deadline| (self.clock, deadline)),
        )?;
        consent.current(self.roster, self.namespaces)?;
        for p in &loaded.actions {
            let local = self
                .places
                .iter_mut()
                .find(|l| l.store.identity() == p.enrollment.store())
                .ok_or(Error::Authority)?;
            let job = self
                .jobs
                .iter()
                .find(|j| &j.store == p.enrollment.store())
                .ok_or(Error::Authority)?;
            provider::admit(local.store, local.ledger, &job.job, p)?;
        }
        self.time(d)?;
        consent.current(self.roster, self.namespaces)?;
        w.publish(0, self.witness.store, self.witness.ledger, self.witness.job)?;
        hook(Stage::Intent)?;
        let mut before = loaded
            .prepared
            .basis
            .members
            .iter()
            .map(|p| (p.namespace.clone(), p.minimum.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut progressed = 0;
        let mut inherited = BTreeMap::new();
        for (s, p) in &loaded.steps {
            let pw = witness(self.e, d, self.jobs, self.witness, Some((s, p)))?;
            let ps = pw.state(self.witness.store, self.witness.ledger, self.witness.job)?;
            if ps.terminal == Some(true) {
                return Err(Error::Recovery);
            }
            let at = self
                .namespaces
                .iter()
                .position(|n| n.identity() == &s.origin.namespace)
                .ok_or(Error::Authority)?;
            let profile = self.e.source(&s.origin.namespace)?;
            let observed = subset::inspect(profile, &mut self.namespaces[at], p)?;
            if ps.terminal == Some(false) {
                if observed
                    .historical_retirement
                    .as_ref()
                    .is_none_or(|r| r.checkpoint != s.retirement_checkpoint)
                {
                    return Err(Error::Recovery);
                }
                if let (Some(proofs), Some(ctx)) = (&proofs, self.age.as_mut()) {
                    let id = s.origin.id()?;
                    let status = proofs.page(
                        self.e,
                        s,
                        p,
                        &mut self.namespaces[at],
                        self.metadata.as_deref_mut().ok_or(Error::Authority)?,
                        ctx,
                        &self.budget,
                        inherited.get(&id),
                        true,
                    )?;
                    if let Some(status) = status {
                        inherited.insert(id, status);
                    }
                }
                pw.close_unused(
                    self.witness.store,
                    self.witness.ledger,
                    self.witness.job,
                    false,
                )?;
                before.insert(s.origin.namespace.clone(), s.retirement_checkpoint.clone());
                continue;
            }
            if progressed >= self.run_limits.pages {
                return self.report(d, Some(&loaded), Vec::new(), Vec::new());
            }
            self.time(d)?;
            consent.current(self.roster, self.namespaces)?;
            if !ps.intent
                && (observed.historical_retirement.is_some()
                    || self.namespaces[at].checkpoint()
                        != *before.get(&s.origin.namespace).ok_or(Error::Authority)?)
            {
                return Err(Error::Recovery);
            }
            let _page_guard =
                pw.admit(self.witness.store, self.witness.ledger, self.witness.job)?;
            hook(Stage::PageAdmitted(s.index))?;
            pw.publish(0, self.witness.store, self.witness.ledger, self.witness.job)?;
            hook(Stage::PageIntent(s.index))?;
            self.time(d)?;
            consent.current(self.roster, self.namespaces)?;
            let clock = self.clock;
            let clock_id = &self.e.clock;
            let started = self.started;
            let millis = self.limits.source.millis;
            let mut interrupted = None;
            let result = subset::apply_metered(
                profile,
                &mut self.namespaces[at],
                self.metadata.as_deref_mut().ok_or(Error::Authority)?,
                p,
                &self.budget,
                |stage| {
                    if let Err(e) = hook(Stage::Source(s.index, stage)) {
                        interrupted = Some(e);
                        return Err(subset::Error::Injected);
                    }
                    if started.elapsed() > Duration::from_millis(millis) {
                        interrupted = Some(Error::Budget);
                        return Err(subset::Error::Injected);
                    }
                    if stage == subset::Stage::BeforeRetire
                        && let Err(e) = d.request.window.check_clock(clock, clock_id, 0)
                    {
                        interrupted = Some(e.into());
                        return Err(subset::Error::Injected);
                    }
                    if stage == subset::Stage::BeforeRetire
                        && let Err(e) = consent.time()
                    {
                        interrupted = Some(e);
                        return Err(subset::Error::Injected);
                    }
                    Ok(())
                },
            );
            if let Some(e) = interrupted {
                return Err(e);
            }
            let result = result?;
            if result
                .historical_retirement
                .as_ref()
                .is_none_or(|r| r.checkpoint != s.retirement_checkpoint)
            {
                return Err(Error::Recovery);
            }
            if let (Some(proofs), Some(ctx)) = (&proofs, self.age.as_mut()) {
                let id = s.origin.id()?;
                let status = proofs.page(
                    self.e,
                    s,
                    p,
                    &mut self.namespaces[at],
                    self.metadata.as_deref_mut().ok_or(Error::Authority)?,
                    ctx,
                    &self.budget,
                    inherited.get(&id),
                    false,
                )?;
                if let Some(status) = status {
                    inherited.insert(id, status);
                }
                hook(Stage::AgeAcknowledged(s.index))?;
                self.time(d)?;
            }
            pw.publish(2, self.witness.store, self.witness.ledger, self.witness.job)?;
            pw.close_unused(
                self.witness.store,
                self.witness.ledger,
                self.witness.job,
                false,
            )?;
            hook(Stage::PageCompleted(s.index))?;
            progressed += 1;
            before.insert(s.origin.namespace.clone(), s.retirement_checkpoint.clone());
        }
        let (effects, holds) = self.actions(d, &loaded, false, &mut hook)?;
        let r = self.report(d, Some(&loaded), effects, holds)?;
        if r.historical_deleted_actions == Some(d.action_count) && r.holds.is_empty() {
            w.publish(2, self.witness.store, self.witness.ledger, self.witness.job)?;
            w.close_unused(
                self.witness.store,
                self.witness.ledger,
                self.witness.job,
                false,
            )?;
            hook(Stage::Completed)?;
            return self.report(d, Some(&loaded), r.effects, Vec::new());
        }
        Ok(r)
    }
    fn cancel_pages(&mut self, d: &Decision, l: &Loaded) -> Result<()> {
        for (s, p) in &l.steps {
            self.timed()?;
            let w = witness(self.e, d, self.jobs, self.witness, Some((s, p)))?;
            let state = w.state(self.witness.store, self.witness.ledger, self.witness.job)?;
            if !state.admitted {
                continue;
            }
            let _guard = w.admit(self.witness.store, self.witness.ledger, self.witness.job)?;
            if state.terminal != Some(false) {
                w.publish(0, self.witness.store, self.witness.ledger, self.witness.job)?;
                w.publish(1, self.witness.store, self.witness.ledger, self.witness.job)?;
            }
            w.close_unused(
                self.witness.store,
                self.witness.ledger,
                self.witness.job,
                state.terminal != Some(false),
            )?;
        }
        Ok(())
    }
    fn actions(
        &mut self,
        d: &Decision,
        l: &Loaded,
        cancel: bool,
        hook: &mut impl FnMut(Stage) -> Result<()>,
    ) -> Result<(Vec<Effect>, Vec<Held>)> {
        let consent = Consent::new(
            self.e,
            l,
            &self.budget,
            self.age_deadline.map(|deadline| (self.clock, deadline)),
        )?;
        let mut effects = Vec::new();
        let mut holds = Vec::new();
        let mut raw_reader = self
            .metadata
            .as_deref_mut()
            .map(|m| m.finished_reader())
            .transpose()?;
        let reader = raw_reader.as_mut().map(|r| Nodes {
            inner: r,
            budget: Some(&self.budget),
        });
        let mut progressed = 0;
        for (i, p) in l.actions.iter().enumerate() {
            self.timed()?;
            let Some(local) = self
                .places
                .iter_mut()
                .find(|l| l.store.identity() == p.enrollment.store())
            else {
                holds.push(Held {
                    kind: "provider_unavailable",
                    index: i,
                });
                continue;
            };
            let job = self
                .jobs
                .iter()
                .find(|j| &j.store == p.enrollment.store())
                .ok_or(Error::Authority)?;
            if cancel && !provider::admitted(local.ledger, &job.job, p)? {
                continue;
            }
            match provider::status::inspect(local.store, local.ledger, &job.job, p) {
                Ok(s)
                    if s.charged_metadata.is_some()
                        && matches!(s.phase, provider::status::Phase::Deleted) =>
                {
                    continue;
                }
                Ok(s)
                    if s.charged_metadata.is_some()
                        && matches!(s.phase, provider::status::Phase::Cancelled) =>
                {
                    if !cancel {
                        holds.push(Held {
                            kind: "provider_already_cancelled",
                            index: i,
                        });
                    }
                    continue;
                }
                Ok(_) => (),
                Err(_) => {
                    holds.push(Held {
                        kind: "provider_status_unknown",
                        index: i,
                    });
                    continue;
                }
            }
            if progressed >= self.run_limits.actions {
                break;
            }
            progressed += 1;
            let mut interrupted = None;
            let result = if cancel {
                provider::Execution {
                    store: local.store,
                    ledger: local.ledger,
                    job: &job.job,
                    roster: self.roster,
                    namespaces: self.namespaces,
                    nodes: &NoNodes,
                    clock: &NoTime,
                    admitted_context: &p.request().context,
                }
                .cancel(p, &p.authorize()?)
            } else {
                let reader = reader.as_ref().ok_or(Error::Authority)?;
                provider::Execution {
                    store: local.store,
                    ledger: local.ledger,
                    job: &job.job,
                    roster: self.roster,
                    namespaces: self.namespaces,
                    nodes: reader,
                    clock: self.clock,
                    admitted_context: &p.request().context,
                }
                .apply_guarded_hook(p, &p.authorize()?, &consent, |stage| {
                    if let Err(e) = hook(Stage::Provider(i, stage)) {
                        interrupted = Some(e);
                        return Err(provider::Error::Injected);
                    }
                    Ok(())
                })
            };
            if let Some(e) = interrupted {
                return Err(e);
            }
            match result {
                Ok(receipt) => {
                    effects.push(Effect { action: i, receipt });
                    hook(Stage::Removed(i))?;
                }
                Err(_) => holds.push(Held {
                    kind: if cancel {
                        "provider_cancellation_held"
                    } else {
                        "provider_held"
                    },
                    index: i,
                }),
            }
        }
        let _ = d;
        Ok((effects, holds))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_vectors_pin_decision_global_and_page_records() {
        let v: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/vectors/paged-control.design-1.json"
        )))
        .unwrap();
        let e: Enrollment = serde_json::from_str(v["enrollment"].as_str().unwrap()).unwrap();
        assert_eq!(
            encode(&e).unwrap(),
            v["enrollment"].as_str().unwrap().as_bytes()
        );
        assert_eq!(
            e.id().unwrap().as_str(),
            v["enrollment_id"].as_str().unwrap()
        );
        let d = Decision::from_json(v["decision"].as_str().unwrap().as_bytes()).unwrap();
        assert_eq!(d.id().unwrap().as_str(), v["decision_id"].as_str().unwrap());
        for case in v["records"].as_array().unwrap() {
            let raw = case["canonical"].as_str().unwrap().as_bytes();
            let actual = match case["kind"].as_str().unwrap() {
                "copy_index" => encode(&serde_json::from_slice::<CopyIndex>(raw).unwrap()).unwrap(),
                "subject" => encode(&serde_json::from_slice::<Subject>(raw).unwrap()).unwrap(),
                _ => panic!("unknown controller record frame"),
            };
            assert_eq!(actual, raw);
            assert_eq!(Digest::blob(raw).as_str(), case["sha256"].as_str().unwrap());
        }
        let job: JournalRef = serde_json::from_value(v["bindings"]["witness"].clone()).unwrap();
        let jobs: Vec<StoreJob> = serde_json::from_value(v["bindings"]["jobs"].clone()).unwrap();
        let s: preparation::Step = serde_json::from_str(v["step"].as_str().unwrap()).unwrap();
        for case in v["layouts"].as_array().unwrap() {
            let page = (case["kind"] == "page").then_some(&s);
            let layout = record_layout(
                &e,
                &d,
                Bindings {
                    witness: &job,
                    jobs: &jobs,
                    metadata: &e.selection.metadata,
                },
                page,
            )
            .unwrap();
            assert_eq!(layout.key.as_str(), case["key"].as_str().unwrap());
            assert_eq!(
                layout.control_binding.as_str(),
                case["control_binding"].as_str().unwrap()
            );
            for (i, raw) in layout.documents.iter().enumerate() {
                assert_eq!(raw, case["documents"][i].as_str().unwrap().as_bytes());
                assert_eq!(
                    Digest::blob(raw).as_str(),
                    case["document_hashes"][i].as_str().unwrap()
                );
            }
        }
    }
}
