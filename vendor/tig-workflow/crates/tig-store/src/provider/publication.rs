//! Place/version-bound immutable publication. No filesystem or custody API.
use super::Version;
use crate::{AckLevel, Request};
use std::io::Read;
use tig_core::Digest;
pub mod native;
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Replay {
    Unsupported,
    ExactOperation,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub place: Digest,
    pub max_object_bytes: u64,
    pub max_ack: AckLevel,
    pub replay: Replay,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Unsupported,
    Budget,
    Stale,
    Conflict,
    Integrity,
    Unavailable,
    Denied,
    Busy,
    Unknown,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
/// Provider evidence remains native/asset-side. Version is not a content checksum,
/// retained-custody grant, negative observation or deletion permission.
pub struct Published<E> {
    pub place: Digest,
    pub request: Request,
    pub version: Version,
    pub level: AckLevel,
    pub evidence: E,
}
/// Trusted admitted adapter. describe is cheap configuration with no IO.
/// publish binds the WHOLE request to the operation at this stable place, verifies
/// SHA-256/length, privately publishes immutable bytes, and acknowledges exactly
/// the achieved persistence. Exact duplicate requests reconcile original outcomes;
/// changed operation bodies conflict. Lost replies/errors are not NoEffect.
/// Reuse may avoid source IO. No Read/Seek/path/list/retain/remove API is required
/// of the destination; source is a bounded sequential stream owned by the caller.
pub trait ExactPublisher {
    type Evidence;
    fn describe(&self) -> Capabilities;
    fn publish(
        &mut self,
        request: &Request,
        source: &mut dyn Read,
    ) -> Result<Published<Self::Evidence>, Error>;
}
/// Checks admission before invoking the publisher, then binds its acknowledgement
/// to the same place/profile/request. A failed postcheck may follow a real effect;
/// recover by exact operation, never by assuming no bytes were published.
pub fn publish_exact<P: ExactPublisher>(
    publisher: &mut P,
    place: &Digest,
    request: &Request,
    source: &mut dyn Read,
) -> Result<Published<P::Evidence>, Error> {
    let profile = publisher.describe();
    if profile.place != *place {
        return Err(Error::Stale);
    }
    if profile.replay != Replay::ExactOperation || request.required > profile.max_ack {
        return Err(Error::Unsupported);
    }
    if request.object.bytes > profile.max_object_bytes {
        return Err(Error::Budget);
    }
    let result = publisher.publish(request, source)?;
    if publisher.describe() != profile || result.place != *place || result.request != *request {
        return Err(Error::Stale);
    }
    if result.level < request.required || result.level > profile.max_ack {
        return Err(Error::Integrity);
    }
    Ok(result)
}
