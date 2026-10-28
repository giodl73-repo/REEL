//! On-demand capacity reservations over the configured existing caches.
use crate::{Config, Result, unique};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct AdmissionDenied(pub String);
impl std::fmt::Display for AdmissionDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "No active cache admitted this operation: {}", self.0)
    }
}
impl std::error::Error for AdmissionDenied {}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub minimum_free_bytes: u64,
    pub minimum_free_basis_points: u64,
    pub max_object_bytes: u64,
    pub tier_minimum_free_bytes: BTreeMap<PathBuf, u64>,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            minimum_free_bytes: 10 * 1024 * 1024 * 1024,
            minimum_free_basis_points: 500,
            max_object_bytes: 8 * 1024 * 1024 * 1024,
            tier_minimum_free_bytes: BTreeMap::new(),
        }
    }
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        if self.minimum_free_basis_points > 10_000 || self.max_object_bytes == 0 {
            return Err("Invalid capacity policy".into());
        }
        Ok(())
    }
    pub fn fits(&self, free: u64, total: u64, reserved: u64, budget: u64) -> bool {
        let fraction =
            (u128::from(total) * u128::from(self.minimum_free_basis_points) / 10_000) as u64;
        let reserve = self.minimum_free_bytes.max(fraction);
        free.saturating_sub(reserve).saturating_sub(reserved) >= budget
            && budget <= self.max_object_bytes
    }
}

pub fn control(root: &Path, config: &Config) -> Result<PathBuf> {
    let dir = config
        .coordination_root
        .clone()
        .unwrap_or_else(|| root.join(".local/control"));
    if !dir.is_absolute() {
        return Err("Coordination directory must be absolute".into());
    }
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub struct MetadataLock(File);

impl Drop for MetadataLock {
    fn drop(&mut self) {
        // Unix flock survives closing one descriptor when a clone (including a
        // briefly inherited pre-exec child descriptor) still references it.
        // Release ownership explicitly before closing our descriptor.
        let _ = fs2::FileExt::unlock(&self.0);
    }
}

/// Serialize only metadata in this producer worktree, never other cache consumers.
/// Registry, version and receipt writers share this domain with reclamation.
pub fn worktree_lock(root: &Path) -> Result<MetadataLock> {
    lock_for(
        &root.join(".local/metadata-control"),
        std::time::Duration::from_secs(2),
    )
}

/// Completed job outputs must survive a busy producer metadata transaction.
/// This changes only publication waiting, never job execution or capacity locks.
pub(crate) fn job_publication_lock(root: &Path) -> Result<MetadataLock> {
    lock_for_budget(
        &root.join(".local/metadata-control"),
        std::time::Duration::from_secs(30),
    )
}

/// Backward-compatible capacity ledger transaction. Never hold this guard while
/// hashing, copying, rendering, or updating a worktree registry.
fn capacity_lock(dir: &Path) -> Result<MetadataLock> {
    lock_for(dir, std::time::Duration::from_secs(2))
}

pub fn lock(dir: &Path) -> Result<MetadataLock> {
    lock_for(dir, std::time::Duration::ZERO)
}

/// Explicit bounded transaction wait. This never waits for a job's lifetime
/// reservation and never releases another process's lock.
pub fn lock_for(dir: &Path, timeout: std::time::Duration) -> Result<MetadataLock> {
    if timeout > std::time::Duration::from_secs(2) {
        return Err("Metadata transaction wait exceeds two-second bound".into());
    }
    lock_for_budget(dir, timeout)
}

fn lock_for_budget(dir: &Path, timeout: std::time::Duration) -> Result<MetadataLock> {
    fs::create_dir_all(dir)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("state.lock"))?;
    let started = std::time::Instant::now();
    loop {
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => break,
            Err(e)
                if (e.kind() == std::io::ErrorKind::WouldBlock || e.raw_os_error() == Some(33))
                    && started.elapsed() < timeout =>
            {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            Err(e) => {
                return Err(format!(
                    "Storage metadata is in use; explicit transaction wait budget exhausted: {e}"
                )
                .into());
            }
        }
    }
    Ok(MetadataLock(file))
}

