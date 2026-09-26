//! One executable boundary for a selected scene language, including a
//! hash-bound editable ASS presentation layer when its template is selected.

use anyhow::{Context, Result, bail};
use reel::scene_authoring_inputs::read_verified_alignments;
use reel_assembly::scene_authoring::{
    Episode, RenderedSpan, Scene, ScenePolicy, ScopedBindings, ScoreUse, TemplateCatalog,
    audit_rendered_compositions, compile_native_event_spans, resolve_scene,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildManifest {
    schema: String,
    scene_id: String,
    language: String,
    catalog: String,
    episode: String,
    scene: String,
    policy: String,
    season_bindings: String,
    episode_bindings: String,
    scene_bindings: String,
    alignment_paths: String,
    semantic_delivery: String,
    #[serde(default)]
    template_receipt: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildIndex {
    schema: String,
    graph_id: String,
    project_root: String,
    jobs: Vec<IndexedJob>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EpisodeBuildManifest {
    schema: String,
    scene_build_index: String,
    conform_template: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexedJob {
    node_id: String,
    build_manifest: String,
    resolved_language: String,
    semantic_delivery: String,
}

#[derive(Serialize)]
struct BuildReceipt {
    schema: String,
    scene_id: String,
    language: String,
    authoring_fingerprint_sha256: String,
    selected_semantic_plan_sha256: String,
    selected_delivery_job_sha256: String,
    scene_delivery_receipt_sha256: String,
    master_sha256: String,
    master_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    template_ass_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_text_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    presentation_role: Option<String>,
    same_asset_composition_runs: usize,
    visual_inspection_state: String,
    publication: String,
}

fn checked(root: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("build manifest requires repository-root relative paths");
    }
    let target = root.join(path).canonicalize()?;
    if !target.starts_with(root.canonicalize()?) || !target.is_file() {
        bail!("build input escapes repository root: {relative}");
    }
    Ok(target)
}

fn read<T: DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&fs::read(path).with_context(|| path.display().to_string())?)
        .with_context(|| path.display().to_string())
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hash_file(path: &Path) -> Result<(String, u64)> {
    let mut input = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        bytes += read as u64;
    }
    Ok((
        digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        bytes,
    ))
}

fn local_file(id: &str, path: &Path) -> Result<serde_json::Value> {
    let path = path.canonicalize()?;
    let bytes = fs::read(&path)?;
    Ok(serde_json::json!({"file_id":id,"path":path,"sha256":hash(&bytes),"bytes":bytes.len()}))
}

fn bound_asset(
    id: &str,
    asset_root: &Path,
    item: &reel::scene_delivery::FileRef,
) -> Result<serde_json::Value> {
    let path = checked(
        asset_root,
        item.path.to_str().context("non-UTF8 selected asset path")?,
    )?;
    let measured = local_file(id, &path)?;
    if measured["sha256"] != item.sha256 || measured["bytes"] != item.bytes {
        bail!("selected asset differs from its SHA-256/byte binding: {id}");
    }
    Ok(measured)
}

fn emit_changed_only_graph(index_path: &str, asset_root: &Path, output_path: &str) -> Result<()> {
    let index_file = Path::new(index_path).canonicalize()?;
    let index: BuildIndex = read(&index_file)?;
    if index.schema != "reel.scene-build-index.v1"
        || index.graph_id.is_empty()
        || index.jobs.is_empty()
    {
        bail!("invalid scene build index");
    }
    let base = index_file.parent().context("build index has no parent")?;
    let root = base.join(&index.project_root).canonicalize()?;
    if !root.is_dir() {
        bail!("scene build index project root is not a directory");
    }
    let recipe = local_file("reel-scene-build", &env::current_exe()?)?;
    let mut names = BTreeSet::new();
    let mut nodes = Vec::new();
    for job in index.jobs {
        if !names.insert(job.node_id.clone()) {
            bail!("duplicate scene build node");
        }
        let manifest_path = checked(&root, &job.build_manifest)?;
        let manifest: BuildManifest = read(&manifest_path)?;
        if manifest.schema != "reel.scene-build.v1" || manifest.language.is_empty() {
            bail!("invalid indexed scene build manifest");
        }
        let delivery_path = checked(&root, &manifest.semantic_delivery)?;
        let indexed_delivery = checked(base, &job.semantic_delivery)?;
        if delivery_path != indexed_delivery {
            bail!("indexed semantic delivery differs from scene build manifest");
        }
        let semantic: serde_yaml::Value = serde_yaml::from_slice(&fs::read(&delivery_path)?)?;
        let job_relative = semantic
            .get("scene_delivery_job")
            .and_then(|value| value.get("path"))
            .and_then(|value| value.as_str())
            .context("indexed semantic delivery lacks scene delivery job path")?;
        let semantic_base = delivery_path
            .parent()
            .context("semantic delivery has no parent")?;
        let selected_job = checked(semantic_base, job_relative)?;
        let scene_job: reel::scene_delivery::Job =
            serde_yaml::from_slice(&fs::read(&selected_job)?)?;
        let (catalog, episode, scene, policy, season, episode_bindings, scene_bindings, _) =
            scene_inputs(&root, &manifest)?;
        let current = resolve_scene(
            &catalog,
            &episode,
            &scene,
            &policy,
            &season,
            &episode_bindings,
            &scene_bindings,
        )?;
        let expected_fingerprint = current
            .language_fingerprints
            .get(&manifest.language)
            .context("indexed scene language missing")?;
        let resolved_path = checked(base, &job.resolved_language)?;
        let resolved_data: serde_json::Value = read(&resolved_path)?;
        if resolved_data["schema"] != "reel.resolved-scene-language.v1"
            || resolved_data["scene_id"] != manifest.scene_id
            || resolved_data["language"] != manifest.language
            || resolved_data["fingerprint_sha256"] != *expected_fingerprint
        {
            bail!("indexed resolved language is stale for selected scene inputs");
        }
        let resolved = local_file("resolved-language", &resolved_path)?;
        let mut inputs = vec![
            local_file("build-manifest", &manifest_path)?,
            resolved,
            local_file("semantic-delivery", &delivery_path)?,
            local_file("scene-delivery-job", &selected_job)?,
        ];
        // The verified language fingerprint already projects only consumed
        // authoring, policy, template and scoped asset selections. Hashing the
        // full shared files here would invalidate unrelated scene languages.
        inputs.push(local_file(
            "alignment-paths",
            &checked(&root, &manifest.alignment_paths)?,
        )?);
        let alignment_manifest = checked(&root, &manifest.alignment_paths)?;
        let alignment_paths: BTreeMap<String, String> = read(&alignment_manifest)?;
        let alignment_base = alignment_manifest
            .parent()
            .context("alignment manifest has no parent")?;
        for (cue_id, relative) in alignment_paths {
            inputs.push(local_file(
                &format!("alignment-{cue_id}"),
                &checked(alignment_base, &relative)?,
            )?);
        }
        if let Some(receipt) = &manifest.template_receipt {
            inputs.push(local_file("template-receipt", &checked(&root, receipt)?)?);
        }
        inputs.push(bound_asset("contract", asset_root, &scene_job.contract)?);
        for (number, picture) in scene_job.pictures.iter().enumerate() {
            inputs.push(bound_asset(
                &format!("picture-{number:03}"),
                asset_root,
                &picture.source,
            )?);
        }
        for (number, audio) in scene_job.audio.iter().enumerate() {
            inputs.push(bound_asset(
                &format!("audio-{number:03}"),
                asset_root,
                &audio.source,
            )?);
        }
        for (number, layer) in scene_job.external_layers.iter().enumerate() {
            inputs.push(bound_asset(
                &format!("external-{number:03}"),
                asset_root,
                &layer.evidence,
            )?);
            if let Some(font) = &layer.font {
                inputs.push(bound_asset(&format!("font-{number:03}"), asset_root, font)?);
            }
        }
        nodes.push(
            serde_json::json!({"node_id":job.node_id,"operation_kind":"scene-delivery",
            "recipe":recipe,"inputs":inputs,"dependencies":[],
            "expected_outputs":["scene-master","scene-build-receipt","scene-delivery-receipt"]}),
        );
    }
    let graph = serde_json::json!({"schema":"reel.changed-only-graph.v0.1",
        "graph_id":index.graph_id,"nodes":nodes});
    use std::io::Write;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?
        .write_all(&serde_json::to_vec_pretty(&graph)?)?;
    println!("{} independent scene build nodes", names.len());
    Ok(())
}

fn execute_changed_only(
    index_path: &str,
    prior_state_path: &str,
    asset_root: &str,
    output_root: &str,
) -> Result<()> {
    let index_file = Path::new(index_path).canonicalize()?;
    let index: BuildIndex = read(&index_file)?;
    let base = index_file.parent().context("build index has no parent")?;
    let project_root = base.join(&index.project_root).canonicalize()?;
    let mut jobs = BTreeMap::new();
    for job in index.jobs {
        if jobs
            .insert(job.node_id.clone(), job.build_manifest)
            .is_some()
        {
            bail!("duplicate scene build node");
        }
    }
    let output = Path::new(output_root);
    fs::create_dir(output).context("changed-only run output must be new")?;
    let graph = output.join("graph.json");
    emit_changed_only_graph(
        index_path,
        Path::new(asset_root),
        graph.to_str().context("non-UTF8 graph path")?,
    )?;
    let mut state = Path::new(prior_state_path).canonicalize()?;
    let mut rebuilt = 0usize;
    let mut reused = 0usize;
    for (ordinal, (node_id, manifest)) in jobs.iter().enumerate() {
        let plan_path = output.join(format!("plan-{ordinal:03}.json"));
        reel::changed_only::write_changed_only_plan(&graph, &state, &plan_path)?;
        let plan: serde_json::Value = read(&plan_path)?;
        let node = plan["nodes"]
            .as_array()
            .and_then(|nodes| nodes.iter().find(|node| node["node_id"] == *node_id))
            .with_context(|| format!("changed-only plan omits indexed node {node_id}"))?;
        match node["status"].as_str() {
            Some("exact-byte-reuse") => {
                reused += 1;
                continue;
            }
            Some("rebuild") => {}
            other => bail!("scene node {node_id} cannot execute from status {other:?}"),
        }
        let action_key = node["action_key"]
            .as_str()
            .context("rebuild plan lacks action key")?;
        let scene_output = output.join(node_id);
        build_scene(
            project_root.to_str().context("non-UTF8 project root")?,
            manifest,
            asset_root,
            scene_output.to_str().context("non-UTF8 scene output")?,
        )?;
        let result_path = output.join(format!("result-{ordinal:03}.json"));
        let scene_receipt = scene_output.join("scene-authoring-build-receipt.json");
        let result = serde_json::json!({
            "schema":"reel.changed-only-result-input.v0.1",
            "graph_id":plan["graph_id"],"node_id":node_id,"action_key":action_key,
            "owner_result_id_sha256":hash(&fs::read(&scene_receipt)?),
            "outcome":"completed",
            "outputs":[
                {"file_id":"scene-master","path":scene_output.join("master.mkv")},
                {"file_id":"scene-build-receipt","path":scene_receipt},
                {"file_id":"scene-delivery-receipt","path":scene_output.join("receipt.json")}
            ]
        });
        fs::write(&result_path, serde_json::to_vec_pretty(&result)?)?;
        let receipt_path = output.join(format!("result-receipt-{ordinal:03}.json"));
        reel::changed_only::write_changed_only_result_receipt(
            &graph,
            &state,
            &plan_path,
            &result_path,
            &receipt_path,
        )?;
        let next_state = output.join(format!("state-{ordinal:03}.json"));
        reel::changed_only::advance_changed_only_state(
            &graph,
            &state,
            &plan_path,
            &result_path,
            &receipt_path,
            &next_state,
        )?;
        state = next_state;
        rebuilt += 1;
    }
    fs::copy(state, output.join("final-state.json"))?;
    println!("rebuilt {rebuilt} scene languages; reused {reused}");
    Ok(())
}

fn episode_output_ref(asset_root: &Path, entry: &serde_json::Value) -> Result<serde_json::Value> {
    let path = entry["path"]
        .as_str()
        .context("scene state output path missing")?;
    let absolute = Path::new(path).canonicalize()?;
    let root = asset_root.canonicalize()?;
    let relative = absolute
        .strip_prefix(&root)
        .context("scene output is outside asset root")?;
    let (digest, bytes) = hash_file(&absolute)?;
    if entry["sha256"] != digest || entry["bytes"] != bytes {
        bail!(
            "scene state output changed since verification: {}",
            absolute.display()
        );
    }
    Ok(serde_json::json!({
        "path":relative.to_string_lossy().replace('\\', "/"),
        "sha256":digest,"bytes":bytes
    }))
}

fn execute_episode(
    project_root: &str,
    episode_manifest: &str,
    prior_state: &str,
    asset_root: &str,
    output_root: &str,
) -> Result<()> {
    let root = Path::new(project_root).canonicalize()?;
    let asset_root = Path::new(asset_root).canonicalize()?;
    let manifest_path = checked(&root, episode_manifest)?;
    let specification: EpisodeBuildManifest = read(&manifest_path)?;
    if specification.schema != "reel.episode-build.v1" {
        bail!("unsupported episode build schema");
    }
    let index_path = checked(&root, &specification.scene_build_index)?;
    let index: BuildIndex = read(&index_path)?;
    let indexed_root = index_path
        .parent()
        .context("scene index has no parent")?
        .join(&index.project_root)
        .canonicalize()?;
    if indexed_root != root {
        bail!("episode scene index project root differs from selected project root");
    }
    let mut conform: serde_json::Value = read(&checked(&root, &specification.conform_template)?)?;
    if conform["schema"] != "reel.episode-conform.v1" {
        bail!("episode build conform template has wrong schema");
    }
    let language = conform["language"]
        .as_str()
        .context("conform language missing")?
        .to_owned();
    let segments = conform["segments"]
        .as_array()
        .context("conform segments missing")?;
    if segments.is_empty() {
        bail!("episode build has no conform segments");
    }
    let segment_count = segments.len();
    let indexed = index
        .jobs
        .iter()
        .map(|job| (job.node_id.as_str(), job))
        .collect::<BTreeMap<_, _>>();
    for segment in segments {
        if segment["kind"] != "scene" {
            continue;
        }
        let node_id = segment["scene_node_id"]
            .as_str()
            .context("scene conform template needs scene_node_id")?;
        let job = indexed
            .get(node_id)
            .with_context(|| format!("scene node {node_id} not indexed"))?;
        let indexed_manifest: BuildManifest = read(&checked(&root, &job.build_manifest)?)?;
        if segment["id"] != indexed_manifest.scene_id || language != indexed_manifest.language {
            bail!("conform scene {node_id} differs from indexed scene/language");
        }
        if segment["delivery_job"].is_null() {
            bail!("conform scene {node_id} needs exact delivery_job reference");
        }
        for field in ["master", "source_receipt", "delivery_receipt"] {
            if segment.get(field).is_some() {
                bail!("conform scene {node_id} must derive {field} from verified scene state");
            }
        }
    }
    let output = Path::new(output_root);
    let parent = output
        .parent()
        .context("episode output has no parent")?
        .canonicalize()?;
    if !parent.starts_with(&asset_root) || output.exists() {
        bail!("episode output must be a new directory within the asset root");
    }
    fs::create_dir(output)?;
    let scene_run = output.join("scenes");
    execute_changed_only(
        index_path.to_str().context("non-UTF8 scene index")?,
        prior_state,
        asset_root.to_str().context("non-UTF8 asset root")?,
        scene_run.to_str().context("non-UTF8 scene output")?,
    )?;
    let state: serde_json::Value = read(&scene_run.join("final-state.json"))?;
    if state["graph_id"] != index.graph_id {
        bail!("scene state graph differs from index");
    }
    for segment in conform["segments"]
        .as_array_mut()
        .context("conform segments missing")?
    {
        if segment["kind"] != "scene" {
            continue;
        }
        let node_id = segment["scene_node_id"]
            .as_str()
            .context("scene_node_id missing")?
            .to_owned();
        let node = state["nodes"]
            .as_array()
            .context("scene state nodes missing")?
            .iter()
            .find(|node| node["node_id"] == node_id)
            .with_context(|| format!("scene node {node_id} absent from final state"))?;
        let outputs = node["outputs"]
            .as_array()
            .context("scene state outputs missing")?;
        for (file_id, field) in [
            ("scene-master", "master"),
            ("scene-build-receipt", "source_receipt"),
            ("scene-delivery-receipt", "delivery_receipt"),
        ] {
            let item = outputs
                .iter()
                .find(|item| item["file_id"] == file_id)
                .with_context(|| format!("scene node {node_id} lacks {file_id}"))?;
            segment[field] = episode_output_ref(&asset_root, item)?;
        }
        segment
            .as_object_mut()
            .context("scene segment is not object")?
            .remove("scene_node_id");
    }
    let conform_path = output.join("conform.json");
    fs::write(&conform_path, serde_json::to_vec_pretty(&conform)?)?;
    let receipt =
        reel::episode_conform::build(&conform_path, &root, &asset_root, &output.join("episode"))?;
    println!(
        "{} {} built from {} ordered segments",
        receipt.episode_id, receipt.language, segment_count
    );
    Ok(())
}

fn scene_inputs(
    root: &Path,
    manifest: &BuildManifest,
) -> Result<(
    TemplateCatalog,
    Episode,
    Scene,
    ScenePolicy,
    ScopedBindings,
    ScopedBindings,
    ScopedBindings,
    PathBuf,
)> {
    Ok((
        read(&checked(root, &manifest.catalog)?)?,
        read(&checked(root, &manifest.episode)?)?,
        read(&checked(root, &manifest.scene)?)?,
        read(&checked(root, &manifest.policy)?)?,
        read(&checked(root, &manifest.season_bindings)?)?,
        read(&checked(root, &manifest.episode_bindings)?)?,
        read(&checked(root, &manifest.scene_bindings)?)?,
        checked(root, &manifest.semantic_delivery)?,
    ))
}

fn content_binding<'a>(
    content: &'a serde_json::Value,
    map_name: &str,
    language: &str,
) -> Result<&'a str> {
    content
        .get(map_name)
        .and_then(|items| items.get(language))
        .and_then(|item| item.as_str())
        .with_context(|| format!("template presentation requires {map_name} for {language}"))
}

