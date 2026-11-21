//! Optional exact retained-claim creation; issuance is not an effective hold.
use super::Commitment;
use crate::{AckLevel, Key, provider::Version};
use tig_core::{Digest, Object};
pub mod native;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub operation: Key,
    pub owner: Key,
    pub place: Digest,
    pub object: Object,
    pub version: Version,
    pub required: AckLevel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Enforcement {
    Cooperative,
    ProviderEnforced,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub place: Digest,
    pub max_object_bytes: u64,
    pub max_ack: AckLevel,
    pub exact_replay: bool,
    pub enforcement: Option<Enforcement>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Unsupported,
    Budget,
    Stale,
    Conflict,
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
pub struct Issued<E> {
    pub request: Request,
    pub commitment: Commitment,
    pub evidence: E,
}
/// Trusted admitted provider with caller-owned authority and resource admission.
/// Verify the exact published version before durably establishing owned retention.
/// Exact retries recover the same claim; changed requests conflict, and released
/// claims cannot be reactivated. Retention survives session exit, unlike a Held
/// guard. Fresh custody acquisition remains mandatory before relying on the claim.
/// Operation identity is scoped to the enrolled place and admitted owner.
/// No read, path, seek, listing, publish, release or removal API is required.
/// Errors/lost replies may follow a real effect and never assert NoEffect.
pub trait Issuer {
    type Evidence;
    fn describe(&self) -> Capabilities;
    fn issue(&mut self, request: &Request) -> Result<Issued<Self::Evidence>, Error>;
}
pub fn issue_exact<I: Issuer>(
    issuer: &mut I,
    request: &Request,
    allow_cooperative: bool,
) -> Result<Issued<I::Evidence>, Error> {
    let p = issuer.describe();
    if request.required < AckLevel::ProcessRestart {
        return Err(Error::Invalid);
    }
    if p.place != request.place {
        return Err(Error::Stale);
    }
    if !p.exact_replay
        || p.enforcement.is_none()
        || (p.enforcement == Some(Enforcement::Cooperative) && !allow_cooperative)
        || request.required > p.max_ack
    {
        return Err(Error::Unsupported);
    }
    if request.object.bytes > p.max_object_bytes {
        return Err(Error::Budget);
    }
    let issued = issuer.issue(request)?;
    if issuer.describe() != p
        || issued.request != *request
        || issued.commitment.place != request.place
        || issued.commitment.object != request.object
        || issued.commitment.owner != request.owner
    {
        return Err(Error::Stale);
    }
    if issued.commitment.level < request.required || issued.commitment.level > p.max_ack {
        return Err(Error::Integrity);
    }
    Ok(issued)
}
