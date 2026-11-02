//! Frozen browser correspondence and exact native-media comparison, without rendering.
use super::*;
use crate::tig_browser;
use tig_core::tree::NodeStore;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    metadata_root: PathBuf,
    metadata_minimum: Checkpoint,
    source: ArchiveProfile,
    context: Selection,
    request: tig_browser::Request,
    producer: Object,
    claim: Object,
    receipt: Object,
    reference: Object,
    byte_source: ByteProfile,
    runtime: Bindings,
    cache: Config,
    preparation_cache: PathBuf,
    max_verified_bytes: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: String,
    recipe: Digest,
    context: Selection,
    producer: Object,
    outputs: BTreeMap<String, scene_delivery::FileRef>,
    frames: u64,
    samples_per_channel: Option<u64>,
    pcm_sha256: Option<String>,
    capture: Capture,
    selected: bool,
    approved: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Capture {
    frames: u64,
    requests: u64,
    frame_sequence_sha256: String,
    served_bytes: u64,
}
fn shape(plan: &tig_browser::Prepared) -> Result<media_parity::Shape> {
    let m = &plan.manifest;
    // The shared exact parity checker currently requires a picture and audio
    // clock. Refuse silent sources explicitly rather than claiming coverage.
    if m.audio.is_none() || m.fps == 0 {
        bail!("browser retained comparison requires an audio clock");
    }
    let numerator = m
        .frame_count
        .checked_mul(48_000)
        .ok_or_else(|| anyhow::anyhow!("browser sample count overflow"))?;
    if numerator % u64::from(m.fps) != 0 {
        bail!("browser duration is not an integral audio sample count");
    }
    Ok(media_parity::Shape {
        width: m.width,
        height: m.height,
        fps_numerator: u64::from(m.fps),
        fps_denominator: 1,
        frames: m.frame_count,
        sample_rate: 48_000,
        samples: numerator / u64::from(m.fps),
    })
}
fn compare(profile: &Path, output: &Path, stage: &mut &'static str) -> Result<Value> {
    let p: Profile = read(profile, 128 * 1024, None)?;
    if p.schema != "reel.tig-browser-retained-comparison.experimental-1"
        || p.max_verified_bytes == 0
        || p.byte_source.required != RequiredProtection::PreventMutation
        || p.source.byte_source.required != RequiredProtection::PreventMutation
    {
        bail!("browser comparison admission differs");
    }
    p.request.validate()?;
    *stage = "store admission";
    directory(&p.metadata_root)?;
    directory(&p.preparation_cache)?;
    directory(
        p.cache
            .coordination_root
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("comparison coordination required"))?,
    )?;
    if !output.is_absolute() || output.file_name().is_none() || output.symlink_metadata().is_ok() {
        bail!("comparison report must be a new absolute file");
    }
    let parent = output
        .parent()
        .ok_or_else(|| anyhow::anyhow!("report parent missing"))?;
    directory(parent)?;
    let volume = |path: &Path| {
        admission::volume_id(path).map_err(|_| anyhow::anyhow!("comparison volume unavailable"))
    };
    if volume(parent)? != volume(&p.preparation_cache)? {
        bail!("comparison report volume differs");
    }
    *stage = "retained metadata";
    let mut cache = ReadCache::new(p.byte_source)?;
    let claim = Claim::parse(&p.claim, &bytes(&mut cache, &p.claim, META_BYTES)?)?;
    let receipt: Receipt = serde_json::from_value(identity::parse_json(&bytes(
        &mut cache, &p.receipt, META_BYTES,
    )?)?)?;
    if receipt.schema != "reel.tig-browser-native-receipt.experimental-1"
        || receipt.selected
        || receipt.approved
        || receipt.outputs.len() != 1
        || receipt.context != p.context
        || receipt.producer != p.producer
        || claim.context().root != p.context.root
        || claim.context().identity != p.context.context
        || claim.operation().producer != p.producer
    {
        bail!("browser receipt or claim differs from independent admission");
    }
    let master = receipt
        .outputs
        .get("master.mkv")
        .ok_or_else(|| anyhow::anyhow!("browser master missing"))?;
    if master.path != Path::new("master.mkv") {
        bail!("browser output path differs");
    }
    let master_pin = Object {
        sha256: Digest::parse(&master.sha256)?,
        bytes: master.bytes,
    };
    let mut verified = BTreeMap::new();
    for object in [&p.claim, &p.receipt, &p.reference, &master_pin] {
        if verified
            .insert(object.sha256.clone(), object.bytes)
            .is_some_and(|bytes| bytes != object.bytes)
        {
            bail!("comparison objects disagree about length");
        }
    }
    let total = verified
        .values()
        .try_fold(0u64, |n, b| n.checked_add(*b))
        .ok_or_else(|| anyhow::anyhow!("comparison byte sum overflow"))?;
    if total > p.max_verified_bytes
        || claim.outputs().is_empty()
        || claim.outputs().iter().any(|s| {
            s.object() != &master_pin
                || s.definition().id == p.request.root_definition
                || p.request.primary_definitions.contains(&s.definition().id)
        })
    {
        bail!("browser output subjects or byte allowance differ");
    }
    *stage = "source replay";
    let mut metadata = MetadataReader::open(&p.metadata_root, &p.metadata_minimum)?;
    if p.producer.bytes > META_BYTES as u64 {
        bail!("original producer declaration exceeds budget");
    }
    let producer = metadata
        .get(&p.producer)?
        .ok_or_else(|| anyhow::anyhow!("original producer declaration missing"))?;
    if Object::from_bytes(&producer) != p.producer
        || identity::parse_json(&producer)?["schema"]
            != "reel.tig-browser-native-producer.experimental-1"
    {
        bail!("original browser producer declaration differs");
    }
    let (mut source, context) = tig_browser::restore_context(
        p.source,
        &p.request,
        &p.context,
        &mut metadata,
        TreeLimits::default(),
    )?;
    for subject in claim.outputs() {
        let admitted = source.admission(&subject.definition().id)?;
        if admitted.object != subject.definition().object
            || !matches!(admitted.payload, crate::tig_inputs::Payload::Selected {})
        {
            bail!("browser output definition differs from admission");
        }
    }
    let recipe = tig_browser::recipe(&mut source, &p.request, &context, &p.producer)?;
    claim.check_plan(&recipe)?;
    let plan = tig_browser::lower(&mut source, &p.request)?;
    let shape = shape(&plan)?;
    if receipt.recipe != *recipe.fingerprint()
        || receipt.frames != shape.frames
        || receipt.capture.frames != shape.frames
        || receipt.samples_per_channel != Some(shape.samples)
        || receipt.capture.requests == 0
        || receipt.capture.served_bytes == 0
    {
        bail!("browser receipt differs from frozen source plan");
    }
    Digest::parse(&receipt.capture.frame_sequence_sha256)?;
    Digest::parse(
        receipt
            .pcm_sha256
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("browser PCM observation missing"))?,
    )?;
    *stage = "primary inputs";
    let files = plan.inputs.values().cloned().collect::<Vec<_>>();
    let _inputs = source.prepare_files(&files)?;
    *stage = "runtime bindings";
    let programs = BoundPrograms::bind(&p.runtime)?;
    let prepared =
        preparation::Preparation::open_on(&p.cache, &p.metadata_root, &p.preparation_cache, 2)?;
    let reference = HeldReference {
        file: scene_delivery::FileRef {
            path: "reference.mkv".into(),
            sha256: p.reference.sha256.as_str().into(),
            bytes: p.reference.bytes,
        },
        held: cache.open_verified(&p.reference)?,
    };
    let candidate = HeldReference {
        file: master.clone(),
        held: cache.open_verified(&master_pin)?,
    };
    *stage = "native output check";
    let bound = programs.with_inputs(&candidate);
    let candidate_path = candidate.held.with_path(Path::to_path_buf)?;
    let facts = scene_delivery::probe_with_inputs(&bound, &candidate_path)?;
    let streams = facts["streams"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("native streams missing"))?;
    if streams.len() != 2
        || !streams.iter().any(|s| {
            s["codec_type"] == "video" && s["codec_name"] == "ffv1" && s["pix_fmt"] == "yuv444p"
        })
        || !streams
            .iter()
            .any(|s| s["codec_type"] == "audio" && s["codec_name"] == "pcm_s24le")
    {
        bail!("retained browser output is not the native master contract");
    }
    *stage = "decoded comparison";
    let parity = media_parity::compare_with_inputs(
        &reference.file,
        &reference,
        &candidate.file,
        &bound,
        &shape,
    )?;
    if receipt.pcm_sha256.as_deref() != Some(parity.candidate.pcm24_sha256.as_str()) {
        bail!("retained browser PCM differs from original observation");
    }
    *stage = "report publication";
    let report = identity::canonical_json(&json!({
        "schema":"reel.tig-browser-retained-comparison-report.experimental-1",
        "original_producer_admission":p.producer,"claim":p.claim,"receipt":p.receipt,
        "reference":p.reference,"master":master_pin,"context":p.context,
        "source_checkpoint":source.checkpoint(),"metadata_minimum":metadata.checkpoint(),
        "validator":pin(&programs)?,"native_checked":true,"parity":parity,
        "execution_attested":false,"complete_semantic_coverage":false,
        "approval_transferred":false,"authorizes_eviction":false
    }))?;
    if report.len() > META_BYTES {
        bail!("comparison report exceeds budget");
    }
    prepared.finish()?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&report)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(output)
        .map_err(|_| anyhow::anyhow!("comparison report publication refused"))?;
    Ok(
        json!({"schema":"reel.tig-browser-retained-comparison-summary.experimental-1",
        "equal":parity.equal,"native_checked":true,"verified_output_bytes":total,
        "materialized_bytes":0,"approval_transferred":false,"authorizes_eviction":false}),
    )
}
pub fn compare_browser_retained(profile: &Path, output: &Path) -> Result<Value> {
    let mut stage = "profile admission";
    compare(profile, output, &mut stage)
        .map_err(|_| anyhow::anyhow!("retained browser comparison refused at {stage}"))
}
