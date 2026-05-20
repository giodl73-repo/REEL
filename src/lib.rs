use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use sha2::{Digest, Sha256};
use tempfile::tempdir;

pub mod adapters;

const SUPPORTED_MANIFEST_VERSION: &str = "reel.manifest.v0.1";
const ARTIFACT_MANIFEST_SCHEMA_VERSION: &str = "reel.artifacts.v0.2";

const REQUIRED_TOP_FIELDS: &[&str] = &[
    "manifest_version",
    "work",
    "title",
    "source_scenario",
    "format",
    "style",
    "audience",
    "platforms",
    "continuity",
    "scenes",
    "shots",
    "audio",
    "captions",
    "renderer_assumptions",
    "exports",
    "review",
];

const REQUIRED_TOP_SCALAR_FIELDS: &[&str] =
    &["manifest_version", "work", "title", "format", "style"];

const REQUIRED_SCENE_FIELDS: &[&str] = &[
    "id",
    "purpose",
    "duration_seconds",
    "story_beat",
    "location",
    "characters",
    "continuity_notes",
];

const REQUIRED_SHOT_FIELDS: &[&str] = &[
    "id",
    "scene_id",
    "start_seconds",
    "duration_seconds",
    "camera",
    "action",
    "visual_prompt",
    "style_constraints",
    "audio",
    "captions",
    "transition_out",
];

const REQUIRED_PLATFORM_FIELDS: &[&str] = &[
    "name",
    "aspect_ratio",
    "target_duration_seconds",
    "sound_optional",
];

const REQUIRED_EXPORT_FIELDS: &[&str] = &["id", "filename", "aspect_ratio", "duration_seconds"];

const REQUIRED_SOURCE_SCENARIO_FIELDS: &[&str] = &["repo", "path", "id", "source_commit"];
const REQUIRED_AUDIENCE_FIELDS: &[&str] = &["primary", "context", "desired_effect"];
const REQUIRED_AUDIO_FIELDS: &[&str] = &[
    "narration_voice",
    "music_direction",
    "effects_direction",
    "silence_notes",
];
const REQUIRED_CAPTIONS_FIELDS: &[&str] = &["required", "style", "language"];
const REQUIRED_RENDERER_ASSUMPTIONS_FIELDS: &[&str] = &["candidates", "blockers"];
const REQUIRED_REVIEW_FIELDS: &[&str] = &["required_roles", "status"];

#[derive(Debug)]
pub struct LoadedManifest {
    pub path: PathBuf,
    raw: Value,
    manifest: Manifest,
}

#[derive(Debug)]
pub struct ValidationReport {
    pub scene_total: f64,
    pub shot_total: f64,
    pub exports: Vec<ExportPlan>,
}

#[derive(Debug, Serialize)]
pub struct CorpusReport {
    pub works_root: String,
    pub works: usize,
    pub manifests: Vec<String>,
    pub manifest_versions: Vec<String>,
    pub work_ids: Vec<String>,
    pub work_titles: Vec<String>,
    pub source_repos: Vec<String>,
    pub source_ids: Vec<String>,
    pub source_paths: Vec<String>,
    pub source_commits: Vec<String>,
    pub audience_primaries: Vec<String>,
    pub audience_contexts: Vec<String>,
    pub audience_desired_effects: Vec<String>,
    pub formats: Vec<String>,
    pub styles: Vec<String>,
    pub alternate_styles: Vec<String>,
    pub platform_names: Vec<String>,
    pub platforms: usize,
    pub scenes: usize,
    pub shots: usize,
    pub exports: usize,
    pub total_scene_duration_seconds: f64,
    pub total_shot_duration_seconds: f64,
    pub reports: Vec<CorpusWorkReport>,
}

#[derive(Debug, Serialize)]
pub struct CorpusWorkReport {
    pub manifest: String,
    pub manifest_version: String,
    pub work: String,
    pub title: String,
    pub source_repo: String,
    pub source_id: String,
    pub source_path: String,
    pub source_commit: String,
    pub audience_primary: String,
    pub audience_context: String,
    pub audience_desired_effect: String,
    pub format: String,
    pub style: String,
    pub alternate_styles: Vec<String>,
    pub platform_names: Vec<String>,
    pub platforms: usize,
    pub scenes: usize,
    pub shots: usize,
    pub exports: usize,
    pub scene_duration_seconds: f64,
    pub shot_duration_seconds: f64,
}

#[derive(Debug, Serialize)]
pub struct ReviewQueueReport {
    pub works_root: String,
    pub works: usize,
    pub manifests: Vec<String>,
    pub review_statuses: Vec<String>,
    pub review_status_counts: BTreeMap<String, usize>,
    pub review_status_work_ids: BTreeMap<String, Vec<String>>,
    pub review_status_work_titles: BTreeMap<String, Vec<String>>,
    pub required_roles: Vec<String>,
    pub required_role_counts: BTreeMap<String, usize>,
    pub required_role_status_counts: BTreeMap<String, BTreeMap<String, usize>>,
    pub reports: Vec<ReviewQueueWorkReport>,
}

