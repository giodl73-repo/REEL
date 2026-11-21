//! Provider-neutral exclusion over the existing journal. This grants no owner,
//! custody or process-termination authority to an adapter.
use super::*;
use crate::source_claims::{
    Guard, Source,
    progress_budget::{self, BudgetRef},
};

const SCHEMA: &str = "tig.protection-effect.design-1";
mod archive;
/// Reserve scarce source slots as well as command bytes/events, in the same
/// existing transaction. Only the new explicit effect profile opts into this.
pub(in crate::source_claims) fn admit_capacity(
    state: &crate::source_claims::State,
    command: &Command,
) -> crate::source_claims::Result<()> {
    let reserved = state
        .progress_budgets
        .values()
        .filter(|r| !r.closed && !state.sources.contains_key(r.source.as_str()))
        .map(|r| r.source.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let source = match crate::source_claims::progress_budget::base(command)? {
        Command::ReserveProgress { source, .. } | Command::Prepare { source, .. } => source,
        _ => return Ok(()),
    };
    if !state.sources.contains_key(source.as_str())
        && !reserved.contains(source.as_str())
        && state.sources.len() + reserved.len() >= crate::source_claims::MAX_SOURCES
    {
        return Err(crate::source_claims::Error::Budget);
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub operation: Key,
    pub object: Object,
    pub store: Digest,
    /// Exact owner-admitted decision/provider binding, not authentication.
    pub context: Digest,
    pub expected: Selection,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Outcome {
    Removed { evidence: Digest },
    NoEffect { evidence: Digest },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    index: Digest,
    request: Request,
    first: u64,
    budget: BudgetRef,
    outcome: Option<Outcome>,
}

pub enum Admission {
    New(HeldEffect),
    Historical(Outcome),
    /// Persisted unresolved work, never fresh effect permission.
    Uncertain(Request),
}
pub struct HeldEffect {
    record: Record,
    _coverage: Fence,
    _object: Fence,
}
/// A cooperating positive/read admission, not retained custody or job coverage.
pub struct HeldObject {
    _coverage: Fence,
    _object: Fence,
}
/// Admitted provider proof that every worker for this exact effect is stopped.
/// Implementations must inspect/terminate the provider's process containment;
/// a timeout, expired lease, PID-only check or parsed JSON is not such a proof.
pub trait Terminated {
    fn effect(&self) -> &Digest;
}
pub fn fingerprint(index: &Digest, request: &Request) -> Result<Digest> {
    Ok(Digest::blob(&encode(&(SCHEMA, index, request))?))
}
impl HeldEffect {
    pub fn request(&self) -> &Request {
        &self.record.request
    }
    /// Caller must establish that its contained worker can no longer act before
    /// supplying an observation. Timeout/unknown outcome must drop this lease
    /// WITHOUT completing it; the persisted pending fence remains effective.
    pub fn finish(
        self,
        index: &mut Index,
        nodes: &mut metadata::MetadataStore,
        outcome: Outcome,
    ) -> Result<()> {
        if index.identity() != &self.record.index {
            return Err(Error::Invalid);
        }
        let source = index
            .journal
            .source(&slot(&self.record.request.object)?)?
            .ok_or(Error::Unknown)?;
        let selected = source.current.as_ref().ok_or(Error::Pending)?;
        let old = read(index, nodes, &selected.selection)?;
        if source.pending.is_some() || old != self.record || old.outcome.is_some() {
            return Err(Error::Conflict);
        }
        let mut record = self.record.clone();
        record.outcome = Some(outcome);
        publish(index, nodes, &record, source.revision, false)?;
        progress_budget::release_mode(&mut index.journal, &record.budget, true)?;
        Ok(())
    }
}

fn marker(terminal: bool) -> Digest {
    Digest::blob(if terminal {
        b"tig.protection-effect.terminal-1"
    } else {
        b"tig.protection-effect.pending-1"
    })
}
fn slot(object: &Object) -> Result<Key> {
    Key::parse(&format!("effect-{}", object.sha256.as_str())).map_err(|_| Error::Invalid)
}
fn command(record: &Record, phase: &str) -> Result<Key> {
    Key::parse(&format!(
        "effect-{}",
        Digest::blob(&encode(&(
            SCHEMA,
            &record.index,
            &record.request,
            record.first,
            phase
        ))?)
        .as_str()
    ))
    .map_err(|_| Error::Invalid)
}
fn budget_id(index: &Digest, request: &Request) -> Result<Key> {
    Key::parse(&format!(
        "effect-budget-{}",
        fingerprint(index, request)?.as_str()
    ))
    .map_err(|_| Error::Invalid)
}
fn no_begin(index: &mut Index, request: &Request, settle: bool) -> Result<bool> {
    let id = budget_id(index.identity(), request)?;
    if let Some((reference, closed, spent)) = progress_budget::reservation(&mut index.journal, &id)?
    {
        if reference.owner() != &index.enrollment.owner {
            return Err(Error::Invalid);
        }
        if spent == 0 {
            if !closed && settle {
                progress_budget::release_mode(&mut index.journal, &reference, true)?;
            }
            return Ok(closed || settle);
        }
    }
    Ok(false)
}
fn read(index: &Index, nodes: &impl NodeStore, selection: &Selection) -> Result<Record> {
    if selection.root.bytes > 4096 {
        return Err(Error::Budget);
    }
    let record: Record = load(nodes, &selection.root)?;
    if record.schema != SCHEMA
        || record.index != *index.identity()
        || selection.context != marker(record.outcome.is_some())
        || record.budget.journal() != index.identity()
        || record.budget.owner() != &index.enrollment.owner
    {
        return Err(Error::Invalid);
    }
    Ok(record)
}
fn publish(
    index: &mut Index,
    nodes: &mut metadata::MetadataStore,
    record: &Record,
    revision: u64,
    begin: bool,
) -> Result<()> {
    if encode(record)?.len() > 4096 {
        return Err(Error::Budget);
    }
    let root = put(nodes, record)?;
    nodes.finish()?;
    read(
        index,
        nodes,
        &Selection {
            root: root.clone(),
            context: marker(!begin),
        },
    )?;
    let phase = if begin { "begin" } else { "finish" };
    let prepare = command(record, &format!("{phase}-prepare"))?;
    let receipt = index.command(
        prepare,
        record.budget.wrap(Command::Prepare {
            source: slot(&record.request.object)?,
            owner: index.enrollment.owner.clone(),
            expected_revision: revision,
            selection: Selection {
                root,
                context: marker(!begin),
            },
        }),
    )?;
    let adopt = if begin {
        let authority = index.source()?.ok_or(Error::Unknown)?;
        Command::AdoptGuarded {
            source: slot(&record.request.object)?,
            owner: index.enrollment.owner.clone(),
            expected_revision: revision + 1,
            prepared: receipt.reference,
            guards: vec![Guard {
                source: key("authority"),
                owner: index.enrollment.owner.clone(),
                expected_revision: authority.revision,
            }],
        }
    } else {
        Command::Adopt {
            source: slot(&record.request.object)?,
            owner: index.enrollment.owner.clone(),
            expected_revision: revision + 1,
            prepared: receipt.reference,
        }
    };
    index.command(
        command(record, &format!("{phase}-adopt"))?,
        record.budget.wrap(adopt),
    )?;
    Ok(())
}
fn historical(
    index: &mut Index,
    nodes: &impl NodeStore,
    state: &Source,
    request: &Request,
) -> Result<Option<Outcome>> {
    if state.owner != index.enrollment.owner || state.cancelled {
        return Err(Error::Invalid);
    }
    if state
        .predecessors
        .iter()
        .filter(|v| archive::is_archive(&v.selection))
        .count()
        > 1
    {
        return Err(Error::Invalid);
    }
    let mut borrowed = archive::ReadOnly(nodes);
    let mut work = Work::bounded(&mut borrowed, archive::limits());
    for version in state.predecessors.iter().chain(state.current.iter()) {
        let record = if archive::is_archive(&version.selection) {
            let Some(record) = archive::lookup(index, &mut work, &version.selection, request)?
            else {
                continue;
            };
            record
        } else {
            read(index, &work, &version.selection)?
        };
        if record.request.operation == request.operation && record.request.store == request.store {
            if record.request != *request {
                return Err(Error::Conflict);
            }
            if let Some(outcome) = record.outcome {
                // A committed outcome may have lost only its settlement reply.
                // Reclaim its original reserved control room, never new authority.
                progress_budget::release_mode(&mut index.journal, &record.budget, true)?;
                return Ok(Some(outcome));
            }
        }
    }
    Ok(None)
}

impl Index {
    /// Metadata-only consolidation of completed effects. Retains old outcomes;
    /// never grants execution permission, clears pending work or deletes bytes.
    pub fn archive_effect_history(
        &mut self,
        object: &Object,
        nodes: &mut metadata::MetadataStore,
    ) -> Result<usize> {
        self.archive_effect_history_with_hook(object, nodes, || Ok(()))
    }
    pub(super) fn archive_effect_history_with_hook(
        &mut self,
        object: &Object,
        nodes: &mut metadata::MetadataStore,
        selected: impl FnMut() -> Result<()>,
    ) -> Result<usize> {
        if !self.supports_effect_fences() {
            return Err(Error::Invalid);
        }
        let _coverage = self.lock_mode("coverage", false)?;
        let _object = self.lock_name(object.sha256.as_str())?;
        let authority = self.source()?.ok_or(Error::Unknown)?;
        if authority.pending.is_some() || authority.cancelled {
            return Err(Error::Pending);
        }
        let Some(state) = self.journal.source(&slot(object)?)? else {
            return Ok(0);
        };
        archive::compact_hook(self, nodes, object, &state, authority.revision, selected)
    }
    /// Existing bounded work only. Never creates a lease or changes protection.
    pub fn inspect_effect(
        &mut self,
        request: &Request,
        nodes: &impl NodeStore,
    ) -> Result<Option<Admission>> {
        if !self.supports_effect_fences() {
            return Err(Error::Invalid);
        }
        if no_begin(self, request, false)? {
            return Ok(Some(Admission::Historical(Outcome::NoEffect {
                evidence: fingerprint(self.identity(), request)?,
            })));
        }
        let Some(state) = self.journal.source(&slot(&request.object)?)? else {
            return Ok(None);
        };
        if let Some(outcome) = historical(self, nodes, &state, request)? {
            return Ok(Some(Admission::Historical(outcome)));
        }
        if state.pending.is_some()
            || state
                .current
                .as_ref()
                .is_none_or(|v| v.selection.context != marker(true))
        {
            let selection = state
                .pending
                .as_ref()
                .map(|p| &p.selection)
                .or_else(|| state.current.as_ref().map(|v| &v.selection))
                .ok_or(Error::Pending)?;
            return Ok(Some(Admission::Uncertain(
                read(self, nodes, selection)?.request,
            )));
        }
        Ok(None)
    }
    /// Reconcile old work without returning an execution lease or issuing an
    /// effect. Observation runs under the original per-object exclusion only
    /// after the adapter has proved all old workers stopped.
    pub fn reconcile_effect(
        &mut self,
        request: &Request,
        stopped: &impl Terminated,
        nodes: &mut metadata::MetadataStore,
        observe: impl FnOnce() -> Result<Outcome>,
    ) -> Result<Outcome> {
        if !self.supports_effect_fences()
            || stopped.effect() != &fingerprint(self.identity(), request)?
        {
            return Err(Error::Invalid);
        }
        let _coverage = self.lock_mode("coverage", false)?;
        let _object = self.lock_name(request.object.sha256.as_str())?;
        let Some(state) = self.journal.source(&slot(&request.object)?)? else {
            no_begin(self, request, true)?;
            return Ok(Outcome::NoEffect {
                evidence: fingerprint(self.identity(), request)?,
            });
        };
        if let Some(outcome) = historical(self, nodes, &state, request)? {
            return Ok(outcome);
        }
        let selected = state
            .pending
            .as_ref()
            .map(|p| &p.selection)
            .or_else(|| state.current.as_ref().map(|v| &v.selection))
            .ok_or(Error::Pending)?;
        let mut record = read(self, nodes, selected)?;
        if record.request != *request {
            if record.outcome.is_some() && state.pending.is_none() {
                no_begin(self, request, true)?;
                return Ok(Outcome::NoEffect {
                    evidence: fingerprint(self.identity(), request)?,
                });
            }
            return Err(Error::Conflict);
        }
        if let Some(pending) = state.pending {
            let terminal = record.outcome.is_some();
            let adopt = if terminal {
                Command::Adopt {
                    source: slot(&request.object)?,
                    owner: self.enrollment.owner.clone(),
                    expected_revision: state.revision,
                    prepared: pending.reference,
                }
            } else {
                let authority = self.source()?.ok_or(Error::Unknown)?;
                Command::AdoptGuarded {
                    source: slot(&request.object)?,
                    owner: self.enrollment.owner.clone(),
                    expected_revision: state.revision,
                    prepared: pending.reference,
                    guards: vec![Guard {
                        source: key("authority"),
                        owner: self.enrollment.owner.clone(),
                        expected_revision: authority.revision,
                    }],
                }
            };
            self.command(
                command(
                    &record,
                    if terminal {
                        "finish-adopt"
                    } else {
                        "begin-adopt"
                    },
                )?,
                record.budget.wrap(adopt),
            )?;
            if let Some(outcome) = record.outcome {
                progress_budget::release_mode(&mut self.journal, &record.budget, true)?;
                return Ok(outcome);
            }
        }
        let outcome = observe()?;
        record.outcome = Some(outcome.clone());
        let revision = self
            .journal
            .source(&slot(&request.object)?)?
            .ok_or(Error::Unknown)?
            .revision;
        publish(self, nodes, &record, revision, false)?;
        progress_budget::release_mode(&mut self.journal, &record.budget, true)?;
        Ok(outcome)
    }
    pub fn hold_object(&mut self, object: &Object, store: &Digest) -> Result<HeldObject> {
        if !self.supports_effect_fences() || !self.enrollment.stores.contains(store) {
            return Err(Error::Invalid);
        }
        let coverage = self.lock_mode("coverage", false)?;
        let fence = self.lock_name(object.sha256.as_str())?;
        self.require_clear_effects(std::slice::from_ref(object))?;
        Ok(HeldObject {
            _coverage: coverage,
            _object: fence,
        })
    }
    pub fn supports_effect_fences(&self) -> bool {
        self.journal.effect_fences
    }
    /// Exact current authored-protection frame. It is a staged input, not an
    /// owner/hosted-current guarantee and not physical availability evidence.
    pub fn effect_frame(&mut self) -> Result<Selection> {
        if !self.supports_effect_fences() {
            return Err(Error::Invalid);
        }
        let source = self.source()?.ok_or(Error::Unknown)?;
        if source.pending.is_some() {
            return Err(Error::Pending);
        }
        source.current.map(|v| v.selection).ok_or(Error::Unknown)
    }
    /// Exact local protection generation, not authenticated hosted-current truth.
    pub fn policy_source(&self, actor: &Key) -> Result<Digest> {
        self.enrollment.actor(actor)?;
        Ok(Digest::blob(&encode(&(
            "tig.effect-policy-source.design-1",
            self.identity(),
            actor,
        ))?))
    }
    pub fn policy_frame(
        &mut self,
        nodes: &impl NodeStore,
        actor: &Key,
    ) -> Result<tig_core::retirement::review::routine_policy::Frame> {
        if !self.supports_effect_fences() {
            return Err(Error::Invalid);
        }
        let source = self.source()?.ok_or(Error::Unknown)?;
        if source.pending.is_some() {
            return Err(Error::Pending);
        }
        let version = source.current.ok_or(Error::Unknown)?;
        let header = self.header(nodes, &version.selection.root)?;
        let selected = header.actors.get(actor.as_str()).ok_or(Error::Invalid)?;
        if !selected.ready {
            return Err(Error::Unknown);
        }
        Ok(tig_core::retirement::review::routine_policy::Frame {
            source: self.policy_source(actor)?,
            authority: self.identity().clone(),
            epoch: self.identity().clone(),
            generation: selected.revision.checked_add(1).ok_or(Error::Budget)?,
            selection: Digest::blob(&encode(&(
                "tig.effect-policy-frame.design-1",
                &selected.selection,
            ))?),
        })
    }
    pub(super) fn require_clear_effects(&mut self, objects: &[Object]) -> Result<()> {
        if !self.supports_effect_fences() {
            return Ok(());
        }
        for object in objects {
            if let Some(state) = self.journal.source(&slot(object)?)?
                && (state.owner != self.enrollment.owner
                    || state.cancelled
                    || state.pending.is_some()
                    || state
                        .current
                        .as_ref()
                        .is_none_or(|v| v.selection.context != marker(true)))
            {
                return Err(Error::Pending);
            }
        }
        Ok(())
    }
    pub(super) fn require_clear_effect_coverage(&mut self) -> Result<()> {
        if !self.supports_effect_fences() {
            return Ok(());
        }
        // Coverage invalidation is the one enrollment-wide change. This is a
        // bounded metadata control query, never a payload inventory or cache lock.
        let entries = self.journal.control_sources(
            &self.enrollment.owner,
            "effect-",
            crate::source_claims::MAX_SOURCES,
        )?;
        if entries.iter().any(|s| {
            s.cancelled
                || s.pending.is_some()
                || s.current
                    .as_ref()
                    .is_none_or(|v| v.selection.context != marker(true))
        }) {
            return Err(Error::Pending);
        }
        Ok(())
    }
    /// Exclusion/bookkeeping only. The caller separately admits owner consent,
    /// current source/policy, retained custody and the exact provider UUID.
    pub fn begin_effect(
        &mut self,
        request: &Request,
        nodes: &mut metadata::MetadataStore,
    ) -> Result<Admission> {
        if !self.supports_effect_fences() || !self.enrollment.stores.contains(&request.store) {
            return Err(Error::Invalid);
        }
        if let Some(existing) = self.inspect_effect(request, nodes)? {
            return Ok(existing);
        }
        let source_key = slot(&request.object)?;
        let mut prior = self.journal.source(&source_key)?;
        if let Some(state) = &prior {
            if let Some(outcome) = historical(self, nodes, state, request)? {
                return Ok(Admission::Historical(outcome));
            }
            if state.pending.is_some()
                || state
                    .current
                    .as_ref()
                    .is_none_or(|v| v.selection.context != marker(true))
            {
                let selection = state
                    .pending
                    .as_ref()
                    .map(|p| &p.selection)
                    .or_else(|| state.current.as_ref().map(|v| &v.selection))
                    .ok_or(Error::Pending)?;
                return Ok(Admission::Uncertain(read(self, nodes, selection)?.request));
            }
        }
        let coverage = self.lock_mode("coverage", false)?;
        let object = self.lock_name(request.object.sha256.as_str())?;
        // Recheck after the resource fence: another effect may have won admission.
        self.require_clear_effects(std::slice::from_ref(&request.object))?;
        prior = self.journal.source(&source_key)?;
        if let Some(state) = &prior
            && let Some(outcome) = historical(self, nodes, state, request)?
        {
            return Ok(Admission::Historical(outcome));
        }
        let authority = self.source()?.ok_or(Error::Unknown)?;
        if authority.pending.is_some()
            || authority.current.as_ref().map(|v| &v.selection) != Some(&request.expected)
        {
            return Err(Error::Stale);
        }
        if !self
            .inspect(nodes, &request.object, &request.store)?
            .protected_by
            .is_empty()
        {
            return Err(Error::Protected);
        }
        prior = self.journal.source(&source_key)?;
        // Begin and outcome each retain one predecessor. Consolidate known
        // terminal history before admitting backend work when that chain fills.
        if prior
            .as_ref()
            .is_some_and(|s| s.predecessors.len() + 2 > crate::source_claims::MAX_PREDECESSORS)
        {
            archive::compact(
                self,
                nodes,
                &request.object,
                prior.as_ref().ok_or(Error::Unknown)?,
                authority.revision,
            )?;
            prior = self.journal.source(&source_key)?;
        }
        let first = prior.as_ref().map_or(0, |s| s.revision);
        let budget_id = budget_id(self.identity(), request)?;
        let budget = progress_budget::reserve_mode(
            &mut self.journal,
            budget_id,
            self.enrollment.owner.clone(),
            source_key,
            first,
            vec![Guard {
                source: key("authority"),
                owner: self.enrollment.owner.clone(),
                expected_revision: authority.revision,
            }],
            true,
        )?;
        let record = Record {
            schema: SCHEMA.into(),
            index: self.identity().clone(),
            request: request.clone(),
            first,
            budget,
            outcome: None,
        };
        publish(self, nodes, &record, first, true)?;
        Ok(Admission::New(HeldEffect {
            record,
            _coverage: coverage,
            _object: object,
        }))
    }
}
