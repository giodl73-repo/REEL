//! Lazy sequential acquisition into the existing publication coordinators.
//! Source streams are not custody or integrity evidence; destination verifies.
use crate::{
    PublicationBackend, Receipt, Request, accounted,
    existing::{self, ByteSource},
    ledger::{CapacityLedger, JournalRef},
    local::{LocalStore, ObjectRead},
    provider::{
        self, ExactSource, Located, Lookup, Opened, ReadCapabilities, ReadProtection, ReadUse,
        Version,
    },
};
use std::io::{self, Read};
use tig_core::Object;

pub const READ_CHUNK: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceError {
    Invalid,
    Unknown,
    Unavailable,
    Denied,
    Busy,
    Budget,
    Integrity,
    Unsupported,
    Stale,
    Io,
}
impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SourceError {}

/// Acquisition capability only: no Seek, local path, listing, write or removal.
/// Opening a stream does not establish integrity, retention or scoped absence.
/// Limits are cheap admitted configuration; open/read may perform bounded IO.
pub trait ObjectSource {
    type Reader: Read;
    fn max_object_bytes(&self) -> u64;
    fn open(&mut self, object: &Object) -> Result<Self::Reader, SourceError>;
}

/// Single-use adapter for an already opened stream or held verified session.
/// It neither reopens nor rewinds. The object argument is an expectation, not
/// integrity evidence; publication still verifies every byte. An unused reader
/// stays owned/protected here until this source is dropped.
pub struct OpenedSource<R: Read> {
    object: Object,
    reader: Option<R>,
}
impl<R: Read> OpenedSource<R> {
    pub fn new(object: Object, reader: R) -> Self {
        Self {
            object,
            reader: Some(reader),
        }
    }
}
impl<R: Read> ObjectSource for OpenedSource<R> {
    type Reader = R;
    fn max_object_bytes(&self) -> u64 {
        self.object.bytes
    }
    fn open(&mut self, object: &Object) -> Result<R, SourceError> {
        if object != &self.object {
            return Err(SourceError::Invalid);
        }
        self.reader.take().ok_or(SourceError::Unavailable)
    }
}

impl ObjectSource for existing::ReadCache {
    type Reader = existing::VerifiedRead;
    fn max_object_bytes(&self) -> u64 {
        self.max_object_bytes()
    }
    fn open(&mut self, object: &Object) -> Result<Self::Reader, SourceError> {
        self.open_verified(object).map_err(|e| match e {
            existing::Error::Invalid => SourceError::Invalid,
            existing::Error::Budget => SourceError::Budget,
            // Existing-layout lookup cannot prove absence across offline places.
            existing::Error::Missing => SourceError::Unknown,
            existing::Error::Integrity => SourceError::Integrity,
            existing::Error::Denied => SourceError::Denied,
            existing::Error::Busy => SourceError::Busy,
            existing::Error::Io => SourceError::Io,
            existing::Error::Unsupported => SourceError::Unsupported,
        })
    }
}

/// Exact completed native publication. Its reader retains the native shared
/// guard for the whole transfer; an operation ID is not inferred from a hash.
pub struct PublicationSource<'a> {
    store: &'a LocalStore,
    publication: Request,
}
impl<'a> PublicationSource<'a> {
    pub fn new(store: &'a LocalStore, publication: Request) -> Self {
        Self { store, publication }
    }
}
impl ObjectSource for PublicationSource<'_> {
    type Reader = ObjectRead;
    fn max_object_bytes(&self) -> u64 {
        self.store.max_object_bytes()
    }
    fn open(&mut self, object: &Object) -> Result<Self::Reader, SourceError> {
        let place = self.store.identity().clone();
        provider::open_exact(
            self,
            &place,
            object,
            ReadUse::Protected {
                allow_cooperative: true,
            },
        )
    }
}
impl ExactSource for PublicationSource<'_> {
    type Reader = ObjectRead;
    fn describe(&self) -> ReadCapabilities {
        ReadCapabilities {
            place: self.store.identity().clone(),
            max_object_bytes: self.store.max_object_bytes(),
            protection: ReadProtection::CooperativeFence,
        }
    }
    fn lookup(&mut self, object: &Object) -> Result<Lookup, SourceError> {
        if object != &self.publication.object {
            return Err(SourceError::Invalid);
        }
        match self
            .store
            .completed_version(&self.publication)
            .map_err(native_error)?
        {
            Some(version) => Ok(Lookup::Present(Located {
                place: self.store.identity().clone(),
                object: object.clone(),
                version: Version::new(version.as_str().as_bytes())?,
            })),
            None => Err(SourceError::Unavailable),
        }
    }
    fn open_version(&mut self, location: &Located) -> Result<Opened<Self::Reader>, SourceError> {
        if location.place != *self.store.identity() || location.object != self.publication.object {
            return Err(SourceError::Stale);
        }
        let version =
            std::str::from_utf8(location.version.as_bytes()).map_err(|_| SourceError::Stale)?;
        let version = tig_core::Digest::parse(version).map_err(|_| SourceError::Stale)?;
        let reader = self
            .store
            .completed_reader_at(&self.publication, Some(&version))
            .map_err(native_error)?
            .map(|(_, reader)| reader)
            .ok_or(SourceError::Unavailable)?;
        // The original shared guard spans version validation, hashing and use.
        Ok(Opened {
            location: location.clone(),
            reader,
        })
    }
}
pub(crate) fn native_error(e: crate::Error) -> SourceError {
    match e {
        crate::Error::InvalidRequest | crate::Error::InvalidRoot => SourceError::Invalid,
        crate::Error::Unsupported => SourceError::Unsupported,
        crate::Error::Budget => SourceError::Budget,
        crate::Error::OperationConflict => SourceError::Stale,
        crate::Error::Integrity
        | crate::Error::CorruptRecord
        | crate::Error::MissingAcknowledgedObject => SourceError::Integrity,
        crate::Error::Busy | crate::Error::Retained => SourceError::Busy,
        crate::Error::Io(kind, _) => io_kind(kind),
    }
}