#[cfg(windows)]
pub fn volume_id(path: &Path) -> Result<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
    };
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut mount = [0u16; 1024];
    let mut volume = [0u16; 1024];
    // Buffers live for both calls; Windows returns a NUL-terminated volume GUID.
    unsafe {
        if GetVolumePathNameW(path.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) == 0
            || GetVolumeNameForVolumeMountPointW(
                mount.as_ptr(),
                volume.as_mut_ptr(),
                volume.len() as u32,
            ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(String::from_utf16_lossy(
        &volume[..volume.iter().position(|c| *c == 0).unwrap_or(volume.len())],
    )
    .to_uppercase())
}

#[cfg(unix)]
pub fn volume_id(path: &Path) -> Result<String> {
    use std::os::unix::fs::MetadataExt;
    Ok(format!("unix-device-{}", fs::metadata(path)?.dev()))
}

#[derive(Serialize, Deserialize)]
struct Entry {
    id: String,
    volume_id: String,
    bytes: u64,
    cache_root: PathBuf,
}

/// Observed data-allocation unit for bounded preparation planning. This is not
/// a bound on filesystem administrative overhead or a capacity reservation.
#[cfg(windows)]
pub fn allocation_unit(path: &Path) -> Result<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceW, GetVolumePathNameW};
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut mount = [0u16; 1024];
    let (mut sectors, mut bytes, mut free, mut total) = (0, 0, 0, 0);
    // Buffers and scalar outputs remain valid for both Windows calls.
    unsafe {
        if GetVolumePathNameW(path.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) == 0
            || GetDiskFreeSpaceW(
                mount.as_ptr(),
                &mut sectors,
                &mut bytes,
                &mut free,
                &mut total,
            ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    u64::from(sectors)
        .checked_mul(u64::from(bytes))
        .filter(|n| *n > 0)
        .ok_or_else(|| "Invalid filesystem allocation unit".into())
}
#[cfg(not(windows))]
pub fn allocation_unit(_path: &Path) -> Result<u64> {
    Err("Native preparation allocation geometry unavailable on this platform".into())
}

pub struct Reservation {
    pub id: String,
    pub volume_id: String,
    pub cache_root: PathBuf,
    dir: PathBuf,
    lease: Option<File>,
    retained: bool,
}
impl Reservation {
    /// End this writer's lease but leave its conservative capacity record for
    /// explicit recovery. Does not authorize deleting any residual workspace.
    pub fn retain_for_recovery(mut self) {
        self.retained = true;
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if self.retained {
            return;
        }
        // If the short metadata lock is unavailable, retain a conservative stale
        // reservation for explicit recovery rather than pretending it expired.
        if let Ok(_lock) = capacity_lock(&self.dir) {
            self.lease.take();
            let _ = fs::remove_file(
                self.dir
                    .join("admissions")
                    .join(format!("{}.json", self.id)),
            );
            let _ = fs::remove_file(
                self.dir
                    .join("admissions")
                    .join(format!("{}.lease", self.id)),
            );
        }
    }
}

fn reserved(dir: &Path, volume: &str) -> Result<u64> {
    let mut total = 0u64;
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|x| x == "json") {
            let e: Entry = serde_json::from_slice(&fs::read(path)?)?;
            if e.volume_id == volume {
                total = total.checked_add(e.bytes).ok_or("Reservation overflow")?;
            }
        }
    }
    Ok(total)
}

fn reserve_candidates(
    root: &Path,
    config: &Config,
    candidates: &[PathBuf],
    bytes: u64,
) -> Result<Reservation> {
    config.capacity.validate()?;
    if bytes > config.capacity.max_object_bytes {
        return Err("Operation exceeds configured byte budget".into());
    }
    let dir = control(root, config)?;
    let _lock = capacity_lock(&dir)?;
    let admissions = dir.join("admissions");
    fs::create_dir_all(&admissions)?;
    let mut reasons = Vec::new();
    for cache in candidates {
        // A missing configured root is not discovered or created by admission.
        if !cache.is_dir() {
            reasons.push(format!("{} unavailable", cache.display()));
            continue;
        }
        let volume = volume_id(cache)?;
        let free = fs2::available_space(cache)?;
        let total = fs2::total_space(cache)?;
        let mut policy = config.capacity.clone();
        if let Some(minimum) = policy.tier_minimum_free_bytes.get(cache) {
            policy.minimum_free_bytes = policy.minimum_free_bytes.max(*minimum);
        }
        if !policy.fits(free, total, reserved(&admissions, &volume)?, bytes) {
            reasons.push(format!(
                "{} below configured reserve/budget",
                cache.display()
            ));
            continue;
        }
        let id = unique(Path::new(""), "job").to_string_lossy().to_string();
        let lease = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(admissions.join(format!("{id}.lease")))?;
        fs2::FileExt::try_lock_exclusive(&lease)?;
        let entry = Entry {
            id: id.clone(),
            volume_id: volume.clone(),
            bytes,
            cache_root: cache.clone(),
        };
        fs::write(
            admissions.join(format!("{id}.json")),
            serde_json::to_vec(&entry)?,
        )?;
        return Ok(Reservation {
            id,
            volume_id: volume,
            cache_root: cache.clone(),
            dir,
            lease: Some(lease),
            retained: false,
        });
    }
    Err(AdmissionDenied(reasons.join("; ")).into())
}

