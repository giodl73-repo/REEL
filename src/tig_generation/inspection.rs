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
#[serde(untagged)]
enum InputSource {
    Git(crate::tig_inputs::Profile),
    Archive(ArchiveProfile),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inspection {
    schema: String,
    metadata_root: Option<PathBuf>,
    metadata_minimum: Option<Checkpoint>,
    source: InputSource,
    request: Request,
    producer: Object,
    original: Option<Original>,
}
fn read_profile<T: serde::de::DeserializeOwned>(path: &Path, maximum: usize) -> Result<T> {
    let mut raw = Vec::new();
    fs::File::open(path)?
        .take(maximum as u64 + 1)
        .read_to_end(&mut raw)?;
    if raw.len() > maximum {
        bail!("inspection profile exceeds budget");
    }
    Ok(serde_json::from_value(identity::parse_json(&raw)?)?)
}
fn open(
    input: InputSource,
    root: Option<PathBuf>,
    minimum: Option<Checkpoint>,
    output: &Path,
) -> Result<(Source, Option<MetadataReader>)> {
    let parent = output
        .parent()
        .ok_or_else(|| anyhow::anyhow!("report parent missing"))?;
    if !output.is_absolute() || output.symlink_metadata().is_ok() || !parent.is_dir() {
        bail!("explicit generation inspection stores and new report required");
    }
    let mut reader = match (&root, &minimum) {
        (Some(root), Some(minimum)) => {
            if !root.is_absolute() || parent.canonicalize()?.starts_with(root.canonicalize()?) {
                bail!("explicit metadata authority required");
            }
            Some(MetadataReader::open(root, minimum)?)
        }
        (None, None) => None,
        _ => bail!("metadata root and trusted minimum must be supplied together"),
    };
    let source = match input {
        InputSource::Git(profile) => Source::open(profile)?,
        InputSource::Archive(profile) => Source::from_archive(
            profile,
            reader
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("archive requires admitted metadata"))?,
            TreeLimits::default(),
        )?,
    };
    Ok((source, reader))
}
fn basis(
    source: &mut Source,
    request: &Request,
    producer: &Object,
    original: Option<Original>,
    reader: &mut Option<MetadataReader>,
) -> Result<(
    tig_assets::graph::recipe::Plan,
    Correspondence,
    Option<Object>,
)> {
    let desired = recipe(source, request, producer)?;
    let (state, original_pin) = original_basis(source, request, Some(&desired), original, reader)?;
    Ok((desired, state.unwrap(), original_pin))
}
// Validate historical evidence even when an unbuilt input prevents a desired
// recipe. A blocked plan must not hide corrupt or foreign original claims.
fn original_basis(
    source: &Source,
    request: &Request,
    desired: Option<&tig_assets::graph::recipe::Plan>,
    original: Option<Original>,
    reader: &mut Option<MetadataReader>,
) -> Result<(Option<Correspondence>, Option<Object>)> {
    let target = source.record(&request.output_id)?;
    let a = source.admission(&request.output_definition)?;
    let subject = BoundSubject::resolve(
        &target,
        &Reference {
            id: a.id.clone(),
            object: a.object.clone(),
        },
        Selector::Selected {},
    )?;
    let original_pin = original.as_ref().map(|o| o.claim.clone());
    let state = if let Some(original) = original {
        if original.request.output_id != request.output_id
            || original.request.output_definition != request.output_definition
        {
            bail!("original generation target differs");
        }
        let reader = reader
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("original evidence requires admitted metadata"))?;
        // Historical evidence uses its independently enrolled providers; a
        // current-only cache profile is not permission to invent its absence.
        let mut original_source =
            Source::from_archive(original.source.clone(), reader, TreeLimits::default())?;
        let bytes = original_source.read_metadata(&original.claim, 65536)?;
        let claim = Claim::parse(&original.claim, &bytes)?;
        let (_, replay) = restore_claim(
            original.source,
            &original.request,
            &original.producer,
            &claim,
            reader,
            TreeLimits::default(),
        )?;
        if desired.is_none()
            && subject.as_ref().is_none_or(|subject| {
                !replay
                    .checked()
                    .claim()
                    .outputs()
                    .contains(subject.subject())
            })
        {
            bail!("blocked original claim does not name the current output subject");
        }
        desired
            .map(|desired| produced::compare(desired, subject.as_ref(), Some(replay.checked())))
            .transpose()?
    } else {
        desired
            .map(|desired| produced::compare(desired, subject.as_ref(), None))
            .transpose()?
    };
    Ok((state, original_pin))
}
fn correspondence_name(state: Correspondence) -> &'static str {
    match state {
        Correspondence::Unbuilt => "unbuilt",
        Correspondence::Unproven => "unproven",
        Correspondence::ClaimedRecipeMatch => "claimed_recipe_match",
        Correspondence::StaleRecipe => "stale_recipe",
    }
}
fn publish(output: &Path, value: &Value, maximum: usize) -> Result<()> {
    let report = identity::canonical_json(value)?;
    if report.len() > maximum {
        bail!("generation report exceeds bound");
    }
    let mut file = tempfile::NamedTempFile::new_in(output.parent().unwrap())?;
    file.write_all(&report)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(output)
        .map_err(|_| anyhow::anyhow!("generation report refused"))?;
    Ok(())
}
fn run(path: &Path, output: &Path) -> Result<Value> {
    let p: Inspection = read_profile(path, 128 * 1024)?;
    if p.schema != "reel.tig-generation-inspection.experimental-1" {
        bail!("invalid generation inspection schema");
    }
    let (mut source, mut reader) = open(p.source, p.metadata_root, p.metadata_minimum, output)?;
    let (desired, state, original_pin) = basis(
        &mut source,
        &p.request,
        &p.producer,
        p.original,
        &mut reader,
    )?;
    let state = correspondence_name(state);
    publish(
        output,
        &json!({"schema":"reel.generation-inspection.experimental-1","output_id":p.request.output_id,"source_checkpoint":source.checkpoint(),"desired_recipe":desired.fingerprint(),"original_claim":original_pin,"state":state,"upstream_generation_checked":false,"primary_payloads_verified":false,"execution_attested":false,"authorizes_execution":false,"authorizes_eviction":false,"complete_semantic_coverage":false}),
        65536,
    )?;
    Ok(
        json!({"schema":"reel.generation-inspection-summary.experimental-1","state":state,"nodes":desired.nodes().len(),"upstream_generation_checked":false,"primary_payloads_verified":false,"authorizes_execution":false,"authorizes_eviction":false}),
    )
}
pub fn inspect(profile: &Path, output: &Path) -> Result<Value> {
    run(profile,output).map_err(|_|anyhow::anyhow!("generation inspection refused; verify source, definitions, request and original evidence"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    request: Request,
    producer: Object,
    original: Option<Original>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetInspection {
    schema: String,
    metadata_root: Option<PathBuf>,
    metadata_minimum: Option<Checkpoint>,
    source: InputSource,
    targets: Vec<Target>,
}
fn state_name(state: tig_assets::graph::recipe::assessment::State) -> &'static str {
    use tig_assets::graph::recipe::assessment::State;
    match state {
        State::Unbuilt => "unbuilt",
        State::Unproven => "unproven",
        State::ClaimedRecipeMatch => "claimed_recipe_match",
        State::StaleRecipe => "stale_recipe",
        State::Blocked => "blocked",
    }
}
fn run_set(path: &Path, output: &Path) -> Result<Value> {
    use tig_assets::graph::recipe::assessment::State;
    let p: SetInspection = read_profile(path, 1024 * 1024)?;
    if p.schema != "reel.tig-generation-set-inspection.experimental-1"
        || p.targets.is_empty()
        || p.targets.len() > 128
    {
        bail!("invalid generation set inspection scope");
    }
    let (mut source, mut reader) = open(p.source, p.metadata_root, p.metadata_minimum, output)?;
    let checkpoint = source.checkpoint().clone();
    let mut nodes = Vec::new();
    let mut details = BTreeMap::new();
    for target in p.targets {
        let request = target.request;
        if details.contains_key(&request.output_id) {
            bail!("duplicate generation target");
        }
        let unavailable = input_readiness(&source, &request, true)?;
        let upstream = upstream(&source, &request)?;
        let (desired, state, original_pin) = if unavailable.is_empty() {
            let (desired, state, original) = basis(
                &mut source,
                &request,
                &target.producer,
                target.original,
                &mut reader,
            )?;
            (Some(desired), Some(state), original)
        } else {
            let (state, original) =
                original_basis(&source, &request, None, target.original, &mut reader)?;
            (None, state, original)
        };
        let state = state.map(|state| match state {
            Correspondence::Unbuilt => State::Unbuilt,
            Correspondence::Unproven => State::Unproven,
            Correspondence::ClaimedRecipeMatch => State::ClaimedRecipeMatch,
            Correspondence::StaleRecipe => State::StaleRecipe,
        });
        details.insert(request.output_id.clone(), json!({"output_id":request.output_id,"desired_recipe":desired.as_ref().map(|p| p.fingerprint()),"original_claim":original_pin,"basis":state.map(state_name),"unavailable_inputs":unavailable,"upstream":upstream}));
        nodes.push(advice::Node {
            asset_id: request.output_id,
            checkpoint: checkpoint.clone(),
            basis: state,
            upstream,
        });
    }
    let missing: BTreeSet<_> = nodes
        .iter()
        .flat_map(|n| n.upstream.iter())
        .filter(|id| !details.contains_key(*id))
        .cloned()
        .collect();
    let ordered = advice::order(&checkpoint, &nodes, 128)?;
    let mut entries = Vec::new();
    let mut blocked = 0;
    let mut candidates = 0;
    for entry in ordered {
        let mut detail = details.remove(&entry.asset_id).unwrap();
        detail["advice"] = json!(state_name(entry.advice));
        detail["catch_up_candidate"] = json!(entry.advice.catch_up_candidate());
        blocked += usize::from(entry.advice == State::Blocked);
        candidates += usize::from(entry.advice.catch_up_candidate());
        entries.push(detail);
    }
    publish(
        output,
        &json!({"schema":"reel.generation-set-inspection.experimental-1","source_checkpoint":checkpoint,"entries":entries,"missing_upstream":missing,"declared_upstream_scope_complete":missing.is_empty(),"primary_payloads_verified":false,"execution_attested":false,"authorizes_execution":false,"authorizes_eviction":false,"complete_semantic_coverage":false}),
        1024 * 1024,
    )?;
    Ok(
        json!({"schema":"reel.generation-set-inspection-summary.experimental-1","targets":nodes.len(),"blocked":blocked,"catch_up_candidates":candidates,"missing_upstream":missing.len(),"declared_upstream_scope_complete":missing.is_empty(),"primary_payloads_verified":false,"authorizes_execution":false,"authorizes_eviction":false}),
    )
}
pub fn inspect_set(profile: &Path, output: &Path) -> Result<Value> {
    run_set(profile, output).map_err(|_|anyhow::anyhow!("generation set inspection refused; verify source, scope, definitions and original evidence"))
}