fn receipt_string<'a>(receipt: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    receipt
        .get(key)
        .and_then(|value| value.as_str())
        .with_context(|| format!("template receipt lacks {key}"))
}

fn validate_template_layer(
    root: &Path,
    manifest: &BuildManifest,
    scene: &Scene,
    resolved: &reel_assembly::scene_authoring::ResolvedScene,
    job: &reel::scene_delivery::Job,
    plan: &reel::scene_delivery::Plan,
) -> Result<Option<(String, String)>> {
    let overlays = job
        .external_layers
        .iter()
        .filter(|layer| {
            layer.render_mode == reel::scene_delivery::ExternalLayerRenderMode::AssOverlay
        })
        .collect::<Vec<_>>();
    let Some(presentation) = &scene.presentation else {
        if manifest.template_receipt.is_some() || !overlays.is_empty() {
            bail!("ordinary scene cannot silently consume a template layer");
        }
        return Ok(None);
    };
    let path = checked(
        root,
        manifest
            .template_receipt
            .as_deref()
            .context("template presentation requires a selected compile receipt")?,
    )?;
    let bytes = fs::read(&path)?;
    let receipt: serde_json::Value = serde_json::from_slice(&bytes)?;
    if receipt_string(&receipt, "schema")? != "reel.editable-layer-compile-receipt.v1"
        || receipt_string(&receipt, "scene_id")? != scene.scene_id
        || receipt_string(&receipt, "language")? != manifest.language
        || receipt_string(&receipt, "template_id")? != presentation.template_id
        || receipt_string(&receipt, "template_definition_sha256")?
            != resolved.template_definitions[&presentation.template_id]
        || receipt
            .get("duration_samples")
            .and_then(|value| value.as_u64())
            != Some(plan.duration_samples)
        || receipt.get("sample_rate").and_then(|value| value.as_u64())
            != Some(plan.sample_rate as u64)
    {
        bail!("template compile receipt differs from selected scene and native clock");
    }
    let content = &presentation.content;
    let receipt_key = content_binding(content, "template_receipt_bindings", &manifest.language)?;
    let source_key = content_binding(content, "source_text_bindings", &manifest.language)?;
    let ass_key = content_binding(content, "ass_layer_bindings", &manifest.language)?;
    let selected = &resolved.selected_inputs;
    let receipt_asset = selected
        .get(receipt_key)
        .context("selected template receipt binding missing")?;
    let source_asset = selected
        .get(source_key)
        .context("selected source-text binding missing")?;
    let ass_asset = selected
        .get(ass_key)
        .context("selected ASS binding missing")?;
    if receipt_asset.sha256 != hash(&bytes)
        || receipt_asset.bytes != bytes.len() as u64
        || receipt_string(&receipt, "source_text_sha256")? != source_asset.sha256
        || receipt_string(&receipt, "ass_sha256")? != ass_asset.sha256
        || overlays.len() != 1
        || overlays[0].evidence.sha256 != ass_asset.sha256
        || overlays[0].evidence.bytes != ass_asset.bytes
    {
        bail!("template layer, source, or receipt differs from selected scoped bytes");
    }
    let font_key = presentation
        .asset_binding
        .as_deref()
        .context("template presentation requires a selected font binding")?;
    let font = selected
        .get(font_key)
        .context("selected template font missing")?;
    if overlays[0]
        .font
        .as_ref()
        .is_none_or(|item| item.sha256 != font.sha256 || item.bytes != font.bytes)
    {
        bail!("rendered template font differs from selected scoped binding");
    }
    Ok(Some((
        ass_asset.sha256.clone(),
        receipt_string(&receipt, "source_text_state")?.to_owned(),
    )))
}

