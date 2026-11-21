//! Native final claim-release plans pinned by guarded progress admission.
use super::*;
use crate::{ledger::JournalRef, retention::release::ReleasePlan};
pub(super) const SCHEMA: &str = "tig.native-claim-release-work.design-1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseWork {
    plan: ReleasePlan,
    job: JournalRef,
}
impl ReleaseWork {
    pub fn new(plan: ReleasePlan, job: JournalRef) -> Self {
        Self { plan, job }
    }
    pub fn plan(&self) -> &ReleasePlan {
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
    attempt: u64,
    plan: ReleasePlan,
    job: JournalRef,
}
fn validate(ctx: &ObservedIntent, frame: &Frame, record: &Record) -> Result<()> {
    let Action::Release { claim } = &frame.action else {
        return Err(Error::Invalid);
    };
    if record.schema != SCHEMA
        || record.attempt != frame.attempt
        || !record.plan.matches_work(ctx, &frame.object, claim)
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn restore(nodes: &impl NodeStore, root: &Object) -> Result<Record> {
    decode(&read(nodes, root)?)
}
fn decode(bytes: &[u8]) -> Result<Record> {
    let record: Record =
        serde_json::from_value(identity::parse_json(bytes).map_err(|_| Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
    if record.schema != SCHEMA || encode(&record)? != bytes {
        return Err(Error::Integrity);
    }
    Ok(record)
}
pub(super) fn verify_bytes(ctx: &ObservedIntent, frame: &Frame, bytes: &[u8]) -> Result<()> {
    validate(ctx, frame, &decode(bytes)?)
}
/// Plan and reserve only inside the factory. Existing or pending work never
/// invokes it again. No provider effect may run before guarded admission.
pub fn start(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    object: &Object,
    claim: &Digest,
    prepare: impl FnOnce(u64) -> Result<ReleaseWork>,
) -> Result<Admission> {
    start_hook(j, nodes, ctx, object, claim, prepare, |_| Ok(()))
}
#[allow(clippy::too_many_arguments)]
fn start_hook(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    object: &Object,
    claim: &Digest,
    prepare: impl FnOnce(u64) -> Result<ReleaseWork>,
    before_adopt: impl FnOnce(&mut SourceJournal) -> Result<()>,
) -> Result<Admission> {
    start_bound(
        j,
        nodes,
        ctx,
        object,
        Action::Release {
            claim: claim.clone(),
        },
        SCHEMA,
        |nodes, frame| {
            let work = prepare(frame.attempt)?;
            let record = Record {
                schema: SCHEMA.into(),
                attempt: frame.attempt,
                plan: work.plan,
                job: work.job,
            };
            validate(ctx, frame, &record)?;
            let bytes = encode(&record)?;
            if bytes.len() > 65_536 {
                return Err(Error::Budget);
            }
            put(nodes, &bytes)
        },
        before_adopt,
    )
}
/// Discover adopted in-flight final-release work, including after cancellation.
/// Discovery does not replace native provider revalidation or grant permission.
pub fn current(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    object: &Object,
) -> Result<Option<ReleaseWork>> {
    let loaded = load(j, nodes, ctx, object)?;
    if loaded.pending.is_some() {
        return Err(Error::Pending);
    }
    let Some((_, frame)) = loaded.frames.last() else {
        return Ok(None);
    };
    if frame.outcome.is_some() || !matches!(frame.action, Action::Release { .. }) {
        return Ok(None);
    }
    typed_execution(nodes, frame, SCHEMA)?
        .map(|bytes| {
            let record = decode(&bytes)?;
            Ok(ReleaseWork::new(record.plan, record.job))
        })
        .transpose()
}
/// Exact adopted attempt and its historical outcome. No execution permission.
pub fn at_attempt(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    object: &Object,
    attempt: u64,
) -> Result<Option<(ReleaseWork, Option<Outcome>)>> {
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
    if !matches!(frame.action, Action::Release { .. }) {
        return Ok(None);
    }
    typed_execution(nodes, frame, SCHEMA)?
        .map(|bytes| {
            let r = decode(&bytes)?;
            validate(ctx, frame, &r)?;
            Ok((ReleaseWork::new(r.plan, r.job), frame.outcome.clone()))
        })
        .transpose()
}

pub(crate) fn check(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    plan: &ReleasePlan,
    job: &JournalRef,
    attempt: u64,
) -> Result<()> {
    // Caller has already checked exact native decision and in-flight admission.
    let object = plan.object();
    let loaded = load(j, nodes, ctx, object)?;
    let frame = loaded
        .frames
        .iter()
        .map(|(_, f)| f)
        .find(|f| f.attempt == attempt && matches!(f.action, Action::Release { .. }))
        .ok_or(Error::Stale)?;
    if let Some(root) = &frame.execution {
        let record = restore(nodes, root)?;
        if record.plan != *plan || record.job != *job {
            return Err(Error::Occupied);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "release_work_tests.rs"]
mod tests;
