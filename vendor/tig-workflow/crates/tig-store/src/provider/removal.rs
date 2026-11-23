//! Exact admitted removal execution; copy versions are separate from read tokens.
use super::Version;
use crate::{AckLevel, Key};
use tig_core::{Digest, Object};
pub mod native;
#[derive(Clone, PartialEq, Eq)]
pub struct CopyVersion(Version);
impl CopyVersion {
    pub fn new(bytes: &[u8]) -> Result<Self, Error> {
        Version::new(bytes).map(Self).map_err(|_| Error::Invalid)
    }
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}
impl std::fmt::Debug for CopyVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CopyVersion(<opaque>)")
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub place: Digest,
    pub object: Object,
    pub version: CopyVersion,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub operation: Digest,
    pub decision: Digest,
    pub owner: Key,
    pub attempt: u64,
    pub target: Target,
    pub preservation: Option<Digest>,
    pub required: AckLevel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Enforcement {
    Unsupported,
    CooperativeIncarnation,
    ConditionalVersion,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub place: Digest,
    pub max_object_bytes: u64,
    pub max_ack: AckLevel,
    pub enforcement: Enforcement,
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
/// Removes only this exact historical copy version. It says nothing about a new
/// incarnation of the same content, net reclaim or absence of other placements.
pub struct Removed<E> {
    pub request: Request,
    pub level: AckLevel,
    pub evidence: E,
}
/// Trusted execution adapter over controller authority AND provider effects.
/// The request is a binding, not permission. The adapter must validate admitted
/// decision/owner/attempt, effective current-protection exclusion, and held retained
/// custody OR exact intentional-loss consent. Those holds span the version-fenced
/// effect/disposition. Unknown coverage refuses. Timeout/error is not NoEffect.
/// Exact retry replays the original outcome without inspecting/removing fresh
/// versions. No path, read, seek, listing, publication or retention API is required.
pub trait ExactRemover {
    type Evidence;
    fn describe(&self) -> Capabilities;
    fn remove(&mut self, request: &Request) -> Result<Removed<Self::Evidence>, Error>;
}
pub fn remove_exact<R: ExactRemover>(
    remover: &mut R,
    request: &Request,
    allow_cooperative: bool,
) -> Result<Removed<R::Evidence>, Error> {
    let profile = remover.describe();
    if request.attempt == 0 || request.required < AckLevel::ProcessRestart {
        return Err(Error::Invalid);
    }
    if request.target.place != profile.place {
        return Err(Error::Stale);
    }
    if profile.enforcement == Enforcement::Unsupported
        || (profile.enforcement == Enforcement::CooperativeIncarnation && !allow_cooperative)
        || request.required > profile.max_ack
    {
        return Err(Error::Unsupported);
    }
    if request.target.object.bytes > profile.max_object_bytes {
        return Err(Error::Budget);
    }
    let result = remover.remove(request)?;
    if remover.describe() != profile || result.request != *request {
        return Err(Error::Stale);
    }
    if result.level < request.required || result.level > profile.max_ack {
        return Err(Error::Integrity);
    }
    Ok(result)
}
