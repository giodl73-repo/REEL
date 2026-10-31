//! Existing shared capacity admission for owned, bounded coordinator preparation.
use super::*;
use reel_assets::admission::{self, Reservation};

#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ContextMode {
    #[default]
    Declared,
    Compiled,
}
impl ContextMode {
    pub(super) fn is_declared(&self) -> bool {
        *self == Self::Declared
    }
}

fn peak(unit: u64, passes: u64) -> Result<u64> {
    if unit == 0 || passes == 0 {
        bail!("invalid preparation geometry or pass count");
    }
    // Data rounding plus a per-entry margin; configured free-space policy still
    // covers administrative filesystem overhead and concurrent external writers.
    crate::scene_delivery_compile::PREPARATION_MAX_FILES
        .checked_add(8)
        .and_then(|n| n.checked_mul(unit))
        .and_then(|n| n.checked_mul(2))
        .and_then(|n| n.checked_add(crate::scene_delivery_compile::PREPARATION_MAX_BYTES))
        .and_then(|n| n.checked_mul(passes))
        .and_then(|n| n.checked_add(1024 * 1024))
        .ok_or_else(|| anyhow::anyhow!("preparation allocation allowance overflow"))
}
pub(crate) fn metadata_allowance(root: &Path, database_bytes: u64, bootstrap: u64) -> Result<u64> {
    let unit = admission::allocation_unit(root)
        .map_err(|_| anyhow::anyhow!("metadata allocation geometry unavailable"))?;
    unit.checked_mul(16)
        .and_then(|margin| database_bytes.checked_mul(2)?.checked_add(margin))
        .and_then(|bytes| bytes.checked_add(bootstrap))
        .ok_or_else(|| anyhow::anyhow!("metadata allocation allowance overflow"))
}
pub(super) fn worker_allowance(profile: &Profile) -> Result<u64> {
    if profile.worker.context_mode == ContextMode::Declared {
        return Ok(0);
    }
    profile
        .cache
        .cache_roots
        .iter()
        .map(|root| {
            peak(
                admission::allocation_unit(root)
                    .map_err(|e| anyhow::anyhow!("preparation geometry unavailable: {e}"))?,
                3,
            )
        })
        .try_fold(0, |maximum, n| n.map(|n| maximum.max(n)))
}

