//! Explicit metadata-only advisory. No model, job admission or primary byte IO.
use super::*;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tig_assets::graph::{
    Reference,
    recipe::produced::{self, BoundSubject, Claim, Correspondence, Selector},
};
use tig_store::metadata::{Checkpoint, MetadataReader};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Original {
    source: ArchiveProfile,
    request: Request,
    producer: Object,
    claim: Object,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inspection {
    schema: String,
    metadata_root: PathBuf,
    metadata_minimum: Checkpoint,
    source: ArchiveProfile,
    request: Request,
    producer: Object,
    original: Option<Original>,
}
fn run(path: &Path, output: &Path) -> Result<Value> {
    let mut raw = Vec::new();
    fs::File::open(path)?
        .take(128 * 1024 + 1)
        .read_to_end(&mut raw)?;
    if raw.len() > 128 * 1024 {
        bail!("inspection profile exceeds budget");
    }
    let p: Inspection = serde_json::from_value(identity::parse_json(&raw)?)?;
    let parent = output
        .parent()
        .ok_or_else(|| anyhow::anyhow!("report parent missing"))?;
    if p.schema != "reel.tig-generation-inspection.experimental-1"
        || !p.metadata_root.is_absolute()
        || !output.is_absolute()
        || output.symlink_metadata().is_ok()
        || !parent.is_dir()
        || parent
            .canonicalize()?
            .starts_with(p.metadata_root.canonicalize()?)
    {
        bail!("explicit generation inspection stores and new report required");
    }
    let mut reader = MetadataReader::open(&p.metadata_root, &p.metadata_minimum)?;
    let mut source = Source::from_archive(p.source, &mut reader, TreeLimits::default())?;
    let desired = recipe(&mut source, &p.request, &p.producer)?;
    let target = source.record(&p.request.output_id)?;
    let a = source.admission(&p.request.output_definition)?;
    let subject = BoundSubject::resolve(
        &target,
        &Reference {
            id: a.id.clone(),
            object: a.object.clone(),
        },
        Selector::Selected {},
    )?;
    let original_pin = p.original.as_ref().map(|o| o.claim.clone());
    let state = if let Some(original) = p.original {
        let bytes = source.read_metadata(&original.claim, 65536)?;
        let claim = Claim::parse(&original.claim, &bytes)?;
        let (_, replay) = restore_claim(
            original.source,
            &original.request,
            &original.producer,
            &claim,
            &mut reader,
            TreeLimits::default(),
        )?;
        produced::compare(&desired, subject.as_ref(), Some(replay.checked()))?
    } else {
        produced::compare(&desired, subject.as_ref(), None)?
    };
    let state = match state {
        Correspondence::Unbuilt => "unbuilt",
        Correspondence::Unproven => "unproven",
        Correspondence::ClaimedRecipeMatch => "claimed_recipe_match",
        Correspondence::StaleRecipe => "stale_recipe",
    };
    let report = identity::canonical_json(
        &json!({"schema":"reel.generation-inspection.experimental-1","output_id":p.request.output_id,"source_checkpoint":source.checkpoint(),"desired_recipe":desired.fingerprint(),"original_claim":original_pin,"state":state,"upstream_generation_checked":false,"primary_payloads_verified":false,"execution_attested":false,"authorizes_execution":false,"authorizes_eviction":false,"complete_semantic_coverage":false}),
    )?;
    if report.len() > 65536 {
        bail!("generation report exceeds bound");
    }
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&report)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(output)
        .map_err(|_| anyhow::anyhow!("generation report refused"))?;
    Ok(
        json!({"schema":"reel.generation-inspection-summary.experimental-1","state":state,"nodes":desired.nodes().len(),"upstream_generation_checked":false,"primary_payloads_verified":false,"authorizes_execution":false,"authorizes_eviction":false}),
    )
}
pub fn inspect(profile: &Path, output: &Path) -> Result<Value> {
    run(profile,output).map_err(|_|anyhow::anyhow!("generation inspection refused; verify source, definitions, request and original evidence"))
}
