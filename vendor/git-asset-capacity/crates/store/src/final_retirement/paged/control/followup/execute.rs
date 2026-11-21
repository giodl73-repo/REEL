use super::super::execute::{Effect, Held};
use super::*;
use crate::{
    final_retirement::bulk::witness::{Layout, Witness},
    metadata::MetadataReader,
};

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RunLimits {
    pub actions: usize,
}
impl Default for RunLimits {
    fn default() -> Self {
        Self { actions: 16 }
    }
}
impl RunLimits {
    fn check(&self) -> Result<()> {
        if self.actions == 0 || self.actions > 32 {
            Err(Error::Budget)
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub decision: Digest,
    pub phase: Phase,
    pub total_actions: usize,
    pub historical_deleted_actions: Option<usize>,
    pub pending_cancellations: Option<usize>,
    pub excluded_objects: Option<usize>,
    pub metadata_available: bool,
    pub cleanup_unknown: bool,
    pub holds: Vec<Held>,
    pub unreported_holds: usize,
    pub effects: Vec<Effect>,
    pub authorizes_removal: bool,
}
pub struct Execution<'a, 'b> {
    pub enrollment: &'a Enrollment,
    pub roster: &'a claims::Roster,
    pub namespaces: &'a mut [Namespace],
    pub metadata: &'a mut MetadataReader,
    pub places: &'a mut [Local<'b>],
    pub jobs: &'a [StoreJob],
    pub witness: Local<'b>,
    pub clock: &'a dyn provider::Clock,
    pub admitted_context: &'a Digest,
    pub limits: &'a Limits,
    pub run_limits: RunLimits,
}
pub struct Inspection<'a, 'b> {
    pub enrollment: &'a Enrollment,
    pub roster: &'a claims::Roster,
    pub metadata: Option<MetadataReader>,
    pub places: &'a mut [Local<'b>],
    pub jobs: &'a [StoreJob],
    pub witness: &'a mut Local<'b>,
    pub admitted_context: &'a Digest,
    pub limits: &'a Limits,
}
pub struct Cancellation<'a, 'b> {
    pub inspection: Inspection<'a, 'b>,
}

