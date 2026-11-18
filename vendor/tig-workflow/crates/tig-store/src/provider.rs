//! Portable provider capabilities; read, publication and custody stay separate.
use crate::acquire::{ObjectSource, SourceError};
use std::io::Read;
use tig_core::{Digest, Object};
pub mod budget;
pub mod custody;
pub mod profile;
pub mod publication;
pub mod removal;
#[cfg(feature = "s3-read")]
pub mod s3_read;

/// Provider-owned comparison token, not a checksum or authority credential.
/// Tokens are bounded and redacted; this runtime port defines no wire format.
#[derive(Clone, PartialEq, Eq)]
pub struct Version(Vec<u8>);
impl Version {
    pub fn new(bytes: &[u8]) -> Result<Self, SourceError> {
        if bytes.is_empty() || bytes.len() > 256 {
            return Err(SourceError::Invalid);
        }
        Ok(Self(bytes.into()))
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}
impl std::fmt::Debug for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Version(<opaque>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReadProtection {
    /// Destination must verify streamed bytes; source may change concurrently.
    Unprotected,
    /// Stable bytes throughout the returned reader's lifetime under admitted
    /// cooperating writers. This does not resist arbitrary external mutation.
    CooperativeFence,
    /// A provider-pinned immutable version for the returned reader's lifetime.
    ImmutableVersion,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadCapabilities {
    pub place: Digest,
    pub max_object_bytes: u64,
    pub protection: ReadProtection,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadUse {
    /// Existing publication verifies exact bytes before success.
    Transfer,
    Protected {
        allow_cooperative: bool,
    },
}
impl ReadCapabilities {
    pub fn check(
        &self,
        place: &Digest,
        object: &Object,
        usage: ReadUse,
    ) -> Result<(), SourceError> {
        if self.place != *place {
            return Err(SourceError::Stale);
        }
        if object.bytes > self.max_object_bytes {
            return Err(SourceError::Budget);
        }
        match (usage, self.protection) {
            (ReadUse::Transfer, _) | (_, ReadProtection::ImmutableVersion) => Ok(()),
            (
                ReadUse::Protected {
                    allow_cooperative: true,
                },
                ReadProtection::CooperativeFence,
            ) => Ok(()),
            _ => Err(SourceError::Unsupported),
        }
    }
}
/// Exact stored publication, distinct from immutable content identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    pub place: Digest,
    pub object: Object,
    pub version: Version,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lookup {
    Present(Located),
    /// Point observation of this exact object at the described place only.
    /// Not a cacheable negative, complete census, or retention/removal evidence.
    Absent,
    Unknown,
}
pub struct Opened<R: Read> {
    pub location: Located,
    pub reader: R,
}
/// Trusted admitted adapter. describe is cheap configuration, with no IO.
/// lookup is bounded exact-key metadata work; no listing or payload hash pass.
/// open_version refuses a different/replaced version and never falls back to
/// latest. Its reader holds the declared protection through its own lifetime.
/// No publish, retain, remove, Seek, local path or enumeration is required.
pub trait ExactSource {
    type Reader: Read;
    fn describe(&self) -> ReadCapabilities;
    fn lookup(&mut self, object: &Object) -> Result<Lookup, SourceError>;
    fn open_version(&mut self, location: &Located) -> Result<Opened<Self::Reader>, SourceError>;
}

pub fn open_exact<S: ExactSource>(
    source: &mut S,
    place: &Digest,
    object: &Object,
    usage: ReadUse,
) -> Result<S::Reader, SourceError> {
    let profile = source.describe();
    profile.check(place, object, usage)?;
    let location = match source.lookup(object)? {
        Lookup::Present(location) => location,
        // Acquisition cannot turn a point miss into a durable negative claim.
        Lookup::Absent | Lookup::Unknown => return Err(SourceError::Unknown),
    };
    if location.place != *place || location.object != *object {
        return Err(SourceError::Stale);
    }
    if source.describe() != profile {
        return Err(SourceError::Stale);
    }
    let opened = source.open_version(&location)?;
    if opened.location != location || source.describe() != profile {
        return Err(SourceError::Stale);
    }
    Ok(opened.reader)
}

/// Lazy bridge into the existing verified/accounted publication coordinators.
/// Construction does no lookup/open; warm destination reuse needs no source IO.
pub struct Source<'a, S> {
    source: &'a mut S,
    place: Digest,
    usage: ReadUse,
}
impl<'a, S: ExactSource> Source<'a, S> {
    pub fn new(source: &'a mut S, place: Digest, usage: ReadUse) -> Self {
        Self {
            source,
            place,
            usage,
        }
    }
}
impl<S: ExactSource> ObjectSource for Source<'_, S> {
    type Reader = S::Reader;
    fn max_object_bytes(&self) -> u64 {
        self.source.describe().max_object_bytes
    }
    fn open(&mut self, object: &Object) -> Result<Self::Reader, SourceError> {
        open_exact(self.source, &self.place, object, self.usage)
    }
}
