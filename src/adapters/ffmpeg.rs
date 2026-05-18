use super::{AdapterDescriptor, AdapterId, AdapterStatus, RenderOperationKind};
use anyhow::{Context, Result, bail};
use std::{
    path::Path,
    process::{Command, Stdio},
};

pub fn descriptor() -> AdapterDescriptor {
    AdapterDescriptor {
        id: AdapterId::Ffmpeg,
        status: AdapterStatus::ImplementedBaseline,
        boundary: "Rust-owned subprocess orchestration around external FFmpeg/ffprobe.",
        operations: vec![
            RenderOperationKind::Smoke,
            RenderOperationKind::ShotCards,
            RenderOperationKind::ContactSheet,
            RenderOperationKind::ReviewPack,
        ],
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FfmpegAdapter;

impl FfmpegAdapter {
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

fn path_for_wsl(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
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
    }

    #[test]
    fn escapes_concat_paths_for_ffmpeg_concat_files() {
        let adapter = FfmpegAdapter;
        let path = Path::new("renders\\shot-cards\\builder's-cut.mp4");

        let escaped = adapter.path_for_concat(path).expect("path converts");

        assert!(escaped.contains("'\\''"));
    }
}
