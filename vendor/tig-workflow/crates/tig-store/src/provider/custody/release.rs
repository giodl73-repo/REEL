//! Admitted exact claim release. No payload deletion or free-space credit.
use super::Commitment;
use crate::{AckLevel, Key};
use tig_core::Digest;
pub mod native;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Admission {
    pub decision: Digest,
    pub attempt: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Authority {
    Handoff {
        review: Digest,
        successor: Commitment,
        admission: Option<Admission>,
    },
    Retirement {
        decision: Digest,
        subject: Digest,
        attempt: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub operation: Digest,
    pub owner: Key,
    pub previous: Commitment,
    pub authority: Authority,
    pub required: AckLevel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Enforcement {
    Cooperative,
    ProviderEnforced,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Replay {
    /// Reconcile exact in-flight work; controller skips durable completed work.
    Admitted,
    /// Also return the original disposition with valid original context/records.
    Historical,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub place: Digest,
    pub max_object_bytes: u64,
    pub max_ack: AckLevel,
    pub enforcement: Option<Enforcement>,
    pub replay: Replay,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Unsupported,
    Budget,
    Stale,
    Conflict,
    Protected,
    Integrity,
    Busy,
    Denied,
    Unavailable,
    Unknown,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub struct Released<E> {
    pub request: Request,
    pub level: AckLevel,
    pub evidence: E,
}
/// Trusted admitted controller+provider execution, not raw claim-drop permission.
/// Validate exact owner/review/decision/attempt and provider-effective exclusion.
/// Handoff holds the independently retained approved successor through disposition;
/// retirement needs separate explicit owned-claim release consent. Header equality
/// authenticates neither. Only the exact old claim may be retired: never payload
/// bytes, another owner's claim, or new retention of identical content. Preserve
/// durable retry fences; errors/lost replies may follow effect, never NoEffect.
/// No paths, read, seek, listing, publish, issuance or payload removal API required.
pub trait Executor {
    type Evidence;
    fn describe(&self) -> Capabilities;
    fn release(&mut self, request: &Request) -> Result<Released<Self::Evidence>, Error>;
}
pub fn release_exact<E: Executor>(
    executor: &mut E,
    request: &Request,
    allow_cooperative: bool,
) -> Result<Released<E::Evidence>, Error> {
    let p = executor.describe();
    if request.owner != request.previous.owner || request.required < AckLevel::ProcessRestart {
        return Err(Error::Invalid);
    }
    match &request.authority {
        Authority::Handoff {
            successor,
            admission,
            ..
        } => {
            if successor.place == request.previous.place
                || successor.object != request.previous.object
                || successor.level < request.previous.level
                || admission.as_ref().is_some_and(|a| a.attempt == 0)
            {
                return Err(Error::Invalid);
            }
        }
        Authority::Retirement { attempt, .. } if *attempt == 0 => return Err(Error::Invalid),
        _ => (),
    }
    if p.place != request.previous.place {
        return Err(Error::Stale);
    }
    if p.enforcement.is_none()
        || (p.enforcement == Some(Enforcement::Cooperative) && !allow_cooperative)
        || request.required > p.max_ack
    {
        return Err(Error::Unsupported);
    }
    if request.previous.object.bytes > p.max_object_bytes {
        return Err(Error::Budget);
    }
    let result = executor.release(request)?;
    if executor.describe() != p || result.request != *request {
        return Err(Error::Stale);
    }
    if result.level < request.required || result.level > p.max_ack {
        return Err(Error::Integrity);
    }
    Ok(result)
}