pub fn reserve(root: &Path, config: &Config, bytes: u64) -> Result<Reservation> {
    reserve_candidates(root, config, &config.cache_roots, bytes)
}
pub fn reserve_on(root: &Path, config: &Config, cache: &Path, bytes: u64) -> Result<Reservation> {
    if !config.cache_roots.contains(&cache.to_path_buf()) {
        return Err("Destination is not an active cache".into());
    }
    reserve_candidates(root, config, &[cache.to_path_buf()], bytes)
}
/// Explicit named archival custody, never an automatic admission fallback.
pub fn reserve_archive_on(
    root: &Path,
    config: &Config,
    cache: &Path,
    bytes: u64,
) -> Result<Reservation> {
    if !config.archive_roots.contains(&cache.to_path_buf()) {
        return Err("Destination is not a configured archive".into());
    }
    reserve_candidates(root, config, &[cache.to_path_buf()], bytes)
}

pub fn release_stale(root: &Path, config: &Config, id: &str) -> Result<()> {
    if id.is_empty() || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
        return Err("Invalid reservation ID".into());
    }
    let dir = control(root, config)?;
    let _lock = capacity_lock(&dir)?;
    let base = dir.join("admissions");
    let lease = OpenOptions::new()
        .read(true)
        .write(true)
        .open(base.join(format!("{id}.lease")))?;
    fs2::FileExt::try_lock_exclusive(&lease)
        .map_err(|_| "Reservation is still held by a live process")?;
    drop(lease);
    fs::remove_file(base.join(format!("{id}.json")))?;
    fs::remove_file(base.join(format!("{id}.lease")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_transaction_wait_is_bounded_and_never_breaks_another_lock() {
        let t = tempfile::tempdir().unwrap();
        let guard = lock(t.path()).unwrap();
        let started = std::time::Instant::now();
        assert!(lock_for(t.path(), std::time::Duration::from_millis(30)).is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert!(lock(t.path()).is_err());
        drop(guard);
        assert!(lock_for(t.path(), std::time::Duration::from_millis(30)).is_ok());
        assert!(lock_for(t.path(), std::time::Duration::from_secs(3)).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn metadata_guard_releases_ownership_even_with_a_surviving_descriptor() {
        let t = tempfile::tempdir().unwrap();
        let guard = lock(t.path()).unwrap();
        let inherited = guard.0.try_clone().unwrap();
        assert!(lock(t.path()).is_err());
        drop(guard);
        let next = lock(t.path()).unwrap();
        assert!(lock(t.path()).is_err());
        drop(next);
        drop(inherited);
        assert!(lock(t.path()).is_ok());
    }
    #[test]
    fn capacity_preserves_reserve_and_other_jobs() {
        let p = Policy {
            minimum_free_bytes: 10,
            minimum_free_basis_points: 1000,
            max_object_bytes: 200,
            tier_minimum_free_bytes: BTreeMap::new(),
        };
        assert!(p.fits(100, 100, 20, 70));
        assert!(!p.fits(100, 100, 20, 71));
        assert!(!p.fits(9, 100, 0, 1));
        assert!(!p.fits(1000, 1000, 0, 201));
    }
    #[test]
    fn shared_volume_reservation_and_live_lease_are_protected() {
        let t = tempfile::tempdir().unwrap();
        let e = t.path().join("E");
        fs::create_dir(&e).unwrap();
        let mut c = Config {
            cache_roots: vec![e.clone()],
            ..Config::default()
        };
        c.capacity.minimum_free_bytes = 0;
        c.capacity.minimum_free_basis_points = 0;
        let r = reserve(t.path(), &c, 123).unwrap();
        let dir = control(t.path(), &c).unwrap();
        assert_eq!(
            reserved(&dir.join("admissions"), &r.volume_id).unwrap(),
            123
        );
        assert!(release_stale(t.path(), &c, &r.id).is_err());
        drop(r);
        assert_eq!(
            reserved(&dir.join("admissions"), &volume_id(&e).unwrap()).unwrap(),
            0
        );
    }
}
