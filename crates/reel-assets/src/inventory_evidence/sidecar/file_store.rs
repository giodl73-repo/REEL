//! Explicit existing-cache backend with optional shared capacity admission and
//! verified staged-link retirement. Drop never deletes staged/CAS/retained nodes.
use super::*;
use std::{
    fs::{self, File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
};

const SESSION_SCHEMA: &str = "reel.inventory-sidecar.session.v1";
const SEAL_SCHEMA: &str = "reel.inventory-sidecar.seal.v1";
const CONTROL_BYTES: u64 = 64 * 1024;
mod capacity;
pub mod catalog;
pub mod collector;
pub mod managed;
pub mod retirement;

/// Logical staged-byte ceiling, not a filesystem free-space reservation.
/// Low-level writers need external shared-volume admission; managed producers
/// use `begin_admitted` and `resume_admitted` to reserve in the existing ledger.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub node_bytes: u64,
    pub staged_bytes: u64,
    pub nodes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            node_bytes: 256 * 1024,
            staged_bytes: 1024 * 1024 * 1024,
            nodes: 100_000,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Session {
    schema: String,
    cache: PathBuf,
    staging_parent: PathBuf,
    id: String,
    limits: Limits,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Seal {
    schema: String,
    root: Descriptor,
}

/// Private local metadata store; no automatic serialization or path resolution.
/// Cache and staging-parent directories must already exist and be trusted local
/// storage on the same filesystem. Creates session/CAS subdirectories explicitly.
pub struct FileStore {
    cache: PathBuf,
    session: PathBuf,
    limits: Limits,
    staged: BTreeMap<String, u64>,
    staged_bytes: u64,
    sealed: Option<Blob>,
    poisoned: bool,
    _owner: File,
    requires_admission: bool,
    _reservation: Option<crate::admission::Reservation>,
}

/// Exact named reads over an explicitly supplied existing cache. No session
/// recovery, directory enumeration, writer ownership or implicit rebuild.
pub struct ReadOnlyStore {
    cache: PathBuf,
    node_bytes: u64,
}
impl ReadOnlyStore {
    pub fn new(cache: &Path, node_bytes: u64) -> Result<Self> {
        check_node_limit(node_bytes)?;
        Ok(Self {
            cache: checked_directory(cache)?,
            node_bytes,
        })
    }
}
impl Store for ReadOnlyStore {
    fn publish(&mut self, _: &[u8]) -> Result<Blob> {
        Err("Read-only sidecar store cannot publish".into())
    }
    fn open(&mut self, blob: &Blob) -> Result<Box<dyn Read + '_>> {
        Ok(Box::new(verified(
            &destination(&self.cache, blob, self.node_bytes, false)?,
            blob,
        )?))
    }
}

fn plain(path: &Path, directory: bool) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = metadata.file_type().is_symlink();
    if reparse || (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err("Sidecar storage requires regular files and non-reparse directories".into());
    }
    Ok(metadata)
}
fn checked_directory(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(
            "Sidecar storage requires explicit absolute paths without parent traversal".into(),
        );
    }
    for ancestor in path.ancestors() {
        plain(ancestor, true)?;
    }
    Ok(path.canonicalize()?)
}
fn directory(path: &Path) -> Result<()> {
    let created = match fs::create_dir(path) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => return Err(e.into()),
    };
    checked_directory(path)?;
    if created {
        sync_directory(path.parent().ok_or("Missing sidecar directory parent")?)?;
    }
    Ok(())
}
fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    // std does not provide a portable Windows directory durability barrier.
    // File data is synced, but power-loss namespace durability is not promised.
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
fn check_limits(limits: Limits) -> Result<()> {
    check_node_limit(limits.node_bytes)?;
    if limits.nodes == 0
        || limits.nodes > 1_000_000
        || limits.staged_bytes == 0
        || limits.staged_bytes > 4 * 1024 * 1024 * 1024
    {
        return Err("Invalid sidecar staging budget".into());
    }
    Ok(())
}
fn session_path(parent: &Path, id: &str) -> Result<PathBuf> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        return Err("Invalid explicit sidecar session ID".into());
    }
    Ok(parent.join(id))
}
fn control<T: serde::de::DeserializeOwned + Serialize>(path: &Path) -> Result<T> {
    if plain(path, false)?.len() > CONTROL_BYTES {
        return Err("Sidecar control byte budget exceeded".into());
    }
    decode(&read_limited(File::open(path)?, CONTROL_BYTES)?)
}
fn create_control(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = encode(value)?;
    if bytes.len() as u64 > CONTROL_BYTES {
        return Err("Sidecar control byte budget exceeded".into());
    }
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    sync_directory(path.parent().ok_or("Missing sidecar control parent")?)
}
fn verified(path: &Path, expected: &Blob) -> Result<File> {
    if plain(path, false)?.len() != expected.bytes {
        return Err("Sidecar stored byte mismatch".into());
    }
    let mut file = File::open(path)?;
    check_blob(&read_limited(&mut file, expected.bytes)?, expected)?;
    file.seek(SeekFrom::Start(0))?;
    Ok(file)
}
fn owner(path: &Path, create: bool) -> Result<File> {
    if !create {
        plain(path, false)?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    if create {
        options.create_new(true);
    }
    let file = options.open(path)?;
    fs2::FileExt::try_lock_exclusive(&file).map_err(|_| "Sidecar session already has an owner")?;
    Ok(file)
}
fn reader_owner(path: &Path) -> Result<File> {
    plain(path, false)?;
    let file = OpenOptions::new().read(true).write(true).open(path)?;
    fs2::FileExt::try_lock_shared(&file)
        .map_err(|_| "Retained root publication is in progress; retry")?;
    Ok(file)
}
fn destination(cache: &Path, blob: &Blob, node_bytes: u64, create: bool) -> Result<PathBuf> {
    if !valid_oid(&blob.sha256) || blob.bytes > node_bytes {
        return Err("Invalid sidecar blob descriptor".into());
    }
    checked_directory(cache)?;
    let mut path = cache.to_owned();
    for part in ["objects", "sha256", &blob.sha256[..2]] {
        path.push(part);
        if create {
            directory(&path)?;
        } else {
            checked_directory(&path)?;
        }
    }
    Ok(path.join(&blob.sha256))
}
impl FileStore {
    /// Explicit per-session creation only, no global lock or implicit recovery.
    /// Reserve capacity externally before beginning. No existing session reused.
    pub fn begin(cache: &Path, staging_parent: &Path, id: &str, limits: Limits) -> Result<Self> {
        check_limits(limits)?;
        let cache = checked_directory(cache)?;
        let parent = checked_directory(staging_parent)?;
        // Staging protection must stay in the explicitly chosen existing cache.
        if parent == cache || !parent.starts_with(&cache) {
            return Err("Sidecar staging parent must be inside the selected existing cache".into());
        }
        if crate::admission::volume_id(&cache)? != crate::admission::volume_id(&parent)? {
            return Err("Sidecar staging and cache must be on the same filesystem".into());
        }
        let session = session_path(&parent, id)?;
        fs::create_dir(&session)?;
        let lock = owner(&session.join("owner.lock"), true)?;
        directory(&session.join("objects"))?;
        create_control(
            &session.join("session.json"),
            &Session {
                schema: SESSION_SCHEMA.into(),
                cache: cache.clone(),
                staging_parent: parent.clone(),
                id: id.into(),
                limits,
            },
        )?;
        sync_directory(&parent)?;
        Ok(Self {
            cache,
            session,
            limits,
            staged: BTreeMap::new(),
            staged_bytes: 0,
            sealed: None,
            poisoned: false,
            _owner: lock,
            requires_admission: false,
            _reservation: None,
        })
    }
    /// Explicit named recovery, bounded by immutable session budgets. Reads and
    /// verifies staged chunks only; no archive/cache enumeration or node repair.
    pub fn resume(cache: &Path, staging_parent: &Path, id: &str) -> Result<Self> {
        let cache = checked_directory(cache)?;
        let parent = checked_directory(staging_parent)?;
        if parent == cache || !parent.starts_with(&cache) {
            return Err("Invalid sidecar staging parent".into());
        }
        if crate::admission::volume_id(&cache)? != crate::admission::volume_id(&parent)? {
            return Err("Sidecar staging and cache must be on the same filesystem".into());
        }
        let session = checked_directory(&session_path(&parent, id)?)?;
        let lock = owner(&session.join("owner.lock"), false)?;
        let intent: Session = control(&session.join("session.json"))?;
        if intent.schema != SESSION_SCHEMA
            || intent.cache != cache
            || intent.staging_parent != parent
            || intent.id != id
        {
            return Err("Sidecar session scope mismatch".into());
        }
        check_limits(intent.limits)?;
        checked_directory(&session.join("objects"))?;
        let mut store = Self {
            cache,
            session,
            limits: intent.limits,
            staged: BTreeMap::new(),
            staged_bytes: 0,
            sealed: None,
            poisoned: false,
            _owner: lock,
            requires_admission: false,
            _reservation: None,
        };
        for entry in fs::read_dir(store.session.join("objects"))? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "Invalid staged node name")?;
            let bytes = plain(&entry.path(), false)?.len();
            let blob = Blob {
                sha256: name,
                bytes,
            };
            store.admit(&blob)?;
            verified(&entry.path(), &blob)?;
            store.staged_bytes += bytes;
            store.staged.insert(blob.sha256, bytes);
        }
        match fs::symlink_metadata(store.session.join("root.json")) {
            Ok(_) => {
                let seal: Seal = control(&store.session.join("root.json"))?;
                if seal.schema != SEAL_SCHEMA {
                    return Err("Unsupported sidecar seal".into());
                }
                let root = seal.root.blob();
                if !retirement::started(&store, &root)? {
                    store.check_staged(&root)?;
                }
                store.sealed = Some(root);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        store.requires_admission = capacity::required(&store)?;
        if store.sealed.is_none() && retirement::marker_exists(&store)? {
            return Err("Retirement marker requires a sealed session".into());
        }
        Ok(store)
    }
    pub fn staging_path(&self) -> &Path {
        &self.session
    }
    pub fn staged_bytes(&self) -> u64 {
        self.staged_bytes
    }
    pub fn staged_nodes(&self) -> usize {
        self.staged.len()
    }
    pub fn sealed_root(&self) -> Option<&Blob> {
        self.sealed.as_ref()
    }
    fn admitted(&self) -> Result<()> {
        if self.requires_admission && self._reservation.is_none() {
            return Err("Session requires shared capacity admission before mutation".into());
        }
        Ok(())
    }
    fn admit(&self, blob: &Blob) -> Result<()> {
        if !valid_oid(&blob.sha256)
            || blob.bytes > self.limits.node_bytes
            || self.staged.len() >= self.limits.nodes
            || self
                .staged_bytes
                .checked_add(blob.bytes)
                .is_none_or(|n| n > self.limits.staged_bytes)
        {
            return Err("Sidecar staging budget/descriptor rejected".into());
        }
        Ok(())
    }
    fn check_staged(&self, blob: &Blob) -> Result<()> {
        if self.staged.get(&blob.sha256) != Some(&blob.bytes) {
            return Err("Root is not protected by this staging session".into());
        }
        verified(&self.session.join("objects").join(&blob.sha256), blob)?;
        Ok(())
    }
    /// Completion marker only, not catalog admission, selection or removal
    /// authority. Keeps ALL staged hard links and allows reads but no new writes.
    pub fn seal(&mut self, root: &Blob) -> Result<()> {
        self.admitted()?;
        if self.poisoned {
            return Err("Failed sidecar session requires explicit recovery".into());
        }
        self.check_staged(root)?;
        if let Some(existing) = &self.sealed {
            if existing != root {
                return Err("Sidecar session already sealed with another root".into());
            }
            return Ok(());
        }
        self.open(root)?;
        let result = create_control(
            &self.session.join("root.json"),
            &Seal {
                schema: SEAL_SCHEMA.into(),
                root: Descriptor::from(root),
            },
        );
        if result.is_err() {
            self.poisoned = true;
        }
        result?;
        self.sealed = Some(root.clone());
        Ok(())
    }
    fn put(&mut self, bytes: &[u8]) -> Result<Blob> {
        let blob = Blob {
            sha256: hash(bytes),
            bytes: bytes.len() as u64,
        };
        let destination = destination(&self.cache, &blob, self.limits.node_bytes, true)?;
        checked_directory(&self.session.join("objects"))?;
        let staged = self.session.join("objects").join(&blob.sha256);
        if self.staged.contains_key(&blob.sha256) {
            self.check_staged(&blob)?;
        } else {
            self.admit(&blob)?;
            match fs::symlink_metadata(&destination) {
                Ok(_) => {
                    verified(&destination, &blob)?;
                    fs::hard_link(&destination, &staged)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let mut file = OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(&staged)?;
                    file.write_all(bytes)?;
                    file.sync_all()?;
                }
                Err(e) => return Err(e.into()),
            }
            verified(&staged, &blob)?;
            sync_directory(&self.session.join("objects"))?;
            self.staged.insert(blob.sha256.clone(), blob.bytes);
            self.staged_bytes += blob.bytes;
        }
        // Protected complete stage precedes the only public CAS-name mutation.
        match fs::hard_link(&staged, &destination) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
        verified(&destination, &blob)?;
        sync_directory(destination.parent().ok_or("Missing CAS parent")?)?;
        Ok(blob)
    }
}
impl Store for FileStore {
    fn publish(&mut self, bytes: &[u8]) -> Result<Blob> {
        self.admitted()?;
        if self.poisoned || self.sealed.is_some() {
            return Err("Sidecar writer is failed or sealed".into());
        }
        let result = self.put(bytes);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn open(&mut self, blob: &Blob) -> Result<Box<dyn Read + '_>> {
        let destination = destination(&self.cache, blob, self.limits.node_bytes, false)?;
        Ok(Box::new(verified(&destination, blob)?))
    }
}
