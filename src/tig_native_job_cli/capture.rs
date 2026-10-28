//! Explicit bounded source capture. No implicit initialization or job adoption.
use super::*;
use reel_assets::admission;

const SCHEMA: &str = "reel.tig-native-capture-profile.experimental-1";
const MAX_OUTPUT: usize = 65_536;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Capture {
    schema: String,
    source: Value,
    request: Request,
    metadata_root: PathBuf,
    metadata_minimum: Checkpoint,
    metadata_cache: PathBuf,
    preparation_cache: PathBuf,
    cache: Config,
}

fn existing_absolute(path: &Path) -> Result<()> {
    if !path.is_absolute() || !path.is_dir() {
        bail!("capture requires existing absolute directories");
    }
    Ok(())
}

fn capture(profile: &Path, output: &Path) -> Result<Value> {
    let capture: Capture = read(profile, 128 * 1024, None)
        .map_err(|_| anyhow::anyhow!("invalid bounded capture profile"))?;
    if capture.schema != SCHEMA {
        bail!("unsupported capture profile");
    }
    capture.request.validate()?;
    let source_profile: crate::tig_inputs::Profile = serde_json::from_value(capture.source.clone())
        .map_err(|_| anyhow::anyhow!("invalid committed-source profile"))?;
    if source_profile.schema != crate::tig_inputs::PROFILE_SCHEMA {
        bail!("capture requires a committed-source profile");
    }
    // Explicit shared coordination, never a new fallback cache namespace.
    let coordination = capture
        .cache
        .coordination_root
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("capture requires shared coordination"))?;
    for root in [
        coordination,
        capture.metadata_root.as_path(),
        capture.metadata_cache.as_path(),
        capture.preparation_cache.as_path(),
    ] {
        existing_absolute(root)?;
    }
    if !output.is_absolute() || output.file_name().is_none() || output.symlink_metadata().is_ok() {
        bail!("capture output must be a new absolute file");
    }
    let parent = output
        .parent()
        .ok_or_else(|| anyhow::anyhow!("capture output parent required"))?;
    existing_absolute(parent)?;
    if !capture.cache.cache_roots.contains(&capture.metadata_cache)
        || !capture
            .cache
            .cache_roots
            .contains(&capture.preparation_cache)
    {
        bail!("capture roots must be explicitly configured active caches");
    }
    let volume_of = |path: &Path| {
        admission::volume_id(path)
            .map_err(|_| anyhow::anyhow!("capture volume geometry unavailable"))
    };
    let volume = volume_of(&capture.metadata_cache)?;
    if volume_of(&capture.metadata_root)? != volume || volume_of(parent)? != volume {
        bail!("metadata and capture output must share the admitted volume");
    }
    let reader = MetadataReader::open(&capture.metadata_root, &capture.metadata_minimum)?;
    let bounds = reader.limits();
    let minimum = reader.checkpoint().clone();
    drop(reader);
    // Worst-case bounded database plus rollback, page/header and small output
    // allowance. Existing bytes may be charged twice: never assume exclusive
    // ownership, sparse allocation or that logical bytes imply physical space.
    let metadata_bytes = preparation::metadata_allowance(
        &capture.metadata_cache,
        bounds.database_bytes,
        1024 * 1024,
    )?;
    let _metadata_reservation = admission::reserve_on(
        &capture.metadata_root,
        &capture.cache,
        &capture.metadata_cache,
        metadata_bytes,
    )
    .map_err(|_| anyhow::anyhow!("metadata capacity not admitted"))?;
    let preparation = preparation::Preparation::open_on(
        &capture.cache,
        &capture.metadata_root,
        &capture.preparation_cache,
        2,
    )
    .map_err(|_| anyhow::anyhow!("capture preparation not admitted"))?;
    let mut source = crate::tig_inputs::Source::open(source_profile)
        .map_err(|_| anyhow::anyhow!("committed-source capture refused"))?;
    let mut store = MetadataStore::open(&capture.metadata_root, &minimum)?;
    if store.limits() != bounds {
        bail!("metadata bounds changed before capture");
    }
    let before = preparation.path().join("capture");
    let context = tig_scene::freeze_render_context(
        &mut source,
        &capture.request,
        &before,
        &mut store,
        TreeLimits::default(),
    )
    .map_err(|_| anyhow::anyhow!("native source compilation or freeze refused"))?;
    let mut archive = json!({
        "schema": crate::tig_inputs::ARCHIVE_PROFILE_SCHEMA,
        "archive": context.archive(), "checkpoint": source.checkpoint(),
        "admitted": capture.source["admitted"],
        "byte_source": capture.source["byte_source"],
    });
    if let Some(limits) = capture.source.get("limits") {
        archive["limits"] = limits.clone();
    }
    let witness = store.finish()?;
    drop(store);
    drop(source);
    let mut reader = MetadataReader::open(&capture.metadata_root, &witness)?;
    let (_, restored) = tig_scene::restore_render_context(
        serde_json::from_value(archive.clone())?,
        &capture.request,
        context.selection(),
        &preparation.path().join("readback"),
        &mut reader,
        TreeLimits::default(),
    )
    .map_err(|_| anyhow::anyhow!("frozen context readback refused"))?;
    if restored.selection() != context.selection()
        || restored.descriptor() != context.descriptor()
        || restored.requirements() != context.requirements()
        || restored.archive() != context.archive()
    {
        bail!("frozen context differs from committed source");
    }
    for name in ["production.json", "contract.json", "job.json"] {
        if read_bytes(
            &before.join("delivery").join(name),
            crate::tig_inputs::JOB_BYTES_MAX,
            None,
        )? != read_bytes(
            &preparation.path().join("readback/delivery").join(name),
            crate::tig_inputs::JOB_BYTES_MAX,
            None,
        )? {
            bail!("capture contracts differ on readback");
        }
    }
    let fragment = json!({"context_mode":"compiled", "metadata_root":capture.metadata_root,
        "metadata_minimum":witness, "source":archive, "context":context.selection(),
        "request":capture.request});
    let bytes = serde_json::to_vec(&fragment)?;
    if bytes.len() > MAX_OUTPUT {
        bail!("capture fragment exceeds budget");
    }
    // Clean only our scratch before publishing a usable fragment. Failed cleanup
    // retains conservative preparation accounting via the existing guard.
    preparation.finish()?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(output)
        .map_err(|_| anyhow::anyhow!("capture output publication refused"))?;
    Ok(
        json!({"schema":"reel.tig-native-capture-summary.experimental-1",
        "readback_equal":true, "fragment_bytes":bytes.len(),
        "roots":context.roots().len(), "primary":context.requirements().primary().len(),
        "controls":context.requirements().controls().len(), "media_rendered":false,
        "job_adopted":false, "authorizes_eviction":false}),
    )
}

/// Explicit write action. Existing stores/coordination only, private output;
/// public diagnostics omit source identity, paths and raw compiler errors.
pub fn capture_context(profile: &Path, output: &Path) -> Result<Value> {
    capture(profile, output).map_err(|_| {
        // Only our bounded stage names are public. Lower-level provider/IO errors
        // may contain private paths; do not format their error chain.
        anyhow::anyhow!(
            "source capture refused; verify profile, roots, capacity and source contracts"
        )
    })
}
