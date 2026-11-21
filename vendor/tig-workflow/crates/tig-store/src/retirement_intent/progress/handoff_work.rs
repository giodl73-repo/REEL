//! Admitted native handoff work; releasing a claim never removes payload bytes.
use super::*;
use crate::{
    ledger::{CapacityLedger, JournalRef},
    local::LocalStore,
    retention::handoff::{self, HandoffAuthorization, HandoffError, HandoffPlan, HandoffReceipt},
};
pub(super) const SCHEMA: &str = "tig.native-handoff-work.design-1";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoffWork {
    plan: HandoffPlan,
    authorization: HandoffAuthorization,
    job: JournalRef,
}
impl HandoffWork {
    pub fn new(plan: HandoffPlan, authorization: HandoffAuthorization, job: JournalRef) -> Self {
        Self {
            plan,
            authorization,
            job,
        }
    }
    pub fn plan(&self) -> &HandoffPlan {
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
    plan: HandoffPlan,
    authorization: HandoffAuthorization,
    job: JournalRef,
}
/// Native operation identity is unique to this decision/object/attempt, so a
/// new progress attempt cannot reuse a partially bound native handoff allocation.
pub fn operation(ctx: &ObservedIntent, object: &Object, attempt: u64) -> Result<Key> {
    if attempt == 0 {
        return Err(Error::Invalid);
    }
    let digest = Digest::blob(&encode(&(
        SCHEMA,
        &ctx.journal,
        ctx.decision().digest().map_err(Error::Decision)?,
        object,
        attempt,
    ))?);
    Key::parse(&format!("handoff-work-{}", digest.as_str())).map_err(|_| Error::Invalid)
}
fn validate(ctx: &ObservedIntent, frame: &Frame, record: &Record) -> Result<()> {
    let Action::Release { claim } = &frame.action else {
        return Err(Error::Invalid);
    };
    if record.schema != SCHEMA
        || record.attempt != frame.attempt
        || record.plan.request().operation != operation(ctx, &frame.object, frame.attempt)?
        || !record
            .plan
            .matches_work(&record.authorization, ctx.owner(), &frame.object, claim)
    {
        return Err(Error::Invalid);
    }
    if let Some(profile) = ctx.execution_profile() {
        let expected = profile
            .policy_proof()
            .map(|_| profile.fingerprint())
            .transpose()?;
        if record.plan.policy_context() != expected.as_ref() {
            return Err(Error::Invalid);
        }
        profile.verify_handoff_binding(record.plan.request())?;
    }
    Ok(())
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
/// The factory performs planning/reservation only, never the native effect.
pub fn start(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    object: &Object,
    claim: &Digest,
    prepare: impl FnOnce(u64) -> Result<HandoffWork>,
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
    prepare: impl FnOnce(u64) -> Result<HandoffWork>,
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
            let mut w = prepare(frame.attempt)?;
            if let Some(profile) = ctx
                .execution_profile()
                .filter(|p| p.policy_proof().is_some())
            {
                w.plan = w.plan.bind_policy(Some(profile.fingerprint()?));
                w.authorization = w.plan.authorize_handoff().map_err(|_| Error::Invalid)?;
            }
            let record = Record {
                schema: SCHEMA.into(),
                attempt: frame.attempt,
                plan: w.plan,
                authorization: w.authorization,
                job: w.job,
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
/// Read adopted in-flight work, including after cancellation. Not effect permission.
pub fn current(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    object: &Object,
) -> Result<Option<HandoffWork>> {
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
            let r = decode(&bytes)?;
            Ok(HandoffWork::new(r.plan, r.authorization, r.job))
        })
        .transpose()
}
/// Exact adopted attempt and its historical outcome. Never renews admission.
pub fn at_attempt(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    object: &Object,
    attempt: u64,
) -> Result<Option<(HandoffWork, Option<Outcome>)>> {
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
            Ok((
                HandoffWork::new(r.plan, r.authorization, r.job),
                frame.outcome.clone(),
            ))
        })
        .transpose()
}

pub struct Execution<'a, N> {
    pub source: &'a LocalStore,
    pub source_ledger: &'a mut CapacityLedger,
    pub successor: &'a LocalStore,
    pub successor_ledger: &'a mut CapacityLedger,
    pub journal: &'a mut SourceJournal,
    pub nodes: &'a N,
    pub intent: &'a ObservedIntent,
}
fn check(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    object: &Object,
    attempt: u64,
    expected: &HandoffWork,
) -> Result<()> {
    let state = inspect(j, nodes, ctx, object)?;
    if state.pending
        || state.in_flight
            != Some((
                attempt,
                Action::Release {
                    claim: expected.plan.request().previous.claim_id().clone(),
                },
            ))
    {
        return Err(Error::Stale);
    }
    if current(j, nodes, ctx, object)?.as_ref() != Some(expected) {
        return Err(Error::Occupied);
    }
    Ok(())
}
/// Discover/reconcile the original admitted native handoff. Finished progress
/// must be skipped by the controller; this entry point cannot renew it.
pub fn apply(
    execution: Execution<'_, impl NodeStore>,
    object: &Object,
    attempt: u64,
) -> handoff::Result<HandoffReceipt> {
    apply_hook(execution, object, attempt, |_| Ok(()))
}
fn apply_hook(
    execution: Execution<'_, impl NodeStore>,
    object: &Object,
    attempt: u64,
    mut before_check: impl FnMut(&mut SourceJournal) -> Result<()>,
) -> handoff::Result<HandoffReceipt> {
    let Execution {
        source,
        source_ledger,
        successor,
        successor_ledger,
        journal,
        nodes,
        intent,
    } = execution;
    if let Some(profile) = intent
        .execution_profile()
        .filter(|p| p.policy_proof().is_some())
    {
        let proof =
            super::super::policy::Proof::load(nodes, profile, intent.review(), intent.decision())
                .map_err(HandoffError::Progress)?;
        let candidate = current(journal, nodes, intent, object)
            .map_err(HandoffError::Progress)?
            .ok_or(HandoffError::Progress(Error::Missing))?;
        proof
            .check_capacity(source.identity(), source_ledger.identity())
            .map_err(HandoffError::Progress)?;
        proof
            .check_capacity(successor.identity(), successor_ledger.identity())
            .map_err(HandoffError::Progress)?;
        proof
            .check_handoff(candidate.plan().request())
            .map_err(HandoffError::Progress)?;
    }
    let work = current(journal, nodes, intent, object)
        .map_err(HandoffError::Progress)?
        .ok_or(HandoffError::Progress(Error::Missing))?;
    handoff::apply_guarded(
        source_ledger,
        source,
        successor_ledger,
        successor,
        &work.job,
        &work.plan,
        &work.authorization,
        || {
            before_check(journal).map_err(HandoffError::Progress)?;
            check(journal, nodes, intent, object, attempt, &work).map_err(HandoffError::Progress)
        },
        || {
            super::super::policy::require_effect_authority(intent, nodes)
                .map_err(HandoffError::Progress)
        },
    )
}

#[cfg(test)]
#[path = "handoff_work_tests.rs"]
mod tests;
