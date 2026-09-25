//! One executable boundary for a selected, ordinary scene language. A scene
//! with template presentation is held until a real template renderer supplies
//! its separately editable layer.

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
}

#[derive(Serialize)]
struct BuildReceipt {
    schema: String,
    scene_id: String,
    language: String,
    authoring_fingerprint_sha256: String,
    selected_semantic_plan_sha256: String,
    scene_delivery_receipt_sha256: String,
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

fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
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
            "usage: reel-scene-build build <project-root> <build.json> --asset-root <cache-root> --output-dir <new-dir>"
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
    if scene.presentation.is_some() {
        bail!(
            "scene template presentation requires a selected generic layer render before this build"
        );
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
    let output = Path::new(output_dir);
    let rendered = reel::semantic_delivery::render(&delivery_path, Path::new(asset_root), output)?;
    let semantic: reel::semantic_delivery::SemanticDelivery =
        serde_yaml::from_slice(&semantic_bytes)?;
    let job_path = delivery_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(&semantic.scene_delivery_job.path);
    let checked_receipt = reel::scene_delivery::check(&job_path, Path::new(asset_root), output)?;
    let job: reel::scene_delivery::Job = serde_yaml::from_slice(&fs::read(&job_path)?)?;
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
