use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
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
    validate_manifest(&loaded)?;
    run_script("scripts/render-review-pack.sh", &[manifest])
}

pub fn render_all_review_packs(root: impl AsRef<Path>) -> Result<PathBuf> {
    run_script("scripts/render-all-review-packs.sh", &[root.as_ref()])
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

fn run_script(script: &str, args: &[&Path]) -> Result<PathBuf> {
    let output = if cfg!(windows) {
        let cwd = std::env::current_dir().context("failed to read current directory")?;
        let mut command = format!(
            "cd {} && bash {}",
            shell_quote(&path_for_wsl(&cwd)?),
            shell_quote(script)
        );
        for arg in args {
            command.push(' ');
            command.push_str(&shell_quote(&path_for_wsl(arg)?));
        }

        Command::new("wsl")
            .args(["--", "bash", "-lc", &command])
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("failed to run {script} through WSL"))?
    } else {
        Command::new("bash")
            .arg(script)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("failed to run {script}"))?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{script} failed: {stderr}");
    }

    let stdout = String::from_utf8(output.stdout)
        .with_context(|| format!("{script} wrote non-utf8 output"))?;
    let path = stdout
        .lines()
        .last()
        .ok_or_else(|| anyhow!("{script} did not print an output path"))?;

    Ok(PathBuf::from(path))
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
}
