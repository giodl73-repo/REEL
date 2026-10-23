use super::*;
use crate::{Config, admission};
const SCHEMA: &str = "reel.inventory-sidecar.capacity.v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    coordination: PathBuf,
    budget: u64,
    overhead: u64,
}

pub(super) fn required(store: &FileStore) -> Result<bool> {
    match fs::symlink_metadata(store.session.join("capacity.json")) {
        Ok(_) => {
            let record: Record = control(&store.session.join("capacity.json"))?;
            if record.schema != SCHEMA
                || record.budget != FileStore::capacity_budget(store.limits, record.overhead)?
            {
                return Err("Invalid persisted sidecar capacity contract".into());
            }
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn coordination(config: &Config) -> Result<PathBuf> {
    checked_directory(
        config
            .coordination_root
            .as_ref()
            .ok_or("Sidecar admission requires an explicit shared coordination directory")?,
    )
}
impl FileStore {
    /// Conservative logical payload + caller-declared filesystem/retained-link
    /// overhead + bounded controls. 4 KiB/node is a floor, not a claim about
    /// actual allocation; projects with larger clusters must budget more.
    pub fn capacity_budget(limits: Limits, overhead: u64) -> Result<u64> {
        check_limits(limits)?;
        let minimum = (limits.nodes as u64)
            .checked_mul(4096)
            .ok_or("Sidecar overhead overflow")?;
        if overhead < minimum {
            return Err("Sidecar overhead allowance must cover at least 4 KiB per node".into());
        }
        limits
            .staged_bytes
            .checked_add(overhead)
            .and_then(|v| v.checked_add(8 * CONTROL_BYTES))
            .ok_or_else(|| "Sidecar capacity budget overflow".into())
    }
    /// Reserve in the caller's shared existing ledger before creating staging.
    /// The ledger lock is released before any node hashing/copying takes place.
    pub fn begin_admitted(
        root: &Path,
        config: &Config,
        cache: &Path,
        staging_parent: &Path,
        id: &str,
        limits: Limits,
        overhead: u64,
    ) -> Result<Self> {
        Self::admit_new(root, config, cache, limits, overhead, || {
            Self::begin(cache, staging_parent, id, limits)
        })
    }
    /// The existing capacity contract plus persisted fence enrollment. Domain
    /// scope is validated before shared-ledger reservation or session creation.
    #[allow(clippy::too_many_arguments)] // existing explicit admission scope plus required domain
    pub fn begin_admitted_fenced(
        root: &Path,
        config: &Config,
        cache: &Path,
        staging_parent: &Path,
        id: &str,
        limits: Limits,
        overhead: u64,
        domain: &collector::fence::Domain,
    ) -> Result<Self> {
        domain.binding(&checked_directory(cache)?)?;
        Self::admit_new(root, config, cache, limits, overhead, || {
            Self::begin_fenced(cache, staging_parent, id, limits, domain)
        })
    }
    /// Capacity-admitted source demand publication before original evidence I/O.
    #[allow(clippy::too_many_arguments)] // existing explicit admission scope plus domain/source
    pub fn begin_admitted_fenced_source(
        root: &Path,
        config: &Config,
        cache: &Path,
        staging_parent: &Path,
        id: &str,
        limits: Limits,
        overhead: u64,
        domain: &collector::fence::Domain,
        source: &Source,
    ) -> Result<Self> {
        domain.binding(&checked_directory(cache)?)?;
        catalog::source_from_key(&source_key(source))?;
        Self::admit_new(root, config, cache, limits, overhead, || {
            Self::begin_protocol(
                cache,
                staging_parent,
                id,
                limits,
                Some(domain),
                Some(source),
                true,
            )
        })
    }
    fn admit_new(
        root: &Path,
        config: &Config,
        cache: &Path,
        limits: Limits,
        overhead: u64,
        begin: impl FnOnce() -> Result<Self>,
    ) -> Result<Self> {
        let coord = coordination(config)?;
        let budget = Self::capacity_budget(limits, overhead)?;
        let reservation = admission::reserve_on(root, config, cache, budget)?;
        let mut store = begin()?;
        create_control(
            &store.session.join("capacity.json"),
            &Record {
                schema: SCHEMA.into(),
                coordination: coord,
                budget,
                overhead,
            },
        )?;
        store.requires_admission = true;
        store._reservation = Some(reservation);
        Ok(store)
    }
    /// Named recovery reads the persisted allowance and reacquires capacity in
    /// the SAME shared ledger. Stale killed-process reservations remain subject
    /// to the existing explicit release_stale contract, never auto-expiration.
    pub fn resume_admitted(
        root: &Path,
        config: &Config,
        cache: &Path,
        staging_parent: &Path,
        id: &str,
    ) -> Result<Self> {
        let mut store = Self::resume(cache, staging_parent, id)?;
        if !store.requires_admission {
            return Err("Session was not created with a capacity contract".into());
        }
        let record: Record = control(&store.session.join("capacity.json"))?;
        if record.coordination != coordination(config)? {
            return Err("Sidecar capacity coordination scope mismatch".into());
        }
        store._reservation = Some(admission::reserve_on(root, config, cache, record.budget)?);
        Ok(store)
    }
}
