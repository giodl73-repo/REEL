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
    collections::BTreeSet,
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
    jobs: Vec<IndexedJob>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexedJob {
    node_id: String,
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

fn emit_changed_only_graph(index_path: &str, output_path: &str) -> Result<()> {
    let index_file = Path::new(index_path).canonicalize()?;
    let index: BuildIndex = read(&index_file)?;
    if index.schema != "reel.scene-build-index.v1"
        || index.graph_id.is_empty()
        || index.jobs.is_empty()
    {
        bail!("invalid scene build index");
    }
    let base = index_file.parent().context("build index has no parent")?;
    let recipe = local_file("reel-scene-build", &env::current_exe()?)?;
    let mut names = BTreeSet::new();
    let mut nodes = Vec::new();
    for job in index.jobs {
        if !names.insert(job.node_id.clone()) {
            bail!("duplicate scene build node");
        }
        let resolved = local_file("resolved-language", &base.join(&job.resolved_language))?;
        let delivery = local_file("semantic-delivery", &base.join(&job.semantic_delivery))?;
        nodes.push(
            serde_json::json!({"node_id":job.node_id,"operation_kind":"scene-delivery",
            "recipe":recipe,"inputs":[resolved,delivery],"dependencies":[],
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

fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if let [_, command, index, flag, output] = args.as_slice() {
        if command == "emit-changed-only-graph" && flag == "--output" {
            return emit_changed_only_graph(index, output);
        }
    }
    let [
        _,
        command,
        project_root,
        manifest_path,
        flag_root,
        asset_root,
        flag_output,
        output_dir,
    ] = args.as_slice()
    else {
        bail!(
            "usage: reel-scene-build build <project-root> <build.json> --asset-root <cache-root> --output-dir <new-dir>\n       reel-scene-build emit-changed-only-graph <index.json> --output <new-graph.json>"
        );
    };
    if command != "build" || flag_root != "--asset-root" || flag_output != "--output-dir" {
        bail!(
            "usage: reel-scene-build build <project-root> <build.json> --asset-root <cache-root> --output-dir <new-dir>"
        );
    }
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

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