#[derive(Debug)]
pub enum AcquireError<E> {
    Source(SourceError),
    Publication(E),
}
impl<E: std::fmt::Debug> std::fmt::Display for AcquireError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl<E: std::error::Error> std::error::Error for AcquireError<E> {}

/// Normal publication semantics, with source open deferred until bytes are needed.
/// Existing verified destination bytes can satisfy the request with no source IO.
pub fn publish<S: ObjectSource>(
    source: &mut S,
    destination: &mut impl PublicationBackend,
    request: &Request,
) -> Result<Receipt, AcquireError<crate::Error>> {
    let mut input = LazyReader::new(source, &request.object);
    let result = crate::publish(destination, request, &mut input);
    input.result(result)
}

/// Uses the original native plan/job/ledger and publication kernel. No separate
/// transfer receipt, reservation engine or success state is introduced.
pub fn publish_accounted<S: ObjectSource>(
    source: &mut S,
    ledger: &mut CapacityLedger,
    destination: &LocalStore,
    job: &JournalRef,
    plan: &accounted::PublicationPlan,
) -> Result<accounted::AccountedReceipt, AcquireError<accounted::AccountedError>> {
    let mut input = LazyReader::new(source, &plan.request().object);
    let result = accounted::publish_accounted(ledger, destination, job, plan, &mut input);
    input.result(result)
}

/// Portable place/version-bound destination, retaining its native evidence.
/// Lazy opening and the same per-read/total source bounds apply to every adapter.
pub fn publish_placed<S: ObjectSource, P: provider::publication::ExactPublisher>(
    source: &mut S,
    destination: &mut P,
    place: &tig_core::Digest,
    request: &Request,
) -> Result<provider::publication::Published<P::Evidence>, AcquireError<provider::publication::Error>>
{
    let mut input = LazyReader::new(source, &request.object);
    let result = provider::publication::publish_exact(destination, place, request, &mut input);
    input.result(result)
}
struct LazyReader<'a, S: ObjectSource> {
    source: &'a mut S,
    object: &'a Object,
    reader: Option<S::Reader>,
    error: Option<SourceError>,
    read: u64,
    eof: bool,
}
impl<'a, S: ObjectSource> LazyReader<'a, S> {
    fn new(source: &'a mut S, object: &'a Object) -> Self {
        Self {
            source,
            object,
            reader: None,
            error: None,
            read: 0,
            eof: false,
        }
    }
    fn fail(&mut self, error: SourceError) -> io::Error {
        self.error = Some(error);
        io::Error::other(error)
    }
    fn result<T, E>(&mut self, result: Result<T, E>) -> Result<T, AcquireError<E>> {
        match self.error {
            Some(error) => Err(AcquireError::Source(error)),
            None => result.map_err(AcquireError::Publication),
        }
    }
}
impl<S: ObjectSource> Read for LazyReader<'_, S> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        if into.is_empty() {
            return Ok(0);
        }
        if let Some(error) = self.error {
            return Err(io::Error::other(error));
        }
        if self.eof {
            return Ok(0);
        }
        if self.reader.is_none() {
            if self.object.bytes > self.source.max_object_bytes() {
                return Err(self.fail(SourceError::Budget));
            }
            self.reader = Some(self.source.open(self.object).map_err(|e| self.fail(e))?);
        }
        let remaining = self
            .object
            .bytes
            .checked_add(1)
            .and_then(|n| n.checked_sub(self.read))
            .filter(|n| *n != 0)
            .ok_or_else(|| self.fail(SourceError::Budget))?;
        let limit = into
            .len()
            .min(READ_CHUNK)
            .min(remaining.min(READ_CHUNK as u64) as usize);
        match self
            .reader
            .as_mut()
            .expect("opened reader")
            .read(&mut into[..limit])
        {
            Ok(n) if n <= limit => {
                self.read += n as u64;
                self.eof = n == 0;
                Ok(n)
            }
            Ok(_) => Err(self.fail(SourceError::Integrity)),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => Err(e),
            Err(e) => {
                let error = e
                    .get_ref()
                    .and_then(|e| e.downcast_ref::<SourceError>())
                    .copied()
                    .unwrap_or_else(|| io_kind(e.kind()));
                Err(self.fail(error))
            }
        }
    }
}
fn io_kind(kind: io::ErrorKind) -> SourceError {
    match kind {
        io::ErrorKind::PermissionDenied => SourceError::Denied,
        io::ErrorKind::NotFound => SourceError::Unavailable,
        io::ErrorKind::WouldBlock => SourceError::Busy,
        io::ErrorKind::InvalidData => SourceError::Integrity,
        io::ErrorKind::Unsupported => SourceError::Unsupported,
        _ => SourceError::Io,
    }
}
