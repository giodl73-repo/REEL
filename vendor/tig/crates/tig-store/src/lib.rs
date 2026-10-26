//! Experimental publication protocol. No startup discovery, eviction or daemon.
#![forbid(unsafe_code)]
pub mod accounted;
pub mod existing;
pub mod ledger;
pub mod local;
pub mod local_capacity;
pub mod metadata;
pub mod replica;
pub mod retention;
pub mod source_claims;

use serde::{Deserialize, Deserializer, Serialize, de};
use std::io::{self, Read};
use tig_core::Object;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Key(String);
impl Key {
    pub fn parse(value: &str) -> Result<Self> {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        {
            return Err(Error::InvalidRequest);
        }
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AckLevel {
    VerifiedLocal,
    ProcessRestart,
    ProviderDurable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub operation: Key,
    pub owner: Key,
    pub object: Object,
    pub required: AckLevel,
}
#[derive(Clone, Copy, Debug)]
pub struct Capabilities {
    pub max_object_bytes: u64,
    pub max_ack: AckLevel,
}
impl Capabilities {
    pub fn check(&self, request: &Request) -> Result<()> {
        if request.required > self.max_ack {
            return Err(Error::Unsupported);
        }
        if request.object.bytes > self.max_object_bytes {
            return Err(Error::Budget);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Protected,
    Committed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    request: Request,
    level: AckLevel,
}
impl Receipt {
    pub fn request(&self) -> &Request {
        &self.request
    }
    pub fn level(&self) -> AckLevel {
        self.level
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidRequest,
    Unsupported,
    Budget,
    OperationConflict,
    Integrity,
    CorruptRecord,
    MissingAcknowledgedObject,
    InvalidRoot,
    Busy,
    Retained,
    Io(io::ErrorKind, String),
}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e.kind(), e.to_string())
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

/// Trusted admitted backend contract, not proof of arbitrary provider correctness.
/// protect binds operation+owner+object+requested level, never a disposable catalog.
/// publish_object must privately verify and no-clobber publish; commit follows
/// verified availability and persists completion at the requested supported level.
pub trait PublicationBackend {
    fn capabilities(&self) -> Capabilities;
    fn publication_guard(&mut self, object: &Object) -> Result<Box<dyn PublicationGuard>>;
    fn protect(&mut self, request: &Request) -> Result<State>;
    fn verified_object(&mut self, object: &Object) -> Result<bool>;
    fn publish_object(&mut self, request: &Request, source: &mut dyn Read) -> Result<()>;
    fn commit(&mut self, request: &Request) -> Result<()>;
}

/// Keeps provider-effective object protection alive for one publication call.
pub trait PublicationGuard {}

/// Source remains caller-owned until acknowledgement. A verified existing object
/// may satisfy a retry without reading source. No cache removal is authorized.
pub fn publish(
    backend: &mut impl PublicationBackend,
    request: &Request,
    source: &mut dyn Read,
) -> Result<Receipt> {
    run(backend, request, Some(source))?.ok_or(Error::Integrity)
}
/// Explicit exact-operation recovery, not a journal/cold-storage census.
/// None means intent protected but no completed final object; no receipt issued.
pub fn recover(
    backend: &mut impl PublicationBackend,
    request: &Request,
) -> Result<Option<Receipt>> {
    run(backend, request, None)
}
fn run(
    backend: &mut impl PublicationBackend,
    request: &Request,
    source: Option<&mut dyn Read>,
) -> Result<Option<Receipt>> {
    backend.capabilities().check(request)?;
    let _guard = backend.publication_guard(&request.object)?;
    let state = backend.protect(request)?;
    if !backend.verified_object(&request.object)? {
        if state == State::Committed {
            return Err(Error::MissingAcknowledgedObject);
        }
        let Some(source) = source else {
            return Ok(None);
        };
        backend.publish_object(request, source)?;
        if !backend.verified_object(&request.object)? {
            return Err(Error::Integrity);
        }
    }
    backend.commit(request)?;
    Ok(Some(Receipt {
        request: request.clone(),
        level: request.required,
    }))
}
