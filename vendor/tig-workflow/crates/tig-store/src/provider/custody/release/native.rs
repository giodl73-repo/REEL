//! Original native claim-release kernels and their original receipts.
use super::*;
use crate::{
    ledger::{CapacityLedger, JournalRef},
    local::LocalStore,
    retention::{handoff as h, release as r},
    retirement_intent::{ObservedIntent, progress::handoff_work as hw},
    source_claims::SourceJournal,
};
use tig_core::{Object, identity, tree::NodeStore};
pub fn retirement_request(plan: &r::ReleasePlan, attempt: u64) -> Result<Request, Error> {
    Ok(Request {
        operation: plan.operation_id(attempt).map_err(release_error)?,
        owner: plan.owner().clone(),
        previous: super::super::native::commitment(plan.grant()),
        authority: Authority::Retirement {
            decision: plan.decision().clone(),
            subject: plan.subject().clone(),
            attempt,
        },
        required: AckLevel::ProcessRestart,
    })
}
pub fn handoff_request(
    plan: &h::HandoffPlan,
    admission: Option<Admission>,
) -> Result<Request, Error> {
    let req = plan.request();
    let id = serde_json::to_value((
        "tig.provider-claim-release.handoff-operation.design-1",
        &req.operation,
    ))
    .map_err(|_| Error::Invalid)?;
    Ok(Request {
        operation: Digest::blob(&identity::canonical_json(&id).map_err(|_| Error::Invalid)?),
        owner: req.owner.clone(),
        previous: super::super::native::commitment(&req.previous),
        authority: Authority::Handoff {
            review: Digest::blob(&plan.to_json().map_err(handoff_error)?),
            successor: super::super::native::commitment(&req.successor),
            admission,
        },
        required: AckLevel::ProcessRestart,
    })
}
/// IO-free native descriptor shared with execution-context preflight.
pub fn capabilities(store: &LocalStore, replay: Replay) -> Capabilities {
    Capabilities {
        place: store.identity().clone(),
        max_object_bytes: store.max_object_bytes(),
        max_ack: AckLevel::ProcessRestart,
        enforcement: Some(Enforcement::Cooperative),
        replay,
    }
}
struct Bound<F> {
    profile: Capabilities,
    wanted: Request,
    run: F,
}
impl<F, E> Executor for Bound<F>
where
    F: FnMut() -> Result<E, Error>,
{
    type Evidence = E;
    fn describe(&self) -> Capabilities {
        self.profile.clone()
    }
    fn release(&mut self, request: &Request) -> Result<Released<E>, Error> {
        if *request != self.wanted {
            return Err(Error::Invalid);
        }
        Ok(Released {
            request: request.clone(),
            level: AckLevel::ProcessRestart,
            evidence: (self.run)()?,
        })
    }
}
pub struct Retirement<'a, N> {
    pub ledger: &'a mut CapacityLedger,
    pub store: &'a LocalStore,
    pub job: &'a JournalRef,
    pub plan: &'a r::ReleasePlan,
    pub journal: &'a mut SourceJournal,
    pub nodes: &'a N,
    pub intent: &'a ObservedIntent,
    pub attempt: u64,
}
pub fn retirement<'a, N: NodeStore + 'a>(
    e: Retirement<'a, N>,
) -> Result<impl Executor<Evidence = r::ReleaseReceipt> + 'a, Error> {
    Ok(Bound {
        profile: capabilities(e.store, Replay::Historical),
        wanted: retirement_request(e.plan, e.attempt)?,
        run: move || {
            r::apply(
                e.ledger, e.store, e.job, e.plan, e.journal, e.nodes, e.intent, e.attempt,
            )
            .map_err(release_error)
        },
    })
}
pub struct Handoff<'a> {
    pub source_ledger: &'a mut CapacityLedger,
    pub source: &'a LocalStore,
    pub successor_ledger: &'a mut CapacityLedger,
    pub successor: &'a LocalStore,
    pub job: &'a JournalRef,
    pub plan: &'a h::HandoffPlan,
    pub authorization: &'a h::HandoffAuthorization,
}
pub fn handoff(e: Handoff<'_>) -> Result<impl Executor<Evidence = h::HandoffReceipt> + '_, Error> {
    Ok(Bound {
        profile: capabilities(e.source, Replay::Historical),
        wanted: handoff_request(e.plan, None)?,
        run: move || {
            h::apply(
                e.source_ledger,
                e.source,
                e.successor_ledger,
                e.successor,
                e.job,
                e.plan,
                e.authorization,
            )
            .map_err(handoff_error)
        },
    })
}
fn admitted_handoff_request<N: NodeStore>(
    e: &mut hw::Execution<'_, N>,
    object: &Object,
    attempt: u64,
) -> Result<Request, Error> {
    let work = hw::current(e.journal, e.nodes, e.intent, object)
        .map_err(|_| Error::Protected)?
        .ok_or(Error::Protected)?;
    handoff_request(
        work.plan(),
        Some(Admission {
            decision: e.intent.decision().digest().map_err(|_| Error::Invalid)?,
            attempt,
        }),
    )
}
pub fn admitted_handoff<'a, N: NodeStore + 'a>(
    mut e: hw::Execution<'a, N>,
    object: &Object,
    attempt: u64,
) -> Result<(Request, impl Executor<Evidence = h::HandoffReceipt> + 'a), Error> {
    let wanted = admitted_handoff_request(&mut e, object, attempt)?;
    let object = object.clone();
    let request = wanted.clone();
    Ok((
        request,
        Bound {
            profile: capabilities(e.source, Replay::Admitted),
            wanted,
            run: move || {
                hw::apply(
                    hw::Execution {
                        source: e.source,
                        source_ledger: &mut *e.source_ledger,
                        successor: e.successor,
                        successor_ledger: &mut *e.successor_ledger,
                        journal: &mut *e.journal,
                        nodes: e.nodes,
                        intent: e.intent,
                    },
                    &object,
                    attempt,
                )
                .map_err(handoff_error)
            },
        },
    ))
}
fn retention_error(e: crate::retention::RetentionError) -> Error {
    use crate::{Error as S, ledger::LedgerError as L, retention::RetentionError as R};
    match e {
        R::Store(S::Busy) | R::Ledger(L::Busy) => Error::Busy,
        R::Store(S::Retained) => Error::Protected,
        R::Released | R::Invalidated => Error::Stale,
        R::Store(S::OperationConflict) | R::Ledger(L::Conflict | L::Cancelled) => Error::Conflict,
        R::Store(S::Budget) | R::Ledger(L::Budget | L::Capacity(_)) => Error::Budget,
        R::Store(S::Unsupported) => Error::Unsupported,
        R::Store(S::Integrity | S::CorruptRecord | S::MissingAcknowledgedObject) => {
            Error::Integrity
        }
        R::InvalidPlan => Error::Invalid,
        _ => Error::Unknown,
    }
}
fn handoff_error(e: h::HandoffError) -> Error {
    match e {
        h::HandoffError::Retention(e) => retention_error(e),
        h::HandoffError::InvalidPlan => Error::Invalid,
        h::HandoffError::WrongAuthorization => Error::Denied,
        h::HandoffError::AlreadyReleased => Error::Conflict,
        h::HandoffError::Progress(_) => Error::Protected,
        _ => Error::Unknown,
    }
}
fn release_error(e: r::ReleaseError) -> Error {
    match e {
        r::ReleaseError::Retention(e) => retention_error(e),
        r::ReleaseError::InvalidPlan => Error::Invalid,
        r::ReleaseError::AlreadyReleased => Error::Conflict,
        r::ReleaseError::NotAdmitted | r::ReleaseError::Progress(_) => Error::Protected,
        _ => Error::Unknown,
    }
}
