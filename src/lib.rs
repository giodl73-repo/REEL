use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_yaml::{Mapping, Value};
use tempfile::tempdir;

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

#[derive(Debug, PartialEq)]
pub struct ExportPlan {
    pub id: String,
    pub aspect_ratio: String,
    pub width: u32,
    pub height: u32,
    pub duration_seconds: f64,
    pub duration_scale: f64,
    pub filename: String,
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

    fs::write(&report_path, markdown)
        .with_context(|| format!("failed to write {}", report_path.display()))?;
    Ok(report_path)
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
    markdown.push_str("# REEL review-pack index\n\n");
    markdown.push_str(&format!("- Works root: `{}`\n", root.display()));
    markdown.push_str(&format!("- Generated unix: `{}`\n\n", unix_now()?));
    markdown.push_str("| Work manifest | Review pack |\n");
    markdown.push_str("|---|---|\n");

    for manifest in manifests {
        let report = render_review_pack(&manifest)?;
        markdown.push_str(&format!(
            "| `{}` | `{}` |\n",
            manifest.display(),
            report.display()
        ));
    }

    fs::write(&index_path, markdown)
        .with_context(|| format!("failed to write {}", index_path.display()))?;
    Ok(index_path)
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
        path_argument(&card_file)?
    );

    run_external(
        "ffmpeg",
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
        &[path_argument(&out_file)?],
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

    run_external(
        "ffmpeg",
        &[
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-y".to_string(),
            "-i".to_string(),
            path_argument(&video_file)?,
            "-vf".to_string(),
            filter,
            "-frames:v".to_string(),
            "1".to_string(),
        ],
        &[path_argument(&out_file)?],
    )?;

    Ok(out_file)
}

fn render_shot_cards_for_export(loaded: &LoadedManifest, export: &ExportPlan) -> Result<PathBuf> {
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
            path_argument(&card_file)?
        );

        run_external(
            "ffmpeg",
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
            &[path_argument(&clip_file)?],
        )?;

        concat.push_str(&format!("file '{}'\n", path_for_concat(&clip_file)?));
    }

    fs::write(&concat_file, concat)
        .with_context(|| format!("failed to write {}", concat_file.display()))?;

    run_external(
        "ffmpeg",
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
            path_argument(&concat_file)?,
            "-fflags".to_string(),
            "+genpts".to_string(),
            "-c:v".to_string(),
            "libx264".to_string(),
            "-pix_fmt".to_string(),
            "yuv420p".to_string(),
            "-avoid_negative_ts".to_string(),
            "make_zero".to_string(),
        ],
        &[path_argument(&out_file)?],
    )?;

    Ok(out_file)
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

fn path_for_concat(path: &Path) -> Result<String> {
    Ok(path_argument(path)?.replace('\'', "'\\''"))
}

fn ffprobe_duration(path: &Path) -> Result<String> {
    let stdout = run_external(
        "ffprobe",
        &[
            "-v".to_string(),
            "error".to_string(),
            "-show_entries".to_string(),
            "format=duration".to_string(),
            "-of".to_string(),
            "default=nw=1:nk=1".to_string(),
        ],
        &[path_argument(path)?],
    )?;

    Ok(stdout.trim().to_string())
}

fn run_external(program: &str, fixed_args: &[String], runtime_args: &[String]) -> Result<String> {
    let output = if cfg!(windows) {
        let cwd = std::env::current_dir().context("failed to read current directory")?;
        let mut command = format!(
            "cd {} && {}",
            shell_quote(&path_for_wsl(&cwd)?),
            shell_quote(program)
        );
        for arg in fixed_args.iter().chain(runtime_args.iter()) {
            command.push(' ');
            command.push_str(&shell_quote(arg));
        }

        Command::new("wsl")
            .args(["--", "bash", "-lc", &command])
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("failed to run {program} through WSL"))?
    } else {
        Command::new(program)
            .args(fixed_args)
            .args(runtime_args)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("failed to run {program}"))?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{program} failed: {stderr}");
    }

    String::from_utf8(output.stdout).with_context(|| format!("{program} wrote non-utf8 output"))
}

fn path_argument(path: &Path) -> Result<String> {
    if cfg!(windows) {
        path_for_wsl(path)
    } else {
        Ok(path.to_string_lossy().to_string())
    }
}

fn path_for_wsl(path: &Path) -> Result<String> {
    let text = path.to_string_lossy().replace('\\', "/");
    let bytes = text.as_bytes();

    if bytes.len() >= 3 && bytes[1] == b':' && bytes[2] == b'/' {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        let rest = &text[3..];
        return Ok(format!("/mnt/{drive}/{rest}"));
    }

    Ok(text)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
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
    fn formats_integer_seconds_for_ffmpeg_ratios() {
        assert_eq!(compact_seconds(45.0), "45");
        assert_eq!(compact_seconds(45.25), "45.250");
    }
}
