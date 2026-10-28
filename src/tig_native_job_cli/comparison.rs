//! Explicit retained-byte verification. Claims remain unsigned correspondence.
use super::*;
use crate::{media_parity, scene_delivery};
use reel_assets::admission;
use tig_assets::graph::recipe::produced::Claim;
use tig_store::existing::{
    ByteSource, Profile as ByteProfile, ReadCache, RequiredProtection, VerifiedRead,
};

const SCHEMA: &str = "reel.tig-retained-comparison.experimental-1";
const DERIVED_SCHEMA: &str = "reel.tig-retained-comparison.experimental-2";
const META_BYTES: usize = 65_536;
const RECEIPT_BYTES: usize = 4 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Comparison {
    schema: String,
    metadata_root: PathBuf,
    metadata_minimum: Checkpoint,
    source: ArchiveProfile,
    context: Selection,
    request: Request,
    producer: Object,
    claim: Object,
    receipt: Object,
    #[serde(default)]
    outputs: Option<BTreeMap<String, Object>>,
    reference: Object,
    byte_source: ByteProfile,
    runtime: Bindings,
    cache: Config,
    preparation_cache: PathBuf,
    max_output_bytes: u64,
}
struct HeldReference {
    file: scene_delivery::FileRef,
    held: VerifiedRead,
}
impl scene_delivery::InputFiles for HeldReference {
    fn verified_path(&self, expected: &scene_delivery::FileRef) -> Result<PathBuf> {
        if expected != &self.file {
            bail!("comparison reference differs from admission");
        }
        Ok(self.held.with_path(Path::to_path_buf)?)
    }
}
fn bytes(cache: &mut ReadCache, object: &Object, max: usize) -> Result<Vec<u8>> {
    if object.bytes > max as u64 {
        bail!("comparison metadata exceeds budget");
    }
    let mut held = cache.open_verified(object)?;
    let mut bytes = Vec::new();
    (&mut held).take(max as u64 + 1).read_to_end(&mut bytes)?;
    if Object::from_bytes(&bytes) != *object {
        bail!("comparison metadata differs");
    }
    Ok(bytes)
}
fn directory(path: &Path) -> Result<()> {
    if !path.is_absolute() || !fs::symlink_metadata(path)?.is_dir() {
        bail!("comparison requires existing absolute directories");
    }
    Ok(())
}
fn compare(profile: &Path, output: &Path) -> Result<Value> {
    let profile: Comparison = read(profile, 128 * 1024, None)?;
    if !matches!(profile.schema.as_str(), SCHEMA | DERIVED_SCHEMA)
        || (profile.schema == SCHEMA
            && profile
                .outputs
                .as_ref()
                .is_none_or(|o| o.is_empty() || o.len() > 511 || o.contains_key("receipt.json")))
        || (profile.schema == DERIVED_SCHEMA && profile.outputs.is_some())
        || profile.max_output_bytes == 0
    {
        bail!("unsupported comparison profile or output set");
    }
    profile.request.validate()?;
    directory(&profile.metadata_root)?;
    directory(&profile.preparation_cache)?;
    directory(
        profile
            .cache
            .coordination_root
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("shared comparison coordination required"))?,
    )?;
    if !output.is_absolute() || output.file_name().is_none() || output.symlink_metadata().is_ok() {
        bail!("comparison report must be a new absolute file");
    }
    let parent = output
        .parent()
        .ok_or_else(|| anyhow::anyhow!("report parent required"))?;
    directory(parent)?;
    let volume = |p: &Path| {
        admission::volume_id(p).map_err(|_| anyhow::anyhow!("comparison volume unavailable"))
    };
    if volume(parent)? != volume(&profile.preparation_cache)? {
        bail!("report volume differs");
    }
    if profile.byte_source.required != RequiredProtection::PreventMutation {
        bail!("retained comparison requires mandatory read protection");
    }
    if profile.receipt.bytes > profile.max_output_bytes
        || profile.claim.bytes > META_BYTES as u64
        || profile.receipt.bytes > RECEIPT_BYTES as u64
    {
        bail!("retained comparison exceeds declared byte allowance");
    }
    let reader_limits = profile.byte_source.limits;
    let mut cache = ReadCache::new(profile.byte_source)?;
    let claim = Claim::parse(
        &profile.claim,
        &bytes(&mut cache, &profile.claim, META_BYTES)?,
    )?;
    if claim.context().root != profile.context.root
        || claim.context().identity != profile.context.context
        || claim.operation().producer != profile.producer
    {
        bail!("retained claim context or producer differs from independent admission");
    }
    let mut receipt_guard = cache.open_verified(&profile.receipt)?;
    let mut receipt_bytes = Vec::new();
    (&mut receipt_guard)
        .take(RECEIPT_BYTES as u64 + 1)
        .read_to_end(&mut receipt_bytes)?;
    if Object::from_bytes(&receipt_bytes) != profile.receipt {
        bail!("comparison receipt differs");
    }
    let receipt: scene_delivery::Receipt =
        serde_json::from_value(identity::parse_json(&receipt_bytes)?)?;
    if receipt.schema != "reel.scene-delivery-receipt.v0.1"
        || receipt.outputs.is_empty()
        || receipt.outputs.len() > 4096
    {
        bail!("retained receipt output set differs");
    }
    let mut outputs = BTreeMap::new();
    let mut total = profile.receipt.bytes;
    for (name, item) in &receipt.outputs {
        LogicalPath::parse(name)?;
        let expected = Object {
            sha256: Digest::parse(&item.sha256)?,
            bytes: item.bytes,
        };
        if crate::tig_scene_outputs::output_name(&item.path)? != *name || name == "receipt.json" {
            bail!("retained output differs from independently admitted map");
        }
        total = total
            .checked_add(expected.bytes)
            .ok_or_else(|| anyhow::anyhow!("output sum overflow"))?;
        outputs.insert(name.clone(), expected);
    }
    if total > profile.max_output_bytes
        || profile
            .outputs
            .as_ref()
            .is_some_and(|admitted| admitted != &outputs)
    {
        bail!("retained output map or byte allowance differs");
    }
    let master = outputs
        .get("master.mkv")
        .ok_or_else(|| anyhow::anyhow!("master missing"))?;
    if !claim
        .outputs()
        .iter()
        .any(|subject| subject.object() == master)
        || claim.outputs().iter().any(|subject| {
            !outputs.values().any(|object| subject.object() == object)
                || subject.definition().id == profile.request.scene_definition
                || profile
                    .request
                    .control_definitions
                    .contains(&subject.definition().id)
                || profile
                    .request
                    .primary_definitions
                    .contains(&subject.definition().id)
        })
    {
        bail!("claim subject differs from admitted native outputs");
    }
    let programs = BoundPrograms::bind(&profile.runtime)?;
    let prepared = preparation::Preparation::open_on(
        &profile.cache,
        &profile.metadata_root,
        &profile.preparation_cache,
        2,
    )?;
    let mut reader = MetadataReader::open(&profile.metadata_root, &profile.metadata_minimum)?;
    let compile = prepared.path().join("compiled");
    let (mut source, _replay) = tig_scene::restore_render_claim(
        profile.source,
        &profile.request,
        &profile.producer,
        &claim,
        &compile,
        &mut reader,
        TreeLimits::default(),
    )?;
    let job_path = compile.join("delivery/job.json");
    let job = crate::tig_inputs::read_job(&job_path)?;
    let inputs = source.prepare(&job)?;
    let bound = programs.with_inputs(&inputs);
    let (_, plan) = scene_delivery::plan_with_inputs(&job_path, &bound)?;
    if serde_json::to_value(&receipt.plan)? != serde_json::to_value(&plan)?
        || receipt.content_samples != plan.duration_samples
        || receipt.delivery_frames != plan.frame_count
        || scene_delivery::output_contract(&job, &plan)?.names
            != outputs.keys().cloned().collect::<BTreeSet<_>>()
    {
        bail!("retained receipt differs from frozen native plan");
    }
    let reference = HeldReference {
        file: scene_delivery::FileRef {
            path: "reference.mkv".into(),
            sha256: profile.reference.sha256.as_str().into(),
            bytes: profile.reference.bytes,
        },
        held: cache.open_verified(&profile.reference)?,
    };
    let bundle = crate::tig_scene_outputs::Bundle::from_held_receipt(
        &mut cache,
        &profile.receipt,
        receipt_guard,
        reader_limits,
    )?;
    scene_delivery::check_with_output_files(&job_path, &bound, &bundle, bundle.receipt_ref())?;
    let shape = media_parity::Shape {
        width: job.width,
        height: job.height,
        fps_numerator: plan.fps_numerator,
        fps_denominator: plan.fps_denominator,
        frames: plan.frame_count,
        sample_rate: plan.sample_rate,
        samples: plan.duration_samples,
    };
    let parity = media_parity::compare_with_inputs(
        &reference.file,
        &reference,
        &receipt.outputs["master.mkv"],
        &programs.with_inputs(&bundle),
        &shape,
    )?;
    let summary = json!({"schema":"reel.tig-retained-comparison-summary.experimental-1",
        "equal":parity.equal,"native_checked":true,"outputs_verified":outputs.len(),
        "materialized_bytes":0,"verified_output_bytes":total,"approval_transferred":false,"authorizes_eviction":false});
    let mut report = json!({"schema":"reel.tig-retained-comparison-report.experimental-1",
        "claim":profile.claim,"receipt":profile.receipt,"producer":profile.producer,
        "reference":profile.reference,"outputs":outputs,"source_checkpoint":source.checkpoint(),
        "validator":pin(&programs)?,"context":profile.context,
        "metadata_minimum":reader.checkpoint(),"native_checked":true,"parity":parity,
        "execution_attested":false,"complete_semantic_coverage":false,"authorizes_eviction":false});
    if profile.schema == DERIVED_SCHEMA {
        report["schema"] = json!("reel.tig-retained-comparison-report.experimental-2");
        report.as_object_mut().unwrap().remove("outputs");
        report["output_manifest"] = json!(profile.receipt);
        report["output_count"] = json!(receipt.outputs.len());
    }
    let report = serde_json::to_vec(&report)?;
    if report.len() > META_BYTES {
        bail!("comparison report exceeds budget");
    }
    prepared.finish()?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&report)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(output)
        .map_err(|_| anyhow::anyhow!("comparison report publication refused"))?;
    Ok(summary)
}
/// Explicit CPU/read action with owned admitted scratch. The report is private;
/// stdout and refusal diagnostics contain no paths, source IDs or raw hashes.
pub fn compare_retained(profile: &Path, output: &Path) -> Result<Value> {
    compare(profile, output).map_err(|_| anyhow::anyhow!(
        "retained comparison refused; verify admissions, pins, native plan, programs and capacity"))
}
