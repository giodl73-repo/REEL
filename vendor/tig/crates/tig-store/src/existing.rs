//! Explicit existing-layout byte reads. No discovery, writes, copies or deletion.
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};
use tig_core::{Digest, Object};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layout {
    ReelCas,
    GitLfs,
    Flat,
}
impl Layout {
    pub fn components(&self, oid: &Digest) -> Vec<String> {
        let s = oid.as_str();
        match self {
            Self::ReelCas => vec!["objects".into(), "sha256".into(), s[..2].into()],
            Self::GitLfs => vec!["objects".into(), s[..2].into(), s[2..4].into()],
            Self::Flat => vec!["objects".into()],
        }
    }
    pub fn object_path(&self, root: &Path, oid: &Digest) -> PathBuf {
        let mut path = root.to_owned();
        for part in self.components(oid) {
            path.push(part);
        }
        path.join(oid.as_str())
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    pub root: PathBuf,
    pub layout: Layout,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RequiredProtection {
    PreventMutation,
    CooperativeImmutable,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Protection {
    WindowsSharing,
    CooperativeFileLock,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub object_bytes: u64,
    pub total_bytes: u64,
    pub requests: u32,
    pub verify_millis: u64,
    pub live_readers: u32,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            object_bytes: 8 * 1024 * 1024 * 1024,
            total_bytes: 32 * 1024 * 1024 * 1024,
            requests: 1024,
            verify_millis: 30_000,
            live_readers: 128,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub places: Vec<Place>,
    pub required: RequiredProtection,
    #[serde(default)]
    pub limits: Limits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Budget,
    Missing,
    Integrity,
    Denied,
    Busy,
    Io,
    Unsupported,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

/// Provider port independent of typed assets. Identity is verified before return.
pub trait ByteSource {
    type Reader: Read + Seek;
    fn open_verified(&mut self, object: &Object) -> Result<Self::Reader>;
}

pub struct ReadCache {
    profile: Profile,
    requests: u32,
    verified_bytes: u64,
    active: Arc<AtomicU32>,
}
impl ReadCache {
    pub fn new(profile: Profile) -> Result<Self> {
        let l = profile.limits;
        if profile.places.is_empty()
            || profile.places.len() > 16
            || l.object_bytes == 0
            || l.object_bytes > 64 * 1024 * 1024 * 1024
            || l.total_bytes == 0
            || l.total_bytes > 1024 * 1024 * 1024 * 1024
            || l.requests == 0
            || l.requests > 65_536
            || l.verify_millis == 0
            || l.verify_millis > 300_000
            || l.live_readers == 0
            || l.live_readers > 1024
        {
            return Err(Error::Invalid);
        }
        for place in &profile.places {
            root_spelling(&place.root)?;
        }
        Ok(Self {
            profile,
            requests: 0,
            verified_bytes: 0,
            active: Arc::new(AtomicU32::new(0)),
        })
    }
    pub fn verification_bytes(&self) -> u64 {
        self.verified_bytes
    }
    pub fn requests(&self) -> u32 {
        self.requests
    }
    pub fn active_readers(&self) -> u32 {
        self.active.load(Ordering::Acquire)
    }
}
impl ByteSource for ReadCache {
    type Reader = VerifiedRead;
    fn open_verified(&mut self, object: &Object) -> Result<VerifiedRead> {
        if !cfg!(windows) && self.profile.required == RequiredProtection::PreventMutation {
            return Err(Error::Unsupported);
        }
        if self.requests == self.profile.limits.requests
            || object.bytes > self.profile.limits.object_bytes
            || self
                .verified_bytes
                .checked_add(object.bytes)
                .is_none_or(|n| n > self.profile.limits.total_bytes)
        {
            return Err(Error::Budget);
        }
        self.requests += 1;
        let mut active = self.active.load(Ordering::Acquire);
        loop {
            if active >= self.profile.limits.live_readers {
                return Err(Error::Budget);
            }
            match self.active.compare_exchange(
                active,
                active + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(current) => active = current,
            }
        }
        let permit = Permit(self.active.clone());
        let strict = self.profile.required == RequiredProtection::PreventMutation;
        for (index, place) in self.profile.places.iter().enumerate() {
            let path = place.layout.object_path(&place.root, &object.sha256);
            let Some(directories) = directories(&path, strict)? else {
                continue;
            };
            let before = match fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(map_io(e)),
            };
            regular(&before)?;
            if before.len() != object.bytes {
                return Err(Error::Integrity);
            }
            let mut file = open_file(&path, strict)?;
            regular(&file.metadata().map_err(map_io)?)?;
            FileExt::try_lock_shared(&file).map_err(|e| {
                if e.raw_os_error() == fs2::lock_contended_error().raw_os_error() {
                    Error::Busy
                } else {
                    map_io(e)
                }
            })?;
            // Charge admitted bytes before IO, including failed verification.
            self.verified_bytes += object.bytes;
            verify(&mut file, object, self.profile.limits.verify_millis)?;
            let after = fs::symlink_metadata(&path).map_err(map_io)?;
            regular(&after)?;
            if after.len() != object.bytes {
                return Err(Error::Integrity);
            }
            return Ok(VerifiedRead {
                file,
                _directories: directories,
                path,
                object: object.clone(),
                place: index,
                protection: if cfg!(windows) && strict {
                    Protection::WindowsSharing
                } else {
                    Protection::CooperativeFileLock
                },
                _permit: permit,
            });
        }
        Err(Error::Missing)
    }
}

/// Holds the file and effective read protection until dropped; no custody claim.
pub struct VerifiedRead {
    file: File,
    _directories: Vec<File>,
    path: PathBuf,
    object: Object,
    place: usize,
    protection: Protection,
    _permit: Permit,
}
struct Permit(Arc<AtomicU32>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
impl VerifiedRead {
    pub fn object(&self) -> &Object {
        &self.object
    }
    pub fn place_index(&self) -> usize {
        self.place
    }
    pub fn protection(&self) -> Protection {
        self.protection
    }
    /// Keep this session alive while the callback's subprocess reads the path.
    /// Returning a cloned path does not extend its protection beyond this guard.
    pub fn with_path<T>(&self, consume: impl FnOnce(&Path) -> T) -> Result<T> {
        if self.protection != Protection::WindowsSharing {
            return Err(Error::Unsupported);
        }
        Ok(consume(&self.path))
    }
}
impl Read for VerifiedRead {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.file.read(b)
    }
}
impl Seek for VerifiedRead {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        self.file.seek(p)
    }
}

fn verify(file: &mut File, object: &Object, millis: u64) -> Result<()> {
    if file.metadata().map_err(map_io)?.len() != object.bytes {
        return Err(Error::Integrity);
    }
    let started = Instant::now();
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = vec![0; 65_536];
    loop {
        if started.elapsed() > Duration::from_millis(millis) {
            return Err(Error::Budget);
        }
        let n = file.read(&mut buffer).map_err(map_io)?;
        if n == 0 {
            break;
        }
        count = count
            .checked_add(n as u64)
            .filter(|n| *n <= object.bytes)
            .ok_or(Error::Integrity)?;
        hash.update(&buffer[..n]);
    }
    if started.elapsed() > Duration::from_millis(millis) {
        return Err(Error::Budget);
    }
    if count != object.bytes
        || Digest::from_sha256(hash.finalize().into()) != object.sha256
        || file.metadata().map_err(map_io)?.len() != object.bytes
    {
        return Err(Error::Integrity);
    }
    file.seek(SeekFrom::Start(0)).map_err(map_io)?;
    Ok(())
}

fn root_spelling(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path.as_os_str().len() > 4096
        || path.components().count() > 64
        || path
            .components()
            .any(|c| matches!(c, Component::CurDir | Component::ParentDir))
    {
        return Err(Error::Invalid);
    }
    #[cfg(windows)]
    if !matches!(path.components().next(),Some(Component::Prefix(p)) if matches!(p.kind(),std::path::Prefix::Disk(_)|std::path::Prefix::VerbatimDisk(_)))
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn regular(meta: &fs::Metadata) -> Result<()> {
    if !meta.is_file() || meta.file_type().is_symlink() || reparse(meta) {
        Err(Error::Integrity)
    } else {
        Ok(())
    }
}
fn reparse(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = meta;
        false
    }
}
fn directories(path: &Path, strict: bool) -> Result<Option<Vec<File>>> {
    #[cfg(not(windows))]
    let _ = strict;
    #[cfg(windows)]
    let mut held = Vec::new();
    #[cfg(not(windows))]
    let held = Vec::new();
    let mut cursor = PathBuf::new();
    for component in path.parent().ok_or(Error::Invalid)?.components() {
        cursor.push(component.as_os_str());
        if !cursor.is_absolute() {
            continue;
        }
        let meta = match fs::symlink_metadata(&cursor) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(map_io(e)),
        };
        if !meta.is_dir() || meta.file_type().is_symlink() || reparse(&meta) {
            return Err(Error::Integrity);
        }
        #[cfg(windows)]
        {
            if !strict {
                continue;
            }
            use std::os::windows::fs::OpenOptionsExt;
            // FILE_SHARE_READ|WRITE, BACKUP_SEMANTICS|OPEN_REPARSE_POINT; no DELETE.
            let guard = OpenOptions::new()
                .read(true)
                .share_mode(3)
                .custom_flags(0x0220_0000)
                .open(&cursor)
                .map_err(map_io)?;
            let opened = guard.metadata().map_err(map_io)?;
            if !opened.is_dir() || reparse(&opened) {
                return Err(Error::Integrity);
            }
            held.push(guard);
        }
    }
    Ok(Some(held))
}
fn open_file(path: &Path, strict: bool) -> Result<File> {
    #[cfg(not(windows))]
    let _ = strict;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .share_mode(if strict { 1 } else { 7 })
            .custom_flags(0x0020_0000);
    }
    options.open(path).map_err(map_io)
}
fn map_io(error: io::Error) -> Error {
    #[cfg(windows)]
    if matches!(error.raw_os_error(), Some(32 | 33)) {
        return Error::Busy;
    }
    match error.kind() {
        io::ErrorKind::PermissionDenied => Error::Denied,
        io::ErrorKind::WouldBlock => Error::Busy,
        _ => Error::Io,
    }
}
