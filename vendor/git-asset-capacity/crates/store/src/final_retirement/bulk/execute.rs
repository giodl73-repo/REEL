use super::witness::{Bindings, Witness};
use super::*;
use crate::{
    ledger::JournalRef, metadata::MetadataStore, source_claims::admission::bulk as history,
};
use git_asset_core::tree::{NodeStore, TreeError, TreeResult};
struct Reader(Option<crate::metadata::MetadataReader>);
impl NodeStore for Reader {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        match &self.0 {
            Some(reader) => reader.get(object),
            None => Err(TreeError::MissingNode(object.clone())),
        }
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        Err(TreeError::InvalidNode)
    }
}

pub struct Execution<'a, 'b> {
    pub enrollment: &'a Enrollment,
    pub roster: &'a claims::Roster,
    pub namespaces: &'a mut [Namespace],
    pub metadata: &'a mut MetadataStore,
    pub places: &'a mut [Local<'b>],
    pub action_jobs: &'a [JournalRef],
    pub witness: Local<'b>,
    pub clock: &'a dyn provider::Clock,
    pub admitted_context: &'a Digest,
}
/// Observations over independently admitted identities, without a metadata writer
/// or clock. Status/peak add no commands, source mutations or provider effects.
pub struct Inspection<'a, 'b> {
    pub enrollment: &'a Enrollment,
    pub roster: &'a claims::Roster,
    pub namespaces: &'a mut [Namespace],
    pub metadata: Digest,
    pub places: &'a mut [Local<'b>],
    pub action_jobs: &'a [JournalRef],
    pub witness: &'a mut Local<'b>,
    pub admitted_context: &'a Digest,
}
/// Explicit cancellation capability, without source metadata or a fresh clock.
/// Inspection itself remains observational; only this distinct port can mutate.
pub struct Cancellation<'a, 'b> {
    pub inspection: Inspection<'a, 'b>,
}
struct Runtime<'a, 'b> {
    enrollment: &'a Enrollment,
    roster: &'a claims::Roster,
    namespaces: &'a mut [Namespace],
    metadata: Option<&'a mut MetadataStore>,
    metadata_identity: Digest,
    places: &'a mut [Local<'b>],
    action_jobs: &'a [JournalRef],
    witness: &'a mut Local<'b>,
    clock: &'a dyn provider::Clock,
    admitted_context: &'a Digest,
}
struct NoTime;
impl provider::Clock for NoTime {
    fn now(&self, _: &Digest) -> provider::Result<u64> {
        Err(provider::Error::Clock)
    }
}
impl Cancellation<'_, '_> {
    pub fn cancel(&mut self, p: &Plan, auth: &Authorization) -> Result<Report> {
        let i = &mut self.inspection;
        Runtime {
            enrollment: i.enrollment,
            roster: i.roster,
            namespaces: i.namespaces,
            metadata: None,
            metadata_identity: i.metadata.clone(),
            places: i.places,
            action_jobs: i.action_jobs,
            witness: i.witness,
            clock: &NoTime,
            admitted_context: i.admitted_context,
        }
        .run(p, auth, true, |_| Ok(()))
    }
}
struct Control<'a, 'b> {
    enrollment: &'a Enrollment,
    roster: &'a claims::Roster,
    metadata: Digest,
    places: &'a [Local<'b>],
    action_jobs: &'a [JournalRef],
    witness: &'a Local<'b>,
    admitted_context: &'a Digest,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Admitted,
    Intent,
    Source(usize, subset::Stage),
    Settled(usize),
    Removed(usize),
    Cancelled,
    Completed,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    NotStarted,
    Admitted,
    Intent,
    Cancelled,
    Completed,
}
#[derive(Debug, Serialize)]
pub struct Failure {
    pub kind: &'static str,
    pub index: usize,
    pub code: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Effect {
    pub action: usize,
    pub receipt: provider::Receipt,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub plan: Digest,
    pub phase: Phase,
    pub intent_acknowledged: bool,
    pub selected_roots: usize,
    pub selected_bindings: usize,
    pub unique_objects: usize,
    pub selected_unique_bytes: u64,
    pub eligible_unique_bytes: u64,
    pub planned_copy_bytes: u64,
    pub new_logical_copy_bytes_removed: u64,
    pub historical_deleted_copy_bytes: u64,
    pub file_allocation_before_new_removals: u64,
    pub required_witness_metadata_peak: u64,
    pub excluded: Vec<Subject>,
    pub source_status: Vec<Option<subset::Status>>,
    pub action_status: Vec<Option<provider::status::Status>>,
    pub effects: Vec<Effect>,
    pub holds: Vec<Failure>,
    pub removal_authority: bool,
}
struct Consent<'a> {
    plan: &'a Plan,
    allowed: BTreeMap<Digest, Vec<(Key, Command)>>,
}
impl Consent<'_> {
    fn new(plan: &Plan) -> Result<Consent<'_>> {
        let mut allowed = BTreeMap::<_, Vec<_>>::new();
        for s in &plan.settlements {
            allowed
                .entry(s.namespace.clone())
                .or_default()
                .extend(s.plan.commands(plan.enrollment.source(&s.namespace)?)?);
        }
        Ok(Consent { plan, allowed })
    }
    fn before_source(&self, trusted: &claims::Roster, namespaces: &mut [Namespace]) -> Result<()> {
        trusted.id()?;
        if trusted.scope()? != self.plan.enrollment.scope
            || namespaces.len() != self.plan.roster.members.len()
        {
            return Err(Error::Authority);
        }
        let mut seen = BTreeSet::new();
        for pin in &self.plan.roster.members {
            let private = trusted
                .members
                .iter()
                .find(|m| m.namespace == pin.namespace && m.policy == pin.policy)
                .ok_or(Error::Authority)?;
            let at = namespaces
                .iter()
                .position(|n| n.identity() == &pin.namespace)
                .ok_or(Error::Authority)?;
            if !seen.insert(at) {
                return Err(Error::Authority);
            }
            namespaces[at].refresh_minimum(&private.minimum)?;
            history::snapshot(&mut namespaces[at])?;
            let actual = claims::NamespacePin::current(&namespaces[at])?;
            if actual.policy != pin.policy
                || !history::only_owned_since(
                    &namespaces[at],
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
        Ok(())
    }
}
impl provider::Condition for Consent<'_> {
    fn check(&self, clearance: &claims::Clearance<'_>) -> provider::Result<()> {
        if clearance.only_owned_commands(&self.plan.roster, &self.allowed)? {
            Ok(())
        } else {
            Err(provider::Error::WrongAuthority)
        }
    }
}
fn provider_code(error: &provider::Error) -> &'static str {
    match error {
        provider::Error::Clock => "clock",
        provider::Error::WrongAuthority => "stale_consent",
        provider::Error::ProtectedGrant | provider::Error::Claims(claims::Error::Protected) => {
            "protected"
        }
        provider::Error::Store(crate::Error::Busy) => "busy",
        _ => "provider_held",
    }
}
fn sum_bytes(mut values: impl Iterator<Item = u64>) -> Result<u64> {
    values.try_fold(0u64, |a, b| a.checked_add(b).ok_or(Error::Budget))
}
impl Control<'_, '_> {
    fn common(&self, p: &Plan) -> Result<(Bindings, Witness)> {
        p.id()?;
        if self.enrollment.id()? != p.enrollment.id()?
            || self.admitted_context != &self.enrollment.context
            || self.roster.scope()? != self.enrollment.scope
            || self.witness.ledger.identity() != &self.enrollment.capacity
            || self.action_jobs.len() != p.actions.len()
        {
            return Err(Error::Authority);
        }
        let mut stores = BTreeSet::new();
        for place in self.places.iter() {
            if !stores.insert(place.store.identity()) {
                return Err(Error::Authority);
            }
        }
        for (index, action) in p.actions.iter().enumerate() {
            if let Some(local) = self
                .places
                .iter()
                .find(|l| l.store.identity() == action.enrollment.store())
                && local.job != &self.action_jobs[index]
            {
                return Err(Error::Authority);
            }
        }
        let bindings = Bindings {
            metadata: self.metadata.clone(),
            witness_job: self.witness.job.clone(),
            action_jobs: self.action_jobs.to_vec(),
        };
        let witness = Witness::new(p, &bindings, self.witness.store)?;
        Ok((bindings, witness))
    }
}
impl Inspection<'_, '_> {
    fn common(&self, p: &Plan) -> Result<(Bindings, Witness)> {
        Control {
            enrollment: self.enrollment,
            roster: self.roster,
            metadata: self.metadata.clone(),
            places: self.places,
            action_jobs: self.action_jobs,
            witness: self.witness,
            admitted_context: self.admitted_context,
        }
        .common(p)
    }
    pub fn required_metadata_peak(&self, p: &Plan) -> Result<u64> {
        Ok(self.common(p)?.1.peak)
    }
    pub fn status(&mut self, p: &Plan) -> Result<Report> {
        let (_, witness) = self.common(p)?;
        self.report(p, &witness, Vec::new(), Vec::new())
    }
    fn report(
        &mut self,
        p: &Plan,
        witness: &Witness,
        effects: Vec<Effect>,
        mut holds: Vec<Failure>,
    ) -> Result<Report> {
        let state = witness.state(self.witness.store, self.witness.ledger, self.witness.job)?;
        let mut source_status = Vec::new();
        for (index, s) in p.settlements.iter().enumerate() {
            let profile = p.enrollment.source(&s.namespace)?;
            let result = self
                .namespaces
                .iter_mut()
                .find(|n| n.identity() == &s.namespace)
                .map(|n| subset::inspect(profile, n, &s.plan))
                .transpose();
            match result {
                Ok(Some(value)) => source_status.push(Some(value)),
                Ok(None) => {
                    source_status.push(None);
                    holds.push(Failure {
                        kind: "source_status",
                        index,
                        code: "unavailable",
                    });
                }
                Err(_) => {
                    source_status.push(None);
                    holds.push(Failure {
                        kind: "source_status",
                        index,
                        code: "unavailable",
                    });
                }
            }
        }
        let mut action_status = Vec::new();
        for (index, action) in p.actions.iter().enumerate() {
            let result = self
                .places
                .iter_mut()
                .find(|l| l.store.identity() == action.enrollment.store())
                .map(|l| {
                    provider::status::inspect(l.store, l.ledger, &self.action_jobs[index], action)
                })
                .transpose();
            match result {
                Ok(Some(value)) => action_status.push(Some(value)),
                Ok(None) => {
                    action_status.push(None);
                    holds.push(Failure {
                        kind: "action_status",
                        index,
                        code: "unavailable",
                    });
                }
                Err(_) => {
                    action_status.push(None);
                    holds.push(Failure {
                        kind: "action_status",
                        index,
                        code: "unavailable",
                    });
                }
            }
        }
        let new_removed = effects.iter().filter(|e| {
            !e.receipt.historical && e.receipt.outcome == Some(provider::Outcome::Deleted)
        });
        let new_logical_copy_bytes_removed =
            sum_bytes(new_removed.clone().map(|e| e.receipt.logical_bytes_removed))?;
        let file_allocation_before_new_removals =
            sum_bytes(new_removed.map(|e| e.receipt.file_allocation_before))?;
        let historical_deleted_copy_bytes = sum_bytes(
            p.actions
                .iter()
                .zip(&action_status)
                .filter(
                    |(_, s)| matches!(s, Some(s) if s.phase == provider::status::Phase::Deleted),
                )
                .map(|(p, _)| p.request().publication.object.bytes),
        )?;
        Ok(Report {
            schema: "tig.bulk-final-status.design-1",
            plan: p.id()?,
            phase: match state.terminal {
                Some(true) => Phase::Cancelled,
                Some(false) => Phase::Completed,
                None if state.intent => Phase::Intent,
                None if state.admitted => Phase::Admitted,
                None => Phase::NotStarted,
            },
            intent_acknowledged: state.intent,
            selected_roots: p.request.selections.len(),
            selected_bindings: p.bindings.len(),
            unique_objects: p.subjects.len(),
            selected_unique_bytes: sum_bytes(p.subjects.iter().map(|s| s.object.bytes))?,
            eligible_unique_bytes: sum_bytes(
                p.subjects
                    .iter()
                    .filter(|s| s.hold.is_none())
                    .map(|s| s.object.bytes),
            )?,
            planned_copy_bytes: sum_bytes(
                p.actions
                    .iter()
                    .map(|a| a.request().publication.object.bytes),
            )?,
            new_logical_copy_bytes_removed,
            historical_deleted_copy_bytes,
            file_allocation_before_new_removals,
            required_witness_metadata_peak: witness.peak,
            excluded: p
                .subjects
                .iter()
                .filter(|s| s.hold.is_some())
                .cloned()
                .collect(),
            source_status,
            action_status,
            effects,
            holds,
            removal_authority: false,
        })
    }
}
impl<'a, 'b> Execution<'a, 'b> {
    fn common(&self, p: &Plan) -> Result<(Bindings, Witness)> {
        Control {
            enrollment: self.enrollment,
            roster: self.roster,
            metadata: self.metadata.checkpoint().identity().clone(),
            places: self.places,
            action_jobs: self.action_jobs,
            witness: &self.witness,
            admitted_context: self.admitted_context,
        }
        .common(p)
    }
    pub fn inspection(&mut self) -> Inspection<'_, 'b> {
        Inspection {
            enrollment: self.enrollment,
            roster: self.roster,
            namespaces: self.namespaces,
            metadata: self.metadata.checkpoint().identity().clone(),
            places: self.places,
            action_jobs: self.action_jobs,
            witness: &mut self.witness,
            admitted_context: self.admitted_context,
        }
    }
    pub fn required_metadata_peak(&self, p: &Plan) -> Result<u64> {
        Ok(self.common(p)?.1.peak)
    }
    pub fn status(&mut self, p: &Plan) -> Result<Report> {
        self.inspection().status(p)
    }
    fn runtime(&mut self) -> Runtime<'_, 'b> {
        Runtime {
            enrollment: self.enrollment,
            roster: self.roster,
            namespaces: self.namespaces,
            metadata_identity: self.metadata.checkpoint().identity().clone(),
            metadata: Some(self.metadata),
            places: self.places,
            action_jobs: self.action_jobs,
            witness: &mut self.witness,
            clock: self.clock,
            admitted_context: self.admitted_context,
        }
    }
    pub fn apply(&mut self, p: &Plan, auth: &Authorization) -> Result<Report> {
        self.apply_hook(p, auth, |_| Ok(()))
    }
    pub fn apply_hook(
        &mut self,
        p: &Plan,
        auth: &Authorization,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Report> {
        self.runtime().run(p, auth, false, hook)
    }
    pub fn cancel(&mut self, p: &Plan, auth: &Authorization) -> Result<Report> {
        self.runtime().run(p, auth, true, |_| Ok(()))
    }
    pub fn cancel_hook(
        &mut self,
        p: &Plan,
        auth: &Authorization,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Report> {
        self.runtime().run(p, auth, true, hook)
    }
}
impl Runtime<'_, '_> {
    fn common(&self, p: &Plan) -> Result<(Bindings, Witness)> {
        Control {
            enrollment: self.enrollment,
            roster: self.roster,
            metadata: self.metadata_identity.clone(),
            places: self.places,
            action_jobs: self.action_jobs,
            witness: self.witness,
            admitted_context: self.admitted_context,
        }
        .common(p)
    }
    fn report(
        &mut self,
        p: &Plan,
        witness: &Witness,
        effects: Vec<Effect>,
        holds: Vec<Failure>,
    ) -> Result<Report> {
        Inspection {
            enrollment: self.enrollment,
            roster: self.roster,
            namespaces: self.namespaces,
            metadata: self.metadata_identity.clone(),
            places: self.places,
            action_jobs: self.action_jobs,
            witness: self.witness,
            admitted_context: self.admitted_context,
        }
        .report(p, witness, effects, holds)
    }
    fn time(&self, p: &Plan) -> Result<()> {
        p.request
            .window
            .check_clock(self.clock, &p.enrollment.clock, 0)?;
        Ok(())
    }
    fn run(
        &mut self,
        p: &Plan,
        auth: &Authorization,
        cancel: bool,
        mut hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Report> {
        let (_, witness) = self.common(p)?;
        if auth.plan != p.id()? {
            return Err(Error::Authority);
        }
        let state = witness.state(self.witness.store, self.witness.ledger, self.witness.job)?;
        if !state.intent && !cancel {
            self.time(p)?;
            let reader = self
                .metadata
                .as_deref_mut()
                .ok_or(Error::Authority)?
                .finished_reader()
                .map_err(|_| Error::Recovery)?;
            let stores = self.places.iter().map(|l| l.store).collect::<Vec<_>>();
            let rebuilt = super::plan(
                self.enrollment,
                p.request.clone(),
                self.roster,
                self.namespaces,
                &reader,
                &stores,
            )?;
            if rebuilt.to_json()? != p.to_json()? {
                return Err(Error::Stale);
            }
        }
        let _control = witness.admit(self.witness.store, self.witness.ledger, self.witness.job)?;
        // A competing call may have completed/cancelled since our first sample.
        // The terminal decision is read again under the exact batch fence.
        let state = witness.state(self.witness.store, self.witness.ledger, self.witness.job)?;
        hook(Stage::Admitted)?;
        if state.terminal == Some(false) {
            witness.publish(2, self.witness.store, self.witness.ledger, self.witness.job)?;
            witness.close_unused(
                self.witness.store,
                self.witness.ledger,
                self.witness.job,
                false,
            )?;
            return self.report(p, &witness, Vec::new(), Vec::new());
        }
        let cancelling = cancel || state.terminal == Some(true);
        let mut holds = Vec::new();
        if !cancelling {
            for (index, action) in p.actions.iter().enumerate() {
                let Some(local) = self
                    .places
                    .iter_mut()
                    .find(|l| l.store.identity() == action.enrollment.store())
                else {
                    holds.push(Failure {
                        kind: "admission",
                        index,
                        code: "unavailable",
                    });
                    continue;
                };
                if let Err(e) =
                    provider::admit(local.store, local.ledger, &self.action_jobs[index], action)
                {
                    holds.push(Failure {
                        kind: "admission",
                        index,
                        code: provider_code(&e),
                    });
                }
            }
            if !holds.is_empty() {
                return self.report(p, &witness, Vec::new(), holds);
            }
        }
        witness.publish(0, self.witness.store, self.witness.ledger, self.witness.job)?;
        hook(Stage::Intent)?;
        if cancelling {
            witness.publish(1, self.witness.store, self.witness.ledger, self.witness.job)?;
            hook(Stage::Cancelled)?;
        }
        let consent = Consent::new(p)?;
        if !cancelling {
            for (index, s) in p.settlements.iter().enumerate() {
                let profile = p.enrollment.source(&s.namespace)?;
                let at = self
                    .namespaces
                    .iter()
                    .position(|n| n.identity() == &s.namespace)
                    .ok_or(Error::Authority)?;
                let status = subset::inspect(profile, &mut self.namespaces[at], &s.plan)?;
                if status.historical_retirement.is_none() {
                    self.time(p)?;
                    consent.before_source(self.roster, self.namespaces)?;
                    let mut injected = None;
                    let result = subset::apply_hook(
                        profile,
                        &mut self.namespaces[at],
                        self.metadata.as_deref_mut().ok_or(Error::Authority)?,
                        &s.plan,
                        |stage| {
                            hook(Stage::Source(index, stage)).map_err(|e| {
                                injected = Some(e);
                                subset::Error::Injected
                            })
                        },
                    );
                    if let Some(e) = injected {
                        return Err(e);
                    }
                    if result.is_err() {
                        holds.push(Failure {
                            kind: "settlement",
                            index,
                            code: "source_held",
                        });
                        return self.report(p, &witness, Vec::new(), holds);
                    }
                }
                hook(Stage::Settled(index))?;
            }
        }
        let mut effects = Vec::new();
        let reader = Reader(if cancelling {
            None
        } else {
            self.metadata
                .as_deref_mut()
                .ok_or(Error::Authority)?
                .finished_reader()
                .ok()
        });
        for (index, action) in p.actions.iter().enumerate() {
            let Some(local) = self
                .places
                .iter_mut()
                .find(|l| l.store.identity() == action.enrollment.store())
            else {
                holds.push(Failure {
                    kind: "removal",
                    index,
                    code: "unavailable",
                });
                continue;
            };
            if cancelling {
                match provider::admitted(local.ledger, &self.action_jobs[index], action) {
                    Ok(false) => continue,
                    Ok(true) => (),
                    Err(_) => {
                        holds.push(Failure {
                            kind: "cancellation",
                            index,
                            code: "admission_unknown",
                        });
                        continue;
                    }
                }
            }
            let mut execution = provider::Execution {
                store: local.store,
                ledger: local.ledger,
                job: &self.action_jobs[index],
                roster: self.roster,
                namespaces: self.namespaces,
                nodes: &reader,
                clock: self.clock,
                admitted_context: &action.request().context,
            };
            let result = if cancelling {
                execution.cancel(action, &action.authorize()?)
            } else {
                execution.apply_guarded(action, &action.authorize()?, &consent)
            };
            match result {
                Ok(receipt) => {
                    effects.push(Effect {
                        action: index,
                        receipt,
                    });
                    hook(Stage::Removed(index))?;
                }
                Err(e) => holds.push(Failure {
                    kind: "removal",
                    index,
                    code: provider_code(&e),
                }),
            }
        }
        if !cancelling && holds.is_empty() && effects.len() == p.actions.len() {
            witness.publish(2, self.witness.store, self.witness.ledger, self.witness.job)?;
            witness.close_unused(
                self.witness.store,
                self.witness.ledger,
                self.witness.job,
                false,
            )?;
            hook(Stage::Completed)?;
        }
        if cancelling {
            witness.close_unused(
                self.witness.store,
                self.witness.ledger,
                self.witness.job,
                true,
            )?;
        }
        self.report(p, &witness, effects, holds)
    }
}