pub(crate) struct Preparation {
    directory: Option<tempfile::TempDir>,
    reservation: Option<Reservation>,
}
impl Preparation {
    fn cache(profile: &Profile) -> Result<&Path> {
        let root = profile.preparation_cache.as_deref().ok_or_else(|| {
            anyhow::anyhow!("compiled context requires explicit preparation cache")
        })?;
        if !root.is_absolute() || !profile.cache.cache_roots.iter().any(|p| p == root) {
            bail!("preparation cache must be an explicitly configured active root");
        }
        Ok(root)
    }
    pub(super) fn open(profile: &Profile) -> Result<Option<Self>> {
        if profile.worker.context_mode == ContextMode::Declared {
            if profile.preparation_cache.is_some() {
                bail!("legacy context cannot select compiled preparation cache");
            }
            return Ok(None);
        }
        let root = Self::cache(profile)?;
        Self::open_on(&profile.cache, &profile.journal_root, root, 2).map(Some)
    }
    pub(crate) fn open_on(
        config: &Config,
        namespace: &Path,
        root: &Path,
        passes: u64,
    ) -> Result<Self> {
        if !root.is_absolute() || !config.cache_roots.iter().any(|p| p == root) {
            bail!("preparation cache must be an explicitly configured active root");
        }
        let bytes = peak(
            admission::allocation_unit(root)
                .map_err(|e| anyhow::anyhow!("preparation geometry unavailable: {e}"))?,
            passes,
        )?;
        let reservation = admission::reserve_on(namespace, config, root, bytes)
            .map_err(|e| anyhow::anyhow!("compiled preparation not admitted: {e}"))?;
        let prefix = format!("native-prepare-{}-", reservation.id);
        Self::from_reservation(reservation, root, &prefix)
    }
    pub(super) fn from_reservation(
        reservation: Reservation,
        parent: &Path,
        prefix: &str,
    ) -> Result<Self> {
        let volume = |path: &Path| {
            admission::volume_id(path)
                .map_err(|_| anyhow::anyhow!("owned directory volume unavailable"))
        };
        if !parent.is_absolute()
            || !parent.is_dir()
            || volume(parent)? != volume(&reservation.cache_root)?
        {
            bail!("owned directory must use the reserved volume");
        }
        let directory = tempfile::Builder::new().prefix(prefix).tempdir_in(parent)?;
        let prepared = Self {
            directory: Some(directory),
            reservation: Some(reservation),
        };
        Ok(prepared)
    }
    /// Call immediately after publishing the independently read-back bindings.
    /// No fallible operation may intervene between publication and retention.
    pub(super) fn keep(mut self) {
        let _ = self.directory.take().expect("live preparation").keep();
        self.reservation.take();
    }
    pub(crate) fn path(&self) -> &Path {
        self.directory.as_ref().expect("live preparation").path()
    }
    pub(crate) fn finish(mut self) -> Result<()> {
        if let Some(directory) = self.directory.take() {
            if let Err(error) = directory.close() {
                // End this writer but retain conservative capacity for explicit
                // recovery; never silently discard failed-cleanup accounting.
                if let Some(reservation) = self.reservation.take() {
                    let id = reservation.id.clone();
                    reservation.retain_for_recovery();
                    bail!(
                        "preparation cleanup failed; capacity record {id} retained for recovery: {error}"
                    );
                }
                return Err(error.into());
            }
        }
        self.reservation.take();
        Ok(())
    }
}
pub(super) fn report(profile: &Profile) -> Result<Value> {
    let worker = worker_allowance(profile)?;
    let coordinator = if profile.worker.context_mode == ContextMode::Compiled {
        peak(
            admission::allocation_unit(Preparation::cache(profile)?)
                .map_err(|e| anyhow::anyhow!("preparation geometry unavailable: {e}"))?,
            2,
        )?
    } else {
        if profile.preparation_cache.is_some() {
            bail!("legacy context cannot select compiled preparation cache");
        }
        0
    };
    Ok(
        json!({"schema":"reel.native-preparation-budget.experimental-1","context_mode":profile.worker.context_mode,
        "coordinator_preparation_bytes":coordinator,"minimum_worker_scratch_bytes":worker,
        "configured_worker_scratch_bytes":profile.scratch_bytes,"configured_workspace_bytes":profile.budget_bytes,
        "logical_bytes_per_pass":crate::scene_delivery_compile::PREPARATION_MAX_BYTES,
        "maximum_files_per_pass":crate::scene_delivery_compile::PREPARATION_MAX_FILES,
        "admitted":false,"creates_files":false,"physical_quota_guaranteed":false}),
    )
}
impl Drop for Preparation {
    fn drop(&mut self) {
        if let Some(directory) = self.directory.take() {
            if directory.close().is_err() {
                if let Some(reservation) = self.reservation.take() {
                    reservation.retain_for_recovery();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    fn profile(root: &Path) -> Profile {
        let cache = root.join("cache");
        fs::create_dir_all(cache.join("objects/sha256")).unwrap();
        let control = root.join("control");
        fs::create_dir(&control).unwrap();
        let metadata = root.join("metadata");
        let mut store = MetadataStore::create(
            &metadata,
            tig_store::metadata::Limits {
                nodes: 8192,
                bytes: 8 * 1024 * 1024,
                database_bytes: 16 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let checkpoint = store.finish().unwrap();
        drop(store);
        let journal = SourceJournal::create(
            &root.join("journal"),
            Key::parse("fixture").unwrap(),
            Key::parse("epoch").unwrap(),
        )
        .unwrap();
        serde_json::from_value(json!({"schema":PROFILE_SCHEMA,
            "worker":{"context_mode":"compiled","metadata_root":metadata,"metadata_minimum":checkpoint,
                "source":null,"context":{"root":Object::from_bytes(b"root"),"context":Digest::blob(b"context")},
                "request":{"schema":tig_scene::SCHEMA,"scene_id":"scene","scene_definition":"types/scene","primary_definitions":["types/primary"],"language":"es","controls":{"catalog":"catalog","episode":"episode","policy":"policy","profile":"profile","template_receipt":null},"delivery_id":"output","delivery_title":"Test"},
                "runtime":{"host":{"sha256":"a".repeat(64),"bytes":1},"ffmpeg":{"path":"C:/fake.exe","sha256":"a".repeat(64),"bytes":1},"ffprobe":{"path":"C:/fake.exe","sha256":"a".repeat(64),"bytes":1},"dependencies":{}},"outputs":{}},
            "preparation_cache":cache,"cache":{"path_policy":"configured-local-roots","cache_roots":[cache],"coordination_root":control,"capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}},
            "journal_root":root.join("journal"),"journal_minimum":journal.checkpoint(),"source_id":"source","owner":"owner","expected_revision":0,"start_operation":"start","retain_operation":"retain","budget_bytes":1073741824_u64,"scratch_bytes":536870912_u64,"timeout_seconds":120})).unwrap()
    }
    #[cfg(windows)]
    #[test]
    fn preparation_reserves_before_writes_and_cleans_only_its_owned_directory() {
        let root = tempfile::tempdir().unwrap();
        let mut profile = profile(root.path());
        let cache = profile.preparation_cache.clone().unwrap();
        let admissions = profile
            .cache
            .coordination_root
            .as_ref()
            .unwrap()
            .join("admissions");
        assert!(!admissions.exists());
        let inspected = report(&profile).unwrap();
        assert_eq!(inspected["creates_files"], false);
        assert!(!admissions.exists());
        fs::write(cache.join("unrelated"), b"keep").unwrap();
        let prepared = Preparation::open(&profile).unwrap().unwrap();
        let owned = prepared.path().to_owned();
        assert!(
            profile
                .cache
                .coordination_root
                .as_ref()
                .unwrap()
                .join("admissions")
                .read_dir()
                .unwrap()
                .any(|e| e.unwrap().path().extension().is_some_and(|e| e == "json"))
        );
        fs::write(owned.join("owned"), b"temporary").unwrap();
        prepared.finish().unwrap();
        assert!(!owned.exists());
        assert_eq!(fs::read(cache.join("unrelated")).unwrap(), b"keep");
        profile.cache.capacity.max_object_bytes = 1;
        assert!(Preparation::open(&profile).is_err());
        assert!(!cache.read_dir().unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("native-prepare-")
        }));
    }
    #[cfg(windows)]
    #[test]
    fn failed_cleanup_retains_record_until_explicit_recovery_without_leaking_lease() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().unwrap();
        let profile = profile(root.path());
        let prepared = Preparation::open(&profile).unwrap().unwrap();
        let path = prepared.path().to_owned();
        let id = prepared.reservation.as_ref().unwrap().id.clone();
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .share_mode(1)
            .open(path.join("held"))
            .unwrap();
        assert!(prepared.finish().is_err());
        let record = profile
            .cache
            .coordination_root
            .as_ref()
            .unwrap()
            .join("admissions")
            .join(format!("{id}.json"));
        assert!(record.is_file());
        drop(file);
        fs::remove_dir_all(&path).unwrap();
        admission::release_stale(&profile.journal_root, &profile.cache, &id).unwrap();
        assert!(!record.exists());
    }
    #[test]
    fn allowance_accounts_for_passes_and_allocation_units_without_overflow() {
        assert!(peak(65536, 3).unwrap() > peak(4096, 3).unwrap());
        assert!(peak(4096, 3).unwrap() > peak(4096, 2).unwrap());
        assert!(peak(0, 2).is_err());
        assert!(peak(u64::MAX, 3).is_err());
        assert!(peak(4096, u64::MAX).is_err());
        let omitted: ContextMode = Default::default();
        assert!(omitted.is_declared());
        assert!(serde_json::from_str::<ContextMode>("\"automatic\"").is_err());
    }
}
