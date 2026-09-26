//! One executable boundary for a selected scene language, including a
//! hash-bound editable ASS presentation layer when its template is selected.

use anyhow::{Context, Result, bail};
use reel_assembly::scene_authoring::{
    Episode, RenderedSpan, Scene, ScenePolicy, ScopedBindings, TemplateCatalog,
    audit_rendered_compositions, resolve_scene,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
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
    scene_delivery_receipt_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    template_ass_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_text_state: Option<String>,
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
        let indexed_delivery = base.join(&job.semantic_delivery).canonicalize()?;
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
        let resolved = local_file("resolved-language", &base.join(&job.resolved_language))?;
        let mut inputs = vec![
            local_file("build-manifest", &manifest_path)?,
            resolved,
            local_file("semantic-delivery", &delivery_path)?,
            local_file("scene-delivery-job", &selected_job)?,
        ];
        for (id, relative) in [
            ("catalog", &manifest.catalog),
            ("episode", &manifest.episode),
            ("scene", &manifest.scene),
            ("policy", &manifest.policy),
            ("season-bindings", &manifest.season_bindings),
            ("episode-bindings", &manifest.episode_bindings),
            ("scene-bindings", &manifest.scene_bindings),
        ] {
            inputs.push(local_file(id, &checked(&root, relative)?)?);
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
            "expected_outputs":["scene-master","scene-build-receipt"]}),
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
                {"file_id":"scene-build-receipt","path":scene_receipt}
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
    let receipt = BuildReceipt {
        schema: "reel.scene-build-receipt.v1".into(),
        scene_id: scene.scene_id,
        language: manifest.language,
        authoring_fingerprint_sha256: language_fingerprint.clone(),
        selected_semantic_plan_sha256: hash(&serde_json::to_vec(&semantic_plan)?),
        scene_delivery_receipt_sha256: hash(&serde_json::to_vec(&checked_receipt)?),
        template_ass_sha256: template.as_ref().map(|item| item.0.clone()),
        source_text_state: template.as_ref().map(|item| item.1.clone()),
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

fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
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
        "usage: reel-scene-build build <project-root> <build.json> --asset-root <cache-root> --output-dir <new-dir>\n       reel-scene-build emit-changed-only-graph <index.json> --asset-root <cache-root> --output <new-graph.json>\n       reel-scene-build execute-changed-only <index.json> <prior-state.json> --asset-root <cache-root> --output-root <new-run-dir>"
    )
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
