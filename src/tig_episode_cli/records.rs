//! Shared output-record contracts; no selection, Git execution or publication.
use super::*;
use crate::tig_native_job_cli::{PhaseResult, records as shared, retained};
use std::collections::BTreeSet;
use tig_store::existing::{ByteSource, ReadCache};

pub(super) fn limits(inputs: &Inputs) -> Result<BTreeMap<String, u64>> {
    shared::artifact_limits(&inputs.record_outputs, inputs.git_handoff.as_ref())
}
pub(super) fn contract(inputs: &Inputs) -> Result<Option<Object>> {
    if inputs.record_outputs.is_empty() && inputs.git_handoff.is_none() {
        return Ok(None);
    }
    Ok(Some(Object::from_bytes(&identity::canonical_json(
        &json!({
            "schema":"reel.tig-episode-record-contract.experimental-1",
            "outputs":inputs.record_outputs,"git_handoff":inputs.git_handoff,
        }),
    )?)))
}
pub(super) fn prepare(
    inputs: &Inputs,
    metadata_root: &Path,
    minimum: &Checkpoint,
    context: &ProductionContext,
    roots: &[PathBuf],
) -> Result<shared::Prepared> {
    let request = &inputs.request;
    let mut input_definitions: BTreeSet<&str> = [
        request.assembly_definition.as_str(),
        request.episode_definition.as_str(),
    ]
    .into_iter()
    .chain(request.scene_definitions.iter().map(String::as_str))
    .chain(request.component_definitions.iter().map(String::as_str))
    .collect();
    for scene in &inputs.scenes {
        input_definitions.insert(&scene.request.scene_definition);
        input_definitions.extend(scene.request.primary_definitions.iter().map(String::as_str));
        input_definitions.extend(scene.request.control_definitions.iter().map(String::as_str));
    }
    for p in &inputs.presentations {
        let r = &p.request;
        input_definitions.extend([
            r.source_definition.as_str(),
            r.template_definition.as_str(),
            r.timing_definition.as_str(),
            r.clock_definition.as_str(),
            r.evidence_definition.as_str(),
        ]);
        input_definitions.extend(r.primary_definitions.iter().map(String::as_str));
    }
    shared::prepare_admission(shared::Admission {
        source: &inputs.source,
        metadata_root,
        metadata_minimum: minimum,
        specs: &inputs.record_outputs,
        handoff: inputs.git_handoff.as_ref(),
        input_definitions,
        context,
        roots,
    })
}

/// Called under the root fence before completing (or resuming) owner handoff.
/// Pins come from the checked runner result tree, not caller-supplied locators.
pub(super) fn verify_completed(
    p: &Profile,
    journal: &SourceJournal,
    current: &Selection,
    resources: &Selection,
    retained_entries: &BTreeMap<LogicalPath, Object>,
    reader: &mut impl NodeStore,
) -> Result<Option<Value>> {
    if p.inputs.record_outputs.is_empty() {
        return Ok(None);
    }
    let mut names = p
        .outputs
        .keys()
        .map(|name| format!("native/{name}"))
        .collect::<Vec<_>>();
    names.push("worker.json".into());
    names.extend(limits(&p.inputs)?.into_keys());
    let mut cache = ReadCache::new(serde_json::from_value(json!({
        "places":p.cache.cache_roots.iter().map(|root| json!({"root":root,"layout":"reel-cas"})).collect::<Vec<_>>(),
        "required":"prevent-mutation",
        "limits":{"object_bytes":p.budget_bytes,"total_bytes":p.budget_bytes,"requests":names.len(),"live_readers":names.len(),"verify_millis":300000},
    }))?)?;
    let mut guards = Vec::new();
    let mut outputs = serde_json::Map::new();
    for name in names {
        let object = retained_entries
            .get(&LogicalPath::parse(&format!(
                "outputs/{}",
                Digest::blob(name.as_bytes()).as_str()
            ))?)
            .context("episode candidate retained output missing")?;
        let guard = cache.open_verified(object)?;
        let path = guard.with_path(Path::to_path_buf)?;
        outputs.insert(
            name,
            json!({"sha256":object.sha256,"bytes":object.bytes,"path":path}),
        );
        guards.push(guard);
    }
    let header = retained_entries
        .get(&LogicalPath::parse("retention/identity")?)
        .context("episode retention header missing")?;
    let header = document(reader, header)?;
    let minimum = MetadataReader::open(&p.metadata_root, &p.metadata_minimum)?
        .checkpoint()
        .clone();
    let phase = PhaseResult {
        value: json!({"outputs":outputs,"job_id":header["job_id"]}),
        revision: p
            .expected_revision
            .checked_add(4)
            .context("episode revision overflow")?,
        resources: resources.clone(),
        minimum,
    };
    let verified = retained::verify_selection(
        &p.metadata_root,
        &p.source_id,
        &p.owner,
        &phase,
        journal,
        current.clone(),
    )?;
    let wait = MetadataWait::default();
    let (mut source, context) = restore(&p.inputs, &p.metadata_root, &phase.minimum, &wait)?;
    let programs = BoundPrograms::bind(&p.inputs.runtime)?;
    let recipe = tig_episode::composition_recipe(
        &mut source,
        &p.inputs.request,
        &context,
        &Object::from_bytes(programs.manifest_bytes()),
    )?;
    let templates = prepare(
        &p.inputs,
        &p.metadata_root,
        &phase.minimum,
        &context,
        &p.cache.cache_roots,
    )?;
    shared::readback_verified(&templates, &p.inputs.context, &recipe, &phase, &verified)
}
fn document(reader: &impl NodeStore, object: &Object) -> Result<Value> {
    if object.bytes > 65536 {
        bail!("episode record metadata exceeds bound");
    }
    Ok(identity::parse_json(
        &reader
            .get(object)?
            .context("episode record metadata missing")?,
    )?)
}
