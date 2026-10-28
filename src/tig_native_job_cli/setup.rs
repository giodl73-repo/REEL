//! Explicit bootstrap through existing providers; private atomic bindings.
use super::*;
use reel_assets::admission;
use tig_store::metadata::Limits;

const SCHEMA: &str = "reel.tig-job-store-setup.experimental-1";
const BINDINGS: &str = "reel.tig-job-store-bindings.experimental-1";
const BOOTSTRAP_ALLOWANCE: u64 = 2 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Setup {
    schema: String,
    state_parent: PathBuf,
    capacity_cache: PathBuf,
    cache: Config,
    metadata_limits: Limits,
    journal_project: Key,
    journal_epoch: Key,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bindings {
    schema: String,
    metadata_root: PathBuf,
    metadata_minimum: Checkpoint,
    journal_root: PathBuf,
    journal_minimum: source_claims::Checkpoint,
}
fn directory(path: &Path) -> Result<()> {
    if !path.is_absolute() || !fs::symlink_metadata(path)?.is_dir() {
        bail!("existing absolute directory required");
    }
    Ok(())
}
fn volume(path: &Path) -> Result<String> {
    admission::volume_id(path).map_err(|_| anyhow::anyhow!("setup volume unavailable"))
}
fn inspect(bindings: &Bindings) -> Result<Value> {
    if bindings.schema != BINDINGS {
        bail!("unsupported store bindings");
    }
    directory(&bindings.metadata_root)?;
    directory(&bindings.journal_root)?;
    let metadata = MetadataReader::open(&bindings.metadata_root, &bindings.metadata_minimum)?;
    let journal = SourceJournal::open(&bindings.journal_root, &bindings.journal_minimum)?;
    Ok(
        json!({"schema":"reel.tig-job-store-check.experimental-1", "checked":true,
        "metadata_nodes":metadata.checkpoint().nodes(),
        "metadata_bytes":metadata.checkpoint().bytes(),
        "journal_commands":journal.checkpoint().sequence(),
        "complete_claim_coverage":false,"authorizes_eviction":false}),
    )
}
fn setup(path: &Path, output: &Path) -> Result<Value> {
    let profile: Setup = read(path, 65_536, None)?;
    if profile.schema != SCHEMA {
        bail!("unsupported setup profile");
    }
    let coordination = profile
        .cache
        .coordination_root
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("setup requires shared coordination"))?;
    for root in [
        coordination,
        profile.state_parent.as_path(),
        profile.capacity_cache.as_path(),
    ] {
        directory(root)?;
    }
    if !profile.cache.cache_roots.contains(&profile.capacity_cache) {
        bail!("setup capacity root must be an explicitly configured active cache");
    }
    if !output.is_absolute() || output.file_name().is_none() || output.symlink_metadata().is_ok() {
        bail!("setup bindings must be a new absolute file");
    }
    let parent = output
        .parent()
        .ok_or_else(|| anyhow::anyhow!("setup output parent required"))?;
    directory(parent)?;
    let capacity_volume = volume(&profile.capacity_cache)?;
    if volume(&profile.state_parent)? != capacity_volume || volume(parent)? != capacity_volume {
        bail!("setup state and bindings must use the admitted volume");
    }
    let budget = preparation::metadata_allowance(
        &profile.capacity_cache,
        profile.metadata_limits.database_bytes,
        BOOTSTRAP_ALLOWANCE,
    )?;
    let reservation = admission::reserve_on(
        &profile.state_parent,
        &profile.cache,
        &profile.capacity_cache,
        budget,
    )
    .map_err(|_| anyhow::anyhow!("setup capacity not admitted"))?;
    let prefix = format!("tig-state-{}-", reservation.id);
    let owned =
        preparation::Preparation::from_reservation(reservation, &profile.state_parent, &prefix)?;
    let metadata_root = owned.path().join("metadata");
    let journal_root = owned.path().join("journal");
    let mut metadata = MetadataStore::create(&metadata_root, profile.metadata_limits)?;
    let metadata_minimum = metadata.finish()?;
    drop(metadata);
    let journal = SourceJournal::create(
        &journal_root,
        profile.journal_project,
        profile.journal_epoch,
    )?;
    let journal_minimum = journal.checkpoint();
    drop(journal);
    let bindings = Bindings {
        schema: BINDINGS.into(),
        metadata_root,
        metadata_minimum,
        journal_root,
        journal_minimum,
    };
    let checked = inspect(&bindings)?;
    if checked["metadata_nodes"] != 0
        || checked["metadata_bytes"] != 0
        || checked["journal_commands"] != 0
    {
        bail!("new providers are not empty");
    }
    // Only newly created empty providers exist in this owned directory. Nothing
    // references them until the no-overwrite bindings publication below.
    let bytes = serde_json::to_vec(&bindings)?;
    if bytes.len() > 65_536 {
        bail!("setup bindings exceed budget");
    }
    let summary = json!({"schema":"reel.tig-job-store-setup-summary.experimental-1",
        "readback_equal":true,"bindings_bytes":bytes.len(),"reserved_bytes":budget,
        "metadata_nodes":0,"journal_commands":0,"job_adopted":false,
        "complete_claim_coverage":false,"authorizes_eviction":false});
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(&bytes)?;
    staged.as_file().sync_all()?;
    staged
        .persist_noclobber(output)
        .map_err(|_| anyhow::anyhow!("setup publication refused"))?;
    // No fallible work after publication: the private file now names this state.
    owned.keep();
    Ok(summary)
}

/// Explicit creation only. Caller keeps private bindings outside Git and reuses
/// them across worktrees. Does not create byte caches or install runtime bindings.
pub fn setup_stores(profile: &Path, output: &Path) -> Result<Value> {
    setup(profile, output).map_err(|_| {
        anyhow::anyhow!(
            "store setup refused; verify profile, existing roots, capacity and new output"
        )
    })
}
/// Open named existing stores with trusted minimums. No discovery or creation.
pub fn check_stores(path: &Path) -> Result<Value> {
    (|| inspect(&read(path, 65_536, None)?))().map_err(|_| {
        anyhow::anyhow!(
            "store check refused; verify private bindings, provider identity and checkpoints"
        )
    })
}