#[derive(Serialize)]
struct Bindings<'a> {
    witness: &'a JournalRef,
    jobs: &'a [StoreJob],
    metadata: &'a Digest,
}
fn layout(e: &Enrollment, d: &Decision, jobs: &[StoreJob], job: &JournalRef) -> Result<Layout> {
    let bindings = Bindings {
        witness: job,
        jobs,
        metadata: &e.selection.metadata,
    };
    let binding = hash(&bindings)?;
    let id = d.id()?;
    let key = hash(
        &serde_json::json!({"schema":"tig.held-copy-followup-key.design-1",
        "authority":e.selection.authority,"epoch":e.selection.epoch,"owner":e.selection.owner,"operation":d.request.operation}),
    )?;
    let terminal = |cancelled: bool| {
        encode(
            &serde_json::json!({"schema":"tig.held-copy-followup-terminal.design-1",
        "decision":id,"bindings":binding,"cancelled":cancelled}),
        )
    };
    let docs = [
        encode(
            &serde_json::json!({"schema":"tig.held-copy-followup-intent.design-1","decision":d,"bindings":bindings}),
        )?,
        terminal(true)?,
        terminal(false)?,
    ];
    Ok(Layout {
        owner: e.selection.owner.clone(),
        key,
        plan: id.clone(),
        control_binding: hash(
            &serde_json::json!({"schema":"tig.held-copy-followup-binding.design-1","decision":id,"bindings":binding}),
        )?,
        control_object: Object::from_bytes(&docs[0]),
        documents: docs,
        prefix: "followup",
        max_metadata_bytes: e.max_metadata_bytes,
    })
}
fn witness(e: &Enrollment, d: &Decision, jobs: &[StoreJob], local: &Local<'_>) -> Result<Witness> {
    Ok(Witness::from_records(
        layout(e, d, jobs, local.job)?,
        local.store,
    )?)
}
fn bound(
    e: &Enrollment,
    d: &Decision,
    roster: &claims::Roster,
    places: &[Local<'_>],
    jobs: &[StoreJob],
    w: &Local<'_>,
    context: &Digest,
) -> Result<()> {
    e.id()?;
    d.id()?;
    d.request.check(e)?;
    roster.id()?;
    if d.enrollment != e.id()?
        || context != &e.context
        || roster.scope()? != e.selection.scope
        || w.ledger.identity() != &e.capacity
        || w.store.supports_retirement()
        || Place::local(w.store, e.witness.generation).map_err(|_| Error::Authority)? != e.witness
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

struct Consent<'a> {
    basis: &'a claims::Roster,
    budget: &'a Budget,
    age: Option<(&'a dyn provider::Clock, &'a Digest, u64)>,
}
impl Consent<'_> {
    fn current(&self, trusted: &claims::Roster, namespaces: &mut [Namespace]) -> Result<()> {
        self.budget.timed()?;
        if current_basis(trusted, namespaces)? != *self.basis {
            return Err(Error::Stale);
        }
        self.time()?;
        self.budget.timed()?;
        Ok(())
    }
    fn time(&self) -> Result<()> {
        if let Some((clock, id, deadline)) = self.age {
            let t = clock.bounds(id)?;
            if t.earliest < deadline || t.latest < t.earliest || t.latest > MAX_NUMBER {
                return Err(Error::Authority);
            }
        }
        Ok(())
    }
}
impl provider::Condition for Consent<'_> {
    fn check(&self, c: &claims::Clearance<'_>) -> provider::Result<()> {
        self.budget
            .timed()
            .map_err(|_| provider::Error::Claims(claims::Error::Budget))?;
        self.time().map_err(|_| provider::Error::Clock)?;
        if !c.only_owned_commands(self.basis, &BTreeMap::new())? {
            return Err(provider::Error::WrongAuthority);
        }
        self.budget
            .timed()
            .map_err(|_| provider::Error::Claims(claims::Error::Budget))?;
        Ok(())
    }
}
struct NoNodes;
impl NodeStore for NoNodes {
    fn get(&self, _: &Object) -> TreeResult<Option<Vec<u8>>> {
        Err(TreeError::Budget)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        Err(TreeError::Budget)
    }
}
struct NoTime;
impl provider::Clock for NoTime {
    fn now(&self, _: &Digest) -> provider::Result<u64> {
        Err(provider::Error::Clock)
    }
}

impl Inspection<'_, '_> {
    fn check(&self, d: &Decision) -> Result<()> {
        if self
            .metadata
            .as_ref()
            .is_some_and(|r| r.checkpoint().identity() != &self.enrollment.selection.metadata)
        {
            return Err(Error::Authority);
        }
        bound(
            self.enrollment,
            d,
            self.roster,
            self.places,
            self.jobs,
            self.witness,
            self.admitted_context,
        )
    }
    pub fn required_witness_peak(&self, d: &Decision) -> Result<u64> {
        self.check(d)?;
        Ok(witness(self.enrollment, d, self.jobs, self.witness)?.peak)
    }
    pub fn status(&mut self, d: &Decision) -> Result<Report> {
        self.check(d)?;
        let budget = Budget::new(&self.limits.source, 0, 0)?;
        let loaded = self
            .metadata
            .as_mut()
            .map(|m| {
                load(
                    self.enrollment,
                    d,
                    &mut Nodes {
                        inner: m,
                        budget: Some(&budget),
                    },
                    self.limits,
                )
            })
            .transpose()
            .ok()
            .flatten();
        self.report(d, loaded.as_ref(), Vec::new(), Vec::new(), &budget)
    }
    fn report(
        &mut self,
        d: &Decision,
        loaded: Option<&Loaded>,
        effects: Vec<Effect>,
        mut holds: Vec<Held>,
        budget: &Budget,
    ) -> Result<Report> {
        budget.timed()?;
        let w = witness(self.enrollment, d, self.jobs, self.witness)?;
        let state = w.state(self.witness.store, self.witness.ledger, self.witness.job)?;
        let mut deleted = 0;
        let mut pending = 0;
        let mut unknown = loaded.is_none();
        if let Some(l) = loaded {
            for (i, p) in l.actions.iter().enumerate() {
                budget.timed()?;
                let local = self
                    .places
                    .iter_mut()
                    .find(|s| s.store.identity() == p.enrollment.store());
                let job = self.jobs.iter().find(|j| &j.store == p.enrollment.store());
                match local.zip(job).map(|(local, job)| {
                    provider::status::inspect(local.store, local.ledger, &job.job, p)
                }) {
                    Some(Ok(s))
                        if s.charged_metadata.is_some()
                            && s.phase == provider::status::Phase::Deleted =>
                    {
                        deleted += 1
                    }
                    Some(Ok(s))
                        if s.charged_metadata.is_some()
                            && s.phase == provider::status::Phase::Cancelled => {}
                    Some(Ok(s)) => {
                        if s.charged_metadata.is_some() {
                            pending += 1;
                        }
                    }
                    _ => {
                        unknown = true;
                        holds.push(Held {
                            kind: "provider_status_unknown",
                            index: i,
                        });
                    }
                }
            }
        }
        let unreported = holds.len().saturating_sub(64);
        holds.truncate(64);
        budget.timed()?;
        Ok(Report {
            schema: "tig.held-copy-followup-status.design-1",
            decision: d.id()?,
            phase: match state.terminal {
                Some(true) => Phase::Cancelled,
                Some(false) => Phase::Completed,
                None if state.intent => Phase::Intent,
                None if state.admitted => Phase::Admitted,
                None => Phase::NotStarted,
            },
            total_actions: d.action_count,
            historical_deleted_actions: (!unknown).then_some(deleted),
            pending_cancellations: (!unknown).then_some(pending),
            excluded_objects: loaded
                .map(|l| l.subjects.iter().filter(|s| s.hold.is_some()).count()),
            metadata_available: loaded.is_some(),
            cleanup_unknown: unknown,
            holds,
            unreported_holds: unreported,
            effects,
            authorizes_removal: false,
        })
    }
}

