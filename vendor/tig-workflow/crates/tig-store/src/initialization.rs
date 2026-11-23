//! Cooperative owned-root initialization namespace, shared by native adapters.
//! No provider discovery, payload operation, cleanup or power-loss guarantee.
use crate::{local, local_enrollment::Error};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tig_core::Digest;
type Result<T> = std::result::Result<T, Error>;
pub fn present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
pub fn directory(path: &Path) -> Result<()> {
    local::directory(path).map_err(|_| Error::Conflict)
}
pub fn regular(path: &Path) -> Result<()> {
    local::regular(path).map_err(|_| Error::Conflict)
}
pub fn read_record<T: serde::de::DeserializeOwned + serde::Serialize>(path: &Path) -> Result<T> {
    local::read_record(path).map_err(|_| Error::PendingInitialization)
}
pub fn canonical_root(root: &Path) -> Result<PathBuf> {
    local::check_root_spelling(root).map_err(|_| Error::Invalid)?;
    let leaf = root
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty() && s.len() <= 64)
        .ok_or(Error::Invalid)?;
    let parent = root.parent().ok_or(Error::Invalid)?;
    directory(parent)?;
    Ok(fs::canonicalize(parent)?.join(leaf))
}
pub struct Namespace {
    root: PathBuf,
    stage: PathBuf,
    lock: PathBuf,
    intent: Digest,
}
pub struct Fence(File);
impl Drop for Fence {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}
impl Namespace {
    pub fn new(root: &Path, intent: Digest) -> Result<Self> {
        let root = canonical_root(root)?;
        let leaf = root
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or(Error::Invalid)?;
        let parent = root.parent().ok_or(Error::Invalid)?;
        let stage = parent.join(format!(".{leaf}.tig-stage-{}", intent.as_str()));
        let lock = parent.join(format!(".{leaf}.tig-init.lock"));
        Ok(Self {
            root,
            stage,
            lock,
            intent,
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn stage(&self) -> &Path {
        &self.stage
    }
    pub fn marker(&self) -> &Path {
        &self.lock
    }
    pub fn fence(&self) -> Result<Fence> {
        self.fence_hook(|_| {})
    }
    pub(crate) fn fence_hook(&self, opened: impl FnOnce(bool)) -> Result<Fence> {
        let (file, new) = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&self.lock)
        {
            Ok(file) => (file, true),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                regular(&self.lock)?;
                (
                    OpenOptions::new().read(true).write(true).open(&self.lock)?,
                    false,
                )
            }
            Err(e) => return Err(e.into()),
        };
        opened(new);
        if new {
            file.lock_exclusive().map_err(|_| Error::Busy)?;
        } else {
            file.try_lock_exclusive().map_err(|_| Error::Busy)?;
        }
        let mut guard = Fence(file);
        let expected = format!("{}\n", self.intent.as_str());
        if new {
            if present(&self.stage)? || present(&self.root)? {
                return Err(Error::Conflict);
            }
            guard.0.write_all(expected.as_bytes())?;
            guard.0.sync_all()?;
        } else {
            let mut bytes = Vec::new();
            (&guard.0).take(66).read_to_end(&mut bytes)?;
            if bytes.len() != 65 {
                return Err(Error::PendingInitialization);
            }
            if bytes != expected.as_bytes() {
                return Err(Error::Conflict);
            }
        }
        Ok(guard)
    }
    /// Caller retains this namespace's fence through setup and publication.
    pub fn publish(&self) -> Result<()> {
        if present(&self.root)? {
            return Err(Error::Conflict);
        }
        directory(&self.stage)?;
        let stage = fs::canonicalize(&self.stage)?;
        if stage.parent() != self.root.parent() {
            return Err(Error::Conflict);
        }
        fs::rename(stage, &self.root)?;
        Ok(())
    }
}
