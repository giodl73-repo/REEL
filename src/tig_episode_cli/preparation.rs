//! Explicit metadata preparation, never source adoption or execution admission.
use super::*;
use reel_assets::admission;

fn report_parent<'a>(p: &Profile, output: &'a Path) -> Result<&'a Path> {
    if !output.is_absolute() || output.file_name().is_none() || output.symlink_metadata().is_ok() {
        bail!("episode preparation report requires a new absolute file");
    }
    LogicalPath::parse(
        output
            .file_name()
            .and_then(|n| n.to_str())
            .context("episode preparation report name must be Unicode")?,
    )?;
    let parent = output
        .parent()
        .context("episode preparation report parent required")?;
    for ancestor in parent.ancestors() {
        let m = fs::symlink_metadata(ancestor)?;
        if !m.is_dir() || m.file_type().is_symlink() {
            bail!("episode preparation report requires ordinary parents");
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                bail!("episode preparation report cannot use reparse parents");
            }
        }
    }
    let resolved = parent.canonicalize()?;
    let roots = [&p.metadata_root, &p.journal_root]
        .into_iter()
        .cloned()
        .chain(p.cache.cache_roots.iter().map(|root| root.join("objects")));
    for root in roots {
        match root.canonicalize() {
            Ok(root) if resolved.starts_with(&root) => {
                bail!("episode preparation report cannot enter authority/object stores")
            }
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    let volume = |path: &Path| {
        admission::volume_id(path)
            .map_err(|_| anyhow::anyhow!("episode preparation volume unavailable"))
    };
    if volume(parent)? != volume(&p.preparation_cache)? {
        bail!(
            "episode preparation report must use the admitted volume outside authority/object stores"
        );
    }
    Ok(parent)
}

/// Writes bounded metadata and a fresh private report using existing providers.
/// Does not open the source journal, verify component media or launch a producer.
pub fn prepare(path: &Path, output: &Path) -> Result<Value> {
    let mut raw_profile = Vec::new();
    fs::File::open(path)?
        .take(128 * 1024 + 1)
        .read_to_end(&mut raw_profile)?;
    if raw_profile.len() > 128 * 1024 {
        bail!("episode preparation profile exceeds budget");
    }
    let p: Profile = serde_json::from_value(identity::parse_json(&raw_profile)?)?;
    validate(&p)?;
    let parent = report_parent(&p, output)?;
    let profile_pin = Object::from_bytes(&raw_profile);
    let output_ceiling = p.outputs.values().try_fold(0u64, |sum, bytes| {
        sum.checked_add(*bytes)
            .context("episode output ceiling overflow")
    })?;
    let wait = MetadataWait::default();
    let programs = BoundPrograms::bind(&p.inputs.runtime)?;
    let preparation = Preparation::open_on(
        &p.cache,
        &p.metadata_root,
        &p.preparation_cache,
        2 * (p.inputs.scenes.len() + p.inputs.presentations.len()) as u64 + 2,
    )?;
    let (mut source, context) = restore(&p.inputs, &p.metadata_root, &p.metadata_minimum, &wait)?;
    records::prepare(
        &p.inputs,
        &p.metadata_root,
        &p.metadata_minimum,
        &context,
        &p.cache.cache_roots,
    )?;
    let mut store = Waiting::new(
        wait.run(|| MetadataStore::open(&p.metadata_root, &p.metadata_minimum))?,
        &wait,
    );
    let root = planning::freeze(
        &p.inputs,
        &mut source,
        &context,
        &programs,
        &mut store,
        preparation.path(),
    )?;
    if !p.outputs.keys().eq(root.outputs.iter()) {
        bail!("episode output names differ from source contract");
    }
    admit(
        &root.resources,
        &protection(p.max_objects, p.max_logical_bytes)?,
    )?;
    let minimum = wait.run(|| store.inner.finish())?;
    drop(store);
    let mut reader = Waiting::new(
        wait.run(|| MetadataReader::open(&p.metadata_root, &minimum))?,
        &wait,
    );
    planning::readback(&p.inputs, &source, &context, &programs, &root, &mut reader)?;
    let summary = json!({"schema":"reel.tig-episode-preparation-summary.experimental-1",
        "resource_objects":root.resources.objects().len(),"logical_resource_bytes":root.resources.logical_bytes(),
        "components":p.inputs.scenes.len()+p.inputs.presentations.len(),"readback_equal":true,
        "component_payloads_verified":false,"production_currency_verified":false,
        "job_adopted":false,"execution_capacity_reserved":false,"media_rendered":false,
        "authorizes_production":false,"authorizes_eviction":false});
    let report = json!({"schema":"reel.tig-episode-preparation-report.experimental-1",
        "profile":profile_pin,"source_checkpoint":source.checkpoint(),"context":p.inputs.context,
        "resources":root.resources.selection(),"recipe":root.recipe.fingerprint(),
        "runtime":Object::from_bytes(programs.manifest_bytes()),"metadata_minimum":minimum,
        "output_ceilings":p.outputs,"total_output_ceiling_bytes":output_ceiling,
        "workspace_budget_bytes":p.budget_bytes,"scratch_budget_bytes":p.scratch_bytes,
        "reference_staging_bytes":p.reference_staging_bytes,"timeout_seconds":p.timeout_seconds,
        "root_retention_enrolled":false,"preparation_capacity_released_on_return":true,"summary":summary});
    let raw = serde_json::to_vec(&report)?;
    if raw.len() > 65536 {
        bail!("episode preparation report exceeds budget");
    }
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&raw)?;
    file.as_file().sync_all()?;
    preparation.finish()?;
    file.persist_noclobber(output)?;
    Ok(summary)
}
