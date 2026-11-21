//! Durable work bookkeeping. Journal admission is not physical executor authority.
use super::*;
use crate::source_claims::{Guard, MAX_PREDECESSORS, Version, progress_budget};
use std::collections::{BTreeMap, BTreeSet};
use tig_core::retirement::review::{Release, decision::Target};
pub mod handoff_work;
pub mod history_work;
pub mod local_work;
pub mod release_work;

/// Exact existing claim-work protocol; completion cannot switch protocols.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimProtocol {
    Retirement,
    Handoff,
}
impl ClaimProtocol {
    fn schema(self) -> &'static str {
        match self {
            Self::Retirement => release_work::SCHEMA,
            Self::Handoff => handoff_work::SCHEMA,
        }
    }
}
/// Complete only an already-prepared Released outcome for exact adopted work.
/// Reuses the original publisher; does not admit Begin or grant provider rights.
pub fn resume_released_outcome(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    object: &Object,
    attempt: u64,
    claim: &Digest,
    protocol: ClaimProtocol,
) -> Result<bool> {
    if attempt == 0 {
        return Err(Error::Invalid);
    }
    let loaded = load(j, nodes, ctx, object)?;
    let Some((_, frame, _)) = &loaded.pending else {
        return Ok(false);
    };
    if frame.attempt != attempt
        || frame.action
            != (Action::Release {
                claim: claim.clone(),
            })
        || !matches!(frame.outcome, Some(Outcome::Released { .. }))
    {
        return Err(Error::Pending);
    }
    if typed_execution(nodes, frame, protocol.schema())?.is_none() {
        return Err(Error::Occupied);
    }
    finish(
        j,
        nodes,
        ctx,
        object,
        attempt,
        frame.action.clone(),
        frame.outcome.clone().ok_or(Error::Invalid)?,
    )?;
    Ok(true)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Action {
    Release { claim: Digest },
    Remove { store: Digest },
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Allocation {
    pub before: Option<u64>,
    pub reclaimed: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Outcome {
    Released {
        evidence: Digest,
    },
    Removed {
        evidence: Digest,
        allocation: Allocation,
    },
    NoEffect {
        evidence: Digest,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    schema: String,
    journal: Digest,
    operation: Digest,
    decision: Digest,
    owner: Key,
    object: Object,
    parent: Option<Object>,
    attempt: u64,
    action: Action,
    outcome: Option<Outcome>,
    released: BTreeSet<Digest>,
    removed: BTreeMap<Digest, Allocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    execution: Option<Object>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    budget: Option<progress_budget::BudgetRef>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdmissionKind {
    New,
    Existing,
    Complete,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Admission {
    pub kind: AdmissionKind,
    pub attempt: u64,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub released: BTreeSet<Digest>,
    pub removed: BTreeMap<Digest, Allocation>,
    pub in_flight: Option<(u64, Action)>,
    pub pending: bool,
    pub cancelled: bool,
}
struct Loaded {
    frames: Vec<(Object, Frame)>,
    pending: Option<(Selection, Frame, Reference)>,
}
fn step(ctx: &ObservedIntent, object: &Object) -> Result<Key> {
    let id = Digest::blob(&encode(&(
        "tig.retirement-object-progress.design-1",
        ctx.decision.approval().operation.clone(),
        object,
    ))?);
    Key::parse(&format!("work-{}", id.as_str())).map_err(|_| Error::Invalid)
}
fn command(id: &Key, n: usize, adopt: bool) -> Key {
    Key::parse(&format!(
        "{}-{}-{n}",
        if adopt { "wa" } else { "wp" },
        id.as_str()
    ))
    .expect("bounded progress key")
}
fn context(frame: &Frame) -> Result<Digest> {
    Ok(Digest::blob(&encode(&(
        "tig.retirement-progress-context.design-1",
        frame,
    ))?))
}
fn retained(j: &mut SourceJournal, ctx: &ObservedIntent, active: bool) -> Result<Source> {
    if j.identity() != &ctx.journal {
        return Err(Error::Invalid);
    }
    let current = j
        .source(&ctx.source)
        .map_err(Error::Journal)?
        .ok_or(Error::Missing)?;
    if current.owner != ctx.state.owner {
        return Err(Error::Owner);
    }
    if active && current.cancelled {
        return Err(Error::Cancelled);
    }
    if current.revision != if current.cancelled { 3 } else { 2 }
        || current.current != ctx.state.current
        || current.pending.is_some()
        || !current.predecessors.is_empty()
    {
        return Err(Error::Stale);
    }
    Ok(current)
}
fn guard(ctx: &ObservedIntent) -> Guard {
    Guard {
        source: ctx.source.clone(),
        owner: ctx.state.owner.clone(),
        expected_revision: 2,
    }
}
fn allowed(ctx: &ObservedIntent, object: &Object, action: &Action) -> bool {
    match action {
        Action::Release { claim } => {
            ctx.decision
                .consent()
                .release_owned_claims
                .contains(&Release {
                    claim: claim.clone(),
                    object: object.clone(),
                })
        }
        Action::Remove { store } => ctx.decision.consent().remove_copies.contains(&Target {
            object: object.clone(),
            store: store.clone(),
        }),
    }
}
fn done(frame: &Frame, action: &Action) -> bool {
    match action {
        Action::Release { claim } => frame.released.contains(claim),
        Action::Remove { store } => frame.removed.contains_key(store),
    }
}
fn build(
    ctx: &ObservedIntent,
    object: &Object,
    previous: Option<&(Object, Frame)>,
    action: Action,
    outcome: Option<Outcome>,
    execution: Option<Object>,
    budget: Option<progress_budget::BudgetRef>,
) -> Result<Frame> {
    if !allowed(ctx, object, &action) {
        return Err(Error::Invalid);
    }
    let mut f = Frame {
        schema: "tig.retirement-progress.design-1".into(),
        journal: ctx.journal.clone(),
        operation: ctx.decision.approval().operation.clone(),
        decision: ctx.decision.digest().map_err(Error::Decision)?,
        owner: ctx.state.owner.clone(),
        object: object.clone(),
        parent: previous.map(|p| p.0.clone()),
        attempt: 1,
        action: action.clone(),
        outcome: outcome.clone(),
        released: BTreeSet::new(),
        removed: BTreeMap::new(),
        execution,
        budget,
    };
    if let Some((_, old)) = previous {
        f.released = old.released.clone();
        f.removed = old.removed.clone();
        if outcome.is_some() {
            if old.outcome.is_some() || old.action != action {
                return Err(Error::Invalid);
            }
            f.attempt = old.attempt;
            if f.execution != old.execution || f.budget != old.budget {
                return Err(Error::Invalid);
            }
        } else {
            if old.outcome.is_none() {
                return Err(Error::Pending);
            }
            f.attempt = old.attempt.checked_add(1).ok_or(Error::Budget)?;
        }
    } else if outcome.is_some() {
        return Err(Error::Invalid);
    }
    if let Some(b) = &f.budget
        && (b.journal() != &ctx.journal
            || b.owner() != ctx.owner()
            || b.id() != &budget_id(ctx, object, &action, f.attempt)?)
    {
        return Err(Error::Invalid);
    }
    if f.execution
        .as_ref()
        .is_some_and(|o| o.bytes == 0 || o.bytes > 65_536)
    {
        return Err(Error::Invalid);
    }
    if previous.is_some_and(|(_, old)| done(old, &action)) {
        return Err(Error::Occupied);
    }
    if matches!(action, Action::Remove { .. })
        && ctx
            .decision
            .consent()
            .release_owned_claims
            .iter()
            .any(|r| r.object == *object && !f.released.contains(&r.claim))
    {
        return Err(Error::Pending);
    }
    if let Some(outcome) = outcome {
        match (&action, outcome) {
            (Action::Release { claim }, Outcome::Released { .. }) => {
                f.released.insert(claim.clone());
            }
            (Action::Remove { store }, Outcome::Removed { allocation, .. }) => {
                if allocation
                    .reclaimed
                    .is_some_and(|n| allocation.before.is_none_or(|before| n > before))
                {
                    return Err(Error::Invalid);
                }
                f.removed.insert(store.clone(), allocation);
            }
            (_, Outcome::NoEffect { .. }) => (),
            _ => return Err(Error::Invalid),
        }
    }
    encode(&f)?;
    Ok(f)
}
fn read_frame(nodes: &impl NodeStore, selection: &Selection) -> Result<Frame> {
    let bytes = read(nodes, &selection.root)?;
    let f: Frame =
        serde_json::from_value(identity::parse_json(&bytes).map_err(|_| Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
    if encode(&f)? != bytes || context(&f)? != selection.context {
        return Err(Error::Integrity);
    }
    Ok(f)
}
fn budget_id(ctx: &ObservedIntent, object: &Object, action: &Action, attempt: u64) -> Result<Key> {
    let d = Digest::blob(&encode(&(
        "tig.progress-headroom.design-1",
        &ctx.journal,
        ctx.decision().digest().map_err(Error::Decision)?,
        object,
        action,
        attempt,
    ))?);
    Key::parse(&format!("pb-{}", d.as_str())).map_err(|_| Error::Invalid)
}
fn reserve_frame(
    j: &mut SourceJournal,
    ctx: &ObservedIntent,
    object: &Object,
    frame: &mut Frame,
) -> Result<()> {
    frame.budget = Some(
        progress_budget::reserve(
            j,
            budget_id(ctx, object, &frame.action, frame.attempt)?,
            ctx.owner().clone(),
            step(ctx, object)?,
            (frame.attempt - 1).checked_mul(4).ok_or(Error::Budget)?,
            vec![guard(ctx)],
        )
        .map_err(Error::Journal)?,
    );
    Ok(())
}
fn wrap(command: Command, budget: Option<&progress_budget::BudgetRef>) -> Command {
    budget.map_or_else(|| command.clone(), |b| b.wrap(command.clone()))
}
fn prepare_command(
    ctx: &ObservedIntent,
    id: &Key,
    n: usize,
    selection: Selection,
    budget: Option<&progress_budget::BudgetRef>,
) -> Command {
    wrap(
        Command::Prepare {
            source: id.clone(),
            owner: ctx.state.owner.clone(),
            expected_revision: 2 * (n as u64 - 1),
            selection,
        },
        budget,
    )
}
fn adopt_command(
    ctx: &ObservedIntent,
    id: &Key,
    n: usize,
    reference: Reference,
    begin: bool,
    budget: Option<&progress_budget::BudgetRef>,
) -> Command {
    wrap(
        if begin {
            Command::AdoptGuarded {
                source: id.clone(),
                owner: ctx.state.owner.clone(),
                expected_revision: 2 * n as u64 - 1,
                prepared: reference,
                guards: vec![guard(ctx)],
            }
        } else {
            Command::Adopt {
                source: id.clone(),
                owner: ctx.state.owner.clone(),
                expected_revision: 2 * n as u64 - 1,
                prepared: reference,
            }
        },
        budget,
    )
}
fn load(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    object: &Object,
) -> Result<Loaded> {
    retained(j, ctx, false)?;
    if !ctx
        .decision
        .consent()
        .remove_copies
        .iter()
        .any(|t| t.object == *object)
    {
        return Err(Error::Invalid);
    }
    let id = step(ctx, object)?;
    let source = j.source(&id).map_err(Error::Journal)?;
    let mut loaded = Loaded {
        frames: Vec::new(),
        pending: None,
    };
    if let Some(s) = &source {
        if s.owner != ctx.state.owner || s.cancelled {
            return Err(Error::Invalid);
        }
        let versions = s
            .predecessors
            .iter()
            .chain(s.current.iter())
            .collect::<Vec<&Version>>();
        if versions.len() > MAX_PREDECESSORS + 1
            || s.revision != versions.len() as u64 * 2 + u64::from(s.pending.is_some())
        {
            return Err(Error::Stale);
        }
        for (index, version) in versions.into_iter().enumerate() {
            let n = index + 1;
            if version.revision != n as u64 * 2 {
                return Err(Error::Invalid);
            }
            let frame = read_frame(nodes, &version.selection)?;
            let expected = build(
                ctx,
                object,
                loaded.frames.last(),
                frame.action.clone(),
                frame.outcome.clone(),
                frame.execution.clone(),
                frame.budget.clone(),
            )?;
            if frame != expected {
                return Err(Error::Integrity);
            }
            verify_execution(nodes, ctx, &frame)?;
            let prepared = j
                .replay(
                    command(&id, n, false),
                    prepare_command(
                        ctx,
                        &id,
                        n,
                        version.selection.clone(),
                        frame.budget.as_ref(),
                    ),
                )
                .map_err(Error::Journal)?
                .ok_or(Error::Invalid)?;
            let adopted = j
                .replay(
                    command(&id, n, true),
                    adopt_command(
                        ctx,
                        &id,
                        n,
                        prepared.reference,
                        frame.outcome.is_none(),
                        frame.budget.as_ref(),
                    ),
                )
                .map_err(Error::Journal)?
                .ok_or(Error::Invalid)?;
            if adopted.source_revision != version.revision {
                return Err(Error::Invalid);
            }
            loaded.frames.push((version.selection.root.clone(), frame));
        }
        if let Some(pending) = &s.pending {
            let n = loaded.frames.len() + 1;
            let frame = read_frame(nodes, &pending.selection)?;
            if frame
                != build(
                    ctx,
                    object,
                    loaded.frames.last(),
                    frame.action.clone(),
                    frame.outcome.clone(),
                    frame.execution.clone(),
                    frame.budget.clone(),
                )?
            {
                return Err(Error::Integrity);
            }
            verify_execution(nodes, ctx, &frame)?;
            let prepared = j
                .replay(
                    command(&id, n, false),
                    prepare_command(
                        ctx,
                        &id,
                        n,
                        pending.selection.clone(),
                        frame.budget.as_ref(),
                    ),
                )
                .map_err(Error::Journal)?
                .ok_or(Error::Invalid)?;
            if prepared.reference != pending.reference || prepared.source_revision != s.revision {
                return Err(Error::Invalid);
            }
            loaded.pending = Some((pending.selection.clone(), frame, pending.reference.clone()));
        }
    }
    if j.source(&id).map_err(Error::Journal)? != source {
        return Err(Error::Stale);
    }
    retained(j, ctx, false)?;
    Ok(loaded)
}
fn publish_frame(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    object: &Object,
    loaded: &Loaded,
    frame: Frame,
) -> Result<Receipt> {
    publish_frame_hook(j, nodes, ctx, object, loaded, frame, |_| Ok(()))
}
#[allow(clippy::too_many_arguments)]
fn publish_frame_hook(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    object: &Object,
    loaded: &Loaded,
    frame: Frame,
    before_adopt: impl FnOnce(&mut SourceJournal) -> Result<()>,
) -> Result<Receipt> {
    let id = step(ctx, object)?;
    let n = loaded.frames.len() + 1;
    let (selection, reference) = if let Some((selection, pending, reference)) = &loaded.pending {
        if *pending != frame {
            return Err(Error::Occupied);
        }
        nodes.finish().map_err(Error::Provider)?;
        if read_frame(nodes, selection)? != frame {
            return Err(Error::Integrity);
        }
        (selection.clone(), reference.clone())
    } else {
        let selection = Selection {
            root: put(nodes, &encode(&frame)?)?,
            context: context(&frame)?,
        };
        nodes.finish().map_err(Error::Provider)?;
        if read_frame(nodes, &selection)? != frame {
            return Err(Error::Integrity);
        }
        verify_execution(nodes, ctx, &frame)?;
        retained(j, ctx, frame.outcome.is_none())?;
        let prepared = j
            .apply(
                command(&id, n, false),
                prepare_command(ctx, &id, n, selection.clone(), frame.budget.as_ref()),
            )
            .map_err(Error::Journal)?;
        (selection, prepared.reference)
    };
    let _ = selection;
    verify_execution(nodes, ctx, &frame)?;
    before_adopt(j)?;
    retained(j, ctx, frame.outcome.is_none())?;
    j.apply(
        command(&id, n, true),
        adopt_command(
            ctx,
            &id,
            n,
            reference,
            frame.outcome.is_none(),
            frame.budget.as_ref(),
        ),
    )
    .map_err(Error::Journal)
}
/// A new journal admission is not native protection/custody or permission to remove.
pub fn start(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    object: &Object,
    action: Action,
) -> Result<Admission> {
    if ctx.execution_profile().is_some() {
        return Err(Error::Invalid); // Profiled intents require typed bound work.
    }
    retained(j, ctx, true)?;
    if !allowed(ctx, object, &action) {
        return Err(Error::Invalid);
    }
    let loaded = load(j, nodes, ctx, object)?;
    if let Some((_, old)) = loaded.frames.last() {
        if done(old, &action) {
            return Ok(Admission {
                kind: AdmissionKind::Complete,
                attempt: old.attempt,
            });
        }
        if old.outcome.is_none() {
            return if old.action == action {
                Ok(Admission {
                    kind: AdmissionKind::Existing,
                    attempt: old.attempt,
                })
            } else {
                Err(Error::Pending)
            };
        }
    }
    if loaded.frames.len() + 2 > MAX_PREDECESSORS + 1 {
        return Err(Error::Budget);
    }
    let frame = if let Some((_, f, _)) = &loaded.pending {
        if f.action != action || f.outcome.is_some() || f.execution.is_some() {
            return Err(Error::Occupied);
        }
        f.clone()
    } else {
        let mut f = build(ctx, object, loaded.frames.last(), action, None, None, None)?;
        reserve_frame(j, ctx, object, &mut f)?;
        f
    };
    let attempt = frame.attempt;
    let receipt = publish_frame(j, nodes, ctx, object, &loaded, frame)?;
    Ok(Admission {
        kind: if receipt.replayed {
            AdmissionKind::Existing
        } else {
            AdmissionKind::New
        },
        attempt,
    })
}
/// Records admitted provider evidence, including an outcome after parent cancellation.
pub fn finish(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    object: &Object,
    attempt: u64,
    action: Action,
    outcome: Outcome,
) -> Result<Origin> {
    let loaded = load(j, nodes, ctx, object)?;
    for (_, old) in &loaded.frames {
        if old.attempt == attempt && old.outcome.is_some() {
            return if old.action == action && old.outcome.as_ref() == Some(&outcome) {
                if let Some(b) = &old.budget {
                    progress_budget::release(j, b).map_err(Error::Journal)?;
                }
                Ok(Origin::Historical)
            } else {
                Err(Error::Occupied)
            };
        }
    }
    let previous = loaded.frames.last().ok_or(Error::Missing)?;
    if previous.1.attempt != attempt || previous.1.action != action || previous.1.outcome.is_some()
    {
        return Err(Error::Stale);
    }
    let frame = build(
        ctx,
        object,
        Some(previous),
        action,
        Some(outcome),
        previous.1.execution.clone(),
        previous.1.budget.clone(),
    )?;
    let budget = frame.budget.clone();
    let receipt = publish_frame(j, nodes, ctx, object, &loaded, frame)?;
    if let Some(b) = &budget {
        progress_budget::release(j, b).map_err(Error::Journal)?;
    }
    Ok(if receipt.replayed {
        Origin::Historical
    } else {
        Origin::New
    })
}
pub fn inspect(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    object: &Object,
) -> Result<Snapshot> {
    let loaded = load(j, nodes, ctx, object)?;
    let current = retained(j, ctx, false)?;
    Ok(Snapshot {
        released: loaded
            .frames
            .last()
            .map_or_else(BTreeSet::new, |(_, f)| f.released.clone()),
        removed: loaded
            .frames
            .last()
            .map_or_else(BTreeMap::new, |(_, f)| f.removed.clone()),
        in_flight: loaded
            .frames
            .last()
            .and_then(|(_, f)| f.outcome.is_none().then(|| (f.attempt, f.action.clone()))),
        pending: loaded.pending.is_some(),
        cancelled: current.cancelled,
    })
}

/// Shared bounded admission protocol for typed native execution records.
#[allow(clippy::too_many_arguments)]
fn start_bound(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    object: &Object,
    action: Action,
    schema: &str,
    prepare: impl FnOnce(&mut MetadataStore, &Frame) -> Result<Object>,
    before_adopt: impl FnOnce(&mut SourceJournal) -> Result<()>,
) -> Result<Admission> {
    retained(j, ctx, true)?;
    if !allowed(ctx, object, &action) {
        return Err(Error::Invalid);
    }
    profile_protocol(ctx, &action, schema)?;
    let loaded = load(j, nodes, ctx, object)?;
    if let Some((_, old)) = loaded.frames.last() {
        if done(old, &action) {
            return Ok(Admission {
                kind: AdmissionKind::Complete,
                attempt: old.attempt,
            });
        }
        if old.outcome.is_none() {
            if old.action != action {
                return Err(Error::Pending);
            }
            let root = old.execution.as_ref().ok_or(Error::Occupied)?;
            if execution_schema(&read(nodes, root)?)? != schema {
                return Err(Error::Occupied);
            }
            return Ok(Admission {
                kind: AdmissionKind::Existing,
                attempt: old.attempt,
            });
        }
    }
    if loaded.frames.len() + 2 > MAX_PREDECESSORS + 1 {
        return Err(Error::Budget);
    }
    let _policy = super::policy::require_effect_authority(ctx, nodes)?;
    let frame = if let Some((_, frame, _)) = &loaded.pending {
        if frame.action != action || frame.outcome.is_some() || frame.execution.is_none() {
            return Err(Error::Occupied);
        }
        let root = frame.execution.as_ref().ok_or(Error::Occupied)?;
        if execution_schema(&read(nodes, root)?)? != schema {
            return Err(Error::Occupied);
        }
        frame.clone()
    } else {
        let mut frame = build(ctx, object, loaded.frames.last(), action, None, None, None)?;
        frame.execution = Some(prepare(nodes, &frame)?);
        reserve_frame(j, ctx, object, &mut frame)?;
        frame
    };
    let attempt = frame.attempt;
    let receipt = publish_frame_hook(j, nodes, ctx, object, &loaded, frame, before_adopt)?;
    Ok(Admission {
        kind: if receipt.replayed {
            AdmissionKind::Existing
        } else {
            AdmissionKind::New
        },
        attempt,
    })
}

fn profile_protocol(ctx: &ObservedIntent, action: &Action, schema: &str) -> Result<()> {
    let Some(context) = ctx.execution_profile() else {
        return Ok(());
    };
    use super::execution_profile::Role;
    match action {
        Action::Remove { store } if schema == local_work::SCHEMA => {
            context.binding(Role::Removal, store)?;
        }
        Action::Release { claim } => match schema {
            release_work::SCHEMA => {
                context.binding(Role::RetirementRelease, claim)?;
            }
            handoff_work::SCHEMA => {
                context.binding(Role::HandoffRelease, claim)?;
            }
            history_work::SCHEMA | history_work::block::SCHEMA
                if context.history_releases().contains(claim) => {}
            _ => return Err(Error::Invalid),
        },
        _ => return Err(Error::Invalid),
    }
    Ok(())
}
fn execution_schema(bytes: &[u8]) -> Result<String> {
    let value = identity::parse_json(bytes).map_err(|_| Error::Invalid)?;
    value
        .get("schema")
        .and_then(|s| s.as_str())
        .map(str::to_owned)
        .ok_or(Error::Invalid)
}
fn verify_execution(nodes: &impl NodeStore, ctx: &ObservedIntent, frame: &Frame) -> Result<()> {
    if ctx.execution_profile().is_some() && frame.execution.is_none() {
        return Err(Error::Invalid);
    }
    if matches!(frame.action, Action::Remove { .. }) {
        profile_protocol(ctx, &frame.action, local_work::SCHEMA)?;
        return local_work::verify_frame(nodes, ctx, frame);
    }
    if let Some(root) = &frame.execution {
        let bytes = read(nodes, root)?;
        let schema = execution_schema(&bytes)?;
        profile_protocol(ctx, &frame.action, &schema)?;
        match schema.as_str() {
            release_work::SCHEMA => release_work::verify_bytes(ctx, frame, &bytes)?,
            handoff_work::SCHEMA => handoff_work::verify_bytes(ctx, frame, &bytes)?,
            history_work::SCHEMA => history_work::verify_bytes(ctx, frame, &bytes)?,
            history_work::block::SCHEMA => {
                history_work::block::verify_bytes(nodes, ctx, frame, &bytes)?
            }
            _ => return Err(Error::Invalid),
        }
    }
    Ok(())
}
fn typed_execution(nodes: &impl NodeStore, frame: &Frame, schema: &str) -> Result<Option<Vec<u8>>> {
    let Some(root) = &frame.execution else {
        return Ok(None);
    };
    let bytes = read(nodes, root)?;
    Ok((execution_schema(&bytes)? == schema).then_some(bytes))
}
