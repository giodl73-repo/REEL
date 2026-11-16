//! Check retained candidate bytes against an independently admitted frozen delivery
//! source. A failed or interrupted job need not have emitted a producer claim.
use super::*;

const SCHEMA: &str = "reel.tig-retained-delivery-check.experimental-1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    metadata_root: PathBuf,
    metadata_minimum: Checkpoint,
    source: ArchiveProfile,
    context: Selection,
    request: crate::tig_delivery_template::Request,
    producer: Object,
    recipe: Digest,
    receipt: Object,
    outputs: BTreeMap<String, Object>,
    byte_source: ByteProfile,
    runtime: Bindings,
    cache: Config,
    preparation_cache: PathBuf,
    max_output_bytes: u64,
}

impl Profile {
    fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA
            || self.max_output_bytes == 0
            || self.receipt.bytes > RECEIPT_BYTES as u64
            || self.byte_source.required != RequiredProtection::PreventMutation
            || self.source.byte_source.required != RequiredProtection::PreventMutation
            || self.outputs.is_empty()
            || self.outputs.len() > 511
        {
            bail!("invalid retained delivery admission");
        }
        self.request.validate()?;
        let mut total = self.receipt.bytes;
        for (name, object) in &self.outputs {
            LogicalPath::parse(name)?;
            if name == "receipt.json" {
                bail!("receipt is not a media output");
            }
            total = total
                .checked_add(object.bytes)
                .ok_or_else(|| anyhow::anyhow!("output sum overflow"))?;
        }
        if total > self.max_output_bytes {
            bail!("retained delivery exceeds admitted bytes");
        }
        Ok(())
    }
}

fn check(profile: &Path, output: &Path, stage: &mut &'static str) -> Result<Value> {
    let profile: Profile = read(profile, 128 * 1024, None)?;
    profile.validate()?;
    *stage = "store admission";
    directory(&profile.metadata_root)?;
    directory(&profile.preparation_cache)?;
    directory(
        profile
            .cache
            .coordination_root
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("shared coordination required"))?,
    )?;
    if !output.is_absolute() || output.file_name().is_none() || output.symlink_metadata().is_ok() {
        bail!("report requires a new absolute file");
    }
    let parent = output
        .parent()
        .ok_or_else(|| anyhow::anyhow!("report parent required"))?;
    directory(parent)?;
    let volume = |path: &Path| {
        admission::volume_id(path).map_err(|_| anyhow::anyhow!("comparison volume unavailable"))
    };
    if volume(parent)? != volume(&profile.preparation_cache)? {
        bail!("report volume differs");
    }
    *stage = "runtime bindings";
    let programs = BoundPrograms::bind(&profile.runtime)?;
    // The current checker may be a different executable from the original
    // renderer. Bind today's validator, but replay the independently admitted
    // recipe with its original producer identity; neither attests execution.
    let validator = Object::from_bytes(programs.manifest_bytes());
    *stage = "scratch admission";
    let preparation = preparation::Preparation::open_on(
        &profile.cache,
        &profile.metadata_root,
        &profile.preparation_cache,
        2,
    )?;
    *stage = "source replay";
    let mut reader = MetadataReader::open(&profile.metadata_root, &profile.metadata_minimum)?;
    let (mut source, context) = crate::tig_delivery_template::restore_context(
        profile.source,
        &profile.request,
        &profile.context,
        &mut reader,
        TreeLimits::default(),
    )?;
    let recipe = crate::tig_delivery_template::recipe(
        &mut source,
        &profile.request,
        &context,
        &profile.producer,
    )?;
    if recipe.fingerprint() != &profile.recipe {
        bail!("retained delivery recipe differs from independent admission");
    }
    *stage = "current delivery controls";
    let lowered = crate::tig_delivery_template::lower(&mut source, &profile.request)?;
    let inputs = source.prepare(&lowered.job)?;
    let execution = programs.with_inputs(&inputs);
    let controls = tig_scene_production::DeliveryControls::new(lowered)?;
    let (job, plan) = scene_delivery::plan_with_control_files(
        &controls.job,
        &controls.production,
        &execution,
        &controls,
    )?;
    if scene_delivery::output_contract(&job, &plan)?.names
        != profile.outputs.keys().cloned().collect()
    {
        bail!("admitted outputs differ from frozen delivery contract");
    }
    *stage = "retained output bundle";
    let bundle = crate::tig_scene_outputs::Bundle::open(profile.byte_source, &profile.receipt)?;
    let receipt = bundle.receipt();
    let observed = receipt
        .outputs
        .iter()
        .map(|(name, file)| {
            Ok((
                name.clone(),
                Object {
                    sha256: Digest::parse(&file.sha256)?,
                    bytes: file.bytes,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    if observed != profile.outputs {
        bail!("retained outputs differ from independent admission");
    }
    *stage = "native output check";
    scene_delivery::check_with_all_files(
        &controls.job,
        &controls.production,
        &execution,
        &bundle,
        bundle.receipt_ref(),
        &controls,
    )?;
    *stage = "report publication";
    let report = json!({"schema":"reel.tig-retained-delivery-check-report.experimental-1",
        "source_checkpoint":source.checkpoint(),"context":profile.context,
        "producer_admission":profile.producer,"recipe":profile.recipe,
        "validator":validator,
        "receipt":profile.receipt,"outputs":profile.outputs,
        "metadata_minimum":reader.checkpoint(),"native_checked":true,
        "frames":plan.frame_count,"samples":plan.duration_samples,
        "sample_rate":plan.sample_rate,"fps_numerator":plan.fps_numerator,"fps_denominator":plan.fps_denominator,
        "execution_attested":false,"production_claim":null,"authorizes_production":false,
        "approval_transferred":false,"authorizes_eviction":false});
    let bytes = serde_json::to_vec(&report)?;
    if bytes.len() > META_BYTES {
        bail!("delivery report exceeds budget");
    }
    preparation.finish()?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(output)
        .map_err(|_| anyhow::anyhow!("report publication refused"))?;
    Ok(
        json!({"schema":"reel.tig-retained-delivery-check-summary.experimental-1",
        "native_checked":true,"outputs_verified":observed.len(),"materialized_bytes":0,
        "execution_attested":false,"production_claim":null,"authorizes_production":false,
        "approval_transferred":false,"authorizes_eviction":false}),
    )
}

/// Explicit CPU/read action with guarded inputs, outputs and admitted scratch.
/// Never changes the source journal, executes a render or repairs failed history.
pub fn check_retained_delivery(profile: &Path, output: &Path) -> Result<Value> {
    let mut stage = "profile admission";
    check(profile, output, &mut stage)
        .map_err(|_| anyhow::anyhow!("retained delivery check refused at {stage}"))
}
