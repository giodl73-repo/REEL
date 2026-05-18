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
    platforms: Vec<Platform>,
    scenes: Vec<Scene>,
    shots: Vec<Shot>,
    exports: Vec<Export>,
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
}

#[derive(Debug, Deserialize)]
struct Export {
    id: String,
    filename: String,
    aspect_ratio: String,
    duration_seconds: f64,
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
        let video = run_script_with_values(
            "scripts/render-shot-cards.sh",
            &[path_argument(manifest)?, export.id.clone()],
        )?;
        let sheet = render_contact_sheet_for_export(manifest, &loaded, export)?;
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

pub fn render_contact_sheet(manifest: impl AsRef<Path>, platform: &str) -> Result<PathBuf> {
    let manifest = manifest.as_ref();
    let loaded = load_manifest(manifest)?;
    let report = validate_manifest(&loaded)?;
    let export = report
        .exports
        .iter()
        .find(|export| export.id == platform)
        .ok_or_else(|| anyhow!("unknown platform in manifest: {platform}"))?;

    render_contact_sheet_for_export(manifest, &loaded, export)
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

fn render_contact_sheet_for_export(
    manifest_path: &Path,
    loaded: &LoadedManifest,
    export: &ExportPlan,
) -> Result<PathBuf> {
    let video_file = PathBuf::from(format!(
        "renders/shot-cards/{}-{}-shot-cards.mp4",
        loaded.manifest.work, export.id
    ));
    if !video_file.is_file() {
        run_script_with_values(
            "scripts/render-shot-cards.sh",
            &[path_argument(manifest_path)?, export.id.clone()],
        )?;
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

fn run_script_with_values(script: &str, args: &[String]) -> Result<PathBuf> {
    let stdout = run_external("bash", &[script.to_string()], args)
        .with_context(|| format!("failed to run {script}"))?;
    let path = stdout
        .lines()
        .last()
        .ok_or_else(|| anyhow!("{script} did not print an output path"))?;

    Ok(PathBuf::from(path))
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
    fn formats_integer_seconds_for_ffmpeg_ratios() {
        assert_eq!(compact_seconds(45.0), "45");
        assert_eq!(compact_seconds(45.25), "45.250");
    }
}
