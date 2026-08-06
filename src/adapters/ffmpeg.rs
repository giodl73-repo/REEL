use super::{AdapterDescriptor, AdapterId, AdapterStatus, RenderOperationKind};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    process::{Command, Stdio},
    sync::OnceLock,
};

static RENDER_ENVIRONMENT: OnceLock<RenderEnvironmentReport> = OnceLock::new();

pub fn descriptor() -> AdapterDescriptor {
    AdapterDescriptor {
        id: AdapterId::Ffmpeg,
        status: AdapterStatus::ImplementedBaseline,
        boundary: "Rust-owned subprocess orchestration around external FFmpeg/ffprobe.",
        dependency_policy: "Requires external FFmpeg/ffprobe at render time; no Rust rewrite or provider SDK.",
        operations: vec![
            RenderOperationKind::Smoke,
            RenderOperationKind::ShotCards,
            RenderOperationKind::ContactSheet,
            RenderOperationKind::ScenePreview,
            RenderOperationKind::ReviewPack,
            RenderOperationKind::AnimaticRender,
        ],
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FfmpegAdapter;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RenderCapabilityCheck {
    pub id: String,
    pub available: bool,
    pub evidence: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RenderEnvironmentReport {
    pub schema: String,
    pub transport: String,
    pub ffmpeg_version: String,
    pub ffprobe_version: String,
    pub checks: Vec<RenderCapabilityCheck>,
    pub passed: bool,
}

impl RenderEnvironmentReport {
    pub fn missing(&self) -> Vec<&str> {
        self.checks
            .iter()
            .filter(|check| !check.available)
            .map(|check| check.id.as_str())
            .collect()
    }
}

impl FfmpegAdapter {
    pub fn render_environment(&self) -> Result<RenderEnvironmentReport> {
        if let Some(report) = RENDER_ENVIRONMENT.get() {
            return Ok(report.clone());
        }
        let ffmpeg_version = self.run_ffmpeg(&["-version".to_string()], &[])?;
        let ffprobe_version = self.run_external("ffprobe", &["-version".to_string()], &[])?;
        let filters =
            self.run_ffmpeg(&["-hide_banner".to_string(), "-filters".to_string()], &[])?;
        let encoders =
            self.run_ffmpeg(&["-hide_banner".to_string(), "-encoders".to_string()], &[])?;
        let perspective_help = self.run_ffmpeg(
            &[
                "-hide_banner".to_string(),
                "-h".to_string(),
                "filter=perspective".to_string(),
            ],
            &[],
        )?;
        let report = render_environment_from_outputs(
            &ffmpeg_version,
            &ffprobe_version,
            &filters,
            &encoders,
            &perspective_help,
        );
        let _ = RENDER_ENVIRONMENT.set(report.clone());
        Ok(report)
    }

    pub fn run_ffmpeg(&self, fixed_args: &[String], runtime_args: &[String]) -> Result<String> {
        self.run_external("ffmpeg", fixed_args, runtime_args)
    }

    pub fn ffprobe_duration(&self, path: &Path) -> Result<String> {
        let stdout = self.run_external(
            "ffprobe",
            &[
                "-v".to_string(),
                "error".to_string(),
                "-show_entries".to_string(),
                "format=duration".to_string(),
                "-of".to_string(),
                "default=nw=1:nk=1".to_string(),
            ],
            &[self.path_argument(path)?],
        )?;

        Ok(stdout.trim().to_string())
    }

    pub fn ffprobe_json(&self, path: &Path) -> Result<String> {
        self.run_external(
            "ffprobe",
            &[
                "-v".to_string(),
                "error".to_string(),
                "-show_entries".to_string(),
                "format=duration:stream=index,codec_type,codec_name,width,height,pix_fmt,r_frame_rate,avg_frame_rate,duration".to_string(),
                "-of".to_string(),
                "json".to_string(),
            ],
            &[self.path_argument(path)?],
        )
    }

    pub fn path_argument(&self, path: &Path) -> Result<String> {
        if cfg!(windows) {
            Ok(path_for_wsl(path))
        } else {
            Ok(path.to_string_lossy().to_string())
        }
    }

    pub fn path_for_concat(&self, path: &Path) -> Result<String> {
        Ok(self.path_argument(path)?.replace('\'', "'\\''"))
    }

    fn run_external(
        &self,
        program: &str,
        fixed_args: &[String],
        runtime_args: &[String],
    ) -> Result<String> {
        let output = if cfg!(windows) {
            let cwd = std::env::current_dir().context("failed to read current directory")?;
            let mut command = format!(
                "cd {} && {}",
                shell_quote(&path_for_wsl(&cwd)),
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
}

fn first_line(value: &str) -> String {
    value.lines().next().unwrap_or("unknown").trim().to_string()
}

fn listing_has(value: &str, required: &str) -> bool {
    value
        .lines()
        .any(|line| line.split_whitespace().any(|token| token == required))
}

fn render_environment_from_outputs(
    ffmpeg_version: &str,
    ffprobe_version: &str,
    filters: &str,
    encoders: &str,
    perspective_help: &str,
) -> RenderEnvironmentReport {
    let mut checks = ["drawtext", "subtitles", "perspective", "framerate", "xfade"]
        .into_iter()
        .map(|id| RenderCapabilityCheck {
            id: format!("filter:{id}"),
            available: listing_has(filters, id),
            evidence: format!("FFmpeg filter `{id}`"),
        })
        .collect::<Vec<_>>();
    checks.push(RenderCapabilityCheck {
        id: "encoder:libx264".to_string(),
        available: listing_has(encoders, "libx264"),
        evidence: "FFmpeg H.264 encoder `libx264`".to_string(),
    });
    checks.push(RenderCapabilityCheck {
        id: "perspective:cubic".to_string(),
        available: perspective_help.contains("cubic"),
        evidence: "perspective filter cubic interpolation option".to_string(),
    });
    let passed = checks.iter().all(|check| check.available);
    RenderEnvironmentReport {
        schema: "reel.render-environment.v0.1".to_string(),
        transport: if cfg!(windows) { "wsl" } else { "native" }.to_string(),
        ffmpeg_version: first_line(ffmpeg_version),
        ffprobe_version: first_line(ffprobe_version),
        checks,
        passed,
    }
}

fn path_for_wsl(path: &Path) -> String {
    let mut text = path.to_string_lossy().replace('\\', "/");
    if let Some(stripped) = text.strip_prefix("//?/") {
        text = stripped.to_string();
    }
    let bytes = text.as_bytes();

    if bytes.len() >= 3 && bytes[1] == b':' && bytes[2] == b'/' {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        let rest = &text[3..];
        return format!("/mnt/{drive}/{rest}");
    }

    text
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn converts_windows_paths_for_wsl_invocation() {
        assert_eq!(
            path_for_wsl(Path::new("C:\\src\\TRACKER\\file name.txt")),
            "/mnt/c/src/TRACKER/file name.txt"
        );
        assert_eq!(
            path_for_wsl(Path::new(r"\\?\C:\src\TRACKER\file name.txt")),
            "/mnt/c/src/TRACKER/file name.txt"
        );
    }

    #[test]
    fn escapes_concat_paths_for_ffmpeg_concat_files() {
        let adapter = FfmpegAdapter;
        let path = Path::new("renders\\shot-cards\\builder's-cut.mp4");

        let escaped = adapter.path_for_concat(path).expect("path converts");

        assert!(escaped.contains("'\\''"));
    }

    #[test]
    fn reports_missing_render_capabilities_without_false_substring_matches() {
        let report = render_environment_from_outputs(
            "ffmpeg version 8.1\nconfiguration",
            "ffprobe version 8.1\nconfiguration",
            " T.C drawtext V->V\n ... perspective V->V\n ... framerate V->V\n ... xfade VV->V",
            " V....D libx264 H.264",
            "perspective AVOptions:\n  interpolation cubic",
        );

        assert!(!report.passed);
        assert_eq!(report.ffmpeg_version, "ffmpeg version 8.1");
        assert_eq!(report.ffprobe_version, "ffprobe version 8.1");
        assert_eq!(report.missing(), vec!["filter:subtitles"]);
    }

    #[test]
    fn accepts_complete_render_environment() {
        let report = render_environment_from_outputs(
            "ffmpeg version 8.1",
            "ffprobe version 8.1",
            " drawtext\n subtitles\n perspective\n framerate\n xfade",
            " libx264",
            "interpolation cubic",
        );

        assert!(report.passed);
        assert!(report.missing().is_empty());
        assert_eq!(report.schema, "reel.render-environment.v0.1");
    }
}
