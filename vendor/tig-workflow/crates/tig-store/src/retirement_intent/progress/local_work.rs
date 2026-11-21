//! Acknowledged executable native work pinned by the same progress admission.
use super::*;
use crate::{ledger::JournalRef, local::removal::Plan};
pub(super) const SCHEMA: &str = "tig.local-retirement-work.design-1";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalWork {
    plan: Plan,
    job: JournalRef,
}
impl LocalWork {
    pub fn new(plan: Plan, job: JournalRef) -> Self {
        Self { plan, job }
    }
    pub fn plan(&self) -> &Plan {
        &self.plan
    }
    pub fn job(&self) -> &JournalRef {
        &self.job
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    plan: Plan,
    job: JournalRef,
}
fn validate(ctx: &ObservedIntent, frame: &Frame, work: &LocalWork) -> Result<()> {
    let s = work.plan.scope();
    if s.owner != *ctx.owner()
        || s.object != frame.object
        || s.attempt != frame.attempt
        || s.decision != ctx.decision().digest().map_err(Error::Decision)?
        || s.operation != ctx.decision().approval().operation
        || frame.action
            != (Action::Remove {
                store: s.store.clone(),
            })
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn restore(nodes: &impl NodeStore, root: &Object) -> Result<LocalWork> {
    let bytes = read(nodes, root)?;
    let record: Record =
        serde_json::from_value(identity::parse_json(&bytes).map_err(|_| Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
    if record.schema != SCHEMA || encode(&record)? != bytes {
        return Err(Error::Integrity);
    }
    Ok(LocalWork {
        plan: record.plan,
        job: record.job,
    })
}
pub(super) fn verify_frame(
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    frame: &Frame,
) -> Result<()> {
    if let Some(root) = &frame.execution {
        validate(ctx, frame, &restore(nodes, root)?)?;
    }
    Ok(())
}
/// Metadata/resource planning only in `prepare`; never perform the provider effect.
/// Existing work resumes without calling it again. New Begin and its work share
/// the original atomic guarded admission, including cancellation exclusion.
pub fn start(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    object: &Object,
    store: &Digest,
    prepare: impl FnOnce(u64) -> Result<LocalWork>,
) -> Result<Admission> {
    start_hook(j, nodes, ctx, object, store, prepare, |_| Ok(()))
}
#[allow(clippy::too_many_arguments)]
fn start_hook(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    object: &Object,
    store: &Digest,
    prepare: impl FnOnce(u64) -> Result<LocalWork>,
    before_adopt: impl FnOnce(&mut SourceJournal) -> Result<()>,
) -> Result<Admission> {
    start_bound(
        j,
        nodes,
        ctx,
        object,
        Action::Remove {
            store: store.clone(),
        },
        SCHEMA,
        |nodes, frame| {
            let work = prepare(frame.attempt)?;
            validate(ctx, frame, &work)?;
            let bytes = encode(&Record {
                schema: SCHEMA.into(),
                plan: work.plan,
                job: work.job,
            })?;
            if bytes.len() > 65_536 {
                return Err(Error::Budget);
            }
            put(nodes, &bytes)
        },
        before_adopt,
    )
}

/// Restore only adopted in-flight work, including after intent cancellation.
/// This is discovery for reconciliation, never renewed effect permission.
pub fn current(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    object: &Object,
) -> Result<Option<LocalWork>> {
    let loaded = load(j, nodes, ctx, object)?;
    if loaded.pending.is_some() {
        return Err(Error::Pending);
    }
    let Some((_, frame)) = loaded.frames.last() else {
        return Ok(None);
    };
    if frame.outcome.is_some() || !matches!(frame.action, Action::Remove { .. }) {
        return Ok(None);
    }
    frame
        .execution
        .as_ref()
        .map(|r| restore(nodes, r))
        .transpose()
}
/// Recover one exact adopted attempt, including historical completed work.
/// This does not admit work or renew effect permission; the original provider
/// still checks its disposition, current protection and any surviving custody.
pub fn at_attempt(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    object: &Object,
    attempt: u64,
) -> Result<Option<LocalWork>> {
    Ok(at_attempt_outcome(j, nodes, ctx, object, attempt)?.map(|(work, _)| work))
}
/// Exact saved attempt and its reported outcome. This grants no new admission;
/// callers must still reconcile the original provider disposition on completion.
pub fn at_attempt_outcome(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    object: &Object,
    attempt: u64,
) -> Result<Option<(LocalWork, Option<Outcome>)>> {
    if attempt == 0 {
        return Err(Error::Invalid);
    }
    let loaded = load(j, nodes, ctx, object)?;
    if loaded.pending.is_some() {
        return Err(Error::Pending);
    }
    let Some((_, frame)) = loaded
        .frames
        .iter()
        .rev()
        .find(|(_, f)| f.attempt == attempt)
    else {
        return Ok(None);
    };
    if !matches!(frame.action, Action::Remove { .. }) {
        return Ok(None);
    }
    let Some(root) = &frame.execution else {
        return Ok(None);
    };
    let work = restore(nodes, root)?;
    validate(ctx, frame, &work)?;
    Ok(Some((work, frame.outcome.clone())))
}

/// Finish only an already-prepared Removed outcome for this exact adopted work.
/// The original outcome publisher adopts the same acknowledged bytes; this does
/// not admit a Begin, create evidence or authorize a provider effect. Callers must
/// still reconcile the original provider disposition before reporting success.
pub fn resume_removed_outcome(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    object: &Object,
    attempt: u64,
    store: &Digest,
    preservation: Option<&Digest>,
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
            != (Action::Remove {
                store: store.clone(),
            })
        || !matches!(frame.outcome, Some(Outcome::Removed { .. }))
    {
        return Err(Error::Pending);
    }
    let root = frame.execution.as_ref().ok_or(Error::Occupied)?;
    let work = restore(nodes, root)?;
    validate(ctx, frame, &work)?;
    if work.plan().scope().preservation.as_ref() != preservation {
        return Err(Error::Invalid);
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

/// Called by the native provider before resource mutation and under its fence.
/// Legacy unbound admissions retain their original external-plan protocol.
pub(crate) fn check(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    plan: &Plan,
    job: &JournalRef,
) -> Result<()> {
    let loaded = load(j, nodes, ctx, &plan.scope().object)?;
    let frame = loaded
        .frames
        .iter()
        .map(|(_, f)| f)
        .find(|f| {
            f.attempt == plan.scope().attempt
                && f.action
                    == (Action::Remove {
                        store: plan.scope().store.clone(),
                    })
        })
        .ok_or(Error::Stale)?;
    if let Some(root) = &frame.execution {
        let work = restore(nodes, root)?;
        if work.plan != *plan || work.job != *job {
            return Err(Error::Occupied);
        }
    }
    Ok(())
}
#[cfg(test)]
#[path = "local_work_tests.rs"]
mod tests;