fn build_scene(
    project_root: &str,
    manifest_path: &str,
    asset_root: &str,
    output_dir: &str,
) -> Result<()> {
    let root = Path::new(project_root).canonicalize()?;
    let manifest: BuildManifest = read(&checked(&root, manifest_path)?)?;
    if manifest.schema != "reel.scene-build.v1" || manifest.language.is_empty() {
        bail!("invalid scene build manifest");
    }
    let (catalog, episode, scene, policy, season, episode_bindings, scene_bindings, delivery_path) =
        scene_inputs(&root, &manifest)?;
    if scene.scene_id != manifest.scene_id {
        bail!("build manifest scene ID mismatch");
    }
    let resolved = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )?;
    let language_fingerprint = resolved
        .language_fingerprints
        .get(&manifest.language)
        .context("requested language not present in scene")?;
    if scene.presentation.is_some() && manifest.template_receipt.is_none() {
        bail!("scene template presentation requires a selected compile receipt");
    }
    let semantic_plan = reel::semantic_delivery::plan(&delivery_path)?;
    if semantic_plan.selection.closure.target != scene.scene_id {
        bail!("semantic delivery target differs from authored scene");
    }
    let active = semantic_plan
        .selection
        .closure
        .semantic_events
        .iter()
        .filter(|event| event.language == manifest.language)
        .map(|event| event.event_id.as_str())
        .collect::<BTreeSet<_>>();
    let authored = scene.languages[&manifest.language]
        .events
        .iter()
        .map(|event| event.event_id.as_str())
        .collect::<BTreeSet<_>>();
    if active != authored {
        bail!("selected REEL event set differs from scene authoring");
    }
    let semantic_bytes = fs::read(&delivery_path)?;
    let semantic: reel::semantic_delivery::SemanticDelivery =
        serde_yaml::from_slice(&semantic_bytes)?;
    let job_path = delivery_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(&semantic.scene_delivery_job.path);
    let job: reel::scene_delivery::Job = serde_yaml::from_slice(&fs::read(&job_path)?)?;
    let (_, delivery_plan) = reel::scene_delivery::plan(&job_path, Path::new(asset_root))?;
    let alignments = read_verified_alignments(
        &scene,
        &manifest.language,
        &checked(&root, &manifest.alignment_paths)?,
        &[&scene_bindings, &episode_bindings, &season],
    )?;
    let native_spans = compile_native_event_spans(
        &manifest.language,
        &scene.languages[&manifest.language],
        &alignments,
    )?;
    validate_authored_semantic_assets(
        &scene,
        &episode,
        &resolved,
        &semantic,
        &semantic_plan.selection,
        &job,
        &manifest.language,
        &alignments,
    )?;
    validate_semantic_timeline(
        &semantic,
        &semantic_plan.selection,
        &job,
        &delivery_plan,
        &manifest.language,
        &native_spans,
        &alignments,
    )?;
    let template =
        validate_template_layer(&root, &manifest, &scene, &resolved, &job, &delivery_plan)?;
    let output = Path::new(output_dir);
    let rendered = reel::semantic_delivery::render(&delivery_path, Path::new(asset_root), output)?;
    let checked_receipt = reel::scene_delivery::check(&job_path, Path::new(asset_root), output)?;
    if checked_receipt.plan.pictures.len() != job.pictures.len() {
        bail!("rendered picture plan differs from selected job");
    }
    let picture_spans = checked_receipt
        .plan
        .pictures
        .iter()
        .zip(&job.pictures)
        .map(|(span, picture)| RenderedSpan {
            composition_id: format!("{}:{:?}", picture.source.sha256, picture.crop),
            start_sample: span.start_sample,
            end_sample: span.end_sample,
        })
        .collect::<Vec<_>>();
    let runs =
        audit_rendered_compositions(&policy, checked_receipt.plan.sample_rate, &picture_spans)?;
    let master = checked_receipt
        .outputs
        .get("master.mkv")
        .context("scene delivery receipt lacks master.mkv")?;
    let receipt = BuildReceipt {
        schema: "reel.scene-build-receipt.v1".into(),
        scene_id: scene.scene_id,
        language: manifest.language,
        authoring_fingerprint_sha256: language_fingerprint.clone(),
        selected_semantic_plan_sha256: hash(&serde_json::to_vec(&semantic_plan)?),
        selected_delivery_job_sha256: hash(&fs::read(&job_path)?),
        scene_delivery_receipt_sha256: hash(&fs::read(output.join("receipt.json"))?),
        master_sha256: master.sha256.clone(),
        master_bytes: master.bytes,
        template_ass_sha256: template.as_ref().map(|item| item.0.clone()),
        source_text_state: template.as_ref().map(|item| item.1.clone()),
        presentation_role: scene.presentation.as_ref().map(|item| item.role.clone()),
        same_asset_composition_runs: runs.len(),
        visual_inspection_state:
            "open; source-and-crop grouping does not prove visible distinctness".into(),
        publication: "not-authorized".into(),
    };
    if serde_json::to_vec(&rendered.outputs)? != serde_json::to_vec(&checked_receipt.outputs)? {
        bail!("render and independent check disagree on output bytes");
    }
    let output_receipt = output.join("scene-authoring-build-receipt.json");
    let bytes = serde_json::to_vec_pretty(&receipt)?;
    use std::io::Write;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_receipt)?
        .write_all(&bytes)?;
    println!(
        "{} {} {}",
        receipt.scene_id, receipt.language, receipt.authoring_fingerprint_sha256
    );
    Ok(())
}

