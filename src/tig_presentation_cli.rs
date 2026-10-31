//! Explicit read-only acceptance checks, separate from managed job adoption.
use crate::{
    media_runtime::{Bindings, BoundPrograms},
    presentation_still_sequence,
    tig_inputs::{Profile as SourceProfile, Source},
    tig_presentation::{self, Request, jobs::JobPlan},
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use tig_core::{
    identity,
    tree::{MemoryNodes, TreeLimits},
};

const SCHEMA: &str = "reel.tig-presentation-check-profile.experimental-1";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    source: SourceProfile,
    request: Request,
    comparison: Option<Comparison>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Comparison {
    output_id: String,
    output_definition: String,
    runtime: Bindings,
    stage_parent: PathBuf,
    max_staged_bytes: u64,
    max_objects: usize,
    max_logical_bytes: u64,
}
fn read(path: &Path) -> Result<Profile> {
    let mut raw = Vec::new();
    fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut raw)?;
    if raw.len() > 1024 * 1024 {
        bail!("presentation check profile exceeds budget");
    }
    let profile: Profile = serde_json::from_value(identity::parse_json(&raw)?)?;
    if profile.schema != SCHEMA {
        bail!("unsupported presentation check profile");
    }
    Ok(profile)
}

/// Compile committed native metadata without media IO, execution or writes.
pub fn inspect(path: &Path) -> Result<Value> {
    let profile = read(path)?;
    let source = Source::open(profile.source)?;
    let compiled = tig_presentation::compile(&source, &profile.request)?;
    Ok(json!({
        "schema":"reel.tig-presentation-inspection.experimental-1",
        "source_checkpoint":source.checkpoint(),"source":compiled.root(),
        "manifest":compiled.manifest(),"records":compiled.records().count(),
        "primary_names":compiled.primary_files().count(),
        "primary_media_opened":source.byte_requests(),
        "production_current":false,"adopted":false,"approval":false,
        "complete_semantic_coverage":false
    }))
}

/// Stream an independent comparison under exact runtime and source byte guards.
/// This is a synchronous acceptance observation, not a scheduler/job capability.
pub fn compare(path: &Path) -> Result<Value> {
    let profile = read(path)?;
    let comparison = profile
        .comparison
        .context("explicit comparison admission required")?;
    if !comparison.stage_parent.is_absolute()
        || !comparison.stage_parent.is_dir()
        || comparison.max_staged_bytes == 0
        || comparison.max_staged_bytes > 64 * 1024 * 1024
        || comparison.max_objects == 0
        || comparison.max_objects > 1_000_000
        || comparison.max_logical_bytes == 0
        || comparison.max_logical_bytes > 9_007_199_254_740_991
    {
        bail!("invalid presentation comparison budget or existing scratch");
    }
    let mut source =
        Source::open(profile.source).context("capture committed presentation source")?;
    let mut metadata = MemoryNodes::default();
    let context = tig_presentation::freeze_context(
        &mut source,
        &profile.request,
        &mut metadata,
        TreeLimits::default(),
    )
    .context("freeze presentation source context")?;
    let programs = BoundPrograms::bind(&comparison.runtime).context("bind comparison runtime")?;
    let plan = JobPlan::from_context(
        &mut source,
        &profile.request,
        &context,
        &programs,
        &comparison.output_id,
        &comparison.output_definition,
    )
    .context("bind selected presentation output")?;
    let resources = plan
        .freeze(&mut metadata, TreeLimits::default())
        .context("freeze presentation resources")?;
    let readback = plan
        .readback(resources.selection(), &mut metadata, TreeLimits::default())
        .context("read back presentation resources")?;
    if readback.resources().objects().len() > comparison.max_objects
        || readback.resources().logical_bytes() > comparison.max_logical_bytes
    {
        bail!("presentation comparison resources exceed admission");
    }
    let inputs = plan
        .compiled()
        .prepare(&mut source)
        .context("open guarded presentation primaries")?;
    let outputs = source
        .prepare_files(std::slice::from_ref(plan.selected_master()))
        .context("open guarded selected presentation master")?;
    let result = presentation_still_sequence::compare_master_to_primaries(
        plan.compiled().manifest(),
        plan.compiled(),
        &programs.with_inputs(&inputs),
        &crate::tig_scene_production::jobs::reference::SelectedOutputs(&outputs),
        plan.selected_master(),
        &comparison.stage_parent,
        comparison.max_staged_bytes,
    )
    .context("compare selected presentation to its primaries")?;
    let (yuv, rgb) = result.picture_sha256();
    Ok(json!({
        "schema":"reel.tig-presentation-comparison.experimental-1","equal":true,
        "source_checkpoint":source.checkpoint(),"source":plan.compiled().root(),
        "recipe":plan.recipe().fingerprint(),"job":resources.selection(),
        "manifest":result.manifest(),"master":result.master(),
        "yuv_sha256":yuv,"rgb_sha256":rgb,"pcm_sha256":result.audio_sha256(),
        "staged_bytes":result.staged_bytes(),"resource_objects":resources.objects().len(),
        "logical_bytes":resources.logical_bytes(),
        "production_current":false,"adopted":false,"approval":false,
        "original_producer_assertion":false,"eviction_authority":false,
        "complete_environment":false,"complete_semantic_coverage":false
    }))
}
