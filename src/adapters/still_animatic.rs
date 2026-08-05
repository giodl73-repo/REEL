use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;

use super::ffmpeg::FfmpegAdapter;
use crate::production::{self, TimingStatus};

#[derive(Clone, Debug)]
pub struct AnimaticRenderOptions {
    pub manifest: PathBuf,
    pub asset_root: PathBuf,
    pub audio: Option<PathBuf>,
    pub silent: bool,
    pub captions: PathBuf,
    pub output: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub transition_seconds: f64,
    pub disclosure: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnimaticRenderReport {
    pub schema: String,
    pub work: String,
    pub timing_status: String,
    pub output: String,
    pub artifact_manifest: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_ms: u64,
    pub ffmpeg_version: String,
    pub dry_run: bool,
    pub silent: bool,
    pub command_arguments: Vec<String>,
    pub inputs: Vec<AnimaticInput>,
    pub output_sha256: Option<String>,
    pub output_bytes: Option<u64>,
    pub output_duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnimaticInput {
    pub kind: String,
    pub id: String,
    pub path: String,
    pub sha256: String,
}

pub fn variant_output(output: &Path, label: &str) -> PathBuf {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let stem = output
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("animatic");
    let extension = output
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("mp4");
    parent.join(format!("{stem}.{label}.{extension}"))
}

pub fn render(options: &AnimaticRenderOptions) -> Result<AnimaticRenderReport> {
    let loaded = production::require_preview_ready(&options.manifest)?;
    if loaded.manifest.timing_status == TimingStatus::Untimed {
        bail!("timing not conformed: animatic rendering is gated");
    }
    if options.fps == 0 || options.width == 0 || options.height == 0 {
        bail!("width, height, and fps must be positive");
    }
    if options.silent == options.audio.is_some() {
        bail!("provide exactly one of audio or silent rendering");
    }
    if !(0.0..=5.0).contains(&options.transition_seconds) {
        bail!("transition-seconds must be within 0..5");
    }
    let asset_root = options.asset_root.canonicalize().with_context(|| {
        format!(
            "failed to resolve asset root {}",
            options.asset_root.display()
        )
    })?;
    let adapter = FfmpegAdapter;
    let mut inputs = vec![AnimaticInput {
        kind: "manifest".to_string(),
        id: loaded.manifest.work.clone(),
        path: options.manifest.display().to_string(),
        sha256: production::sha256_path(&options.manifest)?,
    }];
    let mut args = vec![
        "-y".to_string(),
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "warning".to_string(),
    ];
    let mut durations = Vec::new();
    for (index, shot) in loaded.manifest.shots.iter().enumerate() {
        let duration = shot
            .duration_seconds
            .ok_or_else(|| anyhow!("timing not conformed: shot {} has no duration", shot.id))?;
        let visual = shot
            .visual_asset
            .as_deref()
            .ok_or_else(|| anyhow!("shot {} has no visual_asset", shot.id))?;
        let candidate = if Path::new(visual).is_absolute() {
            PathBuf::from(visual)
        } else {
            asset_root.join(visual)
        };
        let resolved = candidate.canonicalize().with_context(|| {
            format!(
                "missing visual asset for shot {}: {}",
                shot.id,
                candidate.display()
            )
        })?;
        if !Path::new(visual).is_absolute() && !resolved.starts_with(&asset_root) {
            bail!("shot {} visual_asset escapes asset root", shot.id);
        }
        inputs.push(AnimaticInput {
            kind: "visual".to_string(),
            id: shot.id.clone(),
            path: resolved.display().to_string(),
            sha256: production::sha256_path(&resolved)?,
        });
        let tail = if index + 1 < loaded.manifest.shots.len() {
            options.transition_seconds
        } else {
            0.0
        };
        args.extend([
            "-loop".to_string(),
            "1".to_string(),
            "-framerate".to_string(),
            options.fps.to_string(),
            "-t".to_string(),
            format!("{:.3}", duration + tail),
            "-i".to_string(),
            adapter.path_argument(&resolved)?,
        ]);
        durations.push(duration);
    }
    let captions = options
        .captions
        .canonicalize()
        .with_context(|| format!("failed to resolve captions {}", options.captions.display()))?;
    let audio = options
        .audio
        .as_ref()
        .map(|path| {
            path.canonicalize()
                .with_context(|| format!("failed to resolve audio {}", path.display()))
        })
        .transpose()?;
    if let Some(audio) = &audio {
        inputs.push(AnimaticInput {
            kind: "audio".to_string(),
            id: "master-audio".to_string(),
            path: audio.display().to_string(),
            sha256: production::sha256_path(audio)?,
        });
    }
    inputs.push(AnimaticInput {
        kind: "captions".to_string(),
        id: "captions".to_string(),
        path: captions.display().to_string(),
        sha256: production::sha256_path(&captions)?,
    });
    if let Some(audio) = &audio {
        args.extend(["-i".to_string(), adapter.path_argument(audio)?]);
    }
    let mut filters = Vec::new();
    for (index, shot) in loaded.manifest.shots.iter().enumerate() {
        let duration = durations[index];
        let tail = if index + 1 < durations.len() {
            options.transition_seconds
        } else {
            0.0
        };
        filters.push(format!(
            "[{index}:v]{},trim=duration={:.3},setpts=PTS-STARTPTS[v{index}]",
            motion_filter(
                &shot.motion,
                duration + tail,
                options.fps,
                options.width,
                options.height
            ),
            duration + tail
        ));
    }
    let mut cumulative = durations[0];
    let mut previous = "v0".to_string();
    for (index, duration) in durations.iter().enumerate().skip(1) {
        let output = format!("x{index}");
        filters.push(format!(
            "[{previous}][v{index}]xfade=transition=fade:duration={:.3}:offset={cumulative:.3}[{output}]",
            options.transition_seconds
        ));
        previous = output;
        cumulative += duration;
    }
    let caption_path = adapter
        .path_argument(&captions)?
        .replace('\\', "/")
        .replace(':', "\\:")
        .replace('\'', "\\'");
    let disclosure = escape_drawtext(&options.disclosure);
    let caption_font_size = if options.height > options.width {
        20
    } else {
        18
    };
    let caption_margin = if options.height > options.width {
        32
    } else {
        16
    };
    let disclosure_font_size = if options.height > options.width {
        18
    } else {
        14
    };
    filters.push(format!(
        "[{previous}]subtitles=filename='{caption_path}':force_style='FontSize={caption_font_size},MarginV={caption_margin},Outline=2,Shadow=0,Alignment=2',drawtext=text='{disclosure}':fontcolor=white@0.68:fontsize={disclosure_font_size}:x=w-tw-24:y=20:box=1:boxcolor=black@0.3:boxborderw=5,format=yuv420p[finalv]"
    ));
    if let Some(parent) = options.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let output_argument = adapter.path_argument(&options.output)?;
    args.extend([
        "-filter_complex".to_string(),
        filters.join(";"),
        "-map".to_string(),
        "[finalv]".to_string(),
        "-c:v".to_string(),
        "libx264".to_string(),
        "-preset".to_string(),
        "slow".to_string(),
        "-crf".to_string(),
        "18".to_string(),
        "-r".to_string(),
        options.fps.to_string(),
        "-movflags".to_string(),
        "+faststart".to_string(),
        "-metadata".to_string(),
        format!("title={}", loaded.manifest.title),
        "-metadata".to_string(),
        format!("comment={}", options.disclosure),
    ]);
    if audio.is_some() {
        args.extend([
            "-map".to_string(),
            format!("{}:a:0", loaded.manifest.shots.len()),
            "-c:a".to_string(),
            "aac".to_string(),
            "-b:a".to_string(),
            "128k".to_string(),
            "-shortest".to_string(),
        ]);
    }
    args.push(output_argument);
    let ffmpeg_version = if options.dry_run {
        "not-probed-dry-run".to_string()
    } else {
        adapter
            .run_ffmpeg(&["-version".to_string()], &[])?
            .lines()
            .next()
            .unwrap_or("unknown")
            .to_string()
    };
    if !options.dry_run {
        adapter.run_ffmpeg(&args, &[])?;
    }
    let artifact_path = options.output.with_extension("artifacts.json");
    let expected_duration_ms = durations
        .iter()
        .map(|value| (value * 1000.0).round() as u64)
        .sum();
    let (output_sha256, output_bytes, output_duration_ms) = if options.dry_run {
        (None, None, None)
    } else {
        let actual_seconds = adapter
            .ffprobe_duration(&options.output)?
            .parse::<f64>()
            .context("ffprobe returned an invalid animatic duration")?;
        let actual_ms = (actual_seconds * 1000.0).round() as u64;
        let frame_ms = (1000.0 / options.fps as f64).ceil() as u64;
        if actual_ms.abs_diff(expected_duration_ms) > frame_ms {
            bail!(
                "rendered duration {}ms differs from conformed timeline {}ms by more than one frame",
                actual_ms,
                expected_duration_ms
            );
        }
        (
            Some(production::sha256_path(&options.output)?),
            Some(fs::metadata(&options.output)?.len()),
            Some(actual_ms),
        )
    };
    let report = AnimaticRenderReport {
        schema: "reel.animatic-artifacts.v0.1".to_string(),
        work: loaded.manifest.work,
        timing_status: loaded.manifest.timing_status.as_str().to_string(),
        output: options.output.display().to_string(),
        artifact_manifest: artifact_path.display().to_string(),
        width: options.width,
        height: options.height,
        fps: options.fps,
        duration_ms: expected_duration_ms,
        ffmpeg_version,
        dry_run: options.dry_run,
        silent: options.silent,
        command_arguments: args,
        inputs,
        output_sha256,
        output_bytes,
        output_duration_ms,
    };
    fs::write(&artifact_path, serde_json::to_vec_pretty(&report)?)?;
    Ok(report)
}

fn motion_filter(motion: &str, duration: f64, fps: u32, width: u32, height: u32) -> String {
    let frames = (duration * fps as f64).round().max(1.0) as u64;
    let (zoom, x, y) = match motion {
        "pull" => (
            format!("if(eq(on,0),1.04,max(1.0,zoom-0.04/{frames}))"),
            "iw/2-(iw/zoom/2)".to_string(),
            "ih/2-(ih/zoom/2)".to_string(),
        ),
        "pan-right" => (
            "1.035".to_string(),
            format!("(iw-iw/zoom)*on/{frames}"),
            "ih/2-(ih/zoom/2)".to_string(),
        ),
        "pan-left" => (
            "1.035".to_string(),
            format!("(iw-iw/zoom)*(1-on/{frames})"),
            "ih/2-(ih/zoom/2)".to_string(),
        ),
        "hold" | "hold-dark" => ("1.0".to_string(), "0".to_string(), "0".to_string()),
        _ => (
            format!("min(1.04,1.0+0.04*on/{frames})"),
            "iw/2-(iw/zoom/2)".to_string(),
            "ih/2-(ih/zoom/2)".to_string(),
        ),
    };
    let mut filter = format!(
        "scale={width}:{height}:force_original_aspect_ratio=increase,crop={width}:{height},zoompan=z='{zoom}':x='{x}':y='{y}':d=1:s={width}x{height}:fps={fps}"
    );
    if motion == "hold-dark" {
        filter.push_str(",eq=brightness=-0.72:saturation=0.45");
    }
    filter
}

fn escape_drawtext(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(':', "\\:")
        .replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_filters_scale_before_zoom_to_avoid_blank_canvas() {
        let filter = motion_filter("pan-right", 10.0, 24, 1280, 720);
        assert!(
            filter.starts_with("scale=1280:720:force_original_aspect_ratio=increase,crop=1280:720")
        );
        assert!(filter.contains("zoompan"));
    }

    #[test]
    fn disclosure_is_escaped_for_drawtext() {
        assert_eq!(
            escape_drawtext("PRIVATE: author's"),
            "PRIVATE\\: author\\'s"
        );
    }

    #[test]
    fn derives_named_ab_variant_paths() {
        assert_eq!(
            variant_output(Path::new("renders/review.mp4"), "narration-only"),
            Path::new("renders/review.narration-only.mp4")
        );
    }
}
