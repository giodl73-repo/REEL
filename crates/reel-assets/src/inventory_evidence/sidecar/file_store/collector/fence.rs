//! Opt-in per-subject publication/removal coordination. This primitive is not
//! proof of custody, complete enrollment, absence of demand or removal authority.
use super::*;
use std::time::{Duration, Instant};

const NAME: &str = "asset-publication-fence.v1.lock";
const HEADER: &[u8] = b"reel.asset-publication-fence.v1\n";
const MAX_SUBJECTS: usize = 512;

/// Private session control, never a shareable receipt or removal permit.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    schema: String,
    directory: PathBuf,
    identity: String,
}
impl Binding {
    pub(crate) fn separate_from(&self, cache: &Path, staging: &Path) -> Result<()> {
        let path = self.path(cache)?;
        let directory = path.parent().ok_or("Missing bound fence directory")?;
        if directory.starts_with(staging) || staging.starts_with(directory) {
            return Err("Publication fence overlaps the staging namespace".into());
        }
        Ok(())
    }
    pub(crate) fn path(&self, cache: &Path) -> Result<PathBuf> {
        if self.schema != "reel.asset-publication-fence-binding.v1"
            || self.identity.is_empty()
            || self.identity.len() > 256
            || !self.directory.is_absolute()
        {
            return Err("Invalid persisted publication fence binding".into());
        }
        let directory = checked_directory(&self.directory)?;
        if directory == cache
            || !directory.starts_with(cache)
            || crate::admission::volume_id(&directory)? != crate::admission::volume_id(cache)?
        {
            return Err("Persisted publication fence is outside the session cache".into());
        }
        Ok(directory.join(NAME))
    }
    pub(crate) fn validate_observed(&self, cache: &Path, raw: &[u8]) -> Result<()> {
        let path = self.path(cache)?;
        if raw != HEADER
            || crate::demotion::platform::identity(&open(&path)?)?.token() != self.identity
        {
            return Err("Publication fence schema or persisted identity changed".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub enum Mode {
    /// Concurrent publishers of one subject may overlap.
    Publish,
    /// Excludes publishers/removers of the same subject while held.
    Remove,
}

/// A trusted existing metadata directory and stable, pinned control file.
/// It has no automatic serialization and holds no subject lock by itself.
pub struct Domain {
    path: PathBuf,
    anchor: File,
    _directories: Vec<File>,
}

/// Short caller-owned critical section; dropping or process exit releases it.
/// Never retain this guard across renders, media hashing/copy or whole graph
/// scans. The bounded index-node publication helper performs node I/O only.
pub struct Guard {
    file: File,
    offsets: Vec<u64>,
}

#[cfg(windows)]
fn open(path: &Path) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::*;
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?)
}
#[cfg(not(windows))]
fn open(_: &Path) -> Result<File> {
    Err("Publication fences are currently Windows-only; no global-lock fallback".into())
}

fn header(file: &mut File) -> Result<()> {
    if file.metadata()?.len() != HEADER.len() as u64 {
        return Err("Invalid publication fence control size".into());
    }
    file.seek(SeekFrom::Start(0))?;
    if read_limited(file, HEADER.len() as u64)? != HEADER {
        return Err("Invalid publication fence control schema".into());
    }
    Ok(())
}

impl Domain {
    pub(crate) fn belongs_to(&self, cache: &Path) -> bool {
        self.path.starts_with(cache)
    }
    pub(crate) fn binding(&self, cache: &Path) -> Result<Binding> {
        if !self.belongs_to(cache) || self.path.parent() == Some(cache) {
            return Err("Publication domain must use a metadata child of this cache".into());
        }
        let binding = Binding {
            schema: "reel.asset-publication-fence-binding.v1".into(),
            directory: self.path.parent().ok_or("Missing fence parent")?.to_owned(),
            identity: crate::demotion::platform::identity(&self.anchor)?.token(),
        };
        binding.path(cache)?;
        Ok(binding)
    }
    pub(crate) fn from_binding(cache: &Path, binding: &Binding) -> Result<Self> {
        let path = binding.path(cache)?;
        let domain = Self::open(path.parent().ok_or("Missing bound fence directory")?)?;
        if crate::demotion::platform::identity(&domain.anchor)?.token() != binding.identity {
            return Err("Persisted publication fence identity changed".into());
        }
        Ok(domain)
    }
    pub(crate) fn same_domain(&self, other: &Self) -> Result<bool> {
        Ok(self.path == other.path
            && crate::demotion::platform::identity(&self.anchor)?
                .same_file(&crate::demotion::platform::identity(&other.anchor)?))
    }
    /// Explicit initialization of one compact control in an existing trusted
    /// directory. Never creates directories, repairs or overwrites controls.
    pub fn initialize(directory: &Path) -> Result<Self> {
        if !cfg!(windows) {
            return Err("Publication fences are currently Windows-only".into());
        }
        let directory = checked_directory(directory)?;
        let path = directory.join(NAME);
        let _directories = crate::demotion::platform::guard_dirs(&path)?;
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(HEADER)?;
                file.sync_all()?;
                sync_directory(&directory)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
        Self::open(&directory)
    }

    /// Existing control only. A partial/corrupt control needs explicit external
    /// reconciliation, never automatic reset of an active lock namespace.
    pub fn open(directory: &Path) -> Result<Self> {
        let path = checked_directory(directory)?.join(NAME);
        let directories = crate::demotion::platform::guard_dirs(&path)?;
        plain(&path, false)?;
        let mut anchor = open(&path)?;
        crate::demotion::platform::identity(&anchor)?;
        header(&mut anchor)?;
        Ok(Self {
            path,
            anchor,
            _directories: directories,
        })
    }

    /// Acquire a bounded batch in deterministic offset order. Hash-prefix
    /// collisions only add contention, never omit a lock. No control file grows.
    /// All participants must use this exact domain, identity and interpretation.
    pub fn acquire(&self, subjects: &[String], mode: Mode, wait: Duration) -> Result<Guard> {
        if subjects.is_empty() || subjects.len() > MAX_SUBJECTS || wait > Duration::from_secs(2) {
            return Err("Invalid publication fence scope/wait budget".into());
        }
        let mut offsets = BTreeSet::new();
        let mut unique = BTreeSet::new();
        for subject in subjects {
            if !valid_oid(subject) || !unique.insert(subject) {
                return Err("Invalid or duplicate publication fence subject".into());
            }
            // Lock bytes beyond the fixed header without extending the file.
            // 62-bit collision domains conservatively share the same byte.
            offsets.insert(4096 + (u64::from_str_radix(&subject[..16], 16)? & ((1u64 << 62) - 1)));
        }
        plain(&self.path, false)?;
        let mut file = open(&self.path)?;
        if !crate::demotion::platform::identity(&self.anchor)?
            .same_file(&crate::demotion::platform::identity(&file)?)
        {
            return Err("Publication fence identity changed".into());
        }
        header(&mut file)?;
        let mut guard = Guard {
            file,
            offsets: Vec::new(),
        };
        let started = Instant::now();
        for offset in offsets {
            loop {
                match range_lock(&guard.file, offset, mode) {
                    Ok(()) => {
                        guard.offsets.push(offset);
                        break;
                    }
                    Err(e) if busy(&e) && started.elapsed() < wait => {
                        std::thread::sleep(
                            Duration::from_millis(5).min(wait.saturating_sub(started.elapsed())),
                        );
                    }
                    Err(e) => {
                        return Err(format!(
                            "Publication fence unavailable within wait budget: {e}"
                        )
                        .into());
                    }
                }
            }
        }
        Ok(guard)
    }
}

fn busy(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::WouldBlock || error.raw_os_error() == Some(33)
}

#[cfg(windows)]
fn overlap(offset: u64) -> windows_sys::Win32::System::IO::OVERLAPPED {
    let mut value: windows_sys::Win32::System::IO::OVERLAPPED = unsafe { std::mem::zeroed() };
    value.Anonymous.Anonymous.Offset = offset as u32;
    value.Anonymous.Anonymous.OffsetHigh = (offset >> 32) as u32;
    value
}
#[cfg(windows)]
fn range_lock(file: &File, offset: u64, mode: Mode) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::*;
    let mut value = overlap(offset);
    let flags = LOCKFILE_FAIL_IMMEDIATELY
        | match mode {
            Mode::Publish => 0,
            Mode::Remove => LOCKFILE_EXCLUSIVE_LOCK,
        };
    if unsafe { LockFileEx(file.as_raw_handle(), flags, 0, 1, 0, &mut value) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(not(windows))]
fn range_lock(_: &File, _: u64, _: Mode) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Publication fences unsupported",
    ))
}
impl Drop for Guard {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            for offset in self.offsets.iter().rev() {
                let mut value = overlap(*offset);
                unsafe {
                    windows_sys::Win32::Storage::FileSystem::UnlockFileEx(
                        self.file.as_raw_handle(),
                        0,
                        1,
                        0,
                        &mut value,
                    );
                }
            }
        }
    }
}
