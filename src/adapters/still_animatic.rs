use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use tempfile::Builder;

use super::ffmpeg::FfmpegAdapter;
use crate::production::{self, TimingStatus};

pub const NEAR_STATIONARY_LUMA_THRESHOLD: f64 = 0.001;
pub const MAX_NEAR_STATIONARY_FRACTION: f64 = 0.10;
pub const MIN_HOLD_STATIONARY_FRACTION: f64 = 0.85;
const MAX_RENDER_PIXELS: u64 = 1920 * 1080;
const MAX_RENDER_FPS: u32 = 60;
const MAX_ESTIMATED_PEAK_MEMORY_MIB: u64 = 2048;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
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
    #[serde(default)]
    pub perspective_filter_instances: usize,
    #[serde(default)]
    pub maximum_estimated_peak_memory_mib: u64,
    pub maximum_render_pixels: u64,
    pub maximum_render_fps: u32,
    pub quality_override: Option<String>,
    #[serde(default)]
    pub safety: Vec<ShotSafetyReport>,
    pub shots: Vec<ShotMotionLineage>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ShotMotionLineage {
    pub shot_id: String,
    pub treatment: String,
    pub frames: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AnimaticInput {
    pub kind: String,
    pub id: String,
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProtectedRegionSafety {
    pub id: String,
    pub safe: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ShotSafetyReport {
    pub shot_id: String,
    pub treatment: String,
    pub blank_canvas_safe: bool,
    pub focal_point_safe: Option<bool>,
    pub protected_regions: Vec<ProtectedRegionSafety>,
    pub passed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShotCadenceReport {
    pub shot_id: String,
    pub treatment: String,
    pub expectation: String,
    pub start_ms: u64,
    pub duration_ms: u64,
    pub frame_transitions: usize,
    pub near_stationary_transitions: usize,
    pub near_stationary_fraction: f64,
    pub passed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct MotionCheckReport {
    pub schema: String,
    pub manifest: String,
    pub video: String,
    pub work: String,
    pub video_sha256: String,
    pub analyzer_version: String,
    pub near_stationary_luma_threshold: f64,
    pub maximum_moving_near_stationary_fraction: f64,
    pub minimum_hold_stationary_fraction: f64,
    pub shots: Vec<ShotCadenceReport>,
    pub safety: Vec<ShotSafetyReport>,
    pub passed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnimaticCheckReport {
    pub schema: String,
    pub artifact_manifest: String,
    pub output: String,
    pub output_sha256: String,
    pub verified_inputs: usize,
    pub codec: String,
    pub pixel_format: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_ms: u64,
    pub audio_streams: usize,
    pub caption_cues: usize,
    pub passed: bool,
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

#[derive(Clone, Copy)]
struct NormalizedRect {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

fn sampled_rects(motion: &str) -> Vec<NormalizedRect> {
    let full = NormalizedRect {
        left: 0.0,
        top: 0.0,
        right: 1.0,
        bottom: 1.0,
    };
    let zoom = if matches!(motion, "pan-left" | "pan-right") {
        1.035
    } else {
        1.04
    };
    let inset = (1.0 - 1.0 / zoom) / 2.0;
    let centered = NormalizedRect {
        left: inset,
        top: inset,
        right: 1.0 - inset,
        bottom: 1.0 - inset,
    };
    match motion {
        "hold" | "hold-dark" => vec![full],
        "pan-right" => vec![
            NormalizedRect {
                left: 0.0,
                top: inset,
                right: 1.0 / zoom,
                bottom: 1.0 - inset,
            },
            NormalizedRect {
                left: 1.0 - 1.0 / zoom,
                top: inset,
                right: 1.0,
                bottom: 1.0 - inset,
            },
        ],
        "pan-left" => vec![
            NormalizedRect {
                left: 1.0 - 1.0 / zoom,
                top: inset,
                right: 1.0,
                bottom: 1.0 - inset,
            },
            NormalizedRect {
                left: 0.0,
                top: inset,
                right: 1.0 / zoom,
                bottom: 1.0 - inset,
            },
        ],
        "pull" => vec![centered, full],
        _ => vec![full, centered],
    }
}

fn contains(rect: NormalizedRect, left: f64, top: f64, right: f64, bottom: f64) -> bool {
    const EPSILON: f64 = 1e-9;
    left + EPSILON >= rect.left
        && top + EPSILON >= rect.top
        && right <= rect.right + EPSILON
        && bottom <= rect.bottom + EPSILON
}

fn safety_report(shot: &production::Shot) -> ShotSafetyReport {
    let treatment = if shot.motion.is_empty() {
        "push"
    } else {
        &shot.motion
    };
    let rects = sampled_rects(treatment);
    let blank_canvas_safe = rects
        .iter()
        .all(|rect| rect.left >= 0.0 && rect.top >= 0.0 && rect.right <= 1.0 && rect.bottom <= 1.0);
    let focal_point_safe = shot.focal_point.as_ref().map(|point| {
        rects
            .iter()
            .all(|rect| contains(*rect, point.x, point.y, point.x, point.y))
    });
    let protected_regions = shot
        .protected_regions
        .iter()
        .map(|region| ProtectedRegionSafety {
            id: region.id.clone(),
            safe: rects.iter().all(|rect| {
                contains(
                    *rect,
                    region.x,
                    region.y,
                    region.x + region.width,
                    region.y + region.height,
                )
            }),
        })
        .collect::<Vec<_>>();
    let passed = blank_canvas_safe
        && focal_point_safe.unwrap_or(true)
        && protected_regions.iter().all(|region| region.safe);
    ShotSafetyReport {
        shot_id: shot.id.clone(),
        treatment: treatment.to_string(),
        blank_canvas_safe,
        focal_point_safe,
        protected_regions,
        passed,
    }
}

fn render_resource_estimate(
    width: u32,
    height: u32,
    shots: &[production::Shot],
    quality: MotionQuality,
) -> (u64, usize) {
    let per_filter = (u64::from(width) * u64::from(height) * 7).div_ceil(10_000);
    let instances = if quality == MotionQuality::Smooth {
        shots
            .iter()
            .filter(|shot| !matches!(shot.motion.as_str(), "hold" | "hold-dark"))
            .count()
    } else {
        1
    };
    (
        per_filter * u64::try_from(instances.max(1)).unwrap_or(u64::MAX),
        instances,
    )
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
    let safety = loaded
        .manifest
        .shots
        .iter()
        .map(safety_report)
        .collect::<Vec<_>>();
    if let Some(unsafe_shot) = safety.iter().find(|shot| !shot.passed) {
        bail!(
            "motion transform would crop a declared focal point or protected region in shot {}",
            unsafe_shot.shot_id
        );
    }
    let (estimated_peak_memory_mib, perspective_filter_instances) = render_resource_estimate(
        options.width,
        options.height,
        &loaded.manifest.shots,
        options.motion_quality,
    );
    if estimated_peak_memory_mib > MAX_ESTIMATED_PEAK_MEMORY_MIB {
        bail!(
            "requested smooth motion is infeasible: {} perspective filters at {}x{} estimate {} MiB peak memory, above the {} MiB budget; split the render or use --motion-quality legacy",
            perspective_filter_instances,
            options.width,
            options.height,
            estimated_peak_memory_mib,
            MAX_ESTIMATED_PEAK_MEMORY_MIB
        );
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
    let manifest_path = options
        .manifest
        .canonicalize()
        .with_context(|| format!("failed to resolve manifest {}", options.manifest.display()))?;
    let mut inputs = vec![AnimaticInput {
        kind: "manifest".to_string(),
        id: loaded.manifest.work.clone(),
        path: manifest_path.display().to_string(),
        sha256: production::sha256_path(&manifest_path)?,
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
            "[{index}:v]{},fps={},settb=AVTB,trim=duration={:.3},setpts=PTS-STARTPTS[v{index}]",
            motion_filter(
                &shot.motion,
                duration + tail,
                options.fps,
                options.width,
                options.height,
                options.motion_quality,
                options.motion_curve,
            ),
            options.fps,
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
    let output_parent = options
        .output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_parent)?;
    let absolute_output = output_parent.canonicalize()?.join(
        options
            .output
            .file_name()
            .ok_or_else(|| anyhow!("output path has no filename"))?,
    );
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
        estimated_peak_memory_mib,
        perspective_filter_instances,
        maximum_estimated_peak_memory_mib: MAX_ESTIMATED_PEAK_MEMORY_MIB,
        maximum_render_pixels: MAX_RENDER_PIXELS,
        maximum_render_fps: MAX_RENDER_FPS,
        quality_override: (options.motion_quality == MotionQuality::Legacy)
            .then(|| "legacy deterministic reproduction".to_string()),
        safety,
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
        output: absolute_output.display().to_string(),
        artifact_manifest: absolute_output
            .with_extension("artifacts.json")
            .display()
            .to_string(),
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

fn cadence_values(video: &Path, start_seconds: f64, duration_seconds: f64) -> Result<Vec<f64>> {
    let adapter = FfmpegAdapter;
    let filter = format!(
        "trim=start={start_seconds:.3}:duration={duration_seconds:.3},setpts=PTS-STARTPTS,tblend=all_mode=difference,signalstats,metadata=print:file=-"
    );
    let output = adapter.run_ffmpeg(
        &[
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-i".to_string(),
            adapter.path_argument(video)?,
            "-vf".to_string(),
            filter,
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
    Ok(values)
}

pub fn check_motion(
    manifest: impl AsRef<Path>,
    video: impl AsRef<Path>,
) -> Result<MotionCheckReport> {
    let manifest = manifest
        .as_ref()
        .canonicalize()
        .with_context(|| format!("failed to resolve manifest {}", manifest.as_ref().display()))?;
    let video = video
        .as_ref()
        .canonicalize()
        .with_context(|| format!("failed to resolve video {}", video.as_ref().display()))?;
    let loaded = production::require_preview_ready(&manifest)?;
    let adapter = FfmpegAdapter;
    let actual_ms = (adapter
        .ffprobe_duration(&video)?
        .parse::<f64>()
        .context("ffprobe returned an invalid duration")?
        * 1000.0)
        .round() as u64;
    let expected_ms = loaded
        .manifest
        .shots
        .iter()
        .map(|shot| (shot.duration_seconds.unwrap_or_default() * 1000.0).round() as u64)
        .sum::<u64>();
    if actual_ms.abs_diff(expected_ms) > 50 {
        bail!("video duration {actual_ms}ms does not match manifest duration {expected_ms}ms");
    }
    let version = adapter
        .run_ffmpeg(&["-version".to_string()], &[])?
        .lines()
        .next()
        .unwrap_or("unknown")
        .to_string();
    let mut shots = Vec::new();
    for shot in &loaded.manifest.shots {
        let start = shot
            .start_seconds
            .ok_or_else(|| anyhow!("shot {} has no start_seconds", shot.id))?;
        let duration = shot
            .duration_seconds
            .ok_or_else(|| anyhow!("shot {} has no duration_seconds", shot.id))?;
        let values = cadence_values(&video, start, duration)?;
        let stationary = values
            .iter()
            .filter(|value| **value < NEAR_STATIONARY_LUMA_THRESHOLD)
            .count();
        let fraction = stationary as f64 / values.len() as f64;
        let treatment = if shot.motion.is_empty() {
            "push"
        } else {
            &shot.motion
        };
        let hold = matches!(treatment, "hold" | "hold-dark");
        shots.push(ShotCadenceReport {
            shot_id: shot.id.clone(),
            treatment: treatment.to_string(),
            expectation: if hold { "stationary" } else { "moving" }.to_string(),
            start_ms: (start * 1000.0).round() as u64,
            duration_ms: (duration * 1000.0).round() as u64,
            frame_transitions: values.len(),
            near_stationary_transitions: stationary,
            near_stationary_fraction: fraction,
            passed: if hold {
                fraction >= MIN_HOLD_STATIONARY_FRACTION
            } else {
                fraction <= MAX_NEAR_STATIONARY_FRACTION
            },
        });
    }
    let safety = loaded
        .manifest
        .shots
        .iter()
        .map(safety_report)
        .collect::<Vec<_>>();
    let passed = shots.iter().all(|shot| shot.passed) && safety.iter().all(|shot| shot.passed);
    Ok(MotionCheckReport {
        schema: "reel.motion-check.v0.1".to_string(),
        manifest: manifest.display().to_string(),
        video: video.display().to_string(),
        work: loaded.manifest.work,
        video_sha256: production::sha256_path(&video)?,
        analyzer_version: version,
        near_stationary_luma_threshold: NEAR_STATIONARY_LUMA_THRESHOLD,
        maximum_moving_near_stationary_fraction: MAX_NEAR_STATIONARY_FRACTION,
        minimum_hold_stationary_fraction: MIN_HOLD_STATIONARY_FRACTION,
        shots,
        safety,
        passed,
    })
}

#[derive(Deserialize)]
struct ProbeReport {
    streams: Vec<ProbeStream>,
    format: ProbeFormat,
}
#[derive(Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    pix_fmt: Option<String>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
}
#[derive(Deserialize)]
struct ProbeFormat {
    duration: String,
}

fn fraction(value: &str) -> Result<f64> {
    let (numerator, denominator) = value
        .split_once('/')
        .ok_or_else(|| anyhow!("invalid frame rate {value}"))?;
    let numerator = numerator.parse::<f64>()?;
    let denominator = denominator.parse::<f64>()?;
    if denominator == 0.0 {
        bail!("invalid frame rate {value}");
    }
    Ok(numerator / denominator)
}

pub fn check_animatic(artifact_manifest: impl AsRef<Path>) -> Result<AnimaticCheckReport> {
    let artifact_manifest = artifact_manifest.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to resolve artifact report {}",
            artifact_manifest.as_ref().display()
        )
    })?;
    let report: AnimaticRenderReport = serde_json::from_slice(&fs::read(&artifact_manifest)?)
        .context("artifact report is not valid JSON")?;
    if report.schema != "reel.animatic-artifacts.v0.1" {
        bail!("unsupported artifact report schema {}", report.schema);
    }
    if report.dry_run {
        bail!("cannot verify a dry-run artifact report");
    }
    let output = PathBuf::from(&report.output)
        .canonicalize()
        .with_context(|| format!("failed to resolve reported output {}", report.output))?;
    let output_hash = production::sha256_path(&output)?;
    if report.output_sha256.as_deref() != Some(&output_hash) {
        bail!("output SHA-256 does not match artifact report");
    }
    if report.output_bytes != Some(fs::metadata(&output)?.len()) {
        bail!("output byte length does not match artifact report");
    }
    for input in &report.inputs {
        let path = PathBuf::from(&input.path).canonicalize().with_context(|| {
            format!(
                "failed to resolve reported {} input {}",
                input.kind, input.path
            )
        })?;
        if production::sha256_path(&path)? != input.sha256 {
            bail!(
                "{} input {} SHA-256 does not match artifact report",
                input.kind,
                input.id
            );
        }
    }
    let manifest_input = report
        .inputs
        .iter()
        .find(|input| input.kind == "manifest")
        .ok_or_else(|| anyhow!("artifact report has no manifest input"))?;
    let loaded = production::require_preview_ready(&manifest_input.path)?;
    if loaded.manifest.work != report.work {
        bail!("artifact work does not match manifest");
    }
    let expected_duration = loaded
        .manifest
        .shots
        .iter()
        .map(|shot| (shot.duration_seconds.unwrap_or_default() * 1000.0).round() as u64)
        .sum::<u64>();
    if expected_duration != report.duration_ms {
        bail!("artifact duration does not match manifest timeline");
    }
    if report.motion.working_width != report.width
        || report.motion.working_height != report.height
        || report.motion.fps != report.fps
        || report.motion.backend_version != report.ffmpeg_version
    {
        bail!("motion lineage does not match the artifact delivery fields");
    }
    let quality = match report.motion.quality.as_str() {
        "smooth"
            if report.motion.backend == "ffmpeg-perspective"
                && report.motion.interpolation == "cubic" =>
        {
            MotionQuality::Smooth
        }
        "legacy" if report.motion.backend == "ffmpeg-zoompan" => MotionQuality::Legacy,
        _ => bail!("motion backend lineage is inconsistent"),
    };
    let (expected_memory, expected_instances) =
        render_resource_estimate(report.width, report.height, &loaded.manifest.shots, quality);
    if report.motion.estimated_peak_memory_mib != expected_memory
        || report.motion.perspective_filter_instances != expected_instances
        || report.motion.maximum_estimated_peak_memory_mib != MAX_ESTIMATED_PEAK_MEMORY_MIB
    {
        bail!("motion resource lineage does not match the manifest");
    }
    if report.motion.shots.len() != loaded.manifest.shots.len()
        || report
            .motion
            .shots
            .iter()
            .zip(&loaded.manifest.shots)
            .any(|(actual, expected)| {
                let treatment = if expected.motion.is_empty() {
                    "push"
                } else {
                    &expected.motion
                };
                let frames = (expected.duration_seconds.unwrap_or_default() * f64::from(report.fps))
                    .round()
                    .max(1.0) as u64;
                actual.shot_id != expected.id
                    || actual.treatment != treatment
                    || actual.frames != frames
            })
    {
        bail!("motion lineage does not match manifest shots");
    }
    let expected_safety = loaded
        .manifest
        .shots
        .iter()
        .map(safety_report)
        .collect::<Vec<_>>();
    if report.motion.safety != expected_safety || expected_safety.iter().any(|shot| !shot.passed) {
        bail!("artifact motion safety evidence does not match the manifest");
    }
    let captions = report
        .inputs
        .iter()
        .find(|input| input.kind == "captions")
        .ok_or_else(|| anyhow!("artifact report has no captions input"))?;
    let cues = crate::series::parse_srt(&fs::read_to_string(&captions.path)?)?;
    if cues.first().is_none_or(|cue| cue.index != 1) {
        bail!("captions must contain contiguous cues beginning at 1");
    }
    if cues
        .last()
        .is_some_and(|cue| cue.end_ms > report.duration_ms)
    {
        bail!("captions extend beyond the conformed duration");
    }
    let adapter = FfmpegAdapter;
    let probe: ProbeReport = serde_json::from_str(&adapter.ffprobe_json(&output)?)
        .context("ffprobe returned invalid JSON")?;
    let video_streams = probe
        .streams
        .iter()
        .filter(|stream| stream.codec_type.as_deref() == Some("video"))
        .collect::<Vec<_>>();
    if video_streams.len() != 1 {
        bail!("expected exactly one video stream");
    }
    let video = video_streams[0];
    if video.codec_name.as_deref() != Some("h264") {
        bail!("expected H.264 video");
    }
    if video.pix_fmt.as_deref() != Some("yuv420p") {
        bail!("expected yuv420p pixel format");
    }
    if video.width != Some(report.width) || video.height != Some(report.height) {
        bail!("video dimensions do not match artifact report");
    }
    let r_fps = fraction(video.r_frame_rate.as_deref().unwrap_or("0/1"))?;
    let avg_fps = fraction(video.avg_frame_rate.as_deref().unwrap_or("0/1"))?;
    if (r_fps - f64::from(report.fps)).abs() > 0.001
        || (avg_fps - f64::from(report.fps)).abs() > 0.001
    {
        bail!("video is not constant at the reported frame rate");
    }
    let duration_ms = (probe.format.duration.parse::<f64>()? * 1000.0).round() as u64;
    let frame_ms = (1000.0 / f64::from(report.fps)).ceil() as u64;
    if duration_ms.abs_diff(report.duration_ms) > frame_ms {
        bail!("video duration differs from artifact report by more than one frame");
    }
    if report.output_duration_ms != Some(duration_ms) {
        bail!("measured video duration does not match artifact report");
    }
    let audio_streams = probe
        .streams
        .iter()
        .filter(|stream| stream.codec_type.as_deref() == Some("audio"))
        .count();
    if (report.silent && audio_streams != 0) || (!report.silent && audio_streams != 1) {
        bail!("audio stream count does not match silent mode");
    }
    Ok(AnimaticCheckReport {
        schema: "reel.animatic-check.v0.1".to_string(),
        artifact_manifest: artifact_manifest.display().to_string(),
        output: output.display().to_string(),
        output_sha256: output_hash,
        verified_inputs: report.inputs.len(),
        codec: "h264".to_string(),
        pixel_format: "yuv420p".to_string(),
        width: report.width,
        height: report.height,
        fps: r_fps,
        duration_ms,
        audio_streams,
        caption_cues: cues.len(),
        passed: true,
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
    use crate::production::{FocalPoint, ProtectedRegion, Shot};

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

    #[test]
    fn transform_safety_protects_declared_composition() {
        let centered = Shot {
            id: "centered".to_string(),
            motion: "push".to_string(),
            focal_point: Some(FocalPoint { x: 0.5, y: 0.5 }),
            protected_regions: vec![ProtectedRegion {
                id: "caption".to_string(),
                x: 0.2,
                y: 0.7,
                width: 0.6,
                height: 0.2,
            }],
            ..Shot::default()
        };
        assert!(safety_report(&centered).passed);

        let edge = Shot {
            id: "edge".to_string(),
            motion: "pan-right".to_string(),
            focal_point: Some(FocalPoint { x: 0.01, y: 0.5 }),
            ..Shot::default()
        };
        assert!(!safety_report(&edge).passed);
    }

    #[test]
    fn multi_shot_memory_estimate_accounts_for_concurrent_filters() {
        let moving = |id: &str| Shot {
            id: id.to_string(),
            motion: "push".to_string(),
            ..Shot::default()
        };
        let three = vec![moving("one"), moving("two"), moving("three")];
        let four = vec![
            moving("one"),
            moving("two"),
            moving("three"),
            moving("four"),
        ];
        assert!(
            render_resource_estimate(1280, 720, &three, MotionQuality::Smooth).0
                <= MAX_ESTIMATED_PEAK_MEMORY_MIB
        );
        assert!(
            render_resource_estimate(1280, 720, &four, MotionQuality::Smooth).0
                > MAX_ESTIMATED_PEAK_MEMORY_MIB
        );
        assert!(
            render_resource_estimate(1920, 1080, &three[..2], MotionQuality::Smooth).0
                > MAX_ESTIMATED_PEAK_MEMORY_MIB
        );
        assert!(
            render_resource_estimate(1920, 1080, &four, MotionQuality::Legacy).0
                <= MAX_ESTIMATED_PEAK_MEMORY_MIB
        );
    }
}