#[derive(Debug, Serialize)]
pub struct ReviewQueueWorkReport {
    pub manifest: String,
    pub work: String,
    pub title: String,
    pub review_status: String,
    pub required_roles: Vec<String>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct ExportPlan {
    pub id: String,
    pub aspect_ratio: String,
    pub width: u32,
    pub height: u32,
    pub duration_seconds: f64,
    pub duration_scale: f64,
    pub filename: String,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct ScenePlan {
    pub scene_id: String,
    pub platform: String,
    pub source_start_seconds: f64,
    pub source_duration_seconds: f64,
    pub render_duration_seconds: f64,
    pub width: u32,
    pub height: u32,
    pub duration_scale: f64,
    pub shots: Vec<SceneShotPlan>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct SceneShotPlan {
    pub id: String,
    pub source_start_seconds: f64,
    pub source_duration_seconds: f64,
    pub render_start_seconds: f64,
    pub render_duration_seconds: f64,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct AdapterPlanEntry {
    pub id: adapters::AdapterId,
    pub status: adapters::AdapterStatus,
    pub declared_by_manifest: bool,
    pub operations: Vec<adapters::RenderOperationKind>,
    pub boundary: &'static str,
    pub dependency_policy: &'static str,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ArtifactManifest {
    pub schema_version: String,
    pub work: String,
    pub title: String,
    pub manifest: String,
    pub generated_unix: u64,
    pub baseline_adapter: String,
    pub platforms: Vec<ArtifactPlatform>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ArtifactPlatform {
    pub id: String,
    pub aspect_ratio: String,
    pub width: u32,
    pub height: u32,
    pub shot_cards: ArtifactVideo,
    pub contact_sheet: ArtifactImage,
    pub work_preview: ArtifactVideo,
    pub scene_previews: Vec<ArtifactScenePreview>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ArtifactScenePreview {
    pub scene_id: String,
    pub video: ArtifactVideo,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ArtifactVideo {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    pub duration_seconds: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ArtifactImage {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
pub struct ArtifactCheckReport {
    pub artifact_manifest: String,
    pub schema_version: String,
    pub generated_unix: u64,
    pub checked_unix: u64,
    pub source_manifest: String,
    pub work: String,
    pub title: String,
    pub baseline_adapter: String,
    pub platforms: usize,
    pub scene_previews: usize,
    pub video_files: usize,
    pub image_files: usize,
    pub files: usize,
    pub total_bytes: u64,
    pub total_video_duration_seconds: f64,
}

#[derive(Debug, Serialize)]
pub struct ArtifactCheckAllReport {
    pub works_root: String,
    pub works: usize,
    pub work_ids: Vec<String>,
    pub work_titles: Vec<String>,
    pub artifact_manifests: Vec<String>,
    pub source_manifests: Vec<String>,
    pub schema_versions: Vec<String>,
    pub baseline_adapters: Vec<String>,
    pub platforms: usize,
    pub video_files: usize,
    pub image_files: usize,
    pub files: usize,
    pub scene_previews: usize,
    pub total_bytes: u64,
    pub total_video_duration_seconds: f64,
    pub reports: Vec<ArtifactCheckReport>,
}

#[derive(Debug, Serialize)]
pub struct ReviewAllReport {
    pub works_root: String,
    pub index: String,
    pub generated_unix: u64,
    pub checked_unix: u64,
    pub works: usize,
    pub review_packs: Vec<String>,
    pub review_statuses: Vec<String>,
    pub review_status_counts: BTreeMap<String, usize>,
    pub review_status_artifact_manifests: BTreeMap<String, Vec<String>>,
    pub review_status_review_packs: BTreeMap<String, Vec<String>>,
    pub review_status_work_ids: BTreeMap<String, Vec<String>>,
    pub review_status_work_titles: BTreeMap<String, Vec<String>>,
    pub required_roles: Vec<String>,
    pub required_role_counts: BTreeMap<String, usize>,
    pub required_role_status_counts: BTreeMap<String, BTreeMap<String, usize>>,
    pub work_ids: Vec<String>,
    pub work_titles: Vec<String>,
    pub artifact_manifests: Vec<String>,
    pub source_manifests: Vec<String>,
    pub schema_versions: Vec<String>,
    pub baseline_adapters: Vec<String>,
    pub platforms: usize,
    pub video_files: usize,
    pub image_files: usize,
    pub files: usize,
    pub scene_previews: usize,
    pub total_bytes: u64,
    pub total_video_duration_seconds: f64,
    pub reports: Vec<ReviewAllWorkReport>,
}

#[derive(Debug, Serialize)]
pub struct ReviewAllWorkReport {
    pub manifest: String,
    pub review_pack: String,
    pub review_status: String,
    pub required_roles: Vec<String>,
    pub artifact_manifest: String,
    pub artifact_check: ArtifactCheckReport,
}

#[derive(Debug)]
struct ReviewMetadata {
    status: String,
    required_roles: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    work: String,
    title: String,
    source_scenario: SourceScenario,
    format: String,
    style: String,
    platforms: Vec<Platform>,
    scenes: Vec<Scene>,
    shots: Vec<Shot>,
    exports: Vec<Export>,
}

#[derive(Debug, Deserialize)]
struct SourceScenario {
    repo: String,
    id: String,
}

#[derive(Debug, Deserialize)]
struct Platform {
    name: String,
    aspect_ratio: String,
    target_duration_seconds: f64,
}

#[derive(Debug, Deserialize)]
struct Scene {
    id: String,
    duration_seconds: f64,
}

#[derive(Debug, Deserialize)]
struct Shot {
    id: String,
    scene_id: String,
    start_seconds: f64,
    duration_seconds: f64,
    camera: String,
    action: String,
    visual_prompt: String,
    #[serde(default)]
    audio: ShotAudio,
    #[serde(default)]
    captions: ShotCaptions,
}

#[derive(Debug, Deserialize)]
struct Export {
    id: String,
    filename: String,
    aspect_ratio: String,
    duration_seconds: f64,
}

#[derive(Debug, Default, Deserialize)]
struct ShotAudio {
    #[serde(default)]
    narration: String,
}

#[derive(Debug, Default, Deserialize)]
struct ShotCaptions {
    #[serde(default)]
    text: String,
}

pub fn load_manifest(path: impl AsRef<Path>) -> Result<LoadedManifest> {
    let path = path.as_ref();
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read manifest: {}", path.display()))?;
    let raw: Value = serde_yaml::from_str(&text)
        .with_context(|| format!("failed to parse manifest yaml: {}", path.display()))?;
    let manifest: Manifest = serde_yaml::from_str(&text)
        .with_context(|| format!("failed to parse manifest contract: {}", path.display()))?;

    Ok(LoadedManifest {
        path: path.to_path_buf(),
        raw,
        manifest,
    })
}

pub fn validate_manifest(loaded: &LoadedManifest) -> Result<ValidationReport> {
    let top = loaded
        .raw
        .as_mapping()
        .ok_or_else(|| anyhow!("manifest root must be a mapping"))?;
    validate_required_top_fields(top)?;
    validate_top_level_values(top)?;
    validate_required_mapping_fields(top, "source_scenario", REQUIRED_SOURCE_SCENARIO_FIELDS)?;
    validate_required_mapping_fields(top, "audience", REQUIRED_AUDIENCE_FIELDS)?;
    validate_required_mapping_fields(top, "audio", REQUIRED_AUDIO_FIELDS)?;
    validate_required_mapping_fields(top, "captions", REQUIRED_CAPTIONS_FIELDS)?;
    validate_required_mapping_fields(
        top,
        "renderer_assumptions",
        REQUIRED_RENDERER_ASSUMPTIONS_FIELDS,
    )?;
    validate_optional_adapter_metadata(top)?;
    validate_required_mapping_fields(top, "review", REQUIRED_REVIEW_FIELDS)?;
    validate_required_sequence_fields(top, "platforms", REQUIRED_PLATFORM_FIELDS)?;
    validate_required_sequence_fields(top, "scenes", REQUIRED_SCENE_FIELDS)?;
    validate_required_sequence_fields(top, "shots", REQUIRED_SHOT_FIELDS)?;
    validate_required_sequence_fields(top, "exports", REQUIRED_EXPORT_FIELDS)?;
    validate_non_empty_sections(&loaded.manifest)?;
    validate_unique_ids(&loaded.manifest)?;
    validate_positive_timing(&loaded.manifest)?;
    validate_platform_export_coverage(&loaded.manifest)?;

    let scene_total = loaded
        .manifest
        .scenes
        .iter()
        .map(|scene| scene.duration_seconds)
        .sum::<f64>();
    let shot_total = loaded
        .manifest
        .shots
        .iter()
        .map(|shot| shot.duration_seconds)
        .sum::<f64>();

    if !same_duration(scene_total, shot_total) {
        bail!(
            "scene duration total ({scene_total:.3}) does not match shot total ({shot_total:.3})"
        );
    }

    validate_shot_timing(&loaded.manifest.shots)?;
    validate_shot_scenes(&loaded.manifest)?;
    validate_shot_scene_spans(&loaded.manifest)?;
    let exports = export_plans(&loaded.manifest, shot_total)?;

    Ok(ValidationReport {
        scene_total,
        shot_total,
        exports,
    })
}

pub fn render_review_pack(manifest: impl AsRef<Path>) -> Result<PathBuf> {
    let manifest = manifest.as_ref();
    let loaded = load_manifest(manifest)?;
    let report = validate_manifest(&loaded)?;

    let out_dir = PathBuf::from("renders/review-packs");
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    let report_path = out_dir.join(format!("{}-review-pack.md", loaded.manifest.work));

    let mut markdown = String::new();
    markdown.push_str(&format!("# Review pack: {}\n\n", loaded.manifest.title));
    markdown.push_str(&format!("- Manifest: `{}`\n", manifest.display()));
    markdown.push_str(&format!("- Work: `{}`\n", loaded.manifest.work));
    markdown.push_str(&format!("- Generated unix: `{}`\n\n", unix_now()?));
    let artifact_manifest = render_artifact_manifest(manifest)?;
    markdown.push_str(&format!(
        "- Artifact manifest JSON: `{}`\n\n",
        artifact_manifest.display()
    ));
    markdown.push_str(&review_pack_review_summary(&loaded)?);
    markdown.push_str(&review_pack_adapter_summary(&loaded)?);
    markdown.push_str("## FFmpeg baseline renders\n\n");
    markdown.push_str("| Platform | MP4 | Duration | Contact sheet |\n");
    markdown.push_str("|---|---|---:|---|\n");

    for export in &report.exports {
        let video = render_shot_cards_for_export(&loaded, export)?;
        let sheet = render_contact_sheet_for_export(&loaded, export)?;
        let duration = ffprobe_duration(&video)?;

        markdown.push_str(&format!(
            "| `{}` | `{}` | `{}s` | `{}` |\n",
            export.id,
            video.display(),
            duration,
            sheet.display()
        ));
    }
    markdown.push_str("\n## FFmpeg baseline scene previews\n\n");
    markdown.push_str("| Platform | Scene | MP4 | Duration |\n");
    markdown.push_str("|---|---|---|---:|\n");

    for export in &report.exports {
        for scene in &loaded.manifest.scenes {
            let plan = scene_plan_for_loaded(&loaded, &report, &scene.id, &export.id)?;
            let preview = render_scene_preview_for_plan(&loaded, &plan)?;
            markdown.push_str(&format!(
                "| `{}` | `{}` | `{}` | `{}s` |\n",
                export.id,
                scene.id,
                preview.display(),
                compact_seconds(plan.render_duration_seconds)
            ));
        }
    }
    markdown.push_str("\n## FFmpeg baseline work previews\n\n");
    markdown.push_str("| Platform | MP4 | Duration |\n");
    markdown.push_str("|---|---|---:|\n");
    for export in &report.exports {
        let preview = render_work_preview(manifest, &export.id)?;
        let duration = ffprobe_duration_label(&preview)?;
        markdown.push_str(&format!(
            "| `{}` | `{}` | `{}s` |\n",
            export.id,
            preview.display(),
            duration
        ));
    }

    fs::write(&report_path, markdown)
        .with_context(|| format!("failed to write {}", report_path.display()))?;
    Ok(report_path)
}

pub fn scene_plan(manifest: impl AsRef<Path>, scene_id: &str, platform: &str) -> Result<ScenePlan> {
    let loaded = load_manifest(manifest)?;
    let report = validate_manifest(&loaded)?;
    scene_plan_for_loaded(&loaded, &report, scene_id, platform)
}

pub fn render_scene_preview(
    manifest: impl AsRef<Path>,
    scene_id: &str,
    platform: &str,
) -> Result<PathBuf> {
    let loaded = load_manifest(manifest)?;
    let report = validate_manifest(&loaded)?;
    let plan = scene_plan_for_loaded(&loaded, &report, scene_id, platform)?;
    render_scene_preview_for_plan(&loaded, &plan)
}

pub fn render_scene_previews(manifest: impl AsRef<Path>, platform: &str) -> Result<Vec<PathBuf>> {
    let loaded = load_manifest(manifest)?;
    let report = validate_manifest(&loaded)?;
    let mut previews = Vec::new();

    for scene in &loaded.manifest.scenes {
        let plan = scene_plan_for_loaded(&loaded, &report, &scene.id, platform)?;
        previews.push(render_scene_preview_for_plan(&loaded, &plan)?);
    }

    Ok(previews)
}

pub fn render_work_preview(manifest: impl AsRef<Path>, platform: &str) -> Result<PathBuf> {
    let manifest = manifest.as_ref();
    let loaded = load_manifest(manifest)?;
    let previews = render_scene_previews(manifest, platform)?;
    if previews.is_empty() {
        bail!("manifest has no scenes to preview");
    }

    render_work_preview_for_paths(&loaded.manifest.work, platform, &previews)
}

pub fn render_artifact_manifest(manifest: impl AsRef<Path>) -> Result<PathBuf> {
    let manifest = manifest.as_ref();
    let loaded = load_manifest(manifest)?;
    let report = validate_manifest(&loaded)?;
    let out_dir = PathBuf::from("renders/artifacts");
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    let out_file = out_dir.join(format!("{}-artifacts.json", loaded.manifest.work));

    let mut platforms = Vec::new();
    for export in &report.exports {
        let shot_cards = render_shot_cards_for_export(&loaded, export)?;
        let contact_sheet = render_contact_sheet_for_export(&loaded, export)?;
        let mut scene_previews = Vec::new();
        let mut preview_paths = Vec::new();

        for scene in &loaded.manifest.scenes {
            let plan = scene_plan_for_loaded(&loaded, &report, &scene.id, &export.id)?;
            let preview = render_scene_preview_for_plan(&loaded, &plan)?;
            preview_paths.push(preview.clone());
            scene_previews.push(ArtifactScenePreview {
                scene_id: scene.id.clone(),
                video: ArtifactVideo {
                    path: path_text(&preview),
                    bytes: file_bytes(&preview)?,
                    sha256: sha256_file(&preview)?,
                    duration_seconds: plan.render_duration_seconds,
                },
            });
        }

        let work_preview =
            render_work_preview_for_paths(&loaded.manifest.work, &export.id, &preview_paths)?;
        platforms.push(ArtifactPlatform {
            id: export.id.clone(),
            aspect_ratio: export.aspect_ratio.clone(),
            width: export.width,
            height: export.height,
            shot_cards: ArtifactVideo {
                path: path_text(&shot_cards),
                bytes: file_bytes(&shot_cards)?,
                sha256: sha256_file(&shot_cards)?,
                duration_seconds: ffprobe_duration_seconds(&shot_cards)?,
            },
            contact_sheet: ArtifactImage {
                path: path_text(&contact_sheet),
                bytes: file_bytes(&contact_sheet)?,
                sha256: sha256_file(&contact_sheet)?,
            },
            work_preview: ArtifactVideo {
                path: path_text(&work_preview),
                bytes: file_bytes(&work_preview)?,
                sha256: sha256_file(&work_preview)?,
                duration_seconds: ffprobe_duration_seconds(&work_preview)?,
            },
            scene_previews,
        });
    }

    let artifact_manifest = ArtifactManifest {
        schema_version: ARTIFACT_MANIFEST_SCHEMA_VERSION.to_string(),
        work: loaded.manifest.work.clone(),
        title: loaded.manifest.title.clone(),
        manifest: path_text(manifest),
        generated_unix: unix_now()?,
        baseline_adapter: "ffmpeg".to_string(),
        platforms,
    };
    fs::write(&out_file, serde_json::to_string_pretty(&artifact_manifest)?)
        .with_context(|| format!("failed to write {}", out_file.display()))?;
    Ok(out_file)
}

pub fn check_artifact_manifest(path: impl AsRef<Path>) -> Result<ArtifactCheckReport> {
    let path = path.as_ref();
    let json = fs::read_to_string(path)
        .with_context(|| format!("failed to read artifact manifest {}", path.display()))?;
    let manifest: ArtifactManifest = serde_json::from_str(&json)
        .with_context(|| format!("failed to parse artifact manifest {}", path.display()))?;

    if manifest.schema_version != ARTIFACT_MANIFEST_SCHEMA_VERSION {
        bail!(
            "unsupported artifact manifest schema version: {}",
            manifest.schema_version
        );
    }

    if manifest.platforms.is_empty() {
        bail!("artifact manifest has no platforms: {}", path.display());
    }

    let source = load_manifest(&manifest.manifest)?;
    let validation = validate_manifest(&source)?;
    if manifest.work != source.manifest.work {
        bail!(
            "artifact manifest work mismatch: expected {}, found {}",
            source.manifest.work,
            manifest.work
        );
    }
    if manifest.title != source.manifest.title {
        bail!(
            "artifact manifest title mismatch: expected {}, found {}",
            source.manifest.title,
            manifest.title
        );
    }
    if manifest.baseline_adapter != "ffmpeg" {
        bail!(
            "artifact manifest baseline adapter mismatch: expected ffmpeg, found {}",
            manifest.baseline_adapter
        );
    }
    if manifest.platforms.len() != validation.exports.len() {
        bail!(
            "artifact manifest platform count mismatch: expected {}, found {}",
            validation.exports.len(),
            manifest.platforms.len()
        );
    }

    let mut files = 0usize;
    let mut scene_previews = 0usize;
    let mut video_files = 0usize;
    let mut image_files = 0usize;
    let mut total_bytes = 0u64;
    let mut total_video_duration_seconds = 0.0f64;
    for platform in &manifest.platforms {
        let export = validation
            .exports
            .iter()
            .find(|export| export.id == platform.id)
            .ok_or_else(|| anyhow!("artifact manifest has unknown platform: {}", platform.id))?;
        if platform.aspect_ratio != export.aspect_ratio {
            bail!(
                "platform {} aspect ratio mismatch: expected {}, found {}",
                platform.id,
                export.aspect_ratio,
                platform.aspect_ratio
            );
        }
        if platform.width != export.width || platform.height != export.height {
            bail!(
                "platform {} dimensions mismatch: expected {}x{}, found {}x{}",
                platform.id,
                export.width,
                export.height,
                platform.width,
                platform.height
            );
        }

        check_artifact_video(&platform.shot_cards, "shot_cards")?;
        check_artifact_duration(
            &platform.shot_cards,
            export.duration_seconds,
            &format!("platform {} shot_cards", platform.id),
        )?;
        files += 1;
        video_files += 1;
        total_bytes += platform.shot_cards.bytes;
        total_video_duration_seconds += platform.shot_cards.duration_seconds;

        check_artifact_image(&platform.contact_sheet, "contact_sheet")?;
        files += 1;
        image_files += 1;
        total_bytes += platform.contact_sheet.bytes;

        check_artifact_video(&platform.work_preview, "work_preview")?;
        check_artifact_duration(
            &platform.work_preview,
            export.duration_seconds,
            &format!("platform {} work_preview", platform.id),
        )?;
        files += 1;
        video_files += 1;
        total_bytes += platform.work_preview.bytes;
        total_video_duration_seconds += platform.work_preview.duration_seconds;

        if platform.scene_previews.is_empty() {
            bail!("platform {} has no scene previews", platform.id);
        }
        if platform.scene_previews.len() != source.manifest.scenes.len() {
            bail!(
                "platform {} scene preview count mismatch: expected {}, found {}",
                platform.id,
                source.manifest.scenes.len(),
                platform.scene_previews.len()
            );
        }
        for (scene, expected_scene) in platform.scene_previews.iter().zip(&source.manifest.scenes) {
            if scene.scene_id != expected_scene.id {
                bail!(
                    "platform {} scene preview mismatch: expected {}, found {}",
                    platform.id,
                    expected_scene.id,
                    scene.scene_id
                );
            }
            let plan = scene_plan_for_loaded(&source, &validation, &scene.scene_id, &platform.id)?;
            if !same_duration(scene.video.duration_seconds, plan.render_duration_seconds) {
                bail!(
                    "platform {} scene {} duration mismatch: expected {}, found {}",
                    platform.id,
                    scene.scene_id,
                    plan.render_duration_seconds,
                    scene.video.duration_seconds
                );
            }
            check_artifact_video(&scene.video, &format!("scene_preview {}", scene.scene_id))?;
            files += 1;
            video_files += 1;
            scene_previews += 1;
            total_bytes += scene.video.bytes;
            total_video_duration_seconds += scene.video.duration_seconds;
        }
    }

    Ok(ArtifactCheckReport {
        artifact_manifest: path_text(path),
        schema_version: manifest.schema_version,
        generated_unix: manifest.generated_unix,
        checked_unix: unix_now()?,
        source_manifest: manifest.manifest,
        work: manifest.work,
        title: manifest.title,
        baseline_adapter: manifest.baseline_adapter,
        platforms: manifest.platforms.len(),
        scene_previews,
        video_files,
        image_files,
        files,
        total_bytes,
        total_video_duration_seconds,
    })
}

pub fn check_all_artifact_manifests(root: impl AsRef<Path>) -> Result<ArtifactCheckAllReport> {
    let root = root.as_ref();
    if !root.is_dir() {
        bail!("works root not found: {}", root.display());
    }

    let manifests = discover_work_manifests(root)?;
    if manifests.is_empty() {
        bail!("no work manifests found under: {}", root.display());
    }

    let mut reports = Vec::new();
    let mut work_ids = BTreeSet::new();
    let mut work_titles = BTreeSet::new();
    let mut artifact_manifests = BTreeSet::new();
    let mut source_manifests = BTreeSet::new();
    let mut schema_versions = BTreeSet::new();
    let mut baseline_adapters = BTreeSet::new();
    let mut platforms = 0usize;
    let mut files = 0usize;
    let mut scene_previews = 0usize;
    let mut video_files = 0usize;
    let mut image_files = 0usize;
    let mut total_bytes = 0u64;
    let mut total_video_duration_seconds = 0.0f64;
    for manifest in manifests {
        let artifact_manifest = render_artifact_manifest(&manifest)?;
        let report = check_artifact_manifest(&artifact_manifest)?;
        work_ids.insert(report.work.clone());
        work_titles.insert(report.title.clone());
        artifact_manifests.insert(report.artifact_manifest.clone());
        source_manifests.insert(report.source_manifest.clone());
        schema_versions.insert(report.schema_version.clone());
        baseline_adapters.insert(report.baseline_adapter.clone());
        platforms += report.platforms;
        files += report.files;
        scene_previews += report.scene_previews;
        video_files += report.video_files;
        image_files += report.image_files;
        total_bytes += report.total_bytes;
        total_video_duration_seconds += report.total_video_duration_seconds;
        reports.push(report);
    }

    Ok(ArtifactCheckAllReport {
        works_root: path_text(root),
        works: reports.len(),
        work_ids: work_ids.into_iter().collect(),
        work_titles: work_titles.into_iter().collect(),
        artifact_manifests: artifact_manifests.into_iter().collect(),
        source_manifests: source_manifests.into_iter().collect(),
        schema_versions: schema_versions.into_iter().collect(),
        baseline_adapters: baseline_adapters.into_iter().collect(),
        platforms,
        video_files,
        image_files,
        files,
        scene_previews,
        total_bytes,
        total_video_duration_seconds,
        reports,
    })
}

pub fn summarize_work_corpus(root: impl AsRef<Path>) -> Result<CorpusReport> {
    let root = root.as_ref();
    let manifests = discover_work_manifests(root)?;
    if manifests.is_empty() {
        bail!("no work manifests found under: {}", root.display());
    }

    let mut work_ids = BTreeSet::new();
    let mut work_titles = BTreeSet::new();
    let mut manifest_versions = BTreeSet::new();
    let mut source_repos = BTreeSet::new();
    let mut source_ids = BTreeSet::new();
    let mut source_paths = BTreeSet::new();
    let mut source_commits = BTreeSet::new();
    let mut audience_primaries = BTreeSet::new();
    let mut audience_contexts = BTreeSet::new();
    let mut audience_desired_effects = BTreeSet::new();
    let mut formats = BTreeSet::new();
    let mut styles = BTreeSet::new();
    let mut alternate_styles = BTreeSet::new();
    let mut platform_names = BTreeSet::new();
    let mut platforms = 0usize;
    let mut scenes = 0usize;
    let mut shots = 0usize;
    let mut exports = 0usize;
    let mut total_scene_duration_seconds = 0.0f64;
    let mut total_shot_duration_seconds = 0.0f64;
    let mut manifest_paths = Vec::new();
    let mut reports = Vec::new();

    for manifest in manifests {
        let loaded = load_manifest(&manifest)?;
        let validation = validate_manifest(&loaded)?;
        let manifest_path = path_text(&loaded.path);
        let manifest_version = loaded
            .raw
            .get(Value::String("manifest_version".to_string()))
            .and_then(Value::as_str)
            .expect("manifest_version string was checked during validation")
            .to_string();
        let source_scenario = loaded
            .raw
            .get(Value::String("source_scenario".to_string()))
            .and_then(Value::as_mapping)
            .expect("source_scenario mapping was checked during validation");
        let source_path = source_scenario
            .get(Value::String("path".to_string()))
            .and_then(Value::as_str)
            .expect("source_scenario.path string was checked during validation")
            .to_string();
        let source_commit = source_scenario
            .get(Value::String("source_commit".to_string()))
            .and_then(Value::as_str)
            .expect("source_scenario.source_commit string was checked during validation")
            .to_string();
        let audience = loaded
            .raw
            .get(Value::String("audience".to_string()))
            .and_then(Value::as_mapping)
            .expect("audience mapping was checked during validation");
        let audience_primary = audience
            .get(Value::String("primary".to_string()))
            .and_then(Value::as_str)
            .expect("audience.primary string was checked during validation")
            .to_string();
        let audience_context = audience
            .get(Value::String("context".to_string()))
            .and_then(Value::as_str)
            .expect("audience.context string was checked during validation")
            .to_string();
        let audience_desired_effect = audience
            .get(Value::String("desired_effect".to_string()))
            .and_then(Value::as_str)
            .expect("audience.desired_effect string was checked during validation")
            .to_string();
        let work_alternate_styles = loaded
            .raw
            .get(Value::String("alternate_styles".to_string()))
            .map(|styles| {
                styles
                    .as_sequence()
                    .ok_or_else(|| anyhow!("alternate_styles must be a sequence"))?
                    .iter()
                    .enumerate()
                    .map(|(index, style)| {
                        style.as_str().map(str::to_string).ok_or_else(|| {
                            anyhow!("alternate_styles[{}] must be a string", index + 1)
                        })
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default();
        let work_platform_names = loaded
            .manifest
            .platforms
            .iter()
            .map(|platform| platform.name.clone())
            .collect::<Vec<_>>();
        work_ids.insert(loaded.manifest.work.clone());
        work_titles.insert(loaded.manifest.title.clone());
        manifest_versions.insert(manifest_version.clone());
        source_repos.insert(loaded.manifest.source_scenario.repo.clone());
        source_ids.insert(loaded.manifest.source_scenario.id.clone());
        source_paths.insert(source_path.clone());
        source_commits.insert(source_commit.clone());
        audience_primaries.insert(audience_primary.clone());
        audience_contexts.insert(audience_context.clone());
        audience_desired_effects.insert(audience_desired_effect.clone());
        formats.insert(loaded.manifest.format.clone());
        styles.insert(loaded.manifest.style.clone());
        alternate_styles.extend(work_alternate_styles.iter().cloned());
        platform_names.extend(work_platform_names.iter().cloned());
        platforms += loaded.manifest.platforms.len();
        scenes += loaded.manifest.scenes.len();
        shots += loaded.manifest.shots.len();
        exports += validation.exports.len();
        total_scene_duration_seconds += validation.scene_total;
        total_shot_duration_seconds += validation.shot_total;
        manifest_paths.push(manifest_path.clone());
        reports.push(CorpusWorkReport {
            manifest: manifest_path,
            manifest_version,
            work: loaded.manifest.work,
            title: loaded.manifest.title,
            source_repo: loaded.manifest.source_scenario.repo,
            source_id: loaded.manifest.source_scenario.id,
            source_path,
            source_commit,
            audience_primary,
            audience_context,
            audience_desired_effect,
            format: loaded.manifest.format,
            style: loaded.manifest.style,
            alternate_styles: work_alternate_styles,
            platform_names: work_platform_names,
            platforms: loaded.manifest.platforms.len(),
            scenes: loaded.manifest.scenes.len(),
            shots: loaded.manifest.shots.len(),
            exports: validation.exports.len(),
            scene_duration_seconds: validation.scene_total,
            shot_duration_seconds: validation.shot_total,
        });
    }

    Ok(CorpusReport {
        works_root: path_text(root),
        works: reports.len(),
        manifests: manifest_paths,
        manifest_versions: manifest_versions.into_iter().collect(),
        work_ids: work_ids.into_iter().collect(),
        work_titles: work_titles.into_iter().collect(),
        source_repos: source_repos.into_iter().collect(),
        source_ids: source_ids.into_iter().collect(),
        source_paths: source_paths.into_iter().collect(),
        source_commits: source_commits.into_iter().collect(),
        audience_primaries: audience_primaries.into_iter().collect(),
        audience_contexts: audience_contexts.into_iter().collect(),
        audience_desired_effects: audience_desired_effects.into_iter().collect(),
        formats: formats.into_iter().collect(),
        styles: styles.into_iter().collect(),
        alternate_styles: alternate_styles.into_iter().collect(),
        platform_names: platform_names.into_iter().collect(),
        platforms,
        scenes,
        shots,
        exports,
        total_scene_duration_seconds,
        total_shot_duration_seconds,
        reports,
    })
}

pub fn summarize_review_queue(root: impl AsRef<Path>) -> Result<ReviewQueueReport> {
    let root = root.as_ref();
    let manifests = discover_work_manifests(root)?;
    if manifests.is_empty() {
        bail!("no work manifests found under: {}", root.display());
    }

    let mut manifest_paths = Vec::new();
    let mut review_statuses = BTreeSet::new();
    let mut review_status_counts = BTreeMap::new();
    let mut review_status_work_ids: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut review_status_work_titles: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut required_roles = BTreeSet::new();
    let mut required_role_counts = BTreeMap::new();
    let mut required_role_status_counts: BTreeMap<String, BTreeMap<String, usize>> =
        BTreeMap::new();
    let mut reports = Vec::new();

    for manifest in manifests {
        let loaded = load_manifest(&manifest)?;
        validate_manifest(&loaded)?;
        let review = review_metadata(&loaded)?;
        let review_status = review.status.clone();
        let manifest_path = path_text(&loaded.path);
        manifest_paths.push(manifest_path.clone());
        review_statuses.insert(review_status.clone());
        *review_status_counts
            .entry(review_status.clone())
            .or_insert(0) += 1;
        review_status_work_ids
            .entry(review_status.clone())
            .or_default()
            .push(loaded.manifest.work.clone());
        review_status_work_titles
            .entry(review_status.clone())
            .or_default()
            .push(loaded.manifest.title.clone());
        required_roles.extend(review.required_roles.iter().cloned());
        for role in &review.required_roles {
            *required_role_counts.entry(role.clone()).or_insert(0) += 1;
            *required_role_status_counts
                .entry(role.clone())
                .or_default()
                .entry(review_status.clone())
                .or_insert(0) += 1;
        }
        reports.push(ReviewQueueWorkReport {
            manifest: manifest_path,
            work: loaded.manifest.work,
            title: loaded.manifest.title,
            review_status: review.status,
            required_roles: review.required_roles,
        });
    }

    Ok(ReviewQueueReport {
        works_root: path_text(root),
        works: reports.len(),
        manifests: manifest_paths,
        review_statuses: review_statuses.into_iter().collect(),
        review_status_counts,
        review_status_work_ids,
        review_status_work_titles,
        required_roles: required_roles.into_iter().collect(),
        required_role_counts,
        required_role_status_counts,
        reports,
    })
}

pub fn render_demo(manifest: impl AsRef<Path>) -> Result<PathBuf> {
    let manifest = manifest.as_ref();
    let loaded = load_manifest(manifest)?;
    let report = validate_manifest(&loaded)?;

    let out_dir = PathBuf::from("renders/demo");
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    let demo_path = out_dir.join(format!("{}-demo.html", loaded.manifest.work));
    let review_pack = render_review_pack(manifest)?;
    let artifact_manifest = render_artifact_manifest(manifest)?;

    let mut exports = Vec::new();
    for export in &report.exports {
        let video = render_shot_cards_for_export(&loaded, export)?;
        let sheet = render_contact_sheet_for_export(&loaded, export)?;
        let mut previews = Vec::new();
        for scene in &loaded.manifest.scenes {
            let preview_plan = scene_plan_for_loaded(&loaded, &report, &scene.id, &export.id)?;
            let preview = render_scene_preview_for_plan(&loaded, &preview_plan)?;
            let duration = compact_seconds(preview_plan.render_duration_seconds);
            previews.push(DemoScenePreview {
                scene_id: scene.id.clone(),
                preview,
                duration,
            });
        }
        let work_preview = render_work_preview(manifest, &export.id)?;
        let work_preview_duration = ffprobe_duration_label(&work_preview)?;
        let duration = ffprobe_duration(&video)?;
        exports.push(DemoExport {
            export,
            video,
            sheet,
            previews,
            work_preview,
            work_preview_duration,
            duration,
        });
    }

    fs::write(
        &demo_path,
        demo_html(
            &loaded,
            manifest,
            &review_pack,
            &artifact_manifest,
            &exports,
        )?,
    )
    .with_context(|| format!("failed to write {}", demo_path.display()))?;
    Ok(demo_path)
}

pub fn render_remotion_package(manifest: impl AsRef<Path>, platform: &str) -> Result<PathBuf> {
    let manifest = manifest.as_ref();
    let loaded = load_manifest(manifest)?;
    let report = validate_manifest(&loaded)?;
    let scene_id = loaded
        .manifest
        .scenes
        .first()
        .map(|scene| scene.id.as_str())
        .context("manifest has no scenes")?;
    render_remotion_package_for_loaded(manifest, &loaded, &report, platform, scene_id)
}

pub fn render_remotion_package_for_scene(
    manifest: impl AsRef<Path>,
    platform: &str,
    scene_id: &str,
) -> Result<PathBuf> {
    let manifest = manifest.as_ref();
    let loaded = load_manifest(manifest)?;
    let report = validate_manifest(&loaded)?;
    render_remotion_package_for_loaded(manifest, &loaded, &report, platform, scene_id)
}

fn render_remotion_package_for_loaded(
    manifest: &Path,
    loaded: &LoadedManifest,
    report: &ValidationReport,
    platform: &str,
    scene_id: &str,
) -> Result<PathBuf> {
    let export = report
        .exports
        .iter()
        .find(|export| export.id == platform)
        .ok_or_else(|| anyhow!("unknown platform in manifest: {platform}"))?;
    let scene_plan = scene_plan_for_loaded(loaded, report, scene_id, platform)?;

    let package_dir = PathBuf::from(format!(
        "renders/remotion/{}/{}",
        loaded.manifest.work, export.id
    ));
    fs::create_dir_all(&package_dir)
        .with_context(|| format!("failed to create {}", package_dir.display()))?;

    let source_manifest = package_dir.join("manifest.yaml");
    fs::copy(manifest, &source_manifest).with_context(|| {
        format!(
            "failed to copy {} to {}",
            manifest.display(),
            source_manifest.display()
        )
    })?;

    let remotion_plan = adapters::remotion::plan(&source_manifest, &package_dir, &export.id);
    let props = remotion_props_json(loaded, export, &scene_plan, &remotion_plan);
    let props_path = package_dir.join("props.json");
    fs::write(&props_path, serde_json::to_string_pretty(&props)?)
        .with_context(|| format!("failed to write {}", props_path.display()))?;

    let readme_path = package_dir.join("README.md");
    fs::write(
        &readme_path,
        remotion_package_readme(loaded, export, &scene_plan, &remotion_plan, &props_path),
    )
    .with_context(|| format!("failed to write {}", readme_path.display()))?;

    Ok(package_dir)
}

pub fn render_shot_cards(manifest: impl AsRef<Path>, platform: &str) -> Result<PathBuf> {
    let loaded = load_manifest(manifest)?;
    let report = validate_manifest(&loaded)?;
    let export = report
        .exports
        .iter()
        .find(|export| export.id == platform)
        .ok_or_else(|| anyhow!("unknown platform in manifest: {platform}"))?;

    render_shot_cards_for_export(&loaded, export)
}

pub fn render_smoke(manifest: impl AsRef<Path>) -> Result<PathBuf> {
    let loaded = load_manifest(manifest)?;
    render_smoke_for_manifest(&loaded)
}

pub fn render_contact_sheet(manifest: impl AsRef<Path>, platform: &str) -> Result<PathBuf> {
    let manifest = manifest.as_ref();
    let loaded = load_manifest(manifest)?;
    let report = validate_manifest(&loaded)?;
    let export = report
        .exports
        .iter()
        .find(|export| export.id == platform)
        .ok_or_else(|| anyhow!("unknown platform in manifest: {platform}"))?;

    render_contact_sheet_for_export(&loaded, export)
}

pub fn render_all_review_packs(root: impl AsRef<Path>) -> Result<PathBuf> {
    Ok(render_all_review_pack_report(root)?.index.into())
}

pub fn render_all_review_pack_report(root: impl AsRef<Path>) -> Result<ReviewAllReport> {
    let root = root.as_ref();
    if !root.is_dir() {
        bail!("works root not found: {}", root.display());
    }

    let manifests = discover_work_manifests(root)?;
    if manifests.is_empty() {
        bail!("no work manifests found under: {}", root.display());
    }

    let out_dir = PathBuf::from("renders/review-packs");
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    let index_path = out_dir.join("INDEX.md");

    let mut markdown = String::new();
    let generated_unix = unix_now()?;
    markdown.push_str("# REEL review-pack index\n\n");
    markdown.push_str(&format!("- Works root: `{}`\n", root.display()));
    markdown.push_str(&format!("- Generated unix: `{generated_unix}`\n\n"));
    markdown.push_str(
        "| Work manifest | Review pack | Artifact manifest | Generated unix | Checked unix | Verification |\n",
    );
    markdown.push_str("|---|---|---|---:|---:|---:|\n");

    let mut reports = Vec::new();
    let mut review_packs = BTreeSet::new();
    let mut review_statuses = BTreeSet::new();
    let mut review_status_counts = BTreeMap::new();
    let mut review_status_artifact_manifests: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut review_status_review_packs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut review_status_work_ids: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut review_status_work_titles: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut required_roles = BTreeSet::new();
    let mut required_role_counts = BTreeMap::new();
    let mut required_role_status_counts: BTreeMap<String, BTreeMap<String, usize>> =
        BTreeMap::new();
    let mut work_ids = BTreeSet::new();
    let mut work_titles = BTreeSet::new();
    let mut artifact_manifests = BTreeSet::new();
    let mut source_manifests = BTreeSet::new();
    let mut schema_versions = BTreeSet::new();
    let mut baseline_adapters = BTreeSet::new();
    let mut platforms = 0usize;
    let mut files = 0usize;
    let mut scene_previews = 0usize;
    let mut video_files = 0usize;
    let mut image_files = 0usize;
    let mut total_bytes = 0u64;
    let mut total_video_duration_seconds = 0.0f64;
    for manifest in manifests {
        let loaded = load_manifest(&manifest)?;
        validate_manifest(&loaded)?;
        let review_metadata = review_metadata(&loaded)?;
        let report = render_review_pack(&manifest)?;
        let artifact_manifest = render_artifact_manifest(&manifest)?;
        let check = check_artifact_manifest(&artifact_manifest)?;
        let review_status = review_metadata.status.clone();
        review_packs.insert(path_text(&report));
        review_statuses.insert(review_status.clone());
        *review_status_counts
            .entry(review_status.clone())
            .or_insert(0) += 1;
        review_status_artifact_manifests
            .entry(review_status.clone())
            .or_default()
            .push(path_text(&artifact_manifest));
        review_status_review_packs
            .entry(review_status.clone())
            .or_default()
            .push(path_text(&report));
        review_status_work_ids
            .entry(review_status.clone())
            .or_default()
            .push(check.work.clone());
        review_status_work_titles
            .entry(review_status.clone())
            .or_default()
            .push(check.title.clone());
        required_roles.extend(review_metadata.required_roles.iter().cloned());
        for role in &review_metadata.required_roles {
            *required_role_counts.entry(role.clone()).or_insert(0) += 1;
            *required_role_status_counts
                .entry(role.clone())
                .or_default()
                .entry(review_metadata.status.clone())
                .or_insert(0) += 1;
        }
        work_ids.insert(check.work.clone());
        work_titles.insert(check.title.clone());
        artifact_manifests.insert(check.artifact_manifest.clone());
        source_manifests.insert(check.source_manifest.clone());
        schema_versions.insert(check.schema_version.clone());
        baseline_adapters.insert(check.baseline_adapter.clone());
        platforms += check.platforms;
        files += check.files;
        scene_previews += check.scene_previews;
        video_files += check.video_files;
        image_files += check.image_files;
        total_bytes += check.total_bytes;
        total_video_duration_seconds += check.total_video_duration_seconds;
        markdown.push_str(&format!(
            "| `{}` | `{}` | `{}` | `{}` | `{}` | `{} platforms / {} videos / {} images / {} files / {} bytes / {}s` |\n",
            manifest.display(),
            report.display(),
            artifact_manifest.display(),
            check.generated_unix,
            check.checked_unix,
            check.platforms,
            check.video_files,
            check.image_files,
            check.files,
            check.total_bytes,
            compact_seconds(check.total_video_duration_seconds)
        ));
        reports.push(ReviewAllWorkReport {
            manifest: path_text(&manifest),
            review_pack: path_text(&report),
            review_status: review_metadata.status,
            required_roles: review_metadata.required_roles,
            artifact_manifest: path_text(&artifact_manifest),
            artifact_check: check,
        });
    }
    let checked_unix = unix_now()?;
    markdown.push_str("\n## Verification totals\n\n");
    markdown.push_str(&format!("- Checked unix: `{checked_unix}`\n"));
    markdown.push_str(&format!("- Works: `{}`\n", reports.len()));
    let review_packs: Vec<_> = review_packs.into_iter().collect();
    markdown.push_str(&format!("- Review packs: `{}`\n", review_packs.join(", ")));
    let review_statuses: Vec<_> = review_statuses.into_iter().collect();
    markdown.push_str(&format!(
        "- Review statuses: `{}`\n",
        review_statuses.join(", ")
    ));
    markdown.push_str(&format!(
        "- Review status counts: `{}`\n",
        review_status_counts
            .iter()
            .map(|(status, count)| format!("{status}={count}"))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    markdown.push_str(&format!(
        "- Review status artifact manifests: `{}`\n",
        review_status_artifact_manifests
            .iter()
            .map(|(status, manifests)| format!("{status}: {}", manifests.join(", ")))
            .collect::<Vec<_>>()
            .join("; ")
    ));
    markdown.push_str(&format!(
        "- Review status review packs: `{}`\n",
        review_status_review_packs
            .iter()
            .map(|(status, review_packs)| format!("{status}: {}", review_packs.join(", ")))
            .collect::<Vec<_>>()
            .join("; ")
    ));
    markdown.push_str(&format!(
        "- Review status work ids: `{}`\n",
        review_status_work_ids
            .iter()
            .map(|(status, work_ids)| format!("{status}: {}", work_ids.join(", ")))
            .collect::<Vec<_>>()
            .join("; ")
    ));
    markdown.push_str(&format!(
        "- Review status work titles: `{}`\n",
        review_status_work_titles
            .iter()
            .map(|(status, work_titles)| format!("{status}: {}", work_titles.join(" | ")))
            .collect::<Vec<_>>()
            .join("; ")
    ));
    let required_roles: Vec<_> = required_roles.into_iter().collect();
    markdown.push_str(&format!(
        "- Required roles: `{}`\n",
        required_roles.join(", ")
    ));
    markdown.push_str(&format!(
        "- Required role counts: `{}`\n",
        required_role_counts
            .iter()
            .map(|(role, count)| format!("{role}={count}"))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    markdown.push_str(&format!(
        "- Required role status counts: `{}`\n",
        required_role_status_counts
            .iter()
            .map(|(role, statuses)| {
                let status_counts = statuses
                    .iter()
                    .map(|(status, count)| format!("{status}={count}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{role}: {status_counts}")
            })
            .collect::<Vec<_>>()
            .join("; ")
    ));
    let work_ids: Vec<_> = work_ids.into_iter().collect();
    markdown.push_str(&format!("- Work ids: `{}`\n", work_ids.join(", ")));
    let work_titles: Vec<_> = work_titles.into_iter().collect();
    markdown.push_str(&format!("- Work titles: `{}`\n", work_titles.join(" | ")));
    let artifact_manifests: Vec<_> = artifact_manifests.into_iter().collect();
    markdown.push_str(&format!(
        "- Artifact manifests: `{}`\n",
        artifact_manifests.join(", ")
    ));
    let source_manifests: Vec<_> = source_manifests.into_iter().collect();
    markdown.push_str(&format!(
        "- Source manifests: `{}`\n",
        source_manifests.join(", ")
    ));
    let schema_versions: Vec<_> = schema_versions.into_iter().collect();
    markdown.push_str(&format!(
        "- Artifact schemas: `{}`\n",
        schema_versions.join(", ")
    ));
    let baseline_adapters: Vec<_> = baseline_adapters.into_iter().collect();
    markdown.push_str(&format!(
        "- Baseline adapters: `{}`\n",
        baseline_adapters.join(", ")
    ));
    markdown.push_str(&format!("- Platforms: `{platforms}`\n"));
    markdown.push_str(&format!("- Scene previews: `{scene_previews}`\n"));
    markdown.push_str(&format!("- Videos: `{video_files}`\n"));
    markdown.push_str(&format!("- Images: `{image_files}`\n"));
    markdown.push_str(&format!("- Files: `{files}`\n"));
    markdown.push_str(&format!("- Bytes: `{total_bytes}`\n"));
    markdown.push_str(&format!(
        "- Video duration seconds: `{}`\n",
        compact_seconds(total_video_duration_seconds)
    ));

    fs::write(&index_path, markdown)
        .with_context(|| format!("failed to write {}", index_path.display()))?;
    Ok(ReviewAllReport {
        works_root: path_text(root),
        index: path_text(&index_path),
        generated_unix,
        checked_unix,
        works: reports.len(),
        review_packs,
        review_statuses,
        review_status_counts,
        review_status_artifact_manifests,
        review_status_review_packs,
        review_status_work_ids,
        review_status_work_titles,
        required_roles,
        required_role_counts,
        required_role_status_counts,
        work_ids,
        work_titles,
        artifact_manifests,
        source_manifests,
        schema_versions,
        baseline_adapters,
        platforms,
        video_files,
        image_files,
        files,
        scene_previews,
        total_bytes,
        total_video_duration_seconds,
        reports,
    })
}

fn review_pack_adapter_summary(loaded: &LoadedManifest) -> Result<String> {
    let mut markdown = String::new();
    markdown.push_str("## Adapter summary\n\n");
    markdown.push_str(
        "| Adapter | Status | Declared by manifest | Operations | Boundary | Dependency policy |\n",
    );
    markdown.push_str("|---|---|---:|---|---|---|\n");
    for adapter in manifest_adapter_plan(loaded)? {
        let operations = if adapter.operations.is_empty() {
            "none".to_string()
        } else {
            adapter
                .operations
                .iter()
                .map(|operation| operation.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        markdown.push_str(&format!(
            "| `{}` | `{}` | {} | `{}` | {} | {} |\n",
            adapter.id,
            adapter.status.as_str(),
            if adapter.declared_by_manifest {
                "yes"
            } else {
                "no"
            },
            operations,
            adapter.boundary,
            adapter.dependency_policy
        ));
    }
    markdown.push('\n');
    Ok(markdown)
}

fn review_pack_review_summary(loaded: &LoadedManifest) -> Result<String> {
    let review = review_metadata(loaded)?;

    let mut markdown = String::new();
    markdown.push_str("## Review summary\n\n");
    markdown.push_str(&format!("- Status: `{}`\n", review.status));
    markdown.push_str(&format!(
        "- Required roles: `{}`\n\n",
        review.required_roles.join("`, `")
    ));
    markdown.push_str("| Role | Review focus |\n");
    markdown.push_str("|---|---|\n");
    for role in review.required_roles {
        markdown.push_str(&format!("| `{role}` | {} |\n", review_role_focus(&role)));
    }
    markdown.push('\n');
    Ok(markdown)
}

fn review_metadata(loaded: &LoadedManifest) -> Result<ReviewMetadata> {
    let review = loaded
        .raw
        .get(Value::String("review".to_string()))
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("review must be a mapping"))?;
    let status = review
        .get(Value::String("status".to_string()))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("review.status must be a string"))?
        .to_string();
    let required_roles = review
        .get(Value::String("required_roles".to_string()))
        .and_then(Value::as_sequence)
        .ok_or_else(|| anyhow!("review.required_roles must be a sequence"))?
        .iter()
        .enumerate()
        .map(|(index, role)| {
            role.as_str()
                .map(str::to_string)
                .ok_or_else(|| anyhow!("review.required_roles[{}] must be a string", index + 1))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(ReviewMetadata {
        status,
        required_roles,
    })
}

fn review_role_focus(role: &str) -> &'static str {
    match role {
        "story-director" => "Scenario truth, narrative promise, and continuity.",
        "animation-director" => "Motion readability, style fit, and visual continuity.",
        "editor" => "Pacing, cuts, timing, and export structure.",
        "sound-designer" => "Narration, music, effects, silence, and caption support.",
        "platform-audience" => "Platform fit, audience clarity, captions, and safe areas.",
        _ => "Manifest-specific review responsibility.",
    }
}

struct DemoExport<'a> {
    export: &'a ExportPlan,
    video: PathBuf,
    sheet: PathBuf,
    previews: Vec<DemoScenePreview>,
    work_preview: PathBuf,
    work_preview_duration: String,
    duration: String,
}

struct DemoScenePreview {
    scene_id: String,
    preview: PathBuf,
    duration: String,
}

fn demo_html(
    loaded: &LoadedManifest,
    manifest: &Path,
    review_pack: &Path,
    artifact_manifest: &Path,
    exports: &[DemoExport<'_>],
) -> Result<String> {
    let mut html = String::new();
    html.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n");
    html.push_str("<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    html.push_str(&format!(
        "<title>REEL demo: {}</title>\n",
        html_escape(&loaded.manifest.title)
    ));
    html.push_str("<style>body{font-family:system-ui,sans-serif;margin:2rem;max-width:1100px;background:#101820;color:#f7f7f7}a{color:#8fd3ff}.cards{display:grid;gap:1.5rem}.card{background:#182635;border:1px solid #31465d;border-radius:12px;padding:1rem}video,img{max-width:100%;border-radius:8px;background:#000}.meta{color:#b7c6d8}</style>\n");
    html.push_str("</head>\n<body>\n");
    html.push_str(&format!(
        "<h1>{}</h1>\n<p class=\"meta\">Work <code>{}</code> · format <code>{}</code> · style <code>{}</code></p>\n",
        html_escape(&loaded.manifest.title),
        html_escape(&loaded.manifest.work),
        html_escape(&loaded.manifest.format),
        html_escape(&loaded.manifest.style)
    ));
    html.push_str(&format!(
        "<p>Manifest: <code>{}</code><br>Review pack: <a href=\"{}\">{}</a><br>Artifact manifest: <a href=\"{}\">{}</a></p>\n",
        html_escape(&manifest.display().to_string()),
        html_escape(&relative_render_href(review_pack)?),
        html_escape(&review_pack.display().to_string()),
        html_escape(&relative_render_href(artifact_manifest)?),
        html_escape(&artifact_manifest.display().to_string())
    ));
    html.push_str("<h2>Adapter summary</h2>\n");
    html.push_str(&markdown_table_to_html(&review_pack_adapter_summary(
        loaded,
    )?));
    html.push_str("<h2>FFmpeg baseline exports</h2>\n<div class=\"cards\">\n");
    for item in exports {
        html.push_str("<section class=\"card\">\n");
        html.push_str(&format!(
            "<h3>{} · {} · {}s</h3>\n",
            html_escape(&item.export.id),
            html_escape(&item.export.aspect_ratio),
            html_escape(&item.duration)
        ));
        html.push_str(&format!(
            "<video controls src=\"{}\"></video>\n<p><a href=\"{}\">Open MP4</a></p>\n",
            html_escape(&relative_render_href(&item.video)?),
            html_escape(&relative_render_href(&item.video)?)
        ));
        html.push_str(&format!(
            "<h4>Full FFmpeg baseline work preview · not final art · {}s</h4>\n<video controls src=\"{}\"></video>\n<p><a href=\"{}\">Open work preview</a></p>\n",
            html_escape(&item.work_preview_duration),
            html_escape(&relative_render_href(&item.work_preview)?),
            html_escape(&relative_render_href(&item.work_preview)?)
        ));
        html.push_str("<h4>FFmpeg baseline scene previews · not final art</h4>\n");
        for preview in &item.previews {
            html.push_str(&format!(
                "<h5>{} · {}s</h5>\n<video controls src=\"{}\"></video>\n<p><a href=\"{}\">Open scene preview</a></p>\n",
                html_escape(&preview.scene_id),
                html_escape(&preview.duration),
                html_escape(&relative_render_href(&preview.preview)?),
                html_escape(&relative_render_href(&preview.preview)?)
            ));
        }
        html.push_str(&format!(
            "<h4>Contact sheet</h4>\n<img src=\"{}\" alt=\"{} contact sheet\">\n<p><a href=\"{}\">Open contact sheet</a></p>\n",
            html_escape(&relative_render_href(&item.sheet)?),
            html_escape(&item.export.id),
            html_escape(&relative_render_href(&item.sheet)?)
        ));
        html.push_str("</section>\n");
    }
    html.push_str("</div>\n</body>\n</html>\n");
    Ok(html)
}

fn relative_render_href(path: &Path) -> Result<String> {
    let renders = Path::new("renders");
    let relative = path
        .strip_prefix(renders)
        .with_context(|| format!("demo artifact is outside renders: {}", path.display()))?;
    Ok(format!(
        "../{}",
        relative.to_string_lossy().replace('\\', "/")
    ))
}

fn markdown_table_to_html(markdown: &str) -> String {
    let rows = markdown
        .lines()
        .filter(|line| line.starts_with('|') && !line.starts_with("|---"))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return String::new();
    }

    let mut html = String::from("<table>\n");
    for (index, row) in rows.iter().enumerate() {
        let tag = if index == 0 { "th" } else { "td" };
        html.push_str("<tr>");
        for cell in row.trim_matches('|').split('|') {
            let cell = cell.trim().trim_matches('`');
            html.push_str(&format!("<{tag}>{}</{tag}>", html_escape(cell)));
        }
        html.push_str("</tr>\n");
    }
    html.push_str("</table>\n");
    html
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn remotion_props_json(
    loaded: &LoadedManifest,
    export: &ExportPlan,
    scene_plan: &ScenePlan,
    remotion_plan: &adapters::remotion::RemotionPlan,
) -> serde_json::Value {
    let shots = loaded
        .manifest
        .shots
        .iter()
        .map(|shot| {
            serde_json::json!({
                "id": &shot.id,
                "scene_id": &shot.scene_id,
                "start_seconds": shot.start_seconds,
                "duration_seconds": shot.duration_seconds,
                "camera": &shot.camera,
                "action": &shot.action,
                "visual_prompt": &shot.visual_prompt,
                "narration": &shot.audio.narration,
                "caption": &shot.captions.text,
            })
        })
        .collect::<Vec<_>>();
    let scene_shots = loaded
        .manifest
        .shots
        .iter()
        .filter(|shot| scene_plan.shots.iter().any(|planned| planned.id == shot.id))
        .map(|shot| {
            let planned = scene_plan
                .shots
                .iter()
                .find(|planned| planned.id == shot.id)
                .expect("shot was filtered from scene plan");
            serde_json::json!({
                "id": &shot.id,
                "scene_id": &shot.scene_id,
                "source_start_seconds": planned.source_start_seconds,
                "source_duration_seconds": planned.source_duration_seconds,
                "render_start_seconds": planned.render_start_seconds,
                "render_duration_seconds": planned.render_duration_seconds,
                "frame_start": (planned.render_start_seconds * 24.0).round() as u32,
                "frame_duration": (planned.render_duration_seconds * 24.0).round() as u32,
                "camera": &shot.camera,
                "action": &shot.action,
                "visual_prompt": &shot.visual_prompt,
                "narration": &shot.audio.narration,
                "caption": &shot.captions.text,
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "adapter": "remotion",
        "status": "planned-handoff",
        "work": &loaded.manifest.work,
        "title": &loaded.manifest.title,
        "format": &loaded.manifest.format,
        "style": &loaded.manifest.style,
        "source_scenario": {
            "repo": &loaded.manifest.source_scenario.repo,
            "id": &loaded.manifest.source_scenario.id,
        },
        "export": {
            "id": &export.id,
            "filename": &export.filename,
            "aspect_ratio": &export.aspect_ratio,
            "width": export.width,
            "height": export.height,
            "duration_seconds": export.duration_seconds,
            "duration_scale": export.duration_scale,
        },
        "scene": {
            "id": &scene_plan.scene_id,
            "source_start_seconds": scene_plan.source_start_seconds,
            "source_duration_seconds": scene_plan.source_duration_seconds,
            "render_duration_seconds": scene_plan.render_duration_seconds,
            "fps": 24,
            "frame_count": (scene_plan.render_duration_seconds * 24.0).round() as u32,
            "width": scene_plan.width,
            "height": scene_plan.height,
            "shots": scene_shots,
        },
        "shots": shots,
        "command_shape": &remotion_plan.command_shape,
        "dependency_policy": adapters::remotion::descriptor().dependency_policy,
    })
}

fn remotion_package_readme(
    loaded: &LoadedManifest,
    export: &ExportPlan,
    scene_plan: &ScenePlan,
    remotion_plan: &adapters::remotion::RemotionPlan,
    props_path: &Path,
) -> String {
    format!(
        "# Remotion handoff: {} / {}\n\n\
This is a REEL-generated handoff package, not an executed Remotion render.\n\n\
- Work: `{}`\n\
- Export: `{}` (`{}`, {}x{}, {:.3}s)\n\
- Scene: `{}` ({:.3}s source, {:.3}s render, {} shots)\n\
- Props: `{}`\n\
- Dependency policy: {}\n\n\
## Planned command shape\n\n```powershell\n{}\n```\n",
        loaded.manifest.title,
        export.id,
        loaded.manifest.work,
        export.id,
        export.aspect_ratio,
        export.width,
        export.height,
        export.duration_seconds,
        scene_plan.scene_id,
        scene_plan.source_duration_seconds,
        scene_plan.render_duration_seconds,
        scene_plan.shots.len(),
        props_path.display(),
        adapters::remotion::descriptor().dependency_policy,
        remotion_plan.command_shape.join(" ")
    )
}

pub fn adapter_plan(manifest: impl AsRef<Path>) -> Result<Vec<AdapterPlanEntry>> {
    let loaded = load_manifest(manifest)?;
    validate_manifest(&loaded)?;
    manifest_adapter_plan(&loaded)
}

fn manifest_adapter_plan(loaded: &LoadedManifest) -> Result<Vec<AdapterPlanEntry>> {
    let declared = declared_adapter_ids(&loaded.raw)?;
    let catalog = adapters::adapter_catalog();
    let mut plan = Vec::new();

    for adapter_id in &declared {
        let adapter = catalog
            .iter()
            .find(|adapter| adapter.id.as_str() == adapter_id)
            .expect("declared adapter ids are validated before planning");
        plan.push(AdapterPlanEntry {
            id: adapter.id,
            status: adapter.status,
            declared_by_manifest: true,
            operations: adapter.operations.clone(),
            boundary: adapter.boundary,
            dependency_policy: adapter.dependency_policy,
        });
    }

    for adapter in catalog {
        if declared
            .iter()
            .any(|declared| declared == adapter.id.as_str())
        {
            continue;
        }
        plan.push(AdapterPlanEntry {
            id: adapter.id,
            status: adapter.status,
            declared_by_manifest: false,
            operations: adapter.operations,
            boundary: adapter.boundary,
            dependency_policy: adapter.dependency_policy,
        });
    }

    Ok(plan)
}

fn scene_plan_for_loaded(
    loaded: &LoadedManifest,
    report: &ValidationReport,
    scene_id: &str,
    platform: &str,
) -> Result<ScenePlan> {
    let (source_start_seconds, source_duration_seconds) = scene_span(&loaded.manifest, scene_id)?;
    let export = report
        .exports
        .iter()
        .find(|export| export.id == platform)
        .ok_or_else(|| anyhow!("unknown platform in manifest: {platform}"))?;
    let shots = loaded
        .manifest
        .shots
        .iter()
        .filter(|shot| shot.scene_id == scene_id)
        .map(|shot| SceneShotPlan {
            id: shot.id.clone(),
            source_start_seconds: shot.start_seconds,
            source_duration_seconds: shot.duration_seconds,
            render_start_seconds: (shot.start_seconds - source_start_seconds)
                * export.duration_scale,
            render_duration_seconds: shot.duration_seconds * export.duration_scale,
        })
        .collect::<Vec<_>>();

    if shots.is_empty() {
        bail!("scene {scene_id} has no shots");
    }

    Ok(ScenePlan {
        scene_id: scene_id.to_string(),
        platform: platform.to_string(),
        source_start_seconds,
        source_duration_seconds,
        render_duration_seconds: source_duration_seconds * export.duration_scale,
        width: export.width,
        height: export.height,
        duration_scale: export.duration_scale,
        shots,
    })
}

fn scene_span(manifest: &Manifest, scene_id: &str) -> Result<(f64, f64)> {
    let mut start = 0.0;
    for scene in &manifest.scenes {
        if scene.id == scene_id {
            return Ok((start, scene.duration_seconds));
        }
        start += scene.duration_seconds;
    }

    bail!("unknown scene in manifest: {scene_id}")
}

fn validate_required_top_fields(top: &Mapping) -> Result<()> {
    let missing = REQUIRED_TOP_FIELDS
        .iter()
        .filter(|field| !top.contains_key(Value::String((**field).to_string())))
        .copied()
        .collect::<Vec<_>>();

    if !missing.is_empty() {
        bail!("missing required top-level fields: {}", missing.join(", "));
    }

    Ok(())
}

fn validate_top_level_values(top: &Mapping) -> Result<()> {
    for field in REQUIRED_TOP_SCALAR_FIELDS {
        let value = top
            .get(Value::String((*field).to_string()))
            .expect("required top-level field was checked above");
        if is_empty_required_value(value) {
            bail!("{field} must not be empty");
        }
        if !matches!(value, Value::String(_)) {
            bail!("{field} must be a string");
        }
    }

    let manifest_version = top
        .get(Value::String("manifest_version".to_string()))
        .and_then(Value::as_str)
        .expect("manifest_version string was checked above");
    if manifest_version != SUPPORTED_MANIFEST_VERSION {
        bail!(
            "unsupported manifest_version: {manifest_version}; expected {SUPPORTED_MANIFEST_VERSION}"
        );
    }

    Ok(())
}

fn validate_required_mapping_fields(
    top: &Mapping,
    section_name: &str,
    required_fields: &[&str],
) -> Result<()> {
    let section = top
        .get(Value::String(section_name.to_string()))
        .ok_or_else(|| anyhow!("missing required top-level field: {section_name}"))?
        .as_mapping()
        .ok_or_else(|| anyhow!("{section_name} must be a mapping"))?;
    let missing = required_fields
        .iter()
        .filter(|field| !section.contains_key(Value::String((**field).to_string())))
        .copied()
        .collect::<Vec<_>>();

    if !missing.is_empty() {
        bail!(
            "{} missing required fields: {}",
            section_name,
            missing.join(", ")
        );
    }
    for field in required_fields {
        let value = section
            .get(Value::String((*field).to_string()))
            .expect("required field was checked above");
        if is_empty_required_value(value) {
            bail!("{section_name}.{field} must not be empty");
        }
    }

    Ok(())
}

fn validate_required_sequence_fields(
    top: &Mapping,
    section_name: &str,
    required_fields: &[&str],
) -> Result<()> {
    let section = top
        .get(Value::String(section_name.to_string()))
        .ok_or_else(|| anyhow!("missing required top-level field: {section_name}"))?
        .as_sequence()
        .ok_or_else(|| anyhow!("{section_name} must be a sequence"))?;

    for (index, item) in section.iter().enumerate() {
        let item = item
            .as_mapping()
            .ok_or_else(|| anyhow!("{section_name}[{}] must be a mapping", index + 1))?;
        let missing = required_fields
            .iter()
            .filter(|field| !item.contains_key(Value::String((**field).to_string())))
            .copied()
            .collect::<Vec<_>>();

        if !missing.is_empty() {
            bail!(
                "{}[{}] missing required fields: {}",
                section_name,
                index + 1,
                missing.join(", ")
            );
        }
        for field in required_fields {
            let value = item
                .get(Value::String((*field).to_string()))
                .expect("required field was checked above");
            if is_empty_required_value(value) {
                bail!("{section_name}[{}].{field} must not be empty", index + 1);
            }
        }
    }

    Ok(())
}

fn validate_optional_adapter_metadata(top: &Mapping) -> Result<()> {
    let known_adapters = adapters::adapter_catalog()
        .iter()
        .map(|adapter| adapter.id.as_str())
        .collect::<HashSet<_>>();
    let declared_adapters = declared_adapter_ids_from_top(top)?;

    for (index, adapter) in declared_adapters.iter().enumerate() {
        if !known_adapters.contains(adapter.as_str()) {
            bail!(
                "renderer_assumptions.adapters[{}] has unknown adapter id: {}",
                index + 1,
                adapter
            );
        }
    }

    Ok(())
}

fn declared_adapter_ids(raw: &Value) -> Result<Vec<String>> {
    let top = raw
        .as_mapping()
        .ok_or_else(|| anyhow!("manifest root must be a mapping"))?;
    declared_adapter_ids_from_top(top)
}

fn declared_adapter_ids_from_top(top: &Mapping) -> Result<Vec<String>> {
    let renderer_assumptions = top
        .get(Value::String("renderer_assumptions".to_string()))
        .ok_or_else(|| anyhow!("missing required top-level field: renderer_assumptions"))?
        .as_mapping()
        .ok_or_else(|| anyhow!("renderer_assumptions must be a mapping"))?;
    let Some(adapters) = renderer_assumptions.get(Value::String("adapters".to_string())) else {
        return Ok(Vec::new());
    };
    let adapters = adapters
        .as_sequence()
        .ok_or_else(|| anyhow!("renderer_assumptions.adapters must be a sequence"))?;
    if adapters.is_empty() {
        bail!("renderer_assumptions.adapters must not be empty when present");
    }

    let mut declared = Vec::new();
    let mut seen = HashSet::new();
    for (index, adapter) in adapters.iter().enumerate() {
        let adapter = adapter.as_str().ok_or_else(|| {
            anyhow!(
                "renderer_assumptions.adapters[{}] must be an adapter id string",
                index + 1
            )
        })?;
        if adapter.trim().is_empty() {
            bail!(
                "renderer_assumptions.adapters[{}] must not be empty",
                index + 1
            );
        }
        if !seen.insert(adapter) {
            bail!("duplicate renderer_assumptions.adapters id: {adapter}");
        }
        declared.push(adapter.to_string());
    }

    Ok(declared)
}

fn is_empty_required_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Sequence(items) => items.is_empty(),
        Value::Mapping(items) => items.is_empty(),
        Value::Bool(_) | Value::Number(_) => false,
        _ => false,
    }
}

fn validate_non_empty_sections(manifest: &Manifest) -> Result<()> {
    if manifest.platforms.is_empty() {
        bail!("platforms must not be empty");
    }
    if manifest.scenes.is_empty() {
        bail!("scenes must not be empty");
    }
    if manifest.shots.is_empty() {
        bail!("shots must not be empty");
    }
    if manifest.exports.is_empty() {
        bail!("exports must not be empty");
    }
    Ok(())
}

fn validate_unique_ids(manifest: &Manifest) -> Result<()> {
    ensure_unique(
        "scene id",
        manifest.scenes.iter().map(|scene| scene.id.as_str()),
    )?;
    ensure_unique(
        "shot id",
        manifest.shots.iter().map(|shot| shot.id.as_str()),
    )?;
    ensure_unique(
        "platform name",
        manifest
            .platforms
            .iter()
            .map(|platform| platform.name.as_str()),
    )?;
    ensure_unique(
        "export id",
        manifest.exports.iter().map(|export| export.id.as_str()),
    )?;
    Ok(())
}

fn validate_positive_timing(manifest: &Manifest) -> Result<()> {
    for scene in &manifest.scenes {
        if scene.duration_seconds <= 0.0 {
            bail!(
                "scene {} duration must be positive, got {:.3}",
                scene.id,
                scene.duration_seconds
            );
        }
    }

    for shot in &manifest.shots {
        if shot.start_seconds < 0.0 {
            bail!(
                "shot {} start_seconds must not be negative, got {:.3}",
                shot.id,
                shot.start_seconds
            );
        }
        if shot.duration_seconds <= 0.0 {
            bail!(
                "shot {} duration must be positive, got {:.3}",
                shot.id,
                shot.duration_seconds
            );
        }
    }

    Ok(())
}

fn validate_platform_export_coverage(manifest: &Manifest) -> Result<()> {
    let export_ids = manifest
        .exports
        .iter()
        .map(|export| export.id.as_str())
        .collect::<HashSet<_>>();

    for platform in &manifest.platforms {
        if !export_ids.contains(platform.name.as_str()) {
            bail!("platform {} has no matching export", platform.name);
        }
    }

    Ok(())
}

fn ensure_unique<'a>(label: &str, ids: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut seen = HashSet::new();
    for id in ids {
        if id.trim().is_empty() {
            bail!("{label} must not be empty");
        }
        if !seen.insert(id) {
            bail!("duplicate {label}: {id}");
        }
    }
    Ok(())
}

fn validate_shot_timing(shots: &[Shot]) -> Result<()> {
    let mut expected = 0.0;
    for shot in shots {
        if !same_duration(shot.start_seconds, expected) {
            bail!(
                "shot {} starts at {:.3}, expected {:.3}",
                shot.id,
                shot.start_seconds,
                expected
            );
        }
        expected += shot.duration_seconds;
    }
    Ok(())
}

fn validate_shot_scenes(manifest: &Manifest) -> Result<()> {
    let scene_ids = manifest
        .scenes
        .iter()
        .map(|scene| scene.id.as_str())
        .collect::<HashSet<_>>();

    for shot in &manifest.shots {
        if !scene_ids.contains(shot.scene_id.as_str()) {
            bail!(
                "shot {} references unknown scene_id: {}",
                shot.id,
                shot.scene_id
            );
        }
    }

    Ok(())
}

fn validate_shot_scene_spans(manifest: &Manifest) -> Result<()> {
    let mut scene_spans = Vec::new();
    let mut scene_start = 0.0;
    for scene in &manifest.scenes {
        let scene_end = scene_start + scene.duration_seconds;
        scene_spans.push((scene.id.as_str(), scene_start, scene_end));
        scene_start = scene_end;
    }

    for shot in &manifest.shots {
        let (_, scene_start, scene_end) = scene_spans
            .iter()
            .find(|(scene_id, _, _)| *scene_id == shot.scene_id)
            .ok_or_else(|| {
                anyhow!(
                    "shot {} references unknown scene_id: {}",
                    shot.id,
                    shot.scene_id
                )
            })?;
        let shot_end = shot.start_seconds + shot.duration_seconds;
        if shot.start_seconds + 0.001 < *scene_start || shot_end > scene_end + 0.001 {
            bail!(
                "shot {} timeline {:.3}-{:.3} falls outside scene {} span {:.3}-{:.3}",
                shot.id,
                shot.start_seconds,
                shot_end,
                shot.scene_id,
                scene_start,
                scene_end
            );
        }
    }

    Ok(())
}

fn export_plans(manifest: &Manifest, shot_total: f64) -> Result<Vec<ExportPlan>> {
    manifest
        .exports
        .iter()
        .map(|export| {
            let platform = manifest
                .platforms
                .iter()
                .find(|platform| platform.name == export.id)
                .ok_or_else(|| anyhow!("export {} has no matching platform", export.id))?;

            if export.aspect_ratio != platform.aspect_ratio {
                bail!(
                    "export {} aspect ratio ({}) does not match platform ({})",
                    export.id,
                    export.aspect_ratio,
                    platform.aspect_ratio
                );
            }
            if !same_duration(export.duration_seconds, platform.target_duration_seconds) {
                bail!(
                    "export {} duration ({:.3}) does not match platform target ({:.3})",
                    export.id,
                    export.duration_seconds,
                    platform.target_duration_seconds
                );
            }
            if export.filename.trim().is_empty() {
                bail!("export {} is missing filename", export.id);
            }
            if export.duration_seconds <= 0.0 || export.duration_seconds > shot_total {
                bail!(
                    "export {} duration ({:.3}) must be positive and no longer than shot total ({:.3})",
                    export.id,
                    export.duration_seconds,
                    shot_total
                );
            }

            let (width, height) = dimensions_for_aspect(&export.aspect_ratio)?;

            Ok(ExportPlan {
                id: export.id.clone(),
                aspect_ratio: export.aspect_ratio.clone(),
                width,
                height,
                duration_seconds: export.duration_seconds,
                duration_scale: export.duration_seconds / shot_total,
                filename: export.filename.clone(),
            })
        })
        .collect()
}

fn dimensions_for_aspect(aspect_ratio: &str) -> Result<(u32, u32)> {
    match aspect_ratio {
        "16:9" => Ok((1280, 720)),
        "9:16" => Ok((720, 1280)),
        other => bail!("unsupported aspect ratio: {other}"),
    }
}

fn discover_work_manifests(root: &Path) -> Result<Vec<PathBuf>> {
    let mut manifests = Vec::new();

    for entry in fs::read_dir(root).with_context(|| format!("failed to read {}", root.display()))? {
        let entry =
            entry.with_context(|| format!("failed to read entry under {}", root.display()))?;
        let path = entry.path().join("manifest.yaml");
        if path.is_file() {
            manifests.push(path);
        }
    }

    manifests.sort();
    Ok(manifests)
}

fn render_smoke_for_manifest(loaded: &LoadedManifest) -> Result<PathBuf> {
    let ffmpeg = adapters::ffmpeg::FfmpegAdapter;
    let out_dir = PathBuf::from("renders/smoke");
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    let out_file = out_dir.join(format!("{}-smoke.mp4", loaded.manifest.work));

    let temp_dir = tempdir().context("failed to create temporary smoke-render workspace")?;
    let card_file = temp_dir.path().join("card.txt");
    fs::write(&card_file, smoke_card_text(loaded))
        .with_context(|| format!("failed to write {}", card_file.display()))?;

    let filter = format!(
        "drawtext=textfile={}:fontcolor=white:fontsize=42:line_spacing=16:x=(w-text_w)/2:y=(h-text_h)/2,format=yuv420p",
        ffmpeg.path_argument(&card_file)?
    );

    ffmpeg.run_ffmpeg(
        &[
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-y".to_string(),
            "-f".to_string(),
            "lavfi".to_string(),
            "-i".to_string(),
            "color=c=0x041E42:s=1280x720:d=6:r=24".to_string(),
            "-f".to_string(),
            "lavfi".to_string(),
            "-i".to_string(),
            "anullsrc=channel_layout=stereo:sample_rate=48000".to_string(),
            "-vf".to_string(),
            filter,
            "-shortest".to_string(),
            "-c:v".to_string(),
            "libx264".to_string(),
            "-pix_fmt".to_string(),
            "yuv420p".to_string(),
            "-c:a".to_string(),
            "aac".to_string(),
            "-t".to_string(),
            "6".to_string(),
        ],
        &[ffmpeg.path_argument(&out_file)?],
    )?;

    Ok(out_file)
}

fn smoke_card_text(loaded: &LoadedManifest) -> String {
    let caption = loaded
        .manifest
        .shots
        .first()
        .map(|shot| non_empty(&shot.captions.text, "manifest-fed smoke render"))
        .unwrap_or("manifest-fed smoke render");

    format!(
        "{}\nsource: {} / {}\nformat: {}\nstyle: {}\n{}",
        loaded.manifest.title,
        loaded.manifest.source_scenario.repo,
        loaded.manifest.source_scenario.id,
        loaded.manifest.format,
        loaded.manifest.style,
        caption
    )
}

fn render_contact_sheet_for_export(
    loaded: &LoadedManifest,
    export: &ExportPlan,
) -> Result<PathBuf> {
    let ffmpeg = adapters::ffmpeg::FfmpegAdapter;
    let video_file = PathBuf::from(format!(
        "renders/shot-cards/{}-{}-shot-cards.mp4",
        loaded.manifest.work, export.id
    ));
    if !video_file.is_file() {
        render_shot_cards_for_export(loaded, export)?;
    }

    let out_dir = PathBuf::from("renders/contact-sheets");
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    let out_file = out_dir.join(format!(
        "{}-{}-contact-sheet.png",
        loaded.manifest.work, export.id
    ));

    let columns = 4usize;
    let rows = loaded.manifest.shots.len().div_ceil(columns);
    let fps = format!(
        "{}/{}",
        loaded.manifest.shots.len(),
        compact_seconds(export.duration_seconds)
    );
    let filter =
        format!("fps={fps},scale=320:-1,tile={columns}x{rows}:padding=8:margin=8:color=0x111111");

    ffmpeg.run_ffmpeg(
        &[
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-y".to_string(),
            "-i".to_string(),
            ffmpeg.path_argument(&video_file)?,
            "-vf".to_string(),
            filter,
            "-frames:v".to_string(),
            "1".to_string(),
        ],
        &[ffmpeg.path_argument(&out_file)?],
    )?;

    Ok(out_file)
}

fn render_shot_cards_for_export(loaded: &LoadedManifest, export: &ExportPlan) -> Result<PathBuf> {
    let ffmpeg = adapters::ffmpeg::FfmpegAdapter;
    let out_dir = PathBuf::from("renders/shot-cards");
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    let out_file = out_dir.join(format!(
        "{}-{}-shot-cards.mp4",
        loaded.manifest.work, export.id
    ));

    let temp_dir = tempdir().context("failed to create temporary shot-card workspace")?;
    let concat_file = temp_dir.path().join("concat.txt");
    let mut concat = String::new();

    let font_size = if export.aspect_ratio == "9:16" {
        28
    } else {
        32
    };
    let wrap_width = if export.aspect_ratio == "9:16" {
        34
    } else {
        56
    };

    for (index, shot) in loaded.manifest.shots.iter().enumerate() {
        let clip_file = temp_dir.path().join(format!("shot-{}.mp4", index + 1));
        let card_file = temp_dir.path().join(format!("card-{}.txt", index + 1));
        let duration = shot.duration_seconds * export.duration_scale;
        let bg_color = scene_color(&shot.scene_id);

        fs::write(&card_file, shot_card_text(loaded, shot, export, wrap_width))
            .with_context(|| format!("failed to write {}", card_file.display()))?;

        let source = format!(
            "color=c={}:s={}x{}:d={}:r=24",
            bg_color,
            export.width,
            export.height,
            compact_seconds(duration)
        );
        let filter = format!(
            "drawbox=x=0:y=0:w=iw:h=10:color=white@0.45:t=fill,drawbox=x=0:y=ih-90:w=iw:h=90:color=black@0.30:t=fill,drawtext=textfile={}:fontcolor=white:fontsize={font_size}:line_spacing=10:x=70:y=70,format=yuv420p",
            ffmpeg.path_argument(&card_file)?
        );

        ffmpeg.run_ffmpeg(
            &[
                "-hide_banner".to_string(),
                "-loglevel".to_string(),
                "error".to_string(),
                "-y".to_string(),
                "-f".to_string(),
                "lavfi".to_string(),
                "-i".to_string(),
                source,
                "-vf".to_string(),
                filter,
                "-c:v".to_string(),
                "libx264".to_string(),
                "-pix_fmt".to_string(),
                "yuv420p".to_string(),
                "-t".to_string(),
                compact_seconds(duration),
            ],
            &[ffmpeg.path_argument(&clip_file)?],
        )?;

        concat.push_str(&format!("file '{}'\n", ffmpeg.path_for_concat(&clip_file)?));
    }

    fs::write(&concat_file, concat)
        .with_context(|| format!("failed to write {}", concat_file.display()))?;

    ffmpeg.run_ffmpeg(
        &[
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-y".to_string(),
            "-f".to_string(),
            "concat".to_string(),
            "-safe".to_string(),
            "0".to_string(),
            "-i".to_string(),
            ffmpeg.path_argument(&concat_file)?,
            "-fflags".to_string(),
            "+genpts".to_string(),
            "-c:v".to_string(),
            "libx264".to_string(),
            "-pix_fmt".to_string(),
            "yuv420p".to_string(),
            "-avoid_negative_ts".to_string(),
            "make_zero".to_string(),
        ],
        &[ffmpeg.path_argument(&out_file)?],
    )?;

    Ok(out_file)
}

fn render_scene_preview_for_plan(loaded: &LoadedManifest, plan: &ScenePlan) -> Result<PathBuf> {
    let ffmpeg = adapters::ffmpeg::FfmpegAdapter;
    let out_dir = PathBuf::from("renders/scene-previews");
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    let out_file = out_dir.join(format!(
        "{}-{}-{}-preview.mp4",
        loaded.manifest.work, plan.scene_id, plan.platform
    ));

    let temp_dir = tempdir().context("failed to create temporary scene-preview workspace")?;
    let concat_file = temp_dir.path().join("concat.txt");
    let mut concat = String::new();
    let font_size = if plan.height > plan.width { 28 } else { 34 };
    let wrap_width = if plan.height > plan.width { 34 } else { 58 };

    for (index, shot_plan) in plan.shots.iter().enumerate() {
        let shot = loaded
            .manifest
            .shots
            .iter()
            .find(|shot| shot.id == shot_plan.id)
            .ok_or_else(|| anyhow!("scene plan references unknown shot: {}", shot_plan.id))?;
        let clip_file = temp_dir
            .path()
            .join(format!("scene-shot-{}.mp4", index + 1));
        let text_file = temp_dir
            .path()
            .join(format!("scene-shot-{}.txt", index + 1));
        fs::write(
            &text_file,
            scene_preview_text(loaded, shot, shot_plan, plan, wrap_width),
        )
        .with_context(|| format!("failed to write {}", text_file.display()))?;

        let duration = compact_seconds(shot_plan.render_duration_seconds);
        let source = format!(
            "color=c={}:s={}x{}:d={}:r=24",
            scene_color(&plan.scene_id),
            plan.width,
            plan.height,
            duration
        );
        let primary_x = (plan.width as f64 * 0.07).round() as u32;
        let primary_y = (plan.height as f64 * 0.16).round() as u32;
        let primary_w = (plan.width as f64 * 0.38).round() as u32;
        let primary_h = (plan.height as f64 * 0.22).round() as u32;
        let accent_x = (plan.width as f64 * 0.62).round() as u32;
        let accent_y = (plan.height as f64 * 0.46).round() as u32;
        let accent_w = (plan.width as f64 * 0.24).round() as u32;
        let accent_h = (plan.height as f64 * 0.16).round() as u32;
        let caption_h = (plan.height as f64 * 0.24).round() as u32;
        let caption_y = plan.height.saturating_sub(caption_h);
        let filter = format!(
            "drawbox=x={primary_x}:y={primary_y}:w={primary_w}:h={primary_h}:color=white@0.12:t=fill,drawbox=x={accent_x}:y={accent_y}:w={accent_w}:h={accent_h}:color=0x8fd3ff@0.16:t=fill,drawbox=x=0:y={caption_y}:w={}:h={caption_h}:color=black@0.48:t=fill,drawtext=textfile={}:fontcolor=white:fontsize={font_size}:line_spacing=10:x='(w*0.07)+(w*0.012)*t':y='h*0.08',format=yuv420p",
            plan.width,
            ffmpeg.path_argument(&text_file)?
        );

        ffmpeg.run_ffmpeg(
            &[
                "-hide_banner".to_string(),
                "-loglevel".to_string(),
                "error".to_string(),
                "-y".to_string(),
                "-f".to_string(),
                "lavfi".to_string(),
                "-i".to_string(),
                source,
                "-vf".to_string(),
                filter,
                "-c:v".to_string(),
                "libx264".to_string(),
                "-pix_fmt".to_string(),
                "yuv420p".to_string(),
                "-t".to_string(),
                duration,
            ],
            &[ffmpeg.path_argument(&clip_file)?],
        )?;

        concat.push_str(&format!("file '{}'\n", ffmpeg.path_for_concat(&clip_file)?));
    }

    fs::write(&concat_file, concat)
        .with_context(|| format!("failed to write {}", concat_file.display()))?;
    ffmpeg.run_ffmpeg(
        &[
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-y".to_string(),
            "-f".to_string(),
            "concat".to_string(),
            "-safe".to_string(),
            "0".to_string(),
            "-i".to_string(),
            ffmpeg.path_argument(&concat_file)?,
            "-fflags".to_string(),
            "+genpts".to_string(),
            "-c:v".to_string(),
            "libx264".to_string(),
            "-pix_fmt".to_string(),
            "yuv420p".to_string(),
            "-avoid_negative_ts".to_string(),
            "make_zero".to_string(),
        ],
        &[ffmpeg.path_argument(&out_file)?],
    )?;

    Ok(out_file)
}

fn concat_mp4_files(files: &[PathBuf], out_file: &Path) -> Result<()> {
    let ffmpeg = adapters::ffmpeg::FfmpegAdapter;
    let temp_dir = tempdir().context("failed to create temporary concat workspace")?;
    let concat_file = temp_dir.path().join("concat.txt");
    let mut concat = String::new();
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    for file in files {
        let absolute = if file.is_absolute() {
            file.to_path_buf()
        } else {
            cwd.join(file)
        };
        concat.push_str(&format!("file '{}'\n", ffmpeg.path_for_concat(&absolute)?));
    }
    fs::write(&concat_file, concat)
        .with_context(|| format!("failed to write {}", concat_file.display()))?;

    ffmpeg.run_ffmpeg(
        &[
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-y".to_string(),
            "-f".to_string(),
            "concat".to_string(),
            "-safe".to_string(),
            "0".to_string(),
            "-i".to_string(),
            ffmpeg.path_argument(&concat_file)?,
            "-fflags".to_string(),
            "+genpts".to_string(),
            "-c:v".to_string(),
            "libx264".to_string(),
            "-pix_fmt".to_string(),
            "yuv420p".to_string(),
            "-avoid_negative_ts".to_string(),
            "make_zero".to_string(),
        ],
        &[ffmpeg.path_argument(out_file)?],
    )?;

    Ok(())
}

fn render_work_preview_for_paths(
    work: &str,
    platform: &str,
    previews: &[PathBuf],
) -> Result<PathBuf> {
    if previews.is_empty() {
        bail!("no scene previews to concatenate");
    }

    let out_dir = PathBuf::from("renders/work-previews");
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    let out_file = out_dir.join(format!("{work}-{platform}-preview.mp4"));
    concat_mp4_files(previews, &out_file)?;
    Ok(out_file)
}

fn scene_preview_text(
    loaded: &LoadedManifest,
    shot: &Shot,
    shot_plan: &SceneShotPlan,
    plan: &ScenePlan,
    wrap_width: usize,
) -> String {
    format!(
        "{}\n{} | {} | {} | render {:.3}-{:.3}s\n\nCaption: {}\n\nAction: {}\n\nCamera: {}\n\nNarration: {}",
        loaded.manifest.title,
        plan.scene_id,
        shot.id,
        plan.platform,
        shot_plan.render_start_seconds,
        shot_plan.render_start_seconds + shot_plan.render_duration_seconds,
        wrap_text(non_empty(&shot.captions.text, "No caption"), wrap_width),
        wrap_text(non_empty(&shot.action, "No action note"), wrap_width),
        wrap_text(non_empty(&shot.camera, "No camera note"), wrap_width),
        wrap_text(non_empty(&shot.audio.narration, "No narration"), wrap_width),
    )
}

fn shot_card_text(
    loaded: &LoadedManifest,
    shot: &Shot,
    export: &ExportPlan,
    wrap_width: usize,
) -> String {
    format!(
        "{}\n{} | {} | {} | {} | {} | {} | target {}s\n\nCaption: {}\n\nVisual: {}\n\nCamera: {}\n\nAction: {}\n\nNarration: {}\n",
        loaded.manifest.title,
        shot.id,
        shot.scene_id,
        loaded.manifest.format,
        loaded.manifest.style,
        export.id,
        export.aspect_ratio,
        compact_seconds(export.duration_seconds),
        wrap_text(non_empty(&shot.captions.text, "No caption"), wrap_width),
        wrap_text(
            non_empty(&shot.visual_prompt, "No visual prompt"),
            wrap_width
        ),
        wrap_text(non_empty(&shot.camera, "No camera note"), wrap_width),
        wrap_text(non_empty(&shot.action, "No action note"), wrap_width),
        wrap_text(non_empty(&shot.audio.narration, "No narration"), wrap_width),
    )
}

fn non_empty<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

fn wrap_text(value: &str, width: usize) -> String {
    let mut result = String::new();
    let mut current = 0usize;

    for word in value.split_whitespace() {
        let separator = usize::from(current != 0);
        if current != 0 && current + separator + word.len() > width {
            result.push('\n');
            current = 0;
        } else if current != 0 {
            result.push(' ');
            current += 1;
        }
        result.push_str(word);
        current += word.len();
    }

    result
}

fn scene_color(scene_id: &str) -> &'static str {
    match scene_id {
        "scene-01" => "0x102A43",
        "scene-02" => "0x243B2F",
        "scene-03" => "0x3B263A",
        _ => "0x041E42",
    }
}

fn ffprobe_duration(path: &Path) -> Result<String> {
    let ffmpeg = adapters::ffmpeg::FfmpegAdapter;
    ffmpeg.ffprobe_duration(path)
}

fn ffprobe_duration_seconds(path: &Path) -> Result<f64> {
    let duration = ffprobe_duration(path)?;
    duration.parse::<f64>().with_context(|| {
        format!(
            "ffprobe returned non-numeric duration for {}",
            path.display()
        )
    })
}

fn ffprobe_duration_label(path: &Path) -> Result<String> {
    let duration = ffprobe_duration(path)?;
    match duration.parse::<f64>() {
        Ok(value) => Ok(compact_seconds(value)),
        Err(_) => Ok(duration),
    }
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}

fn file_bytes(path: &Path) -> Result<u64> {
    Ok(fs::metadata(path)
        .with_context(|| format!("failed to inspect {}", path.display()))?
        .len())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("failed to hash {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to hash {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    Ok(hex)
}

fn check_artifact_video(video: &ArtifactVideo, label: &str) -> Result<()> {
    check_artifact_file(&video.path, video.bytes, &video.sha256, label)?;
    if video.duration_seconds <= 0.0 {
        bail!(
            "{label} has non-positive duration: {}",
            video.duration_seconds
        );
    }
    Ok(())
}

fn check_artifact_duration(
    video: &ArtifactVideo,
    expected_duration: f64,
    label: &str,
) -> Result<()> {
    if !same_duration(video.duration_seconds, expected_duration) {
        bail!(
            "{label} duration mismatch: expected {expected_duration}, found {}",
            video.duration_seconds
        );
    }
    Ok(())
}

fn check_artifact_image(image: &ArtifactImage, label: &str) -> Result<()> {
    check_artifact_file(&image.path, image.bytes, &image.sha256, label)
}

fn check_artifact_file(
    path: &str,
    expected_bytes: u64,
    expected_sha256: &str,
    label: &str,
) -> Result<()> {
    let actual_bytes = fs::metadata(path)
        .with_context(|| format!("{label} missing artifact file: {path}"))?
        .len();
    if actual_bytes != expected_bytes {
        bail!("{label} byte mismatch for {path}: expected {expected_bytes}, found {actual_bytes}");
    }
    let actual_sha256 = sha256_file(Path::new(path))?;
    if actual_sha256 != expected_sha256 {
        bail!(
            "{label} sha256 mismatch for {path}: expected {expected_sha256}, found {actual_sha256}"
        );
    }
    Ok(())
}

fn same_duration(left: f64, right: f64) -> bool {
    (left - right).abs() < 0.001
}

fn compact_seconds(value: f64) -> String {
    if same_duration(value.fract(), 0.0) {
        format!("{value:.0}")
    } else {
        format!("{value:.3}")
    }
}

fn unix_now() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time is before unix epoch")?
        .as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn validates_ash_vale_manifest_and_derives_exports() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let report = validate_manifest(&manifest).expect("manifest validates");

        assert!(same_duration(report.scene_total, 60.0));
        assert!(same_duration(report.shot_total, 60.0));
        assert_eq!(report.exports.len(), 2);
        assert_eq!(
            report.exports[0],
            ExportPlan {
                id: "iphone-social".to_string(),
                aspect_ratio: "9:16".to_string(),
                width: 720,
                height: 1280,
                duration_seconds: 45.0,
                duration_scale: 0.75,
                filename: "0001-ash-vale-last-road-before-winter-iphone.mp4".to_string(),
            }
        );
        assert_eq!(report.exports[1].id, "youtube-demo");
        assert!(same_duration(report.exports[1].duration_scale, 1.0));
    }

    #[test]
    fn derives_scene_plan_for_first_ash_vale_scene() {
        let plan = scene_plan(
            "works/0001-ash-vale-last-road-before-winter/manifest.yaml",
            "scene-01",
            "youtube-demo",
        )
        .expect("scene plan renders");

        assert_eq!(plan.scene_id, "scene-01");
        assert_eq!(plan.platform, "youtube-demo");
        assert!(same_duration(plan.source_start_seconds, 0.0));
        assert!(same_duration(plan.source_duration_seconds, 11.0));
        assert!(same_duration(plan.render_duration_seconds, 11.0));
        assert_eq!(plan.width, 1280);
        assert_eq!(plan.height, 720);
        assert_eq!(plan.shots.len(), 2);
        assert_eq!(plan.shots[0].id, "shot-001");
        assert!(same_duration(plan.shots[1].render_start_seconds, 5.0));
    }

    #[test]
    fn rejects_unknown_scene_plan_ids() {
        let error = scene_plan(
            "works/0001-ash-vale-last-road-before-winter/manifest.yaml",
            "scene-99",
            "youtube-demo",
        )
        .expect_err("unknown scene is rejected");

        assert!(error.to_string().contains("unknown scene in manifest"));
    }

    #[test]
    fn validates_starter_manifest_template() {
        let manifest =
            load_manifest("manifests/templates/scenario-video.yaml").expect("template loads");
        let report = validate_manifest(&manifest).expect("template validates");

        assert!(same_duration(report.scene_total, 60.0));
        assert!(same_duration(report.shot_total, 60.0));
        assert_eq!(report.exports.len(), 2);
        assert_eq!(report.exports[0].id, "iphone-social");
        assert!(same_duration(report.exports[0].duration_scale, 0.75));
        assert_eq!(report.exports[1].id, "youtube-demo");
        assert!(same_duration(report.exports[1].duration_scale, 1.0));
    }

    #[test]
    fn summarizes_multi_work_corpus_without_rendering() {
        let report = summarize_work_corpus("works").expect("corpus summarizes");

        assert_eq!(report.works_root, "works");
        assert_eq!(report.works, 2);
        assert_eq!(
            report.manifests,
            vec![
                "works\\0001-ash-vale-last-road-before-winter\\manifest.yaml",
                "works\\0002-court-first-rally\\manifest.yaml"
            ]
        );
        assert_eq!(report.manifest_versions, vec!["reel.manifest.v0.1"]);
        assert_eq!(
            report.work_ids,
            vec![
                "0001-ash-vale-last-road-before-winter",
                "0002-court-first-rally"
            ]
        );
        assert_eq!(
            report.work_titles,
            vec!["Ash Vale: Last Road Before Winter", "COURT: First Rally"]
        );
        assert_eq!(report.source_repos, vec!["BANISH", "COURT"]);
        assert_eq!(
            report.source_ids,
            vec![
                "scenario:banish:ash-vale-last-road-before-winter",
                "scenario:court:first-rally"
            ]
        );
        assert_eq!(
            report.source_paths,
            vec![
                "docs/canon-ash-vale-scenario.md",
                "docs/scenarios/first-rally.md"
            ]
        );
        assert_eq!(
            report.source_commits,
            vec!["3f20886eaaae3657562a010d5bdbc6316e1f6fbb", "unknown"]
        );
        assert_eq!(
            report.audience_primaries,
            vec!["portfolio reviewer and Games Design collaborator"]
        );
        assert_eq!(
            report.audience_contexts,
            vec![
                "desktop review of a lightweight second corpus item",
                "phone-first preview and desktop review"
            ]
        );
        assert_eq!(
            report.audience_desired_effects,
            vec![
                "understand BANISH Pilgrim Loss in under 60 seconds",
                "understand COURT's rally loop in under 30 seconds"
            ]
        );
        assert_eq!(report.formats, vec!["trailer"]);
        assert_eq!(report.styles, vec!["isometric-game", "storyboard-animatic"]);
        assert_eq!(
            report.alternate_styles,
            vec!["isometric-game", "storyboard-animatic"]
        );
        assert_eq!(report.platform_names, vec!["iphone-social", "youtube-demo"]);
        assert_eq!(report.platforms, 3);
        assert_eq!(report.scenes, 5);
        assert_eq!(report.shots, 12);
        assert_eq!(report.exports, 3);
        assert!(same_duration(report.total_scene_duration_seconds, 80.0));
        assert!(same_duration(report.total_shot_duration_seconds, 80.0));
    }

    #[test]
    fn summarizes_review_queue_without_rendering() {
        let report = summarize_review_queue("works").expect("review queue summarizes");

        assert_eq!(report.works_root, "works");
        assert_eq!(report.works, 2);
        assert_eq!(
            report.manifests,
            vec![
                "works\\0001-ash-vale-last-road-before-winter\\manifest.yaml",
                "works\\0002-court-first-rally\\manifest.yaml"
            ]
        );
        assert_eq!(report.review_statuses, vec!["not-reviewed", "reviewed"]);
        assert_eq!(report.review_status_counts["not-reviewed"], 1);
        assert_eq!(report.review_status_counts["reviewed"], 1);
        assert_eq!(
            report.review_status_work_ids["not-reviewed"],
            vec!["0002-court-first-rally"]
        );
        assert_eq!(
            report.review_status_work_titles["reviewed"],
            vec!["Ash Vale: Last Road Before Winter"]
        );
        assert_eq!(
            report.required_roles,
            vec![
                "animation-director",
                "editor",
                "platform-audience",
                "sound-designer",
                "story-director"
            ]
        );
        assert_eq!(report.required_role_counts["editor"], 2);
        assert_eq!(
            report.required_role_status_counts["editor"]["not-reviewed"],
            1
        );
        assert_eq!(report.reports[0].review_status, "reviewed");
        assert_eq!(report.reports[1].review_status, "not-reviewed");
    }

    #[test]
    fn rejects_duplicate_manifest_ids() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let mut raw = manifest.raw.clone();
        raw["shots"][1]["id"] = raw["shots"][0]["id"].clone();
        let parsed = serde_yaml::from_value(raw.clone()).expect("duplicate manifest deserializes");
        let duplicate = LoadedManifest {
            path: manifest.path,
            raw,
            manifest: parsed,
        };

        let error = validate_manifest(&duplicate).expect_err("duplicate shot id rejected");
        assert!(error.to_string().contains("duplicate shot id"));
    }

    #[test]
    fn rejects_platforms_without_matching_exports() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let mut raw = manifest.raw.clone();
        raw.as_mapping_mut()
            .expect("raw manifest is mapping")
            .get_mut(&Value::String("exports".to_string()))
            .expect("exports exists")
            .as_sequence_mut()
            .expect("exports is sequence")
            .remove(0);
        let parsed =
            serde_yaml::from_value(raw.clone()).expect("missing export manifest deserializes");
        let missing = LoadedManifest {
            path: manifest.path,
            raw,
            manifest: parsed,
        };

        let error = validate_manifest(&missing).expect_err("missing platform export rejected");
        assert!(
            error
                .to_string()
                .contains("platform iphone-social has no matching export")
        );
    }

    #[test]
    fn rejects_shots_outside_referenced_scene_span() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let mut raw = manifest.raw.clone();
        raw["shots"][2]["scene_id"] = Value::String("scene-01".to_string());
        let parsed = serde_yaml::from_value(raw.clone()).expect("scene span manifest deserializes");
        let invalid = LoadedManifest {
            path: manifest.path,
            raw,
            manifest: parsed,
        };

        let error = validate_manifest(&invalid).expect_err("out-of-scene shot rejected");
        assert!(
            error
                .to_string()
                .contains("falls outside scene scene-01 span")
        );
    }

    #[test]
    fn rejects_scene_missing_documented_required_fields() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let mut raw = manifest.raw.clone();
        raw["scenes"][0]
            .as_mapping_mut()
            .expect("scene is mapping")
            .remove(Value::String("purpose".to_string()));
        let parsed = serde_yaml::from_value(raw.clone()).expect("scene manifest deserializes");
        let invalid = LoadedManifest {
            path: manifest.path,
            raw,
            manifest: parsed,
        };

        let error = validate_manifest(&invalid).expect_err("missing scene field rejected");
        assert!(
            error
                .to_string()
                .contains("scenes[1] missing required fields: purpose")
        );
    }

    #[test]
    fn rejects_shot_missing_documented_required_fields() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let mut raw = manifest.raw.clone();
        raw["shots"][0]
            .as_mapping_mut()
            .expect("shot is mapping")
            .remove(Value::String("transition_out".to_string()));
        let parsed = serde_yaml::from_value(raw.clone()).expect("shot manifest deserializes");
        let invalid = LoadedManifest {
            path: manifest.path,
            raw,
            manifest: parsed,
        };

        let error = validate_manifest(&invalid).expect_err("missing shot field rejected");
        assert!(
            error
                .to_string()
                .contains("shots[1] missing required fields: transition_out")
        );
    }

    #[test]
    fn rejects_platform_missing_documented_required_fields() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let mut raw = manifest.raw.clone();
        raw["platforms"][0]
            .as_mapping_mut()
            .expect("platform is mapping")
            .remove(Value::String("sound_optional".to_string()));
        let parsed = serde_yaml::from_value(raw.clone()).expect("platform manifest deserializes");
        let invalid = LoadedManifest {
            path: manifest.path,
            raw,
            manifest: parsed,
        };

        let error = validate_manifest(&invalid).expect_err("missing platform field rejected");
        assert!(
            error
                .to_string()
                .contains("platforms[1] missing required fields: sound_optional")
        );
    }

    #[test]
    fn rejects_export_missing_documented_required_fields() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let mut raw = manifest.raw.clone();
        raw["exports"][0]
            .as_mapping_mut()
            .expect("export is mapping")
            .remove(Value::String("filename".to_string()));
        let invalid = LoadedManifest {
            path: manifest.path,
            raw,
            manifest: manifest.manifest,
        };

        let error = validate_manifest(&invalid).expect_err("missing export field rejected");
        assert!(
            error
                .to_string()
                .contains("exports[1] missing required fields: filename")
        );
    }

    #[test]
    fn accepts_known_optional_adapter_metadata() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let mut raw = manifest.raw.clone();
        raw["renderer_assumptions"]["adapters"] = Value::Sequence(vec![
            Value::String("ffmpeg".to_string()),
            Value::String("remotion".to_string()),
        ]);
        let with_adapters = LoadedManifest {
            path: manifest.path,
            raw,
            manifest: manifest.manifest,
        };

        validate_manifest(&with_adapters).expect("known adapter ids validate");
    }

    #[test]
    fn rejects_unknown_optional_adapter_metadata() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let mut raw = manifest.raw.clone();
        raw["renderer_assumptions"]["adapters"] =
            Value::Sequence(vec![Value::String("vendor-video".to_string())]);
        let invalid = LoadedManifest {
            path: manifest.path,
            raw,
            manifest: manifest.manifest,
        };

        let error = validate_manifest(&invalid).expect_err("unknown adapter id rejected");
        assert!(
            error
                .to_string()
                .contains("renderer_assumptions.adapters[1] has unknown adapter id: vendor-video")
        );
    }

    #[test]
    fn review_pack_includes_adapter_summary() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let markdown = review_pack_adapter_summary(&manifest).expect("summary renders");

        assert!(markdown.contains("## Adapter summary"));
        assert!(markdown.contains("| `ffmpeg` | `implemented-baseline` | yes |"));
        assert!(markdown.contains("| `remotion` | `planned` | yes |"));
        assert!(markdown.contains("| `blender` | `planned` | no |"));
        assert!(markdown.contains("| `ai-video` | `planned` | yes |"));
    }

    #[test]
    fn review_pack_includes_review_role_summary() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let markdown = review_pack_review_summary(&manifest).expect("summary renders");

        assert!(markdown.contains("## Review summary"));
        assert!(markdown.contains("- Status: `reviewed`"));
        assert!(markdown.contains(
            "- Required roles: `story-director`, `animation-director`, `editor`, `sound-designer`, `platform-audience`"
        ));
        assert!(
            markdown.contains(
                "| `story-director` | Scenario truth, narrative promise, and continuity. |"
            )
        );
        assert!(markdown.contains(
            "| `platform-audience` | Platform fit, audience clarity, captions, and safe areas. |"
        ));
    }

    #[test]
    fn adapter_plan_marks_manifest_declared_adapters() {
        let adapters = adapter_plan("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("adapter plan renders");

        assert_eq!(adapters.len(), 4);
        assert_eq!(adapters[0].id, adapters::AdapterId::Ffmpeg);
        assert_eq!(adapters[1].id, adapters::AdapterId::Remotion);
        assert_eq!(adapters[2].id, adapters::AdapterId::AiVideo);
        assert_eq!(adapters[3].id, adapters::AdapterId::Blender);
        assert!(
            adapters
                .iter()
                .find(|adapter| adapter.id == adapters::AdapterId::Ffmpeg)
                .expect("ffmpeg exists")
                .declared_by_manifest
        );
        assert!(
            !adapters
                .iter()
                .find(|adapter| adapter.id == adapters::AdapterId::Blender)
                .expect("blender exists")
                .declared_by_manifest
        );
    }

    #[test]
    fn rejects_duplicate_optional_adapter_metadata() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let mut raw = manifest.raw.clone();
        raw["renderer_assumptions"]["adapters"] = Value::Sequence(vec![
            Value::String("ffmpeg".to_string()),
            Value::String("ffmpeg".to_string()),
        ]);
        let invalid = LoadedManifest {
            path: manifest.path,
            raw,
            manifest: manifest.manifest,
        };

        let error = validate_manifest(&invalid).expect_err("duplicate adapter id rejected");
        assert!(
            error
                .to_string()
                .contains("duplicate renderer_assumptions.adapters id: ffmpeg")
        );
    }

    #[test]
    fn rejects_source_scenario_missing_documented_required_fields() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let mut raw = manifest.raw.clone();
        raw["source_scenario"]
            .as_mapping_mut()
            .expect("source_scenario is mapping")
            .remove(Value::String("path".to_string()));
        let parsed = serde_yaml::from_value(raw.clone()).expect("metadata manifest deserializes");
        let invalid = LoadedManifest {
            path: manifest.path,
            raw,
            manifest: parsed,
        };

        let error = validate_manifest(&invalid).expect_err("missing source path rejected");
        assert!(
            error
                .to_string()
                .contains("source_scenario missing required fields: path")
        );
    }

    #[test]
    fn rejects_review_missing_documented_required_fields() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let mut raw = manifest.raw.clone();
        raw["review"]
            .as_mapping_mut()
            .expect("review is mapping")
            .remove(Value::String("required_roles".to_string()));
        let parsed = serde_yaml::from_value(raw.clone()).expect("review manifest deserializes");
        let invalid = LoadedManifest {
            path: manifest.path,
            raw,
            manifest: parsed,
        };

        let error = validate_manifest(&invalid).expect_err("missing review roles rejected");
        assert!(
            error
                .to_string()
                .contains("review missing required fields: required_roles")
        );
    }

    #[test]
    fn rejects_empty_required_metadata_values() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let mut raw = manifest.raw.clone();
        raw["renderer_assumptions"]["candidates"] = Value::Sequence(Vec::new());
        let parsed =
            serde_yaml::from_value(raw.clone()).expect("empty metadata manifest deserializes");
        let invalid = LoadedManifest {
            path: manifest.path,
            raw,
            manifest: parsed,
        };

        let error = validate_manifest(&invalid).expect_err("empty metadata rejected");
        assert!(
            error
                .to_string()
                .contains("renderer_assumptions.candidates must not be empty")
        );
    }

    #[test]
    fn rejects_empty_required_scene_and_shot_values() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let mut raw = manifest.raw.clone();
        raw["scenes"][0]["characters"] = Value::Sequence(Vec::new());
        let parsed =
            serde_yaml::from_value(raw.clone()).expect("empty scene manifest deserializes");
        let invalid = LoadedManifest {
            path: manifest.path,
            raw,
            manifest: parsed,
        };

        let error = validate_manifest(&invalid).expect_err("empty scene value rejected");
        assert!(
            error
                .to_string()
                .contains("scenes[1].characters must not be empty")
        );

        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let mut raw = manifest.raw.clone();
        raw["shots"][0]["visual_prompt"] = Value::String(String::new());
        let parsed = serde_yaml::from_value(raw.clone()).expect("empty shot manifest deserializes");
        let invalid = LoadedManifest {
            path: manifest.path,
            raw,
            manifest: parsed,
        };

        let error = validate_manifest(&invalid).expect_err("empty shot value rejected");
        assert!(
            error
                .to_string()
                .contains("shots[1].visual_prompt must not be empty")
        );
    }

