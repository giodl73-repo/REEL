//! Optional Tig protocol adapter over the existing REEL CAS writer/admission.
//! Publication availability is not retained custody, selection or eviction.
use reel_assets::{Config, admission};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    path::PathBuf,
};
use tig_core::{Digest, Object, identity};
use tig_store::existing::{self, ByteSource, Layout, Place, ReadCache, VerifiedRead};
use tig_store::{
    AckLevel, Capabilities, Error, Key, PublicationBackend, PublicationGuard, Request, Result,
    State,
};

/// Explicit private local enrollment. Coordination is shared by all writers;
/// no repository-wide or cache-wide guard is held while hashing/copying.
pub struct Profile {
    pub artifact_root: PathBuf,
    pub namespace: Key,
    pub storage: Config,
    pub destination: PathBuf,
    pub reader_limits: existing::Limits,
    pub protection: existing::RequiredProtection,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    request: Request,
}
struct Guard {
    _lock: admission::MetadataLock,
}
impl PublicationGuard for Guard {}

pub struct ExistingPublisher {
    root: PathBuf,
    storage: Config,
    destination: PathBuf,
    canonical: PathBuf,
    metadata: PathBuf,
    fences: PathBuf,
    reader: ReadCache,
    held: Option<VerifiedRead>,
    max_object_bytes: u64,
}
fn provider_error(error: Box<dyn std::error::Error>) -> Error {
    if error.downcast_ref::<admission::AdmissionDenied>().is_some() {
        Error::Budget
    } else {
        Error::Io(io::ErrorKind::Other, error.to_string())
    }
}
fn read_error(error: existing::Error) -> Error {
    match error {
        existing::Error::Budget => Error::Budget,
        existing::Error::Unsupported => Error::Unsupported,
        existing::Error::Invalid => Error::InvalidRequest,
        existing::Error::Busy => Error::Busy,
        existing::Error::Denied => Error::Io(
            io::ErrorKind::PermissionDenied,
            "existing cache denied access".into(),
        ),
        existing::Error::Io => Error::Io(
            io::ErrorKind::Other,
            "existing cache verification I/O failed".into(),
        ),
        _ => Error::Integrity,
    }
}
impl ExistingPublisher {
    /// Capture explicit enrollment only. Missing roots are never discovered or
    /// created. Operational directories are created by explicit publish/recover.
    pub fn open(profile: Profile) -> Result<Self> {
        if !profile.artifact_root.is_absolute()
            || !profile.artifact_root.is_dir()
            || !profile.destination.is_absolute()
            || !profile.destination.is_dir()
            || !profile.storage.cache_roots.contains(&profile.destination)
        {
            return Err(Error::InvalidRoot);
        }
        profile
            .storage
            .capacity
            .validate()
            .map_err(provider_error)?;
        let control = profile
            .storage
            .coordination_root
            .as_ref()
            .ok_or(Error::InvalidRoot)?;
        if !control.is_absolute() || !control.is_dir() {
            return Err(Error::InvalidRoot);
        }
        if !cfg!(windows) && profile.protection == existing::RequiredProtection::PreventMutation {
            return Err(Error::Unsupported);
        }
        let canonical = profile.destination.canonicalize()?;
        let base = canonical.join("objects/sha256");
        if !fs::symlink_metadata(&base)?.is_dir() || base.canonicalize()? != base {
            return Err(Error::InvalidRoot);
        }
        let place = Digest::blob(canonical.to_str().ok_or(Error::InvalidRoot)?.as_bytes());
        let root = control.canonicalize()?.join("tig-publications");
        let metadata = root
            .join("operations")
            .join(profile.namespace.as_str())
            .join(place.as_str());
        let fences = root.join("fences").join(place.as_str());
        let max_object_bytes = profile
            .storage
            .capacity
            .max_object_bytes
            .min(profile.reader_limits.object_bytes);
        let reader = ReadCache::new(existing::Profile {
            places: vec![Place {
                root: canonical.clone(),
                layout: Layout::ReelCas,
            }],
            required: profile.protection,
            limits: profile.reader_limits,
        })
        .map_err(read_error)?;
        Ok(Self {
            root: profile.artifact_root.canonicalize()?,
            storage: profile.storage,
            destination: profile.destination,
            canonical,
            metadata,
            fences,
            reader,
            held: None,
            max_object_bytes,
        })
    }
    fn path(&self, request: &Request, phase: &str) -> PathBuf {
        self.metadata.join(format!(
            "{}.{phase}.json",
            Digest::blob(request.operation.as_str().as_bytes()).as_str()
        ))
    }
    fn existing_record(&self, request: &Request, phase: &str) -> Result<bool> {
        let path = self.path(request, phase);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
            Ok(metadata)
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.len() > 65_536 =>
            {
                return Err(Error::CorruptRecord);
            }
            _ => (),
        }
        let mut bytes = Vec::new();
        fs::File::open(path)?.take(65_537).read_to_end(&mut bytes)?;
        if bytes.len() > 65_536 {
            return Err(Error::CorruptRecord);
        }
        let value = identity::parse_json(&bytes).map_err(|_| Error::CorruptRecord)?;
        let record: Record = serde_json::from_value(value).map_err(|_| Error::CorruptRecord)?;
        if record.schema != format!("reel.tig-existing-publication.{phase}.experimental-1") {
            return Err(Error::CorruptRecord);
        }
        if record.request != *request {
            return Err(Error::OperationConflict);
        }
        Ok(true)
    }
    fn record(&self, request: &Request, phase: &str) -> Result<()> {
        if self.existing_record(request, phase)? {
            return Ok(());
        }
        fs::create_dir_all(&self.metadata)?;
        let record = Record {
            schema: format!("reel.tig-existing-publication.{phase}.experimental-1"),
            request: request.clone(),
        };
        let bytes = identity::canonical_json(
            &serde_json::to_value(record).map_err(|_| Error::InvalidRequest)?,
        )
        .map_err(|_| Error::InvalidRequest)?;
        let mut stage = tempfile::NamedTempFile::new_in(&self.metadata)?;
        stage.write_all(&bytes)?;
        stage.as_file().sync_all()?;
        match stage.persist_noclobber(self.path(request, phase)) {
            Ok(_) => (),
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.error.into()),
        }
        if !self.existing_record(request, phase)? {
            return Err(Error::CorruptRecord);
        }
        Ok(())
    }
}
impl PublicationBackend for ExistingPublisher {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            max_object_bytes: self.max_object_bytes,
            max_ack: AckLevel::ProcessRestart,
        }
    }
    fn publication_guard(&mut self, object: &Object) -> Result<Box<dyn PublicationGuard>> {
        if object.bytes > self.max_object_bytes {
            return Err(Error::Budget);
        }
        let lock =
            admission::lock(&self.fences.join(object.sha256.as_str())).map_err(|_| Error::Busy)?;
        Ok(Box::new(Guard { _lock: lock }))
    }
    fn protect(&mut self, request: &Request) -> Result<State> {
        self.capabilities().check(request)?;
        let completed = self.existing_record(request, "complete")?;
        let intended = self.existing_record(request, "intent")?;
        if completed && !intended {
            return Err(Error::CorruptRecord);
        }
        if !intended {
            self.record(request, "intent")?;
        }
        Ok(if completed {
            State::Committed
        } else {
            State::Protected
        })
    }
    fn verified_object(&mut self, object: &Object) -> Result<bool> {
        self.held = None;
        match self.reader.open_verified(object) {
            Ok(reader) => {
                self.held = Some(reader);
                Ok(true)
            }
            Err(existing::Error::Missing) => Ok(false),
            Err(error) => Err(read_error(error)),
        }
    }
    fn publish_object(&mut self, request: &Request, source: &mut dyn Read) -> Result<()> {
        self.capabilities().check(request)?;
        if !self.existing_record(request, "intent")? {
            return Err(Error::InvalidRequest);
        }
        let _capacity = admission::reserve_on(
            &self.root,
            &self.storage,
            &self.destination,
            request.object.bytes,
        )
        .map_err(provider_error)?;
        reel_assets::recover_from_reader(
            &self.root,
            source,
            &self.canonical,
            request.object.sha256.as_str(),
            request.object.bytes,
        )
        .map_err(provider_error)?;
        Ok(())
    }
    fn commit(&mut self, request: &Request) -> Result<()> {
        if !self.existing_record(request, "intent")?
            || self
                .held
                .as_ref()
                .is_none_or(|held| held.object() != &request.object)
        {
            return Err(Error::Integrity);
        }
        self.record(request, "complete")
    }
}