fn validate_authored_semantic_assets(
    scene: &Scene,
    episode: &Episode,
    resolved: &reel_assembly::scene_authoring::ResolvedScene,
    semantic: &reel::semantic_delivery::SemanticDelivery,
    selection: &reel_assembly::SelectedClosure,
    job: &reel::scene_delivery::Job,
    language: &str,
    alignments: &BTreeMap<String, reel_assembly::scene_authoring::NativeAlignment>,
) -> Result<()> {
    let lane = &scene.languages[language];
    let selected_hash = |key: &str| -> Result<String> {
        Ok(resolved
            .selected_inputs
            .get(key)
            .with_context(|| format!("selected asset binding {key} missing"))?
            .sha256
            .clone())
    };
    for cue in &lane.cues {
        let key = cue
            .take_binding
            .as_deref()
            .context("ready cue lacks take binding")?;
        if selected_hash(key)? != alignments[&cue.cue_id].selected_take_sha256 {
            bail!(
                "cue {} selected take differs from native alignment",
                cue.cue_id
            );
        }
    }
    for authored in &lane.events {
        let graph = selection
            .closure
            .semantic_events
            .iter()
            .find(|event| event.event_id == authored.event_id)
            .with_context(|| {
                format!(
                    "authored event {} absent from selected graph",
                    authored.event_id
                )
            })?;
        if graph.picture.sha256
            != selected_hash(
                authored
                    .picture_binding
                    .as_deref()
                    .context("ready event lacks picture binding")?,
            )?
        {
            bail!(
                "event {} graph picture differs from authored selection",
                authored.event_id
            );
        }
        let binding = semantic
            .event_bindings
            .iter()
            .find(|binding| binding.event_id == authored.event_id)
            .with_context(|| format!("event {} lacks delivery binding", authored.event_id))?;
        let mut actual_music = Vec::new();
        let mut actual_sonic = Vec::new();
        for id in &binding.audio_attachment_ids {
            let item = job
                .audio
                .iter()
                .find(|item| item.attachment_id == *id)
                .with_context(|| format!("audio attachment {id} missing"))?;
            match item.bus.as_str() {
                "M" => actual_music.push(item.source.sha256.clone()),
                "E" => actual_sonic.push(item.source.sha256.clone()),
                _ => bail!("event {} optional audio must use M or E", authored.event_id),
            }
        }
        let mut expected_music = match &authored.score {
            ScoreUse::Role { role } => {
                let palette = episode
                    .score_palette
                    .iter()
                    .find(|item| item.role == *role)
                    .with_context(|| format!("unknown score role {role}"))?;
                vec![selected_hash(&palette.asset_binding)?]
            }
            ScoreUse::Silence => Vec::new(),
            ScoreUse::Held => bail!("event {} score choice remains held", authored.event_id),
        };
        let mut expected_sonic = authored
            .sonic_bindings
            .iter()
            .map(|key| selected_hash(key))
            .collect::<Result<Vec<_>>>()?;
        actual_music.sort();
        actual_sonic.sort();
        expected_music.sort();
        expected_sonic.sort();
        if actual_music != expected_music || actual_sonic != expected_sonic {
            bail!(
                "event {} M/E attachments differ from authored score or Sonic",
                authored.event_id
            );
        }
        let mut actual_vfx = binding
            .external_layer_attachment_ids
            .iter()
            .map(|id| {
                let item = job
                    .external_layers
                    .iter()
                    .find(|item| item.attachment_id == *id)
                    .with_context(|| format!("external attachment {id} missing"))?;
                if item.render_mode == reel::scene_delivery::ExternalLayerRenderMode::AssOverlay {
                    return Ok(None);
                }
                Ok(Some(item.evidence.sha256.clone()))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let mut expected_vfx = authored
            .vfx_bindings
            .iter()
            .map(|key| selected_hash(key))
            .collect::<Result<Vec<_>>>()?;
        actual_vfx.sort();
        expected_vfx.sort();
        if actual_vfx != expected_vfx {
            bail!(
                "event {} external attachments differ from authored VFX",
                authored.event_id
            );
        }
    }
    Ok(())
}

fn validate_semantic_timeline(
    semantic: &reel::semantic_delivery::SemanticDelivery,
    selection: &reel_assembly::SelectedClosure,
    job: &reel::scene_delivery::Job,
    plan: &reel::scene_delivery::Plan,
    language: &str,
    native_spans: &[reel_assembly::scene_authoring::NativeEventSpan],
    alignments: &BTreeMap<String, reel_assembly::scene_authoring::NativeAlignment>,
) -> Result<()> {
    fn spans<'a>(
        id: &str,
        items: &'a [reel::scene_delivery::Span],
    ) -> Option<&'a reel::scene_delivery::Span> {
        items.iter().find(|span| span.attachment_id == id)
    }
    for event in selection
        .closure
        .semantic_events
        .iter()
        .filter(|event| event.language == language && event.scene_id == semantic.target)
    {
        let native = native_spans
            .iter()
            .find(|span| span.event_id == event.event_id)
            .with_context(|| format!("missing selected native alignment for {}", event.event_id))?;
        let alignment = alignments
            .get(&native.cue_id)
            .with_context(|| format!("missing native cue alignment for {}", event.event_id))?;
        if native.sample_rate != plan.sample_rate
            || event.narration.sha256 != alignment.selected_take_sha256
        {
            bail!(
                "semantic event {} differs from selected native take or sample rate",
                event.event_id
            );
        }
        let binding = semantic
            .event_bindings
            .iter()
            .find(|binding| binding.event_id == event.event_id)
            .with_context(|| format!("missing timeline binding for {}", event.event_id))?;
        let narration = spans(&binding.narration_attachment_id, &plan.audio)
            .with_context(|| format!("missing narration span for {}", event.event_id))?;
        let picture = spans(&binding.picture_attachment_id, &plan.pictures)
            .with_context(|| format!("missing picture span for {}", event.event_id))?;
        let narration_job = job
            .audio
            .iter()
            .find(|item| item.attachment_id == binding.narration_attachment_id)
            .with_context(|| format!("missing narration job for {}", event.event_id))?;
        if narration_job.cue_id.as_deref() != Some(native.cue_id.as_str()) {
            bail!(
                "semantic event {} narration attachment uses another cue",
                event.event_id
            );
        }
        if !event.phrase_start_seconds.is_finite()
            || !event.phrase_end_seconds.is_finite()
            || event.phrase_start_seconds < 0.0
            || event.phrase_end_seconds <= event.phrase_start_seconds
        {
            bail!("invalid native phrase clock for {}", event.event_id);
        }
        let sample = |seconds: f64| -> Result<u64> {
            let value = seconds * f64::from(plan.sample_rate);
            if value > u64::MAX as f64 {
                bail!("native phrase clock overflows for {}", event.event_id);
            }
            Ok(value.round() as u64)
        };
        if sample(event.phrase_start_seconds)? != native.start_sample
            || sample(event.phrase_end_seconds)? != native.end_sample
        {
            bail!(
                "semantic event {} phrase clock differs from selected alignment",
                event.event_id
            );
        }
        let start = narration
            .start_sample
            .checked_add(sample(event.phrase_start_seconds)?)
            .and_then(|value| value.checked_sub(narration_job.source_start_sample))
            .with_context(|| {
                format!(
                    "native phrase start outside narration for {}",
                    event.event_id
                )
            })?;
        let end = narration
            .start_sample
            .checked_add(sample(event.phrase_end_seconds)?)
            .and_then(|value| value.checked_sub(narration_job.source_start_sample))
            .with_context(|| {
                format!("native phrase end outside narration for {}", event.event_id)
            })?;
        if start < narration.start_sample
            || end > narration.end_sample
            || start >= end
            || start < picture.start_sample
            || end > picture.end_sample
        {
            bail!(
                "semantic event {} picture or narration misses native phrase span",
                event.event_id
            );
        }
        for id in &binding.audio_attachment_ids {
            let item = job
                .audio
                .iter()
                .find(|item| item.attachment_id == *id)
                .with_context(|| format!("missing optional audio job {id}"))?;
            let span = spans(id, &plan.audio)
                .with_context(|| format!("missing optional audio span {id}"))?;
            if item.bus == "D" || span.start_sample >= end || span.end_sample <= start {
                bail!(
                    "semantic event {} M/E audio misses native phrase span: {id}",
                    event.event_id
                );
            }
        }
        for id in &binding.external_layer_attachment_ids {
            let span = spans(id, &plan.external_layer_spans)
                .with_context(|| format!("missing external layer span {id}"))?;
            if span.start_sample >= end || span.end_sample <= start {
                bail!(
                    "semantic event {} external layer misses native phrase span: {id}",
                    event.event_id
                );
            }
        }
    }
    Ok(())
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if let [
        _,
        command,
        project_root,
        episode_manifest,
        state,
        asset_flag,
        asset_root,
        output_flag,
        output_root,
    ] = args.as_slice()
    {
        if command == "execute-episode"
            && asset_flag == "--asset-root"
            && output_flag == "--output-root"
        {
            return execute_episode(
                project_root,
                episode_manifest,
                state,
                asset_root,
                output_root,
            );
        }
    }
    if let [
        _,
        command,
        index,
        root_flag,
        asset_root,
        output_flag,
        output,
    ] = args.as_slice()
    {
        if command == "emit-changed-only-graph"
            && root_flag == "--asset-root"
            && output_flag == "--output"
        {
            return emit_changed_only_graph(index, Path::new(asset_root), output);
        }
    }
    if let [
        _,
        command,
        index,
        state,
        root_flag,
        asset_root,
        output_flag,
        output_root,
    ] = args.as_slice()
    {
        if command == "execute-changed-only"
            && root_flag == "--asset-root"
            && output_flag == "--output-root"
        {
            return execute_changed_only(index, state, asset_root, output_root);
        }
        if command == "build" && root_flag == "--asset-root" && output_flag == "--output-dir" {
            return build_scene(index, state, asset_root, output_root);
        }
    }
    bail!(
        "usage: reel-scene-build build <project-root> <build.json> --asset-root <cache-root> --output-dir <new-dir>\n       reel-scene-build emit-changed-only-graph <index.json> --asset-root <cache-root> --output <new-graph.json>\n       reel-scene-build execute-changed-only <index.json> <prior-state.json> --asset-root <cache-root> --output-root <new-run-dir>\n       reel-scene-build execute-episode <project-root> <episode-build.json> <prior-state.json> --asset-root <hydrated-root> --output-root <new-dir-within-hydrated-root>"
    )
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