    #[test]
    fn rejects_empty_top_level_scalar_values() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let mut raw = manifest.raw.clone();
        raw["title"] = Value::String(String::new());
        let parsed =
            serde_yaml::from_value(raw.clone()).expect("empty top-level manifest deserializes");
        let invalid = LoadedManifest {
            path: manifest.path,
            raw,
            manifest: parsed,
        };

        let error = validate_manifest(&invalid).expect_err("empty title rejected");
        assert!(error.to_string().contains("title must not be empty"));
    }

    #[test]
    fn rejects_unsupported_manifest_version() {
        let manifest = load_manifest("works/0001-ash-vale-last-road-before-winter/manifest.yaml")
            .expect("manifest loads");
        let mut raw = manifest.raw.clone();
        raw["manifest_version"] = Value::String("reel.manifest.v9.9".to_string());
        let parsed = serde_yaml::from_value(raw.clone()).expect("version manifest deserializes");
        let invalid = LoadedManifest {
            path: manifest.path,
            raw,
            manifest: parsed,
        };

        let error = validate_manifest(&invalid).expect_err("unsupported version rejected");
        assert!(
            error
                .to_string()
                .contains("unsupported manifest_version: reel.manifest.v9.9")
        );
    }

    #[test]
    fn formats_integer_seconds_for_ffmpeg_ratios() {
        assert_eq!(compact_seconds(45.0), "45");
        assert_eq!(compact_seconds(45.25), "45.250");
    }

    #[test]
    fn computes_sha256_for_artifact_files() {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        write!(file, "reel").expect("write temp file");

        let digest = sha256_file(file.path()).expect("file hashes");

        assert_eq!(
            digest,
            "74a61185e36e3edd483e8b96e93cb0406b0238fcdadda0283a5fa4318c1dcf6d"
        );
    }
}
