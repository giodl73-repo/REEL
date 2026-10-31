//! Bounded metadata candidate creation; source protection remains explicit.
use super::*;
use crate::{
    tig_inputs::{Profile as InputProfile, Source},
    tig_native_job_cli::{
        metadata_wait::{MetadataWait, Waiting},
        preparation::metadata_allowance,
    },
};
use reel_assets::admission;
use serde_json::json;
use std::io::Write;
use tig_store::metadata::MetadataStore;

const CAPTURE_SCHEMA: &str = "reel.tig-frozen-job-capture-profile.experimental-1";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Capture {
    schema: String,
    source: InputProfile,
    source_checkpoint: Digest,
    root_id: String,
    profiles: Vec<graph::Profile>,
    consumption: Vec<Consumption>,
    operation: recipe::Operation,
    descriptor: FileRef,
    runtime: BTreeMap<LogicalPath, Object>,
    manifest: FileRef,
    metadata_root: PathBuf,
    metadata_minimum: Checkpoint,
    metadata_cache: PathBuf,
    cache: reel_assets::Config,
}
fn execute(path: &Path, output: &Path) -> Result<Value> {
    let (_profile_guard, raw) = read(path, 128 * 1024)?;
    let p: Capture = serde_json::from_value(identity::parse_json(&raw)?)?;
    let parent = output
        .parent()
        .ok_or_else(|| anyhow::anyhow!("capture output parent required"))?;
    let coordination = p
        .cache
        .coordination_root
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("capture coordination required"))?;
    if p.schema != CAPTURE_SCHEMA
        || p.profiles.is_empty()
        || p.profiles.len() > 128
        || p.consumption.len() > 128
        || p.runtime.len() > 64
        || !output.is_absolute()
        || output.symlink_metadata().is_ok()
        || [&p.metadata_root, &p.metadata_cache, coordination, parent]
            .iter()
            .any(|root| !root.is_absolute() || !root.is_dir())
        || !p.cache.cache_roots.contains(&p.metadata_cache)
        || p.cache
            .cache_roots
            .iter()
            .any(|root| !root.is_absolute() || !root.join("objects/sha256").is_dir())
        || !p.descriptor.path.is_absolute()
        || !p.manifest.path.is_absolute()
    {
        bail!("capture requires explicit existing stores and new bounded output");
    }
    let volume = admission::volume_id(&p.metadata_cache).map_err(|e| anyhow::anyhow!("{e}"))?;
    for root in [&p.metadata_root, parent] {
        if admission::volume_id(root).map_err(|e| anyhow::anyhow!("{e}"))? != volume {
            bail!("capture metadata/output must share admitted volume");
        }
    }
    let (_descriptor_guard, descriptor) = read(&p.descriptor.path, 65536)?;
    let descriptor_pin = Object::from_bytes(&descriptor);
    if descriptor_pin
        != (Object {
            sha256: Digest::parse(&p.descriptor.sha256)?,
            bytes: p.descriptor.bytes,
        })
        || descriptor_pin != p.operation.parameters
    {
        bail!("capture descriptor differs from operation");
    }
    let (_manifest_guard, request) = read(&p.manifest.path, 256 * 1024)?;
    let request_pin = Object::from_bytes(&request);
    if request_pin
        != (Object {
            sha256: Digest::parse(&p.manifest.sha256)?,
            bytes: p.manifest.bytes,
        })
        || p.runtime.get(&LogicalPath::parse("request")?) != Some(&request_pin)
    {
        bail!("capture request differs from runtime");
    }
    reel_assets::jobs::check_request(&request).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut source = Source::open(p.source)?;
    if source.checkpoint() != &p.source_checkpoint {
        bail!("capture source checkpoint differs");
    }
    let limits = graph::Limits {
        nodes: 4096,
        edges: 16384,
        depth: 64,
        bytes: 16 * 1024 * 1024,
    };
    let trees = TreeLimits::default();
    let consumption: Vec<_> = p
        .consumption
        .iter()
        .map(|c| recipe::Consumption {
            definition_id: c.definition_id.clone(),
            definition: c.definition.clone(),
            fields: c.fields.clone(),
        })
        .collect();
    let plan =
        source.production_recipe(&p.root_id, &p.profiles, &consumption, &p.operation, limits)?;
    let wait = MetadataWait::default();
    let reader = wait.run(|| MetadataReader::open(&p.metadata_root, &p.metadata_minimum))?;
    let bounds = reader.limits();
    let minimum = reader.checkpoint().clone();
    drop(reader);
    let allowance = metadata_allowance(&p.metadata_cache, bounds.database_bytes, 1024 * 1024)?;
    let _reservation =
        admission::reserve_on(&p.metadata_root, &p.cache, &p.metadata_cache, allowance)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut store = Waiting::new(
        wait.run(|| MetadataStore::open(&p.metadata_root, &minimum))?,
        &wait,
    );
    if store.inner.limits() != bounds {
        bail!("capture metadata bounds changed");
    }
    let context = source.freeze_context(
        &p.root_id,
        &p.profiles,
        &descriptor,
        &mut store,
        trees,
        limits,
    )?;
    let candidate = job::freeze(
        &job::Admission {
            context: &context,
            plan: &plan,
            runtime: &p.runtime,
            output: None,
        },
        &mut store,
        trees,
    )?;
    let witness = wait.run(|| store.inner.finish())?;
    drop(store);
    let mut reader = Waiting::new(
        wait.run(|| MetadataReader::open(&p.metadata_root, &witness))?,
        &wait,
    );
    let restored = source.replay_context(
        context.selection(),
        &p.profiles,
        &mut reader,
        Default::default(),
        trees,
        limits,
    )?;
    if restored.descriptor() != descriptor
        || restored.requirements() != context.requirements()
        || restored.archive() != context.archive()
    {
        bail!("capture source readback differs");
    }
    let plan =
        source.production_recipe(&p.root_id, &p.profiles, &consumption, &p.operation, limits)?;
    let resources = job::restore(
        candidate.selection(),
        &job::Admission {
            context: &restored,
            plan: &plan,
            runtime: &p.runtime,
            output: None,
        },
        &mut reader,
        trees,
    )?;
    let fragment = json!({"schema":"reel.tig-frozen-job-fragment.experimental-1","metadata_root":p.metadata_root,"metadata_minimum":witness,
        "source_context":restored.selection(),"source_checkpoint":p.source_checkpoint,"job_context":resources.selection(),
        "profiles":p.profiles,"consumption":p.consumption,"operation":p.operation,"runtime":p.runtime,"manifest":p.manifest});
    let bytes = serde_json::to_vec(&fragment)?;
    if bytes.len() > 128 * 1024 {
        bail!("capture fragment exceeds bound");
    }
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(output)
        .map_err(|_| anyhow::anyhow!("capture fragment publication refused"))?;
    Ok(
        json!({"schema":"reel.tig-frozen-job-capture-summary.experimental-1","readback_equal":true,"job_adopted":false,"outputs_retained":false,"media_rendered":false,"authorizes_eviction":false}),
    )
}
/// Explicit metadata writes only; all byte/source obligations remain external.
pub fn capture(profile: &Path, output: &Path) -> Result<Value> {
    execute(profile, output).map_err(|_| anyhow::anyhow!("frozen capture refused or incomplete; retain existing source and inspect private metadata"))
}
