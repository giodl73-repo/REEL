use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use clap::ValueEnum;
use serde::Serialize;
use tempfile::Builder;

use super::ffmpeg::FfmpegAdapter;
use crate::production::{self, TimingStatus};

pub const NEAR_STATIONARY_LUMA_THRESHOLD: f64 = 0.001;
pub const MAX_NEAR_STATIONARY_FRACTION: f64 = 0.10;
const MAX_RENDER_PIXELS: u64 = 1920 * 1080;
const MAX_RENDER_FPS: u32 = 60;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum MotionQuality {
    #[default]
    Smooth,
    Legacy,
}

impl MotionQuality {
    fn as_str(self) -> &'static str {
        match self {
            Self::Smooth => "smooth",
            Self::Legacy => "legacy",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum MotionCurve {
    #[default]
    EaseInOut,
    Linear,
}

impl MotionCurve {
    fn as_str(self) -> &'static str {
        match self {
            Self::EaseInOut => "ease-in-out",
            Self::Linear => "linear",
        }
    }
}

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
    pub motion_quality: MotionQuality,
    pub motion_curve: MotionCurve,
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
    pub tool_version: String,
    pub ffmpeg_version: String,
    pub motion: MotionLineage,
    pub dry_run: bool,
    pub silent: bool,
    pub command_arguments: Vec<String>,
    pub inputs: Vec<AnimaticInput>,
    pub output_sha256: Option<String>,
    pub output_bytes: Option<u64>,
    pub output_duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MotionLineage {
    pub backend: String,
    pub backend_version: String,
    pub quality: String,
    pub interpolation: String,
    pub curve: String,
    pub sampling_strategy: String,
    pub working_width: u32,
    pub working_height: u32,
    pub fps: u32,
    pub estimated_peak_memory_mib: u64,
    pub maximum_render_pixels: u64,
    pub maximum_render_fps: u32,
    pub quality_override: Option<String>,
    pub shots: Vec<ShotMotionLineage>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShotMotionLineage {
    pub shot_id: String,
    pub treatment: String,
    pub frames: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct MotionCadenceReport {
    pub schema: String,
    pub input: String,
    pub input_sha256: String,
    pub analyzer: String,
    pub analyzer_version: String,
    pub metric: String,
    pub near_stationary_luma_threshold: f64,
    pub maximum_near_stationary_fraction: f64,
    pub frame_transitions: usize,
    pub near_stationary_transitions: usize,
    pub near_stationary_fraction: f64,
    pub passed: bool,
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
    if options.width % 2 != 0 || options.height % 2 != 0 {
        bail!("width and height must be even for yuv420p delivery");
    }
    let pixels = u64::from(options.width) * u64::from(options.height);
    if pixels > MAX_RENDER_PIXELS || options.fps > MAX_RENDER_FPS {
        bail!(
            "requested motion quality is infeasible: {}x{}@{} exceeds the bounded limit of {} pixels at {} fps",
            options.width,
            options.height,
            options.fps,
            MAX_RENDER_PIXELS,
            MAX_RENDER_FPS
        );
    }
    if options.silent == options.audio.is_some() {
        bail!("provide exactly one of audio or silent rendering");
    }
    if !(0.0..=5.0).contains(&options.transition_seconds) {
        bail!("transition-seconds must be within 0..5");
    }
    let artifact_path = options.output.with_extension("artifacts.json");
    if options.output.exists() || artifact_path.exists() {
        bail!(
            "refusing to overwrite existing render or artifact report; choose a new output path: {}",
            options.output.display()
        );
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
                options.height,
                options.motion_quality,
                options.motion_curve,
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
    let output_parent = options.output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_parent)?;
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
        let version = adapter
            .run_ffmpeg(&["-version".to_string()], &[])?
            .lines()
            .next()
            .unwrap_or("unknown")
            .to_string();
        if options.motion_quality == MotionQuality::Smooth {
            let help = adapter.run_ffmpeg(
                &[
                    "-hide_banner".to_string(),
                    "-h".to_string(),
                    "filter=perspective".to_string(),
                ],
                &[],
            )?;
            if !help.contains("perspective") || !help.contains("cubic") {
                bail!("smooth motion requires FFmpeg's cubic perspective filter");
            }
        }
        version
    };
    let expected_duration_ms = durations
        .iter()
        .map(|value| (value * 1000.0).round() as u64)
        .sum();
    let mut rendered_temp = None;
    let (output_sha256, output_bytes, output_duration_ms) = if options.dry_run {
        (None, None, None)
    } else {
        let temp = Builder::new()
            .prefix(".reel-render-")
            .suffix(".mp4")
            .tempfile_in(output_parent)?
            .into_temp_path();
        let mut render_args = args.clone();
        let temp_argument = adapter.path_argument(&temp)?;
        *render_args.last_mut().expect("output argument exists") = temp_argument;
        adapter.run_ffmpeg(&render_args, &[])?;
        let actual_seconds = adapter
            .ffprobe_duration(&temp)?
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
        let measured = (
            Some(production::sha256_path(&temp)?),
            Some(fs::metadata(&temp)?.len()),
            Some(actual_ms),
        );
        rendered_temp = Some(temp);
        measured
    };
    let effective_curve = if options.motion_quality == MotionQuality::Legacy {
        "legacy-linear"
    } else {
        options.motion_curve.as_str()
    };
    let motion = MotionLineage {
        backend: match options.motion_quality {
            MotionQuality::Smooth => "ffmpeg-perspective",
            MotionQuality::Legacy => "ffmpeg-zoompan",
        }
        .to_string(),
        backend_version: ffmpeg_version.clone(),
        quality: options.motion_quality.as_str().to_string(),
        interpolation: match options.motion_quality {
            MotionQuality::Smooth => "cubic",
            MotionQuality::Legacy => "zoompan-default",
        }
        .to_string(),
        curve: effective_curve.to_string(),
        sampling_strategy: match options.motion_quality {
            MotionQuality::Smooth => "frame-evaluated fractional source rectangle",
            MotionQuality::Legacy => "delivery-resolution integer crop coordinates",
        }
        .to_string(),
        working_width: options.width,
        working_height: options.height,
        fps: options.fps,
        estimated_peak_memory_mib: (pixels * 7).div_ceil(10_000),
        maximum_render_pixels: MAX_RENDER_PIXELS,
        maximum_render_fps: MAX_RENDER_FPS,
        quality_override: (options.motion_quality == MotionQuality::Legacy)
            .then(|| "legacy deterministic reproduction".to_string()),
        shots: loaded
            .manifest
            .shots
            .iter()
            .zip(durations.iter())
            .map(|(shot, duration)| ShotMotionLineage {
                shot_id: shot.id.clone(),
                treatment: if shot.motion.is_empty() {
                    "push".to_string()
                } else {
                    shot.motion.clone()
                },
                frames: (duration * f64::from(options.fps)).round().max(1.0) as u64,
            })
            .collect(),
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
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        ffmpeg_version,
        motion,
        dry_run: options.dry_run,
        silent: options.silent,
        command_arguments: args,
        inputs,
        output_sha256,
        output_bytes,
        output_duration_ms,
    };
    let mut report_temp = Builder::new()
        .prefix(".reel-artifacts-")
        .tempfile_in(output_parent)?;
    report_temp.write_all(&serde_json::to_vec_pretty(&report)?)?;
    report_temp.flush()?;
    if let Some(temp) = rendered_temp {
        temp.persist_noclobber(&options.output).with_context(|| {
            format!(
                "failed to publish render atomically: {}",
                options.output.display()
            )
        })?;
        if let Err(error) = report_temp.persist_noclobber(&artifact_path) {
            let _ = fs::remove_file(&options.output);
            return Err(error.error).with_context(|| {
                format!(
                    "failed to publish artifact report atomically: {}",
                    artifact_path.display()
                )
            });
        }
    } else {
        report_temp
            .persist_noclobber(&artifact_path)
            .with_context(|| {
                format!(
                    "failed to publish dry-run artifact report atomically: {}",
                    artifact_path.display()
                )
            })?;
    }
    Ok(report)
}

fn motion_filter(
    motion: &str,
    duration: f64,
    fps: u32,
    width: u32,
    height: u32,
    quality: MotionQuality,
    curve: MotionCurve,
) -> String {
    if quality == MotionQuality::Legacy {
        return legacy_motion_filter(motion, duration, fps, width, height);
    }
    smooth_motion_filter(motion, duration, fps, width, height, curve)
}

fn legacy_motion_filter(motion: &str, duration: f64, fps: u32, width: u32, height: u32) -> String {
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

fn smooth_motion_filter(
    motion: &str,
    duration: f64,
    fps: u32,
    width: u32,
    height: u32,
    curve: MotionCurve,
) -> String {
    let frames = (duration * f64::from(fps)).round().max(1.0) as u64;
    let last_frame = frames.saturating_sub(1).max(1);
    let linear = format!("min(max(on/{last_frame},0),1)");
    let progress = match curve {
        MotionCurve::EaseInOut => format!("0.5-0.5*cos(PI*{linear})"),
        MotionCurve::Linear => linear,
    };
    let base = format!(
        "scale={width}:{height}:force_original_aspect_ratio=increase,crop={width}:{height}"
    );
    if matches!(motion, "hold" | "hold-dark") {
        return if motion == "hold-dark" {
            format!("{base},eq=brightness=-0.72:saturation=0.45")
        } else {
            base
        };
    }
    let (zoom, left) = match motion {
        "pull" => (format!("1.04-0.04*({progress})"), None),
        "pan-right" => (
            "1.035".to_string(),
            Some(format!("(W-W/1.035)*({progress})")),
        ),
        "pan-left" => (
            "1.035".to_string(),
            Some(format!("(W-W/1.035)*(1-({progress}))")),
        ),
        _ => (format!("1+0.04*({progress})"), None),
    };
    let left = left.unwrap_or_else(|| format!("(W-W/({zoom}))/2"));
    let top = format!("(H-H/({zoom}))/2");
    let right = format!("({left})+W/({zoom})");
    let bottom = format!("({top})+H/({zoom})");
    format!(
        "{base},perspective=x0='{left}':y0='{top}':x1='{right}':y1='{top}':x2='{left}':y2='{bottom}':x3='{right}':y3='{bottom}':interpolation=cubic:sense=source:eval=frame"
    )
}

pub fn analyze_motion(video: impl AsRef<Path>) -> Result<MotionCadenceReport> {
    let video = video.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to resolve motion-analysis input {}",
            video.as_ref().display()
        )
    })?;
    let adapter = FfmpegAdapter;
    let version = adapter
        .run_ffmpeg(&["-version".to_string()], &[])?
        .lines()
        .next()
        .unwrap_or("unknown")
        .to_string();
    let output = adapter.run_ffmpeg(
        &[
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-i".to_string(),
            adapter.path_argument(&video)?,
            "-vf".to_string(),
            "tblend=all_mode=difference,signalstats,metadata=print:file=-".to_string(),
            "-f".to_string(),
            "null".to_string(),
            "-".to_string(),
        ],
        &[],
    )?;
    let values = output
        .lines()
        .filter_map(|line| line.strip_prefix("lavfi.signalstats.YAVG="))
        .map(|value| value.parse::<f64>().context("invalid FFmpeg YAVG metric"))
        .collect::<Result<Vec<_>>>()?;
    if values.is_empty() {
        bail!("motion analysis found no frame transitions");
    }
    let near_stationary_transitions = values
        .iter()
        .filter(|value| **value < NEAR_STATIONARY_LUMA_THRESHOLD)
        .count();
    let near_stationary_fraction = near_stationary_transitions as f64 / values.len() as f64;
    Ok(MotionCadenceReport {
        schema: "reel.motion-cadence.v0.1".to_string(),
        input: video.display().to_string(),
        input_sha256: production::sha256_path(&video)?,
        analyzer: "ffmpeg-tblend-signalstats".to_string(),
        analyzer_version: version,
        metric: "adjacent-frame absolute luma difference (YAVG); transition is near-stationary when YAVG < 0.001".to_string(),
        near_stationary_luma_threshold: NEAR_STATIONARY_LUMA_THRESHOLD,
        maximum_near_stationary_fraction: MAX_NEAR_STATIONARY_FRACTION,
        frame_transitions: values.len(),
        near_stationary_transitions,
        near_stationary_fraction,
        passed: near_stationary_fraction <= MAX_NEAR_STATIONARY_FRACTION,
    })
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
    fn smooth_motion_uses_frame_evaluated_cubic_subpixel_sampling() {
        let filter = motion_filter(
            "pan-right",
            10.0,
            24,
            1280,
            720,
            MotionQuality::Smooth,
            MotionCurve::EaseInOut,
        );
        assert!(
            filter.starts_with("scale=1280:720:force_original_aspect_ratio=increase,crop=1280:720")
        );
        assert!(filter.contains("perspective="));
        assert!(filter.contains("interpolation=cubic"));
        assert!(filter.contains("eval=frame"));
        assert!(filter.contains("cos(PI*"));
        assert!(!filter.contains("zoompan"));
    }

    #[test]
    fn legacy_motion_preserves_zoompan_command_shape() {
        let filter = motion_filter(
            "pan-right",
            10.0,
            24,
            1280,
            720,
            MotionQuality::Legacy,
            MotionCurve::EaseInOut,
        );
        assert!(filter.contains("zoompan"));
        assert!(!filter.contains("perspective"));
    }

    #[test]
    fn smooth_holds_have_no_transform_drift() {
        let hold = motion_filter(
            "hold",
            25.0,
            24,
            1280,
            720,
            MotionQuality::Smooth,
            MotionCurve::EaseInOut,
        );
        let dark = motion_filter(
            "hold-dark",
            25.0,
            24,
            1280,
            720,
            MotionQuality::Smooth,
            MotionCurve::EaseInOut,
        );
        assert!(!hold.contains("perspective"));
        assert!(!dark.contains("perspective"));
        assert!(dark.contains("brightness=-0.72"));
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
