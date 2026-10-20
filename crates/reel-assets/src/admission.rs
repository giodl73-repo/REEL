//! On-demand capacity reservations over the configured existing caches.
use crate::{Config, Result, unique};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

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

pub fn lock(dir: &Path) -> Result<File> {
    fs::create_dir_all(dir)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("state.lock"))?;
    fs2::FileExt::try_lock_exclusive(&file)
        .map_err(|e| format!("Storage metadata is in use; no automatic retry: {e}"))?;
    Ok(file)
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

pub struct Reservation {
    pub id: String,
    pub volume_id: String,
    pub cache_root: PathBuf,
    dir: PathBuf,
    lease: Option<File>,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        // If the short metadata lock is unavailable, retain a conservative stale
        // reservation for explicit recovery rather than pretending it expired.
        if let Ok(_lock) = lock(&self.dir) {
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
    let _lock = lock(&dir)?;
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
        });
    }
    Err(format!(
        "No active cache admitted this operation: {}",
        reasons.join("; ")
    )
    .into())
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

pub fn release_stale(root: &Path, config: &Config, id: &str) -> Result<()> {
    if id.is_empty() || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
        return Err("Invalid reservation ID".into());
    }
    let dir = control(root, config)?;
    let _lock = lock(&dir)?;
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
