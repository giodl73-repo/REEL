//! Explicit host-file observations and advisory space checks, not job admission.
use super::Result;
use crate::restore;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};
use tig_core::Object;

pub(super) fn default_host_budget() -> u64 {
    64 * 1024 * 1024
}
const MAX_HOST_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Requirements {
    #[serde(default)]
    host_files: Vec<HostFile>,
    storage: Option<Storage>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HostFile {
    id: String,
    object: Object,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Storage {
    minimum_available_bytes: u64,
    #[serde(default)]
    scratch_estimate_bytes: u64,
}

#[derive(Serialize)]
pub struct HostObservation {
    pub id: String,
    pub object: Object,
    pub outcome: &'static str,
    pub verified_at_preflight: bool,
    pub protection_held: bool,
}
#[derive(Serialize)]
pub struct CapacityObservation {
    pub outcome: &'static str,
    pub new_logical_destination_bytes: u64,
    pub minimum_available_bytes: u64,
    pub scratch_estimate_bytes: u64,
    pub estimated_peak_logical_bytes: u64,
    pub observed_available_bytes: Option<u64>,
    pub sufficient_for_logical_restore: Option<bool>,
    pub sufficient_for_estimated_peak: Option<bool>,
    pub capacity_admission_granted: bool,
    pub quota_enforced: bool,
}

fn valid_path(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
}
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
}
fn ancestor(root: &Path, destination: &Path) -> bool {
    let parts = |p: &Path| {
        p.components()
            .map(|c| {
                let s = c.as_os_str().to_owned();
                if cfg!(windows) {
                    s.to_ascii_lowercase()
                } else {
                    s
                }
            })
            .collect::<Vec<_>>()
    };
    parts(destination).starts_with(&parts(root))
}

fn capacity_probe(root: &Path, destination: &Path) -> std::result::Result<PathBuf, &'static str> {
    #[cfg(not(unix))]
    let _ = root;
    for candidate in destination.ancestors() {
        match std::fs::symlink_metadata(candidate) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err("destination-unavailable"),
            Ok(_) => {
                let metadata =
                    restore::ordinary(candidate).map_err(|_| "destination-unavailable")?;
                if !metadata.is_dir() {
                    return Err("destination-unavailable");
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    let declared = restore::ordinary(root).map_err(|_| "unavailable-root")?;
                    if metadata.dev() != declared.dev() {
                        return Err("different-filesystem");
                    }
                }
                // Windows mount/junction reparses are refused by ordinary parent checks.
                return Ok(candidate.to_owned());
            }
        }
    }
    Err("destination-unavailable")
}

impl Requirements {
    pub(super) fn validate(
        &self,
        bindings: &BTreeMap<String, PathBuf>,
        budget: u64,
        capacity_root: Option<&Path>,
        destination_bytes: u64,
    ) -> Result<()> {
        if self.host_files.len() > 16
            || bindings.len() > 16
            || budget > MAX_HOST_BYTES
            || bindings
                .iter()
                .any(|(id, path)| !valid_id(id) || !valid_path(path))
            || capacity_root.is_some_and(|p| !valid_path(p))
        {
            return Err("invalid host requirements or local bindings".into());
        }
        let mut ids = BTreeSet::new();
        let mut total = 0_u64;
        for file in &self.host_files {
            if !valid_id(&file.id)
                || !ids.insert(&file.id)
                || file.object.bytes > MAX_HOST_BYTES / 2
            {
                return Err("invalid or duplicate host requirement".into());
            }
            total = total
                .checked_add(file.object.bytes)
                .ok_or("host byte overflow")?;
        }
        if total > budget {
            return Err("host verification byte budget exceeded".into());
        }
        if let Some(storage) = &self.storage {
            destination_bytes
                .checked_add(storage.scratch_estimate_bytes)
                .ok_or("space estimate overflow")?;
        }
        Ok(())
    }

    pub(super) fn observe_hosts(
        &self,
        bindings: &BTreeMap<String, PathBuf>,
    ) -> Vec<HostObservation> {
        self.host_files
            .iter()
            .map(|file| {
                let outcome = match bindings.get(&file.id) {
                    None => "unbound",
                    Some(path) => match restore::check_destination(path, &file.object, false) {
                        Ok(true) => "verified",
                        Ok(false) => "missing",
                        Err(_) => "different-or-unreadable",
                    },
                };
                HostObservation {
                    id: file.id.clone(),
                    object: file.object.clone(),
                    outcome,
                    verified_at_preflight: outcome == "verified",
                    protection_held: false,
                }
            })
            .collect()
    }

    pub(super) fn observe_capacity(
        &self,
        root: Option<&Path>,
        destination: &Path,
        new_bytes: u64,
    ) -> Result<Option<CapacityObservation>> {
        let Some(storage) = self.storage.as_ref() else {
            return Ok(None);
        };
        // Prepare checked the larger all-destination estimate; new_bytes is a subset.
        let peak = new_bytes
            .checked_add(storage.scratch_estimate_bytes)
            .ok_or("space estimate overflow")?;
        let mut observation = CapacityObservation {
            outcome: "unbound",
            new_logical_destination_bytes: new_bytes,
            minimum_available_bytes: storage.minimum_available_bytes,
            scratch_estimate_bytes: storage.scratch_estimate_bytes,
            estimated_peak_logical_bytes: peak,
            observed_available_bytes: None,
            sufficient_for_logical_restore: None,
            sufficient_for_estimated_peak: None,
            capacity_admission_granted: false,
            quota_enforced: false,
        };
        let Some(root) = root else {
            return Ok(Some(observation));
        };
        if !ancestor(root, destination) {
            observation.outcome = "unrelated-root";
            return Ok(Some(observation));
        }
        if root
            .ancestors()
            .any(|p| !restore::ordinary(p).is_ok_and(|m| m.is_dir()))
        {
            observation.outcome = "unavailable-root";
            return Ok(Some(observation));
        }
        let probe = match capacity_probe(root, destination) {
            Ok(probe) => probe,
            Err(outcome) => {
                observation.outcome = outcome;
                return Ok(Some(observation));
            }
        };
        match fs2::available_space(&probe) {
            Ok(available) => {
                observation.outcome = "observed";
                observation.observed_available_bytes = Some(available);
                observation.sufficient_for_logical_restore =
                    Some(available >= new_bytes.max(storage.minimum_available_bytes));
                observation.sufficient_for_estimated_peak = Some(available >= peak);
            }
            Err(_) => observation.outcome = "probe-unavailable",
        }
        Ok(Some(observation))
    }
}