impl Execution<'_, '_> {
    pub fn apply_aged(
        &mut self,
        d: &Decision,
        auth: &Authorization,
        age: aged::Context<'_>,
    ) -> Result<Report> {
        self.apply_hook(d, auth, Some(age), |_| Ok(()))
    }
    pub fn apply(&mut self, d: &Decision, auth: &Authorization) -> Result<Report> {
        self.apply_hook(d, auth, None, |_| Ok(()))
    }
    pub fn apply_hook(
        &mut self,
        d: &Decision,
        auth: &Authorization,
        age: Option<aged::Context<'_>>,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Report> {
        if self.metadata.checkpoint().identity() != &self.enrollment.selection.metadata {
            return Err(Error::Authority);
        }
        Runtime {
            e: self.enrollment,
            roster: self.roster,
            namespaces: self.namespaces,
            reader: Some(&mut *self.metadata),
            places: self.places,
            jobs: self.jobs,
            witness: &mut self.witness,
            clock: self.clock,
            context: self.admitted_context,
            limits: self.limits,
            runs: self.run_limits.clone(),
            budget: Budget::new(
                &self.limits.source,
                self.limits.writes,
                self.limits.write_bytes,
            )?,
            age,
        }
        .run(d, auth, false, hook)
    }
}
impl Cancellation<'_, '_> {
    pub fn cancel(&mut self, d: &Decision, auth: &Authorization) -> Result<Report> {
        self.cancel_hook(d, auth, |_| Ok(()))
    }
    pub fn cancel_hook(
        &mut self,
        d: &Decision,
        auth: &Authorization,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Report> {
        let i = &mut self.inspection;
        if i.metadata
            .as_ref()
            .is_some_and(|r| r.checkpoint().identity() != &i.enrollment.selection.metadata)
        {
            return Err(Error::Authority);
        }
        Runtime {
            e: i.enrollment,
            roster: i.roster,
            namespaces: &mut [],
            reader: i.metadata.as_mut(),
            places: i.places,
            jobs: i.jobs,
            witness: i.witness,
            clock: &NoTime,
            context: i.admitted_context,
            limits: i.limits,
            runs: RunLimits::default(),
            budget: Budget::new(&i.limits.source, i.limits.writes, i.limits.write_bytes)?,
            age: None,
        }
        .run(d, auth, true, hook)
    }
}
struct Runtime<'a, 'b, 'c> {
    e: &'a Enrollment,
    roster: &'a claims::Roster,
    namespaces: &'a mut [Namespace],
    reader: Option<&'a mut MetadataReader>,
    places: &'a mut [Local<'b>],
    jobs: &'a [StoreJob],
    witness: &'a mut Local<'b>,
    clock: &'a dyn provider::Clock,
    context: &'a Digest,
    limits: &'a Limits,
    runs: RunLimits,
    budget: Budget,
    age: Option<aged::Context<'c>>,
}
impl Runtime<'_, '_, '_> {
    fn loaded(&mut self, d: &Decision) -> Result<Option<Loaded>> {
        self.reader
            .as_mut()
            .map(|r| {
                load(
                    self.e,
                    d,
                    &mut Nodes {
                        inner: &mut **r,
                        budget: Some(&self.budget),
                    },
                    self.limits,
                )
            })
            .transpose()
    }
    fn report(
        &mut self,
        d: &Decision,
        l: Option<&Loaded>,
        effects: Vec<Effect>,
        holds: Vec<Held>,
    ) -> Result<Report> {
        let mut inspection = Inspection {
            enrollment: self.e,
            roster: self.roster,
            metadata: None,
            places: self.places,
            jobs: self.jobs,
            witness: self.witness,
            admitted_context: self.context,
            limits: self.limits,
        };
        inspection.report(d, l, effects, holds, &self.budget)
    }
    fn run(
        &mut self,
        d: &Decision,
        auth: &Authorization,
        cancel: bool,
        mut hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Report> {
        bound(
            self.e,
            d,
            self.roster,
            self.places,
            self.jobs,
            self.witness,
            self.context,
        )?;
        self.runs.check()?;
        if auth.decision != d.id()? {
            return Err(Error::Authority);
        }
        let w = witness(self.e, d, self.jobs, self.witness)?;
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
            let loaded = self.loaded(d).unwrap_or_default();
            return self.report(d, loaded.as_ref(), Vec::new(), Vec::new());
        }
        if cancel || state.terminal == Some(true) {
            w.publish(0, self.witness.store, self.witness.ledger, self.witness.job)?;
            w.publish(1, self.witness.store, self.witness.ledger, self.witness.job)?;
            hook(Stage::Cancelled)?;
            let loaded = self.loaded(d).unwrap_or_default();
            let (effects, holds) = if let Some(l) = &loaded {
                self.actions(d, l, true, None, &mut hook)?
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
        self.budget.timed()?;
        d.request.window.check_clock(self.clock, &self.e.clock, 0)?;
        let l = self.loaded(d)?.ok_or(Error::Authority)?;
        let origin_state = {
            let mut origin = Local {
                store: self.witness.store,
                ledger: &mut *self.witness.ledger,
                job: &d.request.origin.witness_job,
            };
            d.request
                .origin
                .sealed(self.e, self.roster, &mut origin, self.limits)?
        };
        if origin_state != d.origin_cancelled || self.e.age.is_some() != self.age.is_some() {
            return Err(Error::Authority);
        }
        let mut deadline = None;
        if let Some(ctx) = &mut self.age {
            let mut nodes = Nodes {
                inner: &mut **self.reader.as_mut().ok_or(Error::Authority)?,
                budget: Some(&self.budget),
            };
            let proofs = aged::read(
                self.e,
                &d.request.origin.decision,
                &l.original,
                &mut nodes,
                self.limits,
            )?;
            proofs.verify(
                self.e,
                self.roster,
                self.namespaces,
                &mut nodes,
                ctx,
                &self.budget,
                Some(self.clock),
            )?;
            deadline = Some(proofs.deadline(self.e)?);
        }
        Consent {
            basis: &l.basis,
            budget: &self.budget,
            age: deadline.map(|v| (self.clock, &self.e.clock, v)),
        }
        .current(self.roster, self.namespaces)?;
        // Provider::Execution admits each attempted action. Copy-only work has
        // no source transition requiring every provider's resources up front.
        d.request.window.check_clock(self.clock, &self.e.clock, 0)?;
        w.publish(0, self.witness.store, self.witness.ledger, self.witness.job)?;
        hook(Stage::Intent)?;
        let (effects, holds) = self.actions(d, &l, false, deadline, &mut hook)?;
        let report = self.report(d, Some(&l), effects, holds)?;
        if report.historical_deleted_actions == Some(d.action_count) && report.holds.is_empty() {
            w.publish(2, self.witness.store, self.witness.ledger, self.witness.job)?;
            hook(Stage::Completed)?;
            w.close_unused(
                self.witness.store,
                self.witness.ledger,
                self.witness.job,
                false,
            )?;
            let Report { effects, holds, .. } = report;
            return self.report(d, Some(&l), effects, holds);
        }
        Ok(report)
    }
    fn actions(
        &mut self,
        _: &Decision,
        l: &Loaded,
        cancel: bool,
        deadline: Option<u64>,
        hook: &mut impl FnMut(Stage) -> Result<()>,
    ) -> Result<(Vec<Effect>, Vec<Held>)> {
        let consent = Consent {
            basis: &l.basis,
            budget: &self.budget,
            age: deadline.map(|v| (self.clock, &self.e.clock, v)),
        };
        let mut effects = Vec::new();
        let mut holds = Vec::new();
        let mut attempted = 0;
        for (i, p) in l.actions.iter().enumerate() {
            self.budget.timed()?;
            let Some(local) = self
                .places
                .iter_mut()
                .find(|s| s.store.identity() == p.enrollment.store())
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
                        && s.phase == provider::status::Phase::Deleted =>
                {
                    continue;
                }
                Ok(s)
                    if s.charged_metadata.is_some()
                        && s.phase == provider::status::Phase::Cancelled =>
                {
                    if !cancel {
                        holds.push(Held {
                            kind: "provider_already_cancelled",
                            index: i,
                        });
                    }
                    continue;
                }
                Err(_) => {
                    holds.push(Held {
                        kind: "provider_status_unknown",
                        index: i,
                    });
                    continue;
                }
                _ => (),
            }
            if attempted >= self.runs.actions {
                break;
            }
            attempted += 1;
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
                let nodes = Nodes {
                    inner: &mut **self.reader.as_mut().ok_or(Error::Authority)?,
                    budget: Some(&self.budget),
                };
                let mut interrupted = None;
                let result = provider::Execution {
                    store: local.store,
                    ledger: local.ledger,
                    job: &job.job,
                    roster: self.roster,
                    namespaces: self.namespaces,
                    nodes: &nodes,
                    clock: self.clock,
                    admitted_context: &p.request().context,
                }
                .apply_guarded_hook(p, &p.authorize()?, &consent, |stage| {
                    if let Err(e) = hook(Stage::Provider(i, stage)) {
                        interrupted = Some(e);
                        return Err(provider::Error::Injected);
                    }
                    Ok(())
                });
                if let Some(e) = interrupted {
                    return Err(e);
                }
                result
            };
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
        Ok((effects, holds))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_followup_frames_bind_new_decision_origin_and_accounted_records() {
        let v: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/vectors/held-copy-followup.design-1.json"
        )))
        .unwrap();
        let e: Enrollment = serde_json::from_str(v["enrollment"].as_str().unwrap()).unwrap();
        let job: JournalRef = serde_json::from_value(v["bindings"]["witness"].clone()).unwrap();
        for case in v["cases"].as_array().unwrap() {
            let raw = case["decision"].as_str().unwrap().as_bytes();
            let d = Decision::from_json(raw).unwrap();
            assert_eq!(d.to_json().unwrap(), raw);
            assert_eq!(
                d.id().unwrap().as_str(),
                case["decision_id"].as_str().unwrap()
            );
            let layout = layout(&e, &d, &[], &job).unwrap();
            assert_eq!(
                layout.key.as_str(),
                case["operation_digest"].as_str().unwrap()
            );
            assert_eq!(
                layout.control_binding.as_str(),
                case["binding_digest"].as_str().unwrap()
            );
            for (i, raw) in layout.documents.iter().enumerate() {
                assert_eq!(raw, case["documents"][i].as_str().unwrap().as_bytes());
                assert_eq!(
                    Digest::blob(raw).as_str(),
                    case["document_digests"][i].as_str().unwrap()
                );
            }
        }
    }
}
