use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use tempfile::Builder;

use super::ffmpeg::{FfmpegAdapter, RenderEnvironmentReport};
use crate::{
    audio_quality::{AUDIO_CHECK_SCHEMA, AudioCheckReport},
    caption::CaptionThresholds,
    caption_presentation::{
        self, CaptionLineage, CaptionPresentationOptions, CaptionProfile, SpeakerLabelPolicy,
    },
    production::{self, AudioRole, MediaKind, TimingStatus},
};

pub const NEAR_STATIONARY_LUMA_THRESHOLD: f64 = 0.001;
pub const MAX_NEAR_STATIONARY_FRACTION: f64 = 0.10;
pub const MIN_HOLD_STATIONARY_FRACTION: f64 = 0.85;
const MAX_RENDER_PIXELS: u64 = 1920 * 1080;
const MAX_RENDER_FPS: u32 = 60;
const MAX_ESTIMATED_PEAK_MEMORY_MIB: u64 = 2048;

fn mix_audio_labels(filters: &mut Vec<String>, labels: &[String], output: &str) {
    let inputs = labels
        .iter()
        .map(|label| format!("[{label}]"))
        .collect::<String>();
    if labels.len() == 1 {
        filters.push(format!("{inputs}anull[{output}]"));
    } else {
        filters.push(format!(
            "{inputs}amix=inputs={}:normalize=0:dropout_transition=0[{output}]",
            labels.len()
        ));
    }
}

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
    EaseOut,
    Linear,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum EditMode {
    #[default]
    Cinematic,
    Montage,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum EncodingPreset {
    Medium,
    #[default]
    Slow,
}

impl EncodingPreset {
    fn as_str(self) -> &'static str {
        match self {
            Self::Medium => "medium",
            Self::Slow => "slow",
        }
    }
}

impl MotionCurve {
    fn as_str(self) -> &'static str {
        match self {
            Self::EaseInOut => "ease-in-out",
            Self::EaseOut => "ease-out",
            Self::Linear => "linear",
        }
    }
}

#[derive(Clone, Debug)]
pub struct AnimaticRenderOptions {
    pub manifest: PathBuf,
    pub asset_root: PathBuf,
    pub audio: Option<PathBuf>,
    pub audio_check_report: Option<PathBuf>,
    pub silent: bool,
    pub captions: Option<PathBuf>,
    pub caption_presentation: Option<PathBuf>,
    pub caption_profile: CaptionProfile,
    pub speaker_label_policy: SpeakerLabelPolicy,
    pub speaker_reintroduce_after_ms: Option<u64>,
    pub caption_thresholds: CaptionThresholds,
    pub caption_policy_note: Option<String>,
    pub output: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub transition_seconds: f64,
    pub disclosure: String,
    pub motion_quality: MotionQuality,
    pub motion_curve: MotionCurve,
    pub encoding_preset: EncodingPreset,
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
    #[serde(default)]
    pub edit_assembly: String,
    #[serde(default)]
    pub transition_seconds: f64,
    pub duration_ms: u64,
    pub tool_version: String,
    pub ffmpeg_version: String,
    #[serde(default)]
    pub render_environment: Option<RenderEnvironmentReport>,
    pub motion: MotionLineage,
    #[serde(default)]
    pub captions: Option<CaptionLineage>,
    #[serde(default)]
    pub audio_quality: Option<AudioQualityBinding>,
    #[serde(default)]
    pub mixed_media: MixedMediaLineage,
    pub dry_run: bool,
    pub silent: bool,
    pub command_arguments: Vec<String>,
    pub inputs: Vec<AnimaticInput>,
    pub output_sha256: Option<String>,
    pub output_bytes: Option<u64>,
    pub output_duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MixedMediaLineage {
    pub still_events: usize,
    pub video_events: usize,
    #[serde(default)]
    pub animation_events: usize,
    #[serde(default)]
    pub sprite_animation_events: usize,
    #[serde(default)]
    pub sprite_camera_tracks: usize,
    #[serde(default)]
    pub sprite_asset_occurrences: usize,
    #[serde(default)]
    pub sprite_unique_asset_inputs: usize,
    pub audio_events: usize,
    pub beat_markers: usize,
    pub narration_ducking: bool,
    pub audio_mastering: bool,
}

#[derive(Clone, Debug)]
enum ShotVisualPlan {
    Single {
        input_index: usize,
    },
    Sprites {
        background_input_index: usize,
        segments: Vec<SpriteRenderSegment>,
        camera: Vec<CameraRenderSegment>,
    },
}

#[derive(Clone, Debug)]
struct SpriteRenderSegment {
    input_index: usize,
    input_use_index: usize,
    z_index: i32,
    start_seconds: f64,
    end_seconds: f64,
    start_x: f64,
    start_y: f64,
    end_x: f64,
    end_y: f64,
    start_width: f64,
    end_width: f64,
    anchor_x: f64,
    anchor_y: f64,
    movement: production::SpriteMovement,
    movement_steps: u32,
    start_rotation_radians: f64,
    end_rotation_radians: f64,
    fade_out_start_seconds: Option<f64>,
    visible_start_seconds: f64,
    visible_end_seconds: f64,
}

#[derive(Clone, Debug)]
struct CameraRenderSegment {
    start_seconds: f64,
    end_seconds: f64,
    start_center_x: f64,
    start_center_y: f64,
    end_center_x: f64,
    end_center_y: f64,
    start_zoom: f64,
    end_zoom: f64,
    curve: production::SpriteCameraCurve,
}

#[derive(Clone, Copy, Debug)]
struct SpriteGeometry {
    x: f64,
    y: f64,
    width: f64,
}

fn sprite_geometry_at_frame(track: &production::SpriteTrack, frame: u32) -> SpriteGeometry {
    if let Some(keyframe) = track
        .keyframes
        .iter()
        .find(|keyframe| keyframe.frame == frame)
    {
        return SpriteGeometry {
            x: keyframe.x,
            y: keyframe.y,
            width: keyframe.width,
        };
    }
    if let Some(last) = track.keyframes.last().filter(|last| frame > last.frame) {
        return SpriteGeometry {
            x: last.x,
            y: last.y,
            width: last.width,
        };
    }
    let pair = track
        .keyframes
        .windows(2)
        .find(|pair| pair[0].frame < frame && frame < pair[1].frame)
        .unwrap_or_else(|| panic!("validated sprite frame {frame} must be covered by keyframes"));
    let from = &pair[0];
    let to = &pair[1];
    let raw = f64::from(frame - from.frame) / f64::from(to.frame - from.frame);
    let progress = match track.movement {
        production::SpriteMovement::Linear => raw,
        production::SpriteMovement::Stepped => {
            let steps = f64::from(track.movement_steps.unwrap_or(3));
            (raw * steps).floor() / steps
        }
        production::SpriteMovement::Hold => 0.0,
    };
    SpriteGeometry {
        x: from.x + (to.x - from.x) * progress,
        y: from.y + (to.y - from.y) * progress,
        width: from.width + (to.width - from.width) * progress,
    }
}

fn resolved_sprite_keyframes(
    animation: &production::SpriteAnimation,
    track: &production::SpriteTrack,
    canvas_aspect_ratio: f64,
) -> Vec<production::SpriteKeyframe> {
    let Some(parent_id) = track.parent.as_deref() else {
        return track.keyframes.clone();
    };
    let parent = animation
        .sprites
        .iter()
        .find(|candidate| candidate.id == parent_id)
        .expect("validated sprite parent");
    track
        .keyframes
        .iter()
        .map(|keyframe| {
            let parent_geometry = sprite_geometry_at_frame(parent, keyframe.frame);
            let mut resolved = keyframe.clone();
            resolved.x = parent_geometry.x + keyframe.x * parent_geometry.width;
            resolved.y =
                parent_geometry.y + keyframe.y * parent_geometry.width * canvas_aspect_ratio;
            resolved
        })
        .collect()
}

#[derive(Clone, Copy)]
enum CameraProperty {
    CenterX,
    CenterY,
    Zoom,
}

fn camera_expression(
    segments: &[CameraRenderSegment],
    output_fps: u32,
    property: CameraProperty,
) -> String {
    let value = |segment: &CameraRenderSegment, end: bool| match property {
        CameraProperty::CenterX if end => segment.end_center_x,
        CameraProperty::CenterX => segment.start_center_x,
        CameraProperty::CenterY if end => segment.end_center_y,
        CameraProperty::CenterY => segment.start_center_y,
        CameraProperty::Zoom if end => segment.end_zoom,
        CameraProperty::Zoom => segment.start_zoom,
    };
    let mut expression = format!(
        "{:.9}",
        value(segments.last().expect("camera segment"), true)
    );
    for segment in segments.iter().rev() {
        let start = (segment.start_seconds * f64::from(output_fps)).round() as u64;
        let end = ((segment.end_seconds * f64::from(output_fps)).round() as u64).max(start + 1);
        let span = end - start;
        let progress = format!("(on-{start})/{span}");
        let timed = match segment.curve {
            production::SpriteCameraCurve::Linear => progress.clone(),
            production::SpriteCameraCurve::EaseInOut => {
                format!("({progress})*({progress})*(3-2*({progress}))")
            }
            production::SpriteCameraCurve::EaseOut => {
                format!("1-(1-({progress}))*(1-({progress}))")
            }
            production::SpriteCameraCurve::HoldThenBurst => {
                let burst = format!("((({progress})-0.65)/0.35)");
                format!(r"gte(on\,{})*({burst})*({burst})", start + span * 65 / 100)
            }
        };
        let from = value(segment, false);
        let delta = value(segment, true) - from;
        let interpolated = format!("{from:.9}+{delta:.9}*({timed})");
        expression = format!(r"if(between(on\,{start}\,{end})\,{interpolated}\,{expression})");
    }
    expression
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudioQualityBinding {
    pub schema: String,
    pub report_schema: String,
    pub report_sha256: String,
    pub profile: String,
    pub audio_sha256: String,
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
    pub declared_hold_transitions: usize,
    pub permitted_near_stationary_transitions: usize,
    pub unexpected_near_stationary_transitions: usize,
    pub unexpected_near_stationary_fraction: f64,
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
    pub render_capabilities: usize,
    pub render_environment_fingerprint: Option<String>,
    pub passed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnimaticReceipt {
    pub schema: String,
    pub source_artifact_schema: String,
    pub source_artifact_sha256: String,
    pub tool_version: String,
    pub output_sha256: String,
    pub output_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_ms: u64,
    pub silent: bool,
    pub audio_streams: usize,
    pub caption_cues: usize,
    pub input_kinds: BTreeMap<String, usize>,
    pub motion_backend: String,
    pub motion_quality: String,
    pub motion_interpolation: String,
    pub motion_curve: String,
    pub motion_shots: usize,
    pub motion_safety_passed: bool,
    pub render_transport: Option<String>,
    pub render_environment_fingerprint: Option<String>,
    pub verified: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnimaticReceiptCheckReport {
    pub schema: String,
    pub receipt_sha256: String,
    pub video_sha256: String,
    pub output_bytes: u64,
    pub codec: String,
    pub pixel_format: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_ms: u64,
    pub audio_streams: usize,
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
    let zoom = match motion {
        "pan-left" | "pan-right" => 1.035,
        "whip-left" | "whip-right" => 1.12,
        "slam-in" => 1.28,
        "punch-in" | "punch-out" => 1.20,
        _ => 1.04,
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
        "pan-right" | "whip-right" => vec![
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
        "pan-left" | "whip-left" => vec![
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
        "pull" | "punch-out" => vec![centered, full],
        "punch-in" | "slam-in" => vec![full, centered],
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
    let manifest_audio = !loaded.manifest.audio_events.is_empty();
    let audio_modes = usize::from(options.silent)
        + usize::from(options.audio.is_some())
        + usize::from(manifest_audio);
    if audio_modes != 1 {
        bail!("provide exactly one audio mode: --audio, --silent, or manifest audio_events");
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
    if options.captions.is_none() && options.caption_presentation.is_some() {
        bail!("caption presentation requires captions");
    }
    let captions = options
        .captions
        .as_ref()
        .map(|path| {
            path.canonicalize()
                .with_context(|| format!("failed to resolve captions {}", path.display()))
        })
        .transpose()?;
    let caption_lineage = captions
        .as_ref()
        .map(|captions| {
            caption_presentation::prepare(
                &loaded,
                CaptionPresentationOptions {
                    captions,
                    presentation: options.caption_presentation.as_deref(),
                    profile: options.caption_profile,
                    policy: options.speaker_label_policy,
                    reintroduce_after_ms: options.speaker_reintroduce_after_ms,
                    thresholds: options.caption_thresholds,
                    threshold_policy_note: options.caption_policy_note.as_deref(),
                    width: options.width,
                    height: options.height,
                },
            )
        })
        .transpose()?;
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
    let mut animation_lists = Vec::new();
    let mut visual_plans = Vec::new();
    let mut ffmpeg_input_count = 0_usize;
    for (index, shot) in loaded.manifest.shots.iter().enumerate() {
        let duration = shot
            .duration_seconds
            .ok_or_else(|| anyhow!("timing not conformed: shot {} has no duration", shot.id))?;
        let tail = if index + 1 < loaded.manifest.shots.len() {
            options.transition_seconds
        } else {
            0.0
        };
        match shot.media_kind {
            MediaKind::Still | MediaKind::Video => {
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
                    kind: match shot.media_kind {
                        MediaKind::Still => "still",
                        MediaKind::Video => "video",
                        MediaKind::Animation | MediaKind::SpriteAnimation => unreachable!(),
                    }
                    .to_string(),
                    id: shot.id.clone(),
                    path: resolved.display().to_string(),
                    sha256: production::sha256_path(&resolved)?,
                });
                match shot.media_kind {
                    MediaKind::Still => args.extend([
                        "-threads".to_string(),
                        "1".to_string(),
                        "-loop".to_string(),
                        "1".to_string(),
                        "-framerate".to_string(),
                        options.fps.to_string(),
                        "-t".to_string(),
                        format!("{:.3}", duration + tail),
                        "-i".to_string(),
                        adapter.path_argument(&resolved)?,
                    ]),
                    MediaKind::Video => args.extend([
                        "-ss".to_string(),
                        format!("{:.3}", shot.source_in_seconds),
                        "-t".to_string(),
                        format!("{:.3}", duration + tail),
                        "-i".to_string(),
                        adapter.path_argument(&resolved)?,
                    ]),
                    MediaKind::Animation | MediaKind::SpriteAnimation => unreachable!(),
                }
                visual_plans.push(ShotVisualPlan::Single {
                    input_index: ffmpeg_input_count,
                });
                ffmpeg_input_count += 1;
            }
            MediaKind::Animation => {
                let animation = shot
                    .animation
                    .as_ref()
                    .ok_or_else(|| anyhow!("animation shot {} has no sequence", shot.id))?;
                let mut list = Builder::new().suffix(".ffconcat").tempfile()?;
                writeln!(list, "ffconcat version 1.0")?;
                let mut last_path = None;
                for (frame_index, frame) in animation.frames.iter().enumerate() {
                    let candidate = if Path::new(&frame.asset).is_absolute() {
                        PathBuf::from(&frame.asset)
                    } else {
                        asset_root.join(&frame.asset)
                    };
                    let resolved = candidate.canonicalize().with_context(|| {
                        format!(
                            "missing animation frame for shot {}: {}",
                            shot.id,
                            candidate.display()
                        )
                    })?;
                    if !Path::new(&frame.asset).is_absolute() && !resolved.starts_with(&asset_root)
                    {
                        bail!("shot {} animation frame escapes asset root", shot.id);
                    }
                    inputs.push(AnimaticInput {
                        kind: "animation-frame".to_string(),
                        id: format!("{}:{}", shot.id, frame_index + 1),
                        path: resolved.display().to_string(),
                        sha256: production::sha256_path(&resolved)?,
                    });
                    let concat_path = adapter
                        .path_argument(&resolved)?
                        .replace('\\', "/")
                        .replace('\'', "'\\''");
                    writeln!(list, "file '{concat_path}'")?;
                    let mut frame_duration = frame.hold_frames as f64 / animation.timing_fps as f64;
                    if frame_index + 1 == animation.frames.len() {
                        frame_duration += tail;
                    }
                    writeln!(list, "duration {frame_duration:.9}")?;
                    last_path = Some(concat_path);
                }
                writeln!(
                    list,
                    "file '{}'",
                    last_path.expect("validated animation has at least one frame")
                )?;
                list.flush()?;
                args.extend([
                    "-f".to_string(),
                    "concat".to_string(),
                    "-safe".to_string(),
                    "0".to_string(),
                    "-t".to_string(),
                    format!("{:.3}", duration + tail),
                    "-i".to_string(),
                    adapter.path_argument(list.path())?,
                ]);
                animation_lists.push(list);
                visual_plans.push(ShotVisualPlan::Single {
                    input_index: ffmpeg_input_count,
                });
                ffmpeg_input_count += 1;
            }
            MediaKind::SpriteAnimation => {
                let animation = shot
                    .sprite_animation
                    .as_ref()
                    .ok_or_else(|| anyhow!("sprite-animation shot {} has no sequence", shot.id))?;
                let background_candidate = if Path::new(&animation.background).is_absolute() {
                    PathBuf::from(&animation.background)
                } else {
                    asset_root.join(&animation.background)
                };
                let background = background_candidate.canonicalize().with_context(|| {
                    format!(
                        "missing sprite-animation background for shot {}: {}",
                        shot.id,
                        background_candidate.display()
                    )
                })?;
                if !Path::new(&animation.background).is_absolute()
                    && !background.starts_with(&asset_root)
                {
                    bail!("shot {} sprite background escapes asset root", shot.id);
                }
                inputs.push(AnimaticInput {
                    kind: "sprite-background".to_string(),
                    id: format!("{}:background", shot.id),
                    path: background.display().to_string(),
                    sha256: production::sha256_path(&background)?,
                });
                args.extend([
                    "-threads".to_string(),
                    "1".to_string(),
                    "-loop".to_string(),
                    "1".to_string(),
                    "-framerate".to_string(),
                    options.fps.to_string(),
                    "-t".to_string(),
                    format!("{:.3}", duration + tail),
                    "-i".to_string(),
                    adapter.path_argument(&background)?,
                ]);
                let background_input_index = ffmpeg_input_count;
                ffmpeg_input_count += 1;
                let mut segments = Vec::new();
                let mut sprite_input_cache: BTreeMap<PathBuf, (usize, usize)> = BTreeMap::new();
                for track in &animation.sprites {
                    let resolved_keyframes = resolved_sprite_keyframes(
                        animation,
                        track,
                        options.width as f64 / options.height as f64,
                    );
                    for (keyframe_index, keyframe) in resolved_keyframes.iter().enumerate() {
                        let candidate = if Path::new(&keyframe.asset).is_absolute() {
                            PathBuf::from(&keyframe.asset)
                        } else {
                            asset_root.join(&keyframe.asset)
                        };
                        let resolved = candidate.canonicalize().with_context(|| {
                            format!(
                                "missing sprite asset for shot {} track {}: {}",
                                shot.id,
                                track.id,
                                candidate.display()
                            )
                        })?;
                        if !Path::new(&keyframe.asset).is_absolute()
                            && !resolved.starts_with(&asset_root)
                        {
                            bail!("shot {} sprite asset escapes asset root", shot.id);
                        }
                        inputs.push(AnimaticInput {
                            kind: "sprite-pose".to_string(),
                            id: format!("{}:{}:{}", shot.id, track.id, keyframe_index + 1),
                            path: resolved.display().to_string(),
                            sha256: production::sha256_path(&resolved)?,
                        });
                        let (sprite_input_index, input_use_index, is_new_input) =
                            if let Some((input_index, use_count)) =
                                sprite_input_cache.get_mut(&resolved)
                            {
                                let use_index = *use_count;
                                *use_count += 1;
                                (*input_index, use_index, false)
                            } else {
                                let input_index = ffmpeg_input_count;
                                sprite_input_cache.insert(resolved.clone(), (input_index, 1));
                                (input_index, 0, true)
                            };
                        if is_new_input {
                            args.extend([
                                "-threads".to_string(),
                                "1".to_string(),
                                "-loop".to_string(),
                                "1".to_string(),
                                "-framerate".to_string(),
                                options.fps.to_string(),
                                "-t".to_string(),
                                format!("{:.3}", duration + tail),
                                "-i".to_string(),
                                adapter.path_argument(&resolved)?,
                            ]);
                            ffmpeg_input_count += 1;
                        }
                        let next = resolved_keyframes.get(keyframe_index + 1);
                        let end_seconds = next.map_or(duration + tail, |next| {
                            next.frame as f64 / animation.timing_fps as f64
                        });
                        segments.push(SpriteRenderSegment {
                            input_index: sprite_input_index,
                            input_use_index,
                            z_index: keyframe.z_index.unwrap_or(track.z_index),
                            start_seconds: keyframe.frame as f64 / animation.timing_fps as f64,
                            end_seconds,
                            start_x: keyframe.x,
                            start_y: keyframe.y,
                            end_x: next.map_or(keyframe.x, |next| next.x),
                            end_y: next.map_or(keyframe.y, |next| next.y),
                            start_width: keyframe.width,
                            end_width: next.map_or(keyframe.width, |next| next.width),
                            anchor_x: track.anchor_x.unwrap_or(0.5),
                            anchor_y: track.anchor_y.unwrap_or(0.5),
                            movement: track.movement,
                            movement_steps: track.movement_steps.unwrap_or(3),
                            start_rotation_radians: 0.0,
                            end_rotation_radians: 0.0,
                            fade_out_start_seconds: None,
                            visible_start_seconds: track.visible_start_frame.unwrap_or(0) as f64
                                / animation.timing_fps as f64,
                            visible_end_seconds: track.visible_end_frame.map_or(
                                duration + tail,
                                |frame| {
                                    f64::from(frame.saturating_add(1)) / animation.timing_fps as f64
                                },
                            ),
                        });
                    }
                }
                for emission in &animation.emissions {
                    let candidate = if Path::new(&emission.asset).is_absolute() {
                        PathBuf::from(&emission.asset)
                    } else {
                        asset_root.join(&emission.asset)
                    };
                    let resolved = candidate.canonicalize().with_context(|| {
                        format!(
                            "missing sprite emission asset for shot {} emission {}: {}",
                            shot.id,
                            emission.id,
                            candidate.display()
                        )
                    })?;
                    if !Path::new(&emission.asset).is_absolute()
                        && !resolved.starts_with(&asset_root)
                    {
                        bail!("shot {} sprite emission asset escapes asset root", shot.id);
                    }
                    inputs.push(AnimaticInput {
                        kind: "sprite-emission".to_string(),
                        id: format!("{}:{}", shot.id, emission.id),
                        path: resolved.display().to_string(),
                        sha256: production::sha256_path(&resolved)?,
                    });
                    let (sprite_input_index, input_use_index, is_new_input) =
                        if let Some((input_index, use_count)) =
                            sprite_input_cache.get_mut(&resolved)
                        {
                            let use_index = *use_count;
                            *use_count += 1;
                            (*input_index, use_index, false)
                        } else {
                            let input_index = ffmpeg_input_count;
                            sprite_input_cache.insert(resolved.clone(), (input_index, 1));
                            (input_index, 0, true)
                        };
                    if is_new_input {
                        args.extend([
                            "-threads".to_string(),
                            "1".to_string(),
                            "-loop".to_string(),
                            "1".to_string(),
                            "-framerate".to_string(),
                            options.fps.to_string(),
                            "-t".to_string(),
                            format!("{:.3}", duration + tail),
                            "-i".to_string(),
                            adapter.path_argument(&resolved)?,
                        ]);
                        ffmpeg_input_count += 1;
                    }
                    let parent = animation
                        .sprites
                        .iter()
                        .find(|track| track.id == emission.parent)
                        .expect("validated emission parent");
                    let parent_geometry = sprite_geometry_at_frame(parent, emission.frame);
                    let start_x = parent_geometry.x + emission.offset_x * parent_geometry.width;
                    let start_y = parent_geometry.y
                        + emission.offset_y
                            * parent_geometry.width
                            * (options.width as f64 / options.height as f64);
                    let start_seconds = emission.frame as f64 / animation.timing_fps as f64;
                    let end_seconds = (emission.frame + emission.duration_frames) as f64
                        / animation.timing_fps as f64;
                    let fade_out_start_seconds = (emission.fade_out_frames > 0).then(|| {
                        end_seconds - emission.fade_out_frames as f64 / animation.timing_fps as f64
                    });
                    segments.push(SpriteRenderSegment {
                        input_index: sprite_input_index,
                        input_use_index,
                        z_index: emission.z_index,
                        start_seconds,
                        end_seconds,
                        start_x,
                        start_y,
                        end_x: start_x + emission.drift_x,
                        end_y: start_y + emission.drift_y,
                        start_width: emission.width,
                        end_width: emission.end_width.unwrap_or(emission.width),
                        anchor_x: emission.anchor_x,
                        anchor_y: emission.anchor_y,
                        movement: production::SpriteMovement::Linear,
                        movement_steps: 1,
                        start_rotation_radians: emission.rotation_degrees.to_radians(),
                        end_rotation_radians: emission.end_rotation_degrees.to_radians(),
                        fade_out_start_seconds,
                        visible_start_seconds: start_seconds,
                        visible_end_seconds: end_seconds,
                    });
                }
                segments.sort_by_key(|segment| segment.z_index);
                let camera = animation
                    .camera
                    .windows(2)
                    .map(|pair| CameraRenderSegment {
                        start_seconds: pair[0].frame as f64 / animation.timing_fps as f64,
                        end_seconds: pair[1].frame as f64 / animation.timing_fps as f64,
                        start_center_x: pair[0].center_x,
                        start_center_y: pair[0].center_y,
                        end_center_x: pair[1].center_x,
                        end_center_y: pair[1].center_y,
                        start_zoom: pair[0].zoom,
                        end_zoom: pair[1].zoom,
                        curve: pair[0].curve_to_next,
                    })
                    .collect();
                visual_plans.push(ShotVisualPlan::Sprites {
                    background_input_index,
                    segments,
                    camera,
                });
            }
        }
        durations.push(duration);
    }
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
    for event in &loaded.manifest.audio_events {
        let candidate = if Path::new(&event.source).is_absolute() {
            PathBuf::from(&event.source)
        } else {
            asset_root.join(&event.source)
        };
        let resolved = candidate.canonicalize().with_context(|| {
            format!(
                "missing source for audio event {}: {}",
                event.id,
                candidate.display()
            )
        })?;
        if !Path::new(&event.source).is_absolute() && !resolved.starts_with(&asset_root) {
            bail!("audio event {} source escapes asset root", event.id);
        }
        inputs.push(AnimaticInput {
            kind: format!(
                "audio-{}",
                match event.role {
                    AudioRole::Music => "music",
                    AudioRole::Ambience => "ambience",
                    AudioRole::Effect => "effect",
                    AudioRole::Narration => "narration",
                }
            ),
            id: event.id.clone(),
            path: resolved.display().to_string(),
            sha256: production::sha256_path(&resolved)?,
        });
        if event.loop_source {
            args.extend(["-stream_loop".to_string(), "-1".to_string()]);
        }
        args.extend(["-i".to_string(), adapter.path_argument(&resolved)?]);
    }
    let audio_quality = match (&audio, &options.audio_check_report) {
        (None, Some(_)) => bail!("silent rendering cannot bind an audio-check report"),
        (Some(audio), Some(report_path)) => {
            let report_path = report_path.canonicalize().with_context(|| {
                format!(
                    "failed to resolve audio-check report {}",
                    report_path.display()
                )
            })?;
            let report: AudioCheckReport = serde_json::from_slice(&fs::read(&report_path)?)
                .context("audio-check report is not valid JSON")?;
            if report.schema != AUDIO_CHECK_SCHEMA || !report.passed {
                bail!("audio-check report is unsupported or did not pass");
            }
            if report.audio.sha256 != production::sha256_path(audio)? {
                bail!("audio-check report hash does not match render audio");
            }
            let expected_duration_ms = durations
                .iter()
                .map(|value| (value * 1000.0).round() as u64)
                .sum::<u64>();
            if report.audio.duration_ms.abs_diff(expected_duration_ms) > 50 {
                bail!("audio-check duration does not match the conformed timeline");
            }
            let report_sha256 = production::sha256_path(&report_path)?;
            inputs.push(AnimaticInput {
                kind: "audio-check-report".to_string(),
                id: "audio-quality".to_string(),
                path: report_path.display().to_string(),
                sha256: report_sha256.clone(),
            });
            Some(AudioQualityBinding {
                schema: "reel.audio-binding.v0.1".to_string(),
                report_schema: report.schema,
                report_sha256,
                profile: report.profile,
                audio_sha256: report.audio.sha256,
            })
        }
        _ => None,
    };
    if let Some(captions) = &captions {
        inputs.push(AnimaticInput {
            kind: "captions".to_string(),
            id: "captions".to_string(),
            path: captions.display().to_string(),
            sha256: production::sha256_path(captions)?,
        });
    }
    if let Some(presentation) = &options.caption_presentation {
        let presentation = presentation.canonicalize().with_context(|| {
            format!(
                "failed to resolve caption presentation {}",
                presentation.display()
            )
        })?;
        inputs.push(AnimaticInput {
            kind: "caption-presentation".to_string(),
            id: "caption-presentation".to_string(),
            path: presentation.display().to_string(),
            sha256: production::sha256_path(&presentation)?,
        });
    }
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
        match &visual_plans[index] {
            ShotVisualPlan::Single { input_index } => {
                let treatment = match shot.media_kind {
                    MediaKind::Still => motion_filter(
                        &shot.motion,
                        duration + tail,
                        options.fps,
                        options.width,
                        options.height,
                        options.motion_quality,
                        options.motion_curve,
                    ),
                    MediaKind::Video | MediaKind::Animation => format!(
                        "scale={}:{}:force_original_aspect_ratio=increase,crop={}:{},setsar=1",
                        options.width, options.height, options.width, options.height
                    ),
                    MediaKind::SpriteAnimation => unreachable!(),
                };
                filters.push(format!(
                    "[{input_index}:v]{treatment},framerate=fps={},setsar=1,settb=AVTB,trim=duration={:.3},setpts=PTS-STARTPTS[v{index}]",
                    options.fps,
                    duration + tail
                ));
            }
            ShotVisualPlan::Sprites {
                background_input_index,
                segments,
                camera,
            } => {
                let background = format!("sprite{index}_base");
                let background_treatment = motion_filter(
                    &shot.motion,
                    duration + tail,
                    options.fps,
                    options.width,
                    options.height,
                    options.motion_quality,
                    options.motion_curve,
                );
                filters.push(format!(
                    "[{background_input_index}:v]{background_treatment},framerate=fps={},setsar=1,settb=AVTB,trim=duration={:.3},setpts=PTS-STARTPTS[{background}]",
                    options.fps,
                    duration + tail
                ));
                let mut sprite_input_use_counts = BTreeMap::new();
                for segment in segments {
                    sprite_input_use_counts
                        .entry(segment.input_index)
                        .and_modify(|count: &mut usize| {
                            *count = (*count).max(segment.input_use_index + 1)
                        })
                        .or_insert(segment.input_use_index + 1);
                }
                for (input_index, use_count) in &sprite_input_use_counts {
                    if *use_count > 1 {
                        let outputs = (0..*use_count)
                            .map(|use_index| format!("[sprite_input_{input_index}_{use_index}]"))
                            .collect::<String>();
                        filters.push(format!("[{input_index}:v]split={use_count}{outputs}"));
                    }
                }
                let mut previous = background;
                for (segment_index, segment) in segments.iter().enumerate() {
                    let sprite = format!("sprite{index}_{segment_index}");
                    let output = format!("sprite{index}_out{segment_index}");
                    let span = segment.end_seconds - segment.start_seconds;
                    let start_width = (options.width as f64 * segment.start_width).max(1.0);
                    let width_delta =
                        options.width as f64 * (segment.end_width - segment.start_width);
                    let progress = match segment.movement {
                        production::SpriteMovement::Linear => {
                            format!("(t-{:.9})/{span:.9}", segment.start_seconds)
                        }
                        production::SpriteMovement::Stepped => format!(
                            "floor(((t-{:.9})/{span:.9})*{})/{}",
                            segment.start_seconds, segment.movement_steps, segment.movement_steps
                        ),
                        production::SpriteMovement::Hold => "0".to_string(),
                    };
                    let rotation_delta =
                        segment.end_rotation_radians - segment.start_rotation_radians;
                    let input_use_count = sprite_input_use_counts[&segment.input_index];
                    let input_label = if input_use_count > 1 {
                        format!(
                            "sprite_input_{}_{}",
                            segment.input_index, segment.input_use_index
                        )
                    } else {
                        format!("{}:v", segment.input_index)
                    };
                    let mut sprite_filter = format!(
                        "[{input_label}]trim=start={:.9}:end={:.9},scale=w='{start_width:.9}+{width_delta:.9}*({progress})':h=-1:eval=frame:flags=lanczos,format=rgba",
                        segment.start_seconds, segment.end_seconds,
                    );
                    if segment.start_rotation_radians.abs() > f64::EPSILON
                        || rotation_delta.abs() > f64::EPSILON
                    {
                        sprite_filter.push_str(&format!(
                            ",rotate=angle='{:.9}+{rotation_delta:.9}*({progress})':ow=rotw(iw):oh=roth(ih):c=none",
                            segment.start_rotation_radians,
                        ));
                    }
                    if let Some(fade_start) = segment.fade_out_start_seconds {
                        let fade_duration = segment.end_seconds - fade_start;
                        sprite_filter.push_str(&format!(
                            ",fade=t=out:st={fade_start:.9}:d={fade_duration:.9}:alpha=1"
                        ));
                    }
                    filters.push(format!("{sprite_filter}[{sprite}]"));
                    let x_delta = segment.end_x - segment.start_x;
                    let y_delta = segment.end_y - segment.start_y;
                    let visible_start = segment.start_seconds.max(segment.visible_start_seconds);
                    let visible_end = segment.end_seconds.min(segment.visible_end_seconds);
                    filters.push(format!(
                        "[{previous}][{sprite}]overlay=x='W*({:.9}+{x_delta:.9}*({progress}))-w*{:.9}':y='H*({:.9}+{y_delta:.9}*({progress}))-h*{:.9}':eval=frame:enable='gte(t,{:.9})*lt(t,{:.9})'[{output}]",
                        segment.start_x,
                        segment.anchor_x,
                        segment.start_y,
                        segment.anchor_y,
                        visible_start,
                        visible_end,
                    ));
                    previous = output;
                }
                if camera.is_empty() {
                    filters.push(format!(
                        "[{previous}]framerate=fps={},setsar=1,settb=AVTB,trim=duration={:.3},setpts=PTS-STARTPTS[v{index}]",
                        options.fps,
                        duration + tail
                    ));
                } else {
                    let zoom = camera_expression(camera, options.fps, CameraProperty::Zoom);
                    let center_x = camera_expression(camera, options.fps, CameraProperty::CenterX);
                    let center_y = camera_expression(camera, options.fps, CameraProperty::CenterY);
                    let x = format!("max(0\\,min(iw-iw/zoom\\,iw*({center_x})-iw/zoom/2))");
                    let y = format!("max(0\\,min(ih-ih/zoom\\,ih*({center_y})-ih/zoom/2))");
                    filters.push(format!(
                        "[{previous}]zoompan=z='{zoom}':x='{x}':y='{y}':d=1:s={}x{}:fps={},framerate=fps={},setsar=1,settb=AVTB,trim=duration={:.3},setpts=PTS-STARTPTS[v{index}]",
                        options.width,
                        options.height,
                        options.fps,
                        options.fps,
                        duration + tail
                    ));
                }
            }
        }
    }
    let previous = if durations.len() == 1 {
        "v0".to_string()
    } else if options.transition_seconds == 0.0 {
        let inputs = (0..durations.len())
            .map(|index| format!("[v{index}]"))
            .collect::<String>();
        filters.push(format!(
            "{inputs}concat=n={}:v=1:a=0[sequence]",
            durations.len()
        ));
        "sequence".to_string()
    } else {
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
        previous
    };
    let timeline_frames = (durations.iter().sum::<f64>() * f64::from(options.fps))
        .round()
        .max(1.0) as u64;
    let disclosure = escape_drawtext(&options.disclosure);
    let disclosure_font_size = if options.height > options.width {
        18
    } else {
        14
    };
    let mut presentation_filters = Vec::new();
    if let (Some(captions), Some(caption_lineage)) = (&captions, &caption_lineage) {
        let caption_path = adapter
            .path_argument(captions)?
            .replace('\\', "/")
            .replace(':', "\\:")
            .replace('\'', "\\'");
        let caption_font_size = caption_lineage.style.caption_font_size;
        let caption_margin = options.height.saturating_sub(
            caption_lineage.style.caption_region.y + caption_lineage.style.caption_region.height,
        );
        let caption_margin_x = caption_lineage.style.caption_region.x;
        presentation_filters.push(format!(
            "subtitles=filename='{caption_path}':force_style='FontName=Sans,FontSize={caption_font_size},MarginL={caption_margin_x},MarginR={caption_margin_x},MarginV={caption_margin},Outline={},Shadow=0,Alignment=2'",
            caption_lineage.style.caption_outline_px
        ));
        for event in &caption_lineage.label_events {
            let label = escape_drawtext(&event.audience_label);
            presentation_filters.push(format!(
                "drawtext=text='{label}':fontcolor={}:fontsize={}:x={}:y={}:box=1:boxcolor={}:boxborderw={}:enable='between(t,{:.3},{:.3})'",
                caption_lineage.style.badge_text_color,
                caption_lineage.style.badge_font_size_px,
                caption_lineage.style.badge_region.x,
                caption_lineage.style.badge_region.y,
                caption_lineage.style.badge_background,
                caption_lineage.style.badge_padding_px,
                event.start_ms as f64 / 1000.0,
                event.end_ms as f64 / 1000.0,
            ));
        }
    }
    if !options.disclosure.is_empty() {
        presentation_filters.push(format!(
            "drawtext=text='{disclosure}':fontcolor=white@0.68:fontsize={disclosure_font_size}:x=w-tw-24:y=20:box=1:boxcolor=black@0.3:boxborderw=5"
        ));
    }
    let presentation_filters = if presentation_filters.is_empty() {
        "null".to_string()
    } else {
        presentation_filters.join(",")
    };
    filters.push(format!(
        "[{previous}]trim=end_frame={timeline_frames},setpts=PTS-STARTPTS,{presentation_filters},format=yuv420p[finalv]"
    ));
    let timeline_seconds = durations.iter().sum::<f64>();
    let audio_map = if manifest_audio {
        let mut narration = Vec::new();
        let mut background = Vec::new();
        for (index, event) in loaded.manifest.audio_events.iter().enumerate() {
            let input_index = ffmpeg_input_count + index;
            let duration = event
                .duration_seconds
                .unwrap_or(timeline_seconds - event.start_seconds);
            let mut chain = format!(
                "[{input_index}:a:0]atrim=start={:.3}:duration={duration:.3},asetpts=PTS-STARTPTS,volume={:.3}dB",
                event.source_in_seconds, event.gain_db
            );
            if event.fade_in_ms > 0 {
                chain.push_str(&format!(
                    ",afade=t=in:st=0:d={:.3}",
                    event.fade_in_ms as f64 / 1000.0
                ));
            }
            if event.fade_out_ms > 0 {
                let fade_duration = event.fade_out_ms as f64 / 1000.0;
                chain.push_str(&format!(
                    ",afade=t=out:st={:.3}:d={fade_duration:.3}",
                    (duration - fade_duration).max(0.0)
                ));
            }
            chain.push_str(&format!(
                ",adelay={}:all=1[ae{index}]",
                (event.start_seconds * 1000.0).round() as u64
            ));
            filters.push(chain);
            let label = format!("ae{index}");
            if event.role == AudioRole::Narration {
                narration.push(label);
            } else {
                background.push(label);
            }
        }
        let mixed = match (background.is_empty(), narration.is_empty()) {
            (false, false) => {
                mix_audio_labels(&mut filters, &background, "background");
                mix_audio_labels(&mut filters, &narration, "narration");
                if let Some(ducking) = &loaded.manifest.narration_ducking {
                    filters.push(
                        "[narration]asplit=2[narration_detector][narration_program]".to_string(),
                    );
                    filters.push(format!(
                        "[background][narration_detector]sidechaincompress=threshold={:.6}:ratio={:.3}:attack={}:release={}[ducked]",
                        ducking.threshold,
                        ducking.ratio,
                        ducking.attack_ms,
                        ducking.release_ms
                    ));
                    filters.push(
                        "[ducked][narration_program]amix=inputs=2:normalize=0:dropout_transition=0[mixedaudio]"
                            .to_string(),
                    );
                } else {
                    filters.push(
                        "[background][narration]amix=inputs=2:normalize=0:dropout_transition=0[mixedaudio]"
                            .to_string(),
                    );
                }
                "mixedaudio"
            }
            (false, true) => {
                mix_audio_labels(&mut filters, &background, "mixedaudio");
                "mixedaudio"
            }
            (true, false) => {
                mix_audio_labels(&mut filters, &narration, "mixedaudio");
                "mixedaudio"
            }
            (true, true) => unreachable!("manifest audio mode requires audio events"),
        };
        let mastering =
            loaded
                .manifest
                .audio_mastering
                .as_ref()
                .map_or_else(String::new, |mastering| {
                    format!(
                        ",loudnorm=I={:.3}:LRA={:.3}:TP={:.3},alimiter=limit={:.3}:level=false",
                        mastering.integrated_lufs,
                        mastering.loudness_range_lu,
                        mastering.true_peak_dbfs,
                        mastering.limiter
                    )
                });
        filters.push(format!(
            "[{mixed}]aresample=async=1:first_pts=0,apad{mastering},atrim=duration={timeline_seconds:.3}[finala]"
        ));
        Some("[finala]".to_string())
    } else if audio.is_some() {
        Some(format!(
            "{}:a:0",
            ffmpeg_input_count + loaded.manifest.audio_events.len()
        ))
    } else {
        None
    };
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
    let filter_graph = filters.join(";");
    args.extend([
        "-filter_complex_threads".to_string(),
        "2".to_string(),
        "-filter_complex".to_string(),
        filter_graph.clone(),
        "-map".to_string(),
        "[finalv]".to_string(),
        "-c:v".to_string(),
        "libx264".to_string(),
        "-preset".to_string(),
        options.encoding_preset.as_str().to_string(),
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
    if let Some(audio_map) = audio_map {
        args.extend([
            "-map".to_string(),
            audio_map,
            "-c:a".to_string(),
            "aac".to_string(),
            "-b:a".to_string(),
            "128k".to_string(),
            "-shortest".to_string(),
        ]);
    }
    args.push(output_argument);
    let render_environment = if options.dry_run {
        None
    } else {
        let environment = adapter.render_environment()?;
        let missing = environment
            .missing()
            .into_iter()
            .filter(|id| {
                options.motion_quality == MotionQuality::Smooth
                    || !matches!(*id, "filter:perspective" | "perspective:cubic")
            })
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            bail!(
                "render environment is missing required capabilities: {}; run `reel render-doctor --output json` for evidence",
                missing.join(", ")
            );
        }
        Some(environment)
    };
    let ffmpeg_version = render_environment
        .as_ref()
        .map(|environment| environment.ffmpeg_version.clone())
        .unwrap_or_else(|| "not-probed-dry-run".to_string());
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
        let mut filter_script = Builder::new()
            .prefix(".reel-filter-")
            .suffix(".txt")
            .tempfile_in(output_parent)?;
        filter_script.write_all(filter_graph.as_bytes())?;
        filter_script.flush()?;
        let filter_index = render_args
            .iter()
            .position(|argument| argument == "-filter_complex")
            .ok_or_else(|| anyhow!("render command has no filter graph"))?;
        render_args[filter_index] = "-filter_complex_script".to_string();
        render_args[filter_index + 1] = adapter.path_argument(filter_script.path())?;
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
    let sprite_asset_occurrences = inputs
        .iter()
        .filter(|input| matches!(input.kind.as_str(), "sprite-pose" | "sprite-emission"))
        .count();
    let sprite_unique_asset_inputs = inputs
        .iter()
        .filter(|input| matches!(input.kind.as_str(), "sprite-pose" | "sprite-emission"))
        .map(|input| input.path.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
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
        edit_assembly: if options.transition_seconds == 0.0 {
            "hard-cut-concat"
        } else {
            "crossfade"
        }
        .to_string(),
        transition_seconds: options.transition_seconds,
        duration_ms: expected_duration_ms,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        ffmpeg_version,
        render_environment,
        motion,
        captions: caption_lineage,
        audio_quality,
        mixed_media: MixedMediaLineage {
            still_events: loaded
                .manifest
                .shots
                .iter()
                .filter(|shot| shot.media_kind == MediaKind::Still)
                .count(),
            video_events: loaded
                .manifest
                .shots
                .iter()
                .filter(|shot| shot.media_kind == MediaKind::Video)
                .count(),
            animation_events: loaded
                .manifest
                .shots
                .iter()
                .filter(|shot| shot.media_kind == MediaKind::Animation)
                .count(),
            sprite_animation_events: loaded
                .manifest
                .shots
                .iter()
                .filter(|shot| shot.media_kind == MediaKind::SpriteAnimation)
                .count(),
            sprite_camera_tracks: loaded
                .manifest
                .shots
                .iter()
                .filter_map(|shot| shot.sprite_animation.as_ref())
                .filter(|animation| !animation.camera.is_empty())
                .count(),
            sprite_asset_occurrences,
            sprite_unique_asset_inputs,
            audio_events: loaded.manifest.audio_events.len(),
            beat_markers: loaded.manifest.beat_markers.len(),
            narration_ducking: loaded.manifest.narration_ducking.is_some(),
            audio_mastering: loaded.manifest.audio_mastering.is_some(),
        },
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
        "punch-in" => (
            format!("min(1.20,1.0+0.20*on/{frames})"),
            "iw/2-(iw/zoom/2)".to_string(),
            "ih/2-(ih/zoom/2)".to_string(),
        ),
        "punch-out" => (
            format!("if(eq(on,0),1.20,max(1.0,zoom-0.20/{frames}))"),
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
        "whip-right" => (
            "1.12".to_string(),
            format!("(iw-iw/zoom)*on/{frames}"),
            "ih/2-(ih/zoom/2)".to_string(),
        ),
        "whip-left" => (
            "1.12".to_string(),
            format!("(iw-iw/zoom)*(1-on/{frames})"),
            "ih/2-(ih/zoom/2)".to_string(),
        ),
        "slam-in" => (
            format!("min(1.28,1.0+0.28*on/{frames})"),
            "iw/2-(iw/zoom/2)".to_string(),
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
        MotionCurve::EaseOut => format!("1-pow(1-({linear}),3)"),
        MotionCurve::Linear => linear.clone(),
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
        "punch-in" => (format!("1+0.20*({progress})"), None),
        "punch-out" => (format!("1.20-0.20*({progress})"), None),
        "slam-in" => (format!("1+0.28*(1-pow(1-({linear}),4))"), None),
        "pan-right" => (
            "1.035".to_string(),
            Some(format!("(W-W/1.035)*({progress})")),
        ),
        "pan-left" => (
            "1.035".to_string(),
            Some(format!("(W-W/1.035)*(1-({progress}))")),
        ),
        "whip-right" => (
            "1.12".to_string(),
            Some(format!("(W-W/1.12)*(1-pow(1-({linear}),4))")),
        ),
        "whip-left" => (
            "1.12".to_string(),
            Some(format!("(W-W/1.12)*(1-(1-pow(1-({linear}),4)))")),
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

fn sprite_animation_has_authored_motion(animation: &production::SpriteAnimation) -> bool {
    let camera_moves = animation.camera.windows(2).any(|keyframes| {
        let from = &keyframes[0];
        let to = &keyframes[1];
        (from.center_x - to.center_x).abs() > f64::EPSILON
            || (from.center_y - to.center_y).abs() > f64::EPSILON
            || (from.zoom - to.zoom).abs() > f64::EPSILON
    });
    let sprite_moves = animation.sprites.iter().any(|track| {
        track.keyframes.windows(2).any(|keyframes| {
            let from = &keyframes[0];
            let to = &keyframes[1];
            from.asset != to.asset
                || from.z_index != to.z_index
                || (from.x - to.x).abs() > f64::EPSILON
                || (from.y - to.y).abs() > f64::EPSILON
                || (from.width - to.width).abs() > f64::EPSILON
        })
    });
    let visibility_changes = animation
        .sprites
        .iter()
        .any(|track| track.visible_start_frame.is_some() || track.visible_end_frame.is_some());
    camera_moves || sprite_moves || visibility_changes || !animation.emissions.is_empty()
}

fn intentional_hold_mask(
    animation: &production::SpriteAnimation,
    transition_count: usize,
    duration_seconds: f64,
) -> Vec<bool> {
    if animation.intentional_holds.is_empty() || transition_count == 0 {
        return vec![false; transition_count];
    }
    let total_frames = (duration_seconds * f64::from(animation.timing_fps)).round() as usize;
    (0..transition_count)
        .map(|index| {
            let frame = (((index + 1) as f64 * total_frames as f64) / (transition_count + 1) as f64)
                .round() as u32;
            animation
                .intentional_holds
                .iter()
                .any(|hold| frame > hold.start_frame && frame <= hold.end_frame)
        })
        .collect()
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
        let authored_sprite_motion = shot
            .sprite_animation
            .as_ref()
            .is_some_and(sprite_animation_has_authored_motion);
        let hold = matches!(treatment, "hold" | "hold-dark") && !authored_sprite_motion;
        let hold_mask = shot.sprite_animation.as_ref().map_or_else(
            || vec![false; values.len()],
            |animation| intentional_hold_mask(animation, values.len(), duration),
        );
        let declared_hold_transitions = hold_mask.iter().filter(|declared| **declared).count();
        let permitted_near_stationary_transitions = values
            .iter()
            .zip(&hold_mask)
            .filter(|(value, declared)| **declared && **value < NEAR_STATIONARY_LUMA_THRESHOLD)
            .count();
        let unexpected_near_stationary_transitions =
            stationary.saturating_sub(permitted_near_stationary_transitions);
        let unexpected_transition_count = values.len().saturating_sub(declared_hold_transitions);
        let unexpected_near_stationary_fraction = if unexpected_transition_count == 0 {
            0.0
        } else {
            unexpected_near_stationary_transitions as f64 / unexpected_transition_count as f64
        };
        shots.push(ShotCadenceReport {
            shot_id: shot.id.clone(),
            treatment: treatment.to_string(),
            expectation: if hold {
                "stationary"
            } else if declared_hold_transitions > 0 {
                "moving-with-intentional-holds"
            } else {
                "moving"
            }
            .to_string(),
            start_ms: (start * 1000.0).round() as u64,
            duration_ms: (duration * 1000.0).round() as u64,
            frame_transitions: values.len(),
            near_stationary_transitions: stationary,
            near_stationary_fraction: fraction,
            declared_hold_transitions,
            permitted_near_stationary_transitions,
            unexpected_near_stationary_transitions,
            unexpected_near_stationary_fraction,
            passed: if hold {
                fraction >= MIN_HOLD_STATIONARY_FRACTION
            } else {
                unexpected_near_stationary_fraction <= MAX_NEAR_STATIONARY_FRACTION
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
    let tool_version = parse_tool_version(&report.tool_version)?;
    if tool_version >= (0, 2, 5) {
        let environment = report
            .render_environment
            .as_ref()
            .ok_or_else(|| anyhow!("v0.2.5+ artifact report has no render environment lineage"))?;
        environment.validate_lineage(report.motion.quality == "smooth")?;
        if environment.ffmpeg_version != report.ffmpeg_version {
            bail!("render environment does not match artifact FFmpeg lineage");
        }
    } else if let Some(environment) = &report.render_environment {
        environment.validate_lineage(report.motion.quality == "smooth")?;
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
    let aspect_ratio = report.width as f64 / report.height as f64;
    let expected_sprite_input_ids = loaded
        .manifest
        .shots
        .iter()
        .filter_map(|shot| {
            shot.sprite_animation
                .as_ref()
                .map(|animation| (shot, animation))
        })
        .flat_map(|(shot, animation)| {
            let mut ids = Vec::new();
            for track in &animation.sprites {
                ids.extend(
                    resolved_sprite_keyframes(animation, track, aspect_ratio)
                        .iter()
                        .enumerate()
                        .map(|(index, _)| format!("{}:{}:{}", shot.id, track.id, index + 1)),
                );
            }
            ids.extend(
                animation
                    .emissions
                    .iter()
                    .map(|emission| format!("{}:{}", shot.id, emission.id)),
            );
            ids
        })
        .collect::<BTreeSet<_>>();
    let actual_sprite_input_ids = report
        .inputs
        .iter()
        .filter(|input| matches!(input.kind.as_str(), "sprite-pose" | "sprite-emission"))
        .map(|input| input.id.clone())
        .collect::<BTreeSet<_>>();
    let actual_sprite_input_count = report
        .inputs
        .iter()
        .filter(|input| matches!(input.kind.as_str(), "sprite-pose" | "sprite-emission"))
        .count();
    let expected_mixed_media = MixedMediaLineage {
        still_events: loaded
            .manifest
            .shots
            .iter()
            .filter(|shot| shot.media_kind == MediaKind::Still)
            .count(),
        video_events: loaded
            .manifest
            .shots
            .iter()
            .filter(|shot| shot.media_kind == MediaKind::Video)
            .count(),
        animation_events: loaded
            .manifest
            .shots
            .iter()
            .filter(|shot| shot.media_kind == MediaKind::Animation)
            .count(),
        sprite_animation_events: loaded
            .manifest
            .shots
            .iter()
            .filter(|shot| shot.media_kind == MediaKind::SpriteAnimation)
            .count(),
        sprite_camera_tracks: loaded
            .manifest
            .shots
            .iter()
            .filter_map(|shot| shot.sprite_animation.as_ref())
            .filter(|animation| !animation.camera.is_empty())
            .count(),
        sprite_asset_occurrences: expected_sprite_input_ids.len(),
        sprite_unique_asset_inputs: report
            .inputs
            .iter()
            .filter(|input| matches!(input.kind.as_str(), "sprite-pose" | "sprite-emission"))
            .map(|input| input.path.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        audio_events: loaded.manifest.audio_events.len(),
        beat_markers: loaded.manifest.beat_markers.len(),
        narration_ducking: loaded.manifest.narration_ducking.is_some(),
        audio_mastering: loaded.manifest.audio_mastering.is_some(),
    };
    if tool_version >= (0, 2, 20)
        && (report.mixed_media.still_events != expected_mixed_media.still_events
            || report.mixed_media.video_events != expected_mixed_media.video_events
            || report.mixed_media.animation_events != expected_mixed_media.animation_events
            || report.mixed_media.sprite_animation_events
                != expected_mixed_media.sprite_animation_events
            || report.mixed_media.audio_events != expected_mixed_media.audio_events
            || report.mixed_media.beat_markers != expected_mixed_media.beat_markers
            || report.mixed_media.narration_ducking != expected_mixed_media.narration_ducking
            || report.mixed_media.audio_mastering != expected_mixed_media.audio_mastering)
    {
        bail!("mixed-media lineage does not match manifest");
    }
    if tool_version >= (0, 2, 27)
        && report.mixed_media.sprite_camera_tracks != expected_mixed_media.sprite_camera_tracks
    {
        bail!("sprite camera lineage does not match manifest");
    }
    if tool_version >= (0, 2, 35)
        && (actual_sprite_input_ids != expected_sprite_input_ids
            || actual_sprite_input_count != expected_sprite_input_ids.len()
            || report.mixed_media.sprite_asset_occurrences
                != expected_mixed_media.sprite_asset_occurrences
            || report.mixed_media.sprite_unique_asset_inputs
                != expected_mixed_media.sprite_unique_asset_inputs)
    {
        bail!("sprite render-locality lineage does not match artifact inputs");
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
                && report.motion.interpolation == "cubic"
                && matches!(report.motion.curve.as_str(), "ease-in-out" | "linear") =>
        {
            MotionQuality::Smooth
        }
        "legacy"
            if report.motion.backend == "ffmpeg-zoompan"
                && report.motion.interpolation == "zoompan-default"
                && report.motion.curve == "legacy-linear" =>
        {
            MotionQuality::Legacy
        }
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
    let caption_inputs = report
        .inputs
        .iter()
        .filter(|input| input.kind == "captions")
        .collect::<Vec<_>>();
    let presentation_inputs = report
        .inputs
        .iter()
        .filter(|input| input.kind == "caption-presentation")
        .collect::<Vec<_>>();
    let cues = if let Some(lineage) = &report.captions {
        if caption_inputs.len() != 1 {
            bail!("captioned artifact must contain exactly one captions input");
        }
        let captions = caption_inputs[0];
        if lineage.schema != caption_presentation::CAPTION_LINEAGE_SCHEMA
            || lineage.captions_sha256 != captions.sha256
            || !lineage.passed
        {
            bail!("caption lineage does not match the artifact captions input");
        }
        let presentation = match &lineage.presentation_input_sha256 {
            Some(expected) => {
                if presentation_inputs.len() != 1 || presentation_inputs[0].sha256 != *expected {
                    bail!("caption presentation lineage does not match artifact inputs");
                }
                Some(Path::new(&presentation_inputs[0].path))
            }
            None => {
                if !presentation_inputs.is_empty() {
                    bail!("artifact has an unexpected caption presentation input");
                }
                None
            }
        };
        let reconstructed = caption_presentation::prepare(
            &loaded,
            CaptionPresentationOptions {
                captions: Path::new(&captions.path),
                presentation,
                profile: CaptionProfile::parse(&lineage.profile)?,
                policy: SpeakerLabelPolicy::parse(&lineage.speaker_label_policy)?,
                reintroduce_after_ms: lineage.speaker_reintroduce_after_ms,
                thresholds: CaptionThresholds {
                    max_chars_per_line: lineage.thresholds.max_chars_per_line,
                    max_lines_per_cue: lineage.thresholds.max_lines_per_cue,
                    max_reading_speed_cps: lineage.thresholds.max_reading_speed_cps,
                    min_duration_ms: lineage.thresholds.min_duration_ms,
                },
                threshold_policy_note: lineage.threshold_policy_note.as_deref(),
                width: report.width,
                height: report.height,
            },
        )?;
        if !caption_lineage_equivalent(&reconstructed, lineage) {
            bail!("caption preflight or presentation lineage is inconsistent");
        }
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
        cues
    } else if tool_version >= (0, 2, 20) {
        if !caption_inputs.is_empty() || !presentation_inputs.is_empty() {
            bail!("caption-free artifact carries unexpected caption inputs");
        }
        Vec::new()
    } else {
        bail!("v0.2.9 through v0.2.19 artifact report has no caption preflight lineage");
    };
    let audio_inputs = report
        .inputs
        .iter()
        .filter(|input| input.kind == "audio")
        .collect::<Vec<_>>();
    let audio_check_inputs = report
        .inputs
        .iter()
        .filter(|input| input.kind == "audio-check-report")
        .collect::<Vec<_>>();
    match &report.audio_quality {
        Some(binding) => {
            if binding.schema != "reel.audio-binding.v0.1"
                || binding.report_schema != AUDIO_CHECK_SCHEMA
                || audio_inputs.len() != 1
                || audio_check_inputs.len() != 1
                || audio_check_inputs[0].sha256 != binding.report_sha256
                || audio_inputs[0].sha256 != binding.audio_sha256
            {
                bail!("audio-quality binding does not match artifact inputs");
            }
            let checked: AudioCheckReport =
                serde_json::from_slice(&fs::read(&audio_check_inputs[0].path)?)
                    .context("bound audio-check report is not valid JSON")?;
            if checked.schema != binding.report_schema
                || checked.profile != binding.profile
                || checked.audio.sha256 != binding.audio_sha256
                || checked.audio.duration_ms.abs_diff(report.duration_ms) > 50
                || !checked.passed
            {
                bail!("bound audio-check evidence is inconsistent");
            }
        }
        None if !audio_check_inputs.is_empty() => {
            bail!("artifact has an unbound audio-check report input");
        }
        None => {}
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
        render_capabilities: report
            .render_environment
            .as_ref()
            .map_or(0, |environment| environment.checks.len()),
        render_environment_fingerprint: report
            .render_environment
            .as_ref()
            .map(|environment| environment.fingerprint_sha256.clone()),
        passed: true,
    })
}

fn caption_lineage_equivalent(left: &CaptionLineage, right: &CaptionLineage) -> bool {
    let mut normalized = left.clone();
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9;
    if !close(
        normalized.thresholds.max_reading_speed_cps,
        right.thresholds.max_reading_speed_cps,
    ) || !close(
        normalized.check.thresholds.max_reading_speed_cps,
        right.check.thresholds.max_reading_speed_cps,
    ) || !close(
        normalized.check.max_reading_speed_cps,
        right.check.max_reading_speed_cps,
    ) || normalized.check.violations.len() != right.check.violations.len()
    {
        return false;
    }
    normalized.thresholds.max_reading_speed_cps = right.thresholds.max_reading_speed_cps;
    normalized.check.thresholds.max_reading_speed_cps =
        right.check.thresholds.max_reading_speed_cps;
    normalized.check.max_reading_speed_cps = right.check.max_reading_speed_cps;
    for (left, right) in normalized
        .check
        .violations
        .iter_mut()
        .zip(&right.check.violations)
    {
        if !close(left.measured, right.measured) || !close(left.limit, right.limit) {
            return false;
        }
        left.measured = right.measured;
        left.limit = right.limit;
    }
    normalized == *right
}

pub fn write_animatic_receipt(
    artifact_manifest: impl AsRef<Path>,
    output: impl AsRef<Path>,
) -> Result<AnimaticReceipt> {
    let artifact_manifest = artifact_manifest.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to resolve artifact report {}",
            artifact_manifest.as_ref().display()
        )
    })?;
    let output = output.as_ref();
    if output.exists() {
        bail!(
            "refusing to overwrite existing animatic receipt: {}",
            output.display()
        );
    }
    let check = check_animatic(&artifact_manifest)?;
    let report: AnimaticRenderReport = serde_json::from_slice(&fs::read(&artifact_manifest)?)
        .context("artifact report is not valid JSON")?;
    let mut input_kinds = BTreeMap::new();
    for input in &report.inputs {
        let kind = if matches!(input.kind.as_str(), "still" | "video" | "visual") {
            "visual"
        } else if input.kind == "audio" || input.kind.starts_with("audio-") {
            "audio"
        } else if matches!(input.kind.as_str(), "manifest" | "captions") {
            input.kind.as_str()
        } else {
            "other"
        };
        *input_kinds.entry(kind.to_string()).or_insert(0) += 1;
    }
    let environment = report.render_environment.as_ref();
    let receipt = AnimaticReceipt {
        schema: "reel.animatic-receipt.v0.1".to_string(),
        source_artifact_schema: report.schema,
        source_artifact_sha256: production::sha256_path(&artifact_manifest)?,
        tool_version: report.tool_version,
        output_sha256: check.output_sha256,
        output_bytes: report
            .output_bytes
            .ok_or_else(|| anyhow!("verified artifact report has no output byte length"))?,
        width: check.width,
        height: check.height,
        fps: report.fps,
        duration_ms: check.duration_ms,
        silent: report.silent,
        audio_streams: check.audio_streams,
        caption_cues: check.caption_cues,
        input_kinds,
        motion_backend: report.motion.backend,
        motion_quality: report.motion.quality,
        motion_interpolation: report.motion.interpolation,
        motion_curve: report.motion.curve,
        motion_shots: report.motion.shots.len(),
        motion_safety_passed: report.motion.safety.iter().all(|shot| shot.passed),
        render_transport: environment.map(|value| value.transport.clone()),
        render_environment_fingerprint: environment.map(|value| value.fingerprint_sha256.clone()),
        verified: true,
    };
    validate_animatic_receipt(&receipt)?;
    let output_parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_parent)?;
    let mut temp = Builder::new()
        .prefix(".reel-receipt-")
        .tempfile_in(output_parent)?;
    temp.write_all(&serde_json::to_vec_pretty(&receipt)?)?;
    temp.flush()?;
    temp.persist_noclobber(output).with_context(|| {
        format!(
            "failed to publish animatic receipt atomically: {}",
            output.display()
        )
    })?;
    Ok(receipt)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_animatic_receipt(receipt: &AnimaticReceipt) -> Result<()> {
    if receipt.schema != "reel.animatic-receipt.v0.1" {
        bail!("unsupported animatic receipt schema {}", receipt.schema);
    }
    if receipt.source_artifact_schema != "reel.animatic-artifacts.v0.1" {
        bail!(
            "unsupported source artifact schema {}",
            receipt.source_artifact_schema
        );
    }
    let tool_version = parse_tool_version(&receipt.tool_version)?;
    if tool_version < (0, 2, 6) {
        bail!("animatic receipt tool version predates v0.2.6");
    }
    if !is_sha256(&receipt.source_artifact_sha256)
        || !is_sha256(&receipt.output_sha256)
        || receipt
            .render_environment_fingerprint
            .as_deref()
            .is_none_or(|value| !is_sha256(value))
    {
        bail!("animatic receipt contains an invalid SHA-256 value");
    }
    if receipt.output_bytes == 0
        || receipt.width == 0
        || receipt.height == 0
        || receipt.width % 2 != 0
        || receipt.height % 2 != 0
        || receipt.fps == 0
        || receipt.duration_ms == 0
        || (receipt.caption_cues == 0 && tool_version < (0, 2, 20))
        || receipt.motion_shots == 0
    {
        bail!("animatic receipt contains invalid delivery counts");
    }
    if !receipt.verified || !receipt.motion_safety_passed {
        bail!("animatic receipt does not record successful verification");
    }
    if !matches!(receipt.render_transport.as_deref(), Some("native" | "wsl")) {
        bail!("animatic receipt has an invalid render transport");
    }
    let motion_valid = match receipt.motion_quality.as_str() {
        "smooth" => {
            receipt.motion_backend == "ffmpeg-perspective"
                && receipt.motion_interpolation == "cubic"
                && matches!(receipt.motion_curve.as_str(), "ease-in-out" | "linear")
        }
        "legacy" => {
            receipt.motion_backend == "ffmpeg-zoompan"
                && receipt.motion_interpolation == "zoompan-default"
                && receipt.motion_curve == "legacy-linear"
        }
        _ => false,
    };
    if !motion_valid {
        bail!("animatic receipt has inconsistent motion lineage");
    }
    if receipt.input_kinds.keys().any(|kind| {
        !matches!(
            kind.as_str(),
            "manifest" | "visual" | "audio" | "captions" | "other"
        )
    }) || receipt.input_kinds.get("manifest") != Some(&1)
        || (receipt.caption_cues == 0
            && receipt
                .input_kinds
                .get("captions")
                .copied()
                .unwrap_or_default()
                != 0)
        || (receipt.caption_cues > 0 && receipt.input_kinds.get("captions") != Some(&1))
        || receipt
            .input_kinds
            .get("visual")
            .copied()
            .unwrap_or_default()
            + receipt
                .input_kinds
                .get("other")
                .copied()
                .unwrap_or_default()
            == 0
    {
        bail!("animatic receipt has invalid input-kind counts");
    }
    let audio_inputs = receipt
        .input_kinds
        .get("audio")
        .copied()
        .unwrap_or_default();
    if (receipt.silent && (receipt.audio_streams != 0 || audio_inputs != 0))
        || (!receipt.silent && (receipt.audio_streams != 1 || audio_inputs == 0))
    {
        bail!("animatic receipt audio counts do not match silent mode");
    }
    Ok(())
}

pub fn check_animatic_receipt(
    receipt_path: impl AsRef<Path>,
    video: impl AsRef<Path>,
) -> Result<AnimaticReceiptCheckReport> {
    let receipt_path = receipt_path.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to resolve animatic receipt {}",
            receipt_path.as_ref().display()
        )
    })?;
    let video = video.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to resolve receipt video {}",
            video.as_ref().display()
        )
    })?;
    let receipt: AnimaticReceipt = serde_json::from_slice(&fs::read(&receipt_path)?)
        .context("animatic receipt is not valid strict JSON")?;
    validate_animatic_receipt(&receipt)?;
    let video_sha256 = production::sha256_path(&video)?;
    if video_sha256 != receipt.output_sha256 {
        bail!("video SHA-256 does not match animatic receipt");
    }
    let output_bytes = fs::metadata(&video)?.len();
    if output_bytes != receipt.output_bytes {
        bail!("video byte length does not match animatic receipt");
    }
    let adapter = FfmpegAdapter;
    let probe: ProbeReport = serde_json::from_str(&adapter.ffprobe_json(&video)?)
        .context("ffprobe returned invalid JSON")?;
    let video_streams = probe
        .streams
        .iter()
        .filter(|stream| stream.codec_type.as_deref() == Some("video"))
        .collect::<Vec<_>>();
    if video_streams.len() != 1 {
        bail!("receipt video must contain exactly one video stream");
    }
    let stream = video_streams[0];
    if stream.codec_name.as_deref() != Some("h264") || stream.pix_fmt.as_deref() != Some("yuv420p")
    {
        bail!("receipt video must be H.264 yuv420p");
    }
    if stream.width != Some(receipt.width) || stream.height != Some(receipt.height) {
        bail!("video dimensions do not match animatic receipt");
    }
    let r_fps = fraction(stream.r_frame_rate.as_deref().unwrap_or("0/1"))?;
    let avg_fps = fraction(stream.avg_frame_rate.as_deref().unwrap_or("0/1"))?;
    if (r_fps - f64::from(receipt.fps)).abs() > 0.001
        || (avg_fps - f64::from(receipt.fps)).abs() > 0.001
    {
        bail!("video frame rate does not match animatic receipt");
    }
    let duration_ms = (probe.format.duration.parse::<f64>()? * 1000.0).round() as u64;
    let frame_ms = (1000.0 / f64::from(receipt.fps)).ceil() as u64;
    if duration_ms.abs_diff(receipt.duration_ms) > frame_ms {
        bail!("video duration does not match animatic receipt");
    }
    let audio_streams = probe
        .streams
        .iter()
        .filter(|stream| stream.codec_type.as_deref() == Some("audio"))
        .count();
    if audio_streams != receipt.audio_streams {
        bail!("video audio streams do not match animatic receipt");
    }
    Ok(AnimaticReceiptCheckReport {
        schema: "reel.animatic-receipt-check.v0.1".to_string(),
        receipt_sha256: production::sha256_path(&receipt_path)?,
        video_sha256,
        output_bytes,
        codec: "h264".to_string(),
        pixel_format: "yuv420p".to_string(),
        width: receipt.width,
        height: receipt.height,
        fps: r_fps,
        duration_ms,
        audio_streams,
        passed: true,
    })
}

fn parse_tool_version(value: &str) -> Result<(u64, u64, u64)> {
    let mut parts = value.split('.');
    let major = parts
        .next()
        .ok_or_else(|| anyhow!("invalid artifact tool version {value}"))?
        .parse()?;
    let minor = parts
        .next()
        .ok_or_else(|| anyhow!("invalid artifact tool version {value}"))?
        .parse()?;
    let patch = parts
        .next()
        .ok_or_else(|| anyhow!("invalid artifact tool version {value}"))?
        .parse()?;
    if parts.next().is_some() {
        bail!("invalid artifact tool version {value}");
    }
    Ok((major, minor, patch))
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
    use tempfile::tempdir;

    #[test]
    fn parses_semantic_tool_versions_for_lineage_gates() {
        assert_eq!(parse_tool_version("0.2.4").unwrap(), (0, 2, 4));
        assert_eq!(parse_tool_version("0.2.5").unwrap(), (0, 2, 5));
        assert!(parse_tool_version("0.2").is_err());
        assert!(parse_tool_version("0.2.5.1").is_err());
    }

    #[test]
    fn dry_run_compiles_mixed_media_audio_and_ducking_into_one_graph() {
        let temp = tempdir().unwrap();
        let fixture_root = Path::new("manifests/fixtures/vertical-sound-off")
            .canonicalize()
            .unwrap();
        let mut manifest = production::load(fixture_root.join("manifest.yaml"))
            .unwrap()
            .manifest;
        manifest.shots[0].beat_marker_id = Some("downbeat".to_string());
        manifest.shots[1].media_kind = MediaKind::Video;
        manifest.shots[1].source_in_seconds = 1.25;
        manifest.beat_markers = vec![production::BeatMarker {
            id: "downbeat".to_string(),
            time_seconds: 0.0,
            label: "opening beat".to_string(),
            accent: true,
        }];
        manifest.audio_events = vec![
            production::AudioEvent {
                id: "room".to_string(),
                role: AudioRole::Ambience,
                source: "frame-hook.ppm".to_string(),
                start_seconds: 0.0,
                duration_seconds: Some(6.0),
                source_in_seconds: 0.0,
                gain_db: -10.0,
                loop_source: true,
                fade_in_ms: 100,
                fade_out_ms: 200,
                beat_marker_id: Some("downbeat".to_string()),
            },
            production::AudioEvent {
                id: "voice".to_string(),
                role: AudioRole::Narration,
                source: "frame-landing.ppm".to_string(),
                start_seconds: 0.5,
                duration_seconds: Some(2.0),
                source_in_seconds: 0.0,
                gain_db: 0.0,
                loop_source: false,
                fade_in_ms: 0,
                fade_out_ms: 0,
                beat_marker_id: None,
            },
        ];
        manifest.narration_ducking = Some(production::NarrationDucking {
            threshold: 0.03,
            ratio: 8.0,
            attack_ms: 20,
            release_ms: 300,
        });
        let manifest_path = temp.path().join("manifest.yaml");
        fs::write(&manifest_path, serde_yaml::to_string(&manifest).unwrap()).unwrap();

        let report = render(&AnimaticRenderOptions {
            manifest: manifest_path,
            asset_root: fixture_root.clone(),
            audio: None,
            audio_check_report: None,
            silent: false,
            captions: Some(fixture_root.join("captions.srt")),
            caption_presentation: None,
            caption_profile: CaptionProfile::YoutubeReview,
            speaker_label_policy: SpeakerLabelPolicy::None,
            speaker_reintroduce_after_ms: None,
            caption_thresholds: CaptionThresholds::default(),
            caption_policy_note: None,
            output: temp.path().join("mixed.mp4"),
            width: 1280,
            height: 720,
            fps: 24,
            transition_seconds: 0.0,
            disclosure: "FIXTURE".to_string(),
            motion_quality: MotionQuality::Smooth,
            motion_curve: MotionCurve::EaseInOut,
            encoding_preset: EncodingPreset::Slow,
            dry_run: true,
        })
        .unwrap();

        let command = report.command_arguments.join(" ");
        assert!(command.contains("-ss 1.250"));
        assert!(command.contains("-stream_loop -1"));
        assert!(command.contains("sidechaincompress="));
        assert!(command.contains("adelay=500:all=1"));
        assert!(command.contains("trim=end_frame=144"));
        assert!(command.contains("[finala]"));
        assert_eq!(report.mixed_media.video_events, 1);
        assert_eq!(report.mixed_media.audio_events, 2);
        assert!(report.mixed_media.narration_ducking);
    }

    #[test]
    fn dry_run_compiles_limited_animation_frames_as_one_timed_shot() {
        let temp = tempdir().unwrap();
        let fixture_root = Path::new("manifests/fixtures/vertical-sound-off")
            .canonicalize()
            .unwrap();
        let mut manifest = production::load(fixture_root.join("manifest.yaml"))
            .unwrap()
            .manifest;
        manifest.shots[0].media_kind = MediaKind::Animation;
        manifest.shots[0].visual_asset = None;
        manifest.shots[0].animation = Some(production::AnimationSequence {
            timing_fps: 24,
            frames: vec![
                production::AnimationFrame {
                    asset: "frame-hook.ppm".to_string(),
                    hold_frames: 24,
                    pose: Some("anticipation".to_string()),
                },
                production::AnimationFrame {
                    asset: "frame-landing.ppm".to_string(),
                    hold_frames: 42,
                    pose: Some("impact".to_string()),
                },
            ],
        });
        let manifest_path = temp.path().join("animation.yaml");
        fs::write(&manifest_path, serde_yaml::to_string(&manifest).unwrap()).unwrap();

        let report = render(&AnimaticRenderOptions {
            manifest: manifest_path,
            asset_root: fixture_root,
            audio: None,
            audio_check_report: None,
            silent: true,
            captions: None,
            caption_presentation: None,
            caption_profile: CaptionProfile::YoutubeReview,
            speaker_label_policy: SpeakerLabelPolicy::None,
            speaker_reintroduce_after_ms: None,
            caption_thresholds: CaptionThresholds::default(),
            caption_policy_note: None,
            output: temp.path().join("animation.mp4"),
            width: 1280,
            height: 720,
            fps: 24,
            transition_seconds: 0.0,
            disclosure: "FIXTURE".to_string(),
            motion_quality: MotionQuality::Smooth,
            motion_curve: MotionCurve::EaseInOut,
            encoding_preset: EncodingPreset::Slow,
            dry_run: true,
        })
        .unwrap();

        assert_eq!(report.mixed_media.animation_events, 1);
        assert_eq!(
            report
                .inputs
                .iter()
                .filter(|input| input.kind == "animation-frame")
                .count(),
            2
        );
        let command = report.command_arguments.join(" ");
        assert!(command.contains("-f concat -safe 0"));
    }

    #[test]
    fn dry_run_compiles_keyframed_sprite_motion_and_pose_swaps() {
        let temp = tempdir().unwrap();
        let fixture_root = Path::new("manifests/fixtures/vertical-sound-off")
            .canonicalize()
            .unwrap();
        let mut manifest = production::load(fixture_root.join("manifest.yaml"))
            .unwrap()
            .manifest;
        manifest.shots[0].media_kind = MediaKind::SpriteAnimation;
        manifest.shots[0].visual_asset = None;
        manifest.shots[0].sprite_animation = Some(production::SpriteAnimation {
            background: "frame-hook.ppm".to_string(),
            timing_fps: 24,
            intentional_holds: Vec::new(),
            emissions: vec![production::SpriteEmission {
                id: "contact-snow".to_string(),
                asset: "frame-hook.ppm".to_string(),
                parent: "puck".to_string(),
                frame: 12,
                duration_frames: 6,
                offset_x: 0.25,
                offset_y: 0.1,
                width: 0.05,
                end_width: Some(0.07),
                drift_x: -0.02,
                drift_y: 0.01,
                rotation_degrees: 0.0,
                end_rotation_degrees: 30.0,
                fade_out_frames: 3,
                z_index: 20,
                anchor_x: 0.5,
                anchor_y: 0.5,
            }],
            camera: vec![
                production::SpriteCameraKeyframe {
                    frame: 0,
                    center_x: 0.5,
                    center_y: 0.5,
                    zoom: 1.0,
                    curve_to_next: production::SpriteCameraCurve::EaseInOut,
                },
                production::SpriteCameraKeyframe {
                    frame: 47,
                    center_x: 0.65,
                    center_y: 0.45,
                    zoom: 1.5,
                    curve_to_next: production::SpriteCameraCurve::Linear,
                },
            ],
            sprites: vec![production::SpriteTrack {
                id: "puck".to_string(),
                z_index: 10,
                visible_start_frame: Some(4),
                visible_end_frame: Some(40),
                anchor_x: Some(0.5),
                anchor_y: Some(0.75),
                movement: production::SpriteMovement::Stepped,
                movement_steps: Some(3),
                parent: None,
                position_space: production::SpritePositionSpace::Canvas,
                keyframes: vec![
                    production::SpriteKeyframe {
                        frame: 0,
                        asset: "frame-landing.ppm".to_string(),
                        z_index: None,
                        x: 0.2,
                        y: 0.6,
                        width: 0.1,
                    },
                    production::SpriteKeyframe {
                        frame: 33,
                        asset: "frame-landing.ppm".to_string(),
                        z_index: Some(20),
                        x: 0.8,
                        y: 0.4,
                        width: 0.05,
                    },
                ],
            }],
        });
        let manifest_path = temp.path().join("sprites.yaml");
        fs::write(&manifest_path, serde_yaml::to_string(&manifest).unwrap()).unwrap();

        let report = render(&AnimaticRenderOptions {
            manifest: manifest_path,
            asset_root: fixture_root,
            audio: None,
            audio_check_report: None,
            silent: true,
            captions: None,
            caption_presentation: None,
            caption_profile: CaptionProfile::YoutubeReview,
            speaker_label_policy: SpeakerLabelPolicy::None,
            speaker_reintroduce_after_ms: None,
            caption_thresholds: CaptionThresholds::default(),
            caption_policy_note: None,
            output: temp.path().join("sprites.mp4"),
            width: 1280,
            height: 720,
            fps: 24,
            transition_seconds: 0.0,
            disclosure: "FIXTURE".to_string(),
            motion_quality: MotionQuality::Smooth,
            motion_curve: MotionCurve::EaseInOut,
            encoding_preset: EncodingPreset::Slow,
            dry_run: true,
        })
        .unwrap();

        assert_eq!(report.mixed_media.sprite_animation_events, 1);
        assert_eq!(report.mixed_media.sprite_asset_occurrences, 3);
        assert_eq!(report.mixed_media.sprite_unique_asset_inputs, 2);
        assert_eq!(
            report
                .inputs
                .iter()
                .filter(|input| input.kind == "sprite-pose")
                .count(),
            2
        );
        assert_eq!(
            report
                .inputs
                .iter()
                .filter(|input| input.kind == "sprite-emission")
                .count(),
            1
        );
        let command = report.command_arguments.join(" ");
        assert!(command.contains("overlay=x="));
        assert!(command.contains("eval=frame:flags=lanczos"));
        assert!(command.contains("floor(((t-0.000000000)/1.375000000)*3)/3"));
        assert!(command.contains("-w*0.500000000"));
        assert!(command.contains("-h*0.750000000"));
        assert!(command.contains("gte(t,0.166666667)*lt(t,1.375000000)"));
        assert!(command.contains("zoompan=z="));
        assert!(command.contains("between(on\\,0\\,47)"));
        assert!(command.contains("max(0\\,min(iw-iw/zoom"));
        assert!(command.contains("rotate=angle="));
        assert!(command.contains("fade=t=out"));
        assert!(command.contains("split=2"));
        assert!(command.contains("trim=start=0.000000000:end=1.375000000"));
        assert!(command.contains("setsar=1,settb=AVTB,trim=duration="));
    }

    #[test]
    fn motion_check_treats_authored_sprite_and_camera_changes_as_motion() {
        let moving = production::SpriteAnimation {
            background: "rink.ppm".to_string(),
            timing_fps: 24,
            intentional_holds: Vec::new(),
            emissions: Vec::new(),
            camera: vec![
                production::SpriteCameraKeyframe {
                    frame: 0,
                    center_x: 0.5,
                    center_y: 0.5,
                    zoom: 1.0,
                    curve_to_next: production::SpriteCameraCurve::EaseInOut,
                },
                production::SpriteCameraKeyframe {
                    frame: 23,
                    center_x: 0.6,
                    center_y: 0.5,
                    zoom: 1.5,
                    curve_to_next: production::SpriteCameraCurve::Linear,
                },
            ],
            sprites: vec![production::SpriteTrack {
                id: "puck".to_string(),
                z_index: 10,
                visible_start_frame: None,
                visible_end_frame: None,
                anchor_x: Some(0.5),
                anchor_y: Some(0.5),
                movement: production::SpriteMovement::Linear,
                movement_steps: None,
                parent: None,
                position_space: production::SpritePositionSpace::Canvas,
                keyframes: vec![
                    production::SpriteKeyframe {
                        frame: 0,
                        asset: "puck.png".to_string(),
                        z_index: None,
                        x: 0.2,
                        y: 0.6,
                        width: 0.03,
                    },
                    production::SpriteKeyframe {
                        frame: 23,
                        asset: "puck.png".to_string(),
                        z_index: None,
                        x: 0.8,
                        y: 0.4,
                        width: 0.02,
                    },
                ],
            }],
        };
        assert!(sprite_animation_has_authored_motion(&moving));

        let stationary = production::SpriteAnimation {
            camera: vec![moving.camera[0].clone(), moving.camera[0].clone()],
            sprites: vec![production::SpriteTrack {
                keyframes: vec![
                    moving.sprites[0].keyframes[0].clone(),
                    moving.sprites[0].keyframes[0].clone(),
                ],
                ..moving.sprites[0].clone()
            }],
            ..moving
        };
        assert!(!sprite_animation_has_authored_motion(&stationary));
    }

    #[test]
    fn parent_width_tracks_resolve_offsets_against_parent_geometry() {
        let parent = production::SpriteTrack {
            id: "skater".to_string(),
            z_index: 10,
            visible_start_frame: None,
            visible_end_frame: None,
            anchor_x: Some(0.5),
            anchor_y: Some(0.5),
            movement: production::SpriteMovement::Linear,
            movement_steps: None,
            parent: None,
            position_space: production::SpritePositionSpace::Canvas,
            keyframes: vec![
                production::SpriteKeyframe {
                    frame: 0,
                    asset: "skater-a.png".to_string(),
                    z_index: None,
                    x: 0.4,
                    y: 0.5,
                    width: 0.2,
                },
                production::SpriteKeyframe {
                    frame: 10,
                    asset: "skater-b.png".to_string(),
                    z_index: None,
                    x: 0.6,
                    y: 0.5,
                    width: 0.2,
                },
            ],
        };
        let child = production::SpriteTrack {
            id: "puck".to_string(),
            z_index: 20,
            visible_start_frame: None,
            visible_end_frame: None,
            anchor_x: Some(0.5),
            anchor_y: Some(0.5),
            movement: production::SpriteMovement::Linear,
            movement_steps: None,
            parent: Some("skater".to_string()),
            position_space: production::SpritePositionSpace::ParentWidth,
            keyframes: vec![
                production::SpriteKeyframe {
                    frame: 0,
                    asset: "puck.png".to_string(),
                    z_index: None,
                    x: 0.5,
                    y: 0.25,
                    width: 0.01,
                },
                production::SpriteKeyframe {
                    frame: 10,
                    asset: "puck.png".to_string(),
                    z_index: None,
                    x: 0.5,
                    y: 0.25,
                    width: 0.01,
                },
            ],
        };
        let animation = production::SpriteAnimation {
            background: "rink.png".to_string(),
            timing_fps: 24,
            sprites: vec![parent, child.clone()],
            camera: Vec::new(),
            intentional_holds: Vec::new(),
            emissions: Vec::new(),
        };
        let resolved = resolved_sprite_keyframes(&animation, &child, 16.0 / 9.0);
        assert!((resolved[0].x - 0.5).abs() < 1e-9);
        assert!((resolved[1].x - 0.7).abs() < 1e-9);
        assert!((resolved[0].y - 0.5888888889).abs() < 1e-9);
    }

    #[test]
    fn intentional_hold_mask_maps_authored_frames_to_video_transitions() {
        let animation = production::SpriteAnimation {
            background: "rink.png".to_string(),
            timing_fps: 24,
            sprites: Vec::new(),
            camera: Vec::new(),
            intentional_holds: vec![
                production::SpriteIntentionalHold {
                    start_frame: 0,
                    end_frame: 8,
                    reason: "anticipation".to_string(),
                },
                production::SpriteIntentionalHold {
                    start_frame: 12,
                    end_frame: 15,
                    reason: "concealment".to_string(),
                },
            ],
            emissions: Vec::new(),
        };
        let mask = intentional_hold_mask(&animation, 32, 1.375);
        assert_eq!(mask.iter().filter(|declared| **declared).count(), 11);
        assert!(mask[0]);
        assert!(mask[7]);
        assert!(!mask[8]);
        assert!(!mask[11]);
        assert!(mask[12]);
        assert!(mask[14]);
        assert!(!mask[15]);
    }

    #[test]
    #[ignore = "requires external FFmpeg/ffprobe and renders a six-second mixed-media fixture"]
    fn real_mixed_media_render_verifies_audio_event_lineage() {
        let temp = tempdir().unwrap();
        let fixture_root = Path::new("manifests/fixtures/vertical-sound-off")
            .canonicalize()
            .unwrap();
        fs::copy(
            fixture_root.join("frame-hook.ppm"),
            temp.path().join("still.ppm"),
        )
        .unwrap();
        fs::copy(
            fixture_root.join("captions.srt"),
            temp.path().join("captions.srt"),
        )
        .unwrap();
        let adapter = FfmpegAdapter;
        let video = temp.path().join("clip.mp4");
        adapter
            .run_ffmpeg(
                &[
                    "-y".to_string(),
                    "-loop".to_string(),
                    "1".to_string(),
                    "-i".to_string(),
                    adapter
                        .path_argument(&fixture_root.join("frame-landing.ppm"))
                        .unwrap(),
                    "-t".to_string(),
                    "4".to_string(),
                    "-vf".to_string(),
                    "scale=640:360".to_string(),
                    "-r".to_string(),
                    "24".to_string(),
                    "-pix_fmt".to_string(),
                    "yuv420p".to_string(),
                    adapter.path_argument(&video).unwrap(),
                ],
                &[],
            )
            .unwrap();
        for (name, frequency, duration) in [("bed.wav", 220, 6), ("voice.wav", 660, 2)] {
            let output = temp.path().join(name);
            adapter
                .run_ffmpeg(
                    &[
                        "-y".to_string(),
                        "-f".to_string(),
                        "lavfi".to_string(),
                        "-i".to_string(),
                        format!("sine=frequency={frequency}:sample_rate=48000"),
                        "-t".to_string(),
                        duration.to_string(),
                        adapter.path_argument(&output).unwrap(),
                    ],
                    &[],
                )
                .unwrap();
        }

        let mut manifest = production::load(fixture_root.join("manifest.yaml"))
            .unwrap()
            .manifest;
        manifest.shots[0].visual_asset = Some("still.ppm".to_string());
        manifest.shots[1].visual_asset = Some("clip.mp4".to_string());
        manifest.shots[1].media_kind = MediaKind::Video;
        manifest.shots[1].source_in_seconds = 0.2;
        manifest.audio_events = vec![
            production::AudioEvent {
                id: "bed".to_string(),
                role: AudioRole::Music,
                source: "bed.wav".to_string(),
                start_seconds: 0.0,
                duration_seconds: Some(6.0),
                source_in_seconds: 0.0,
                gain_db: -12.0,
                loop_source: false,
                fade_in_ms: 100,
                fade_out_ms: 200,
                beat_marker_id: None,
            },
            production::AudioEvent {
                id: "voice".to_string(),
                role: AudioRole::Narration,
                source: "voice.wav".to_string(),
                start_seconds: 0.5,
                duration_seconds: Some(2.0),
                source_in_seconds: 0.0,
                gain_db: -3.0,
                loop_source: false,
                fade_in_ms: 0,
                fade_out_ms: 0,
                beat_marker_id: None,
            },
        ];
        manifest.narration_ducking = Some(production::NarrationDucking {
            threshold: 0.03,
            ratio: 8.0,
            attack_ms: 20,
            release_ms: 300,
        });
        let manifest_path = temp.path().join("manifest.yaml");
        fs::write(&manifest_path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
        let output = temp.path().join("mixed.mp4");
        let report = render(&AnimaticRenderOptions {
            manifest: manifest_path,
            asset_root: temp.path().to_path_buf(),
            audio: None,
            audio_check_report: None,
            silent: false,
            captions: Some(temp.path().join("captions.srt")),
            caption_presentation: None,
            caption_profile: CaptionProfile::YoutubeReview,
            speaker_label_policy: SpeakerLabelPolicy::None,
            speaker_reintroduce_after_ms: None,
            caption_thresholds: CaptionThresholds::default(),
            caption_policy_note: None,
            output,
            width: 1280,
            height: 720,
            fps: 24,
            transition_seconds: 0.0,
            disclosure: "FIXTURE".to_string(),
            motion_quality: MotionQuality::Legacy,
            motion_curve: MotionCurve::EaseInOut,
            encoding_preset: EncodingPreset::Slow,
            dry_run: false,
        })
        .unwrap();
        let checked = check_animatic(&report.artifact_manifest).unwrap();
        assert!(checked.passed);
        assert_eq!(checked.audio_streams, 1);
        assert_eq!(report.mixed_media.video_events, 1);
        assert_eq!(report.mixed_media.audio_events, 2);
    }

    #[test]
    fn shareable_receipt_schema_has_no_path_bearing_fields() {
        let receipt = AnimaticReceipt {
            schema: "reel.animatic-receipt.v0.1".to_string(),
            source_artifact_schema: "reel.animatic-artifacts.v0.1".to_string(),
            source_artifact_sha256: "a".repeat(64),
            tool_version: "0.2.6".to_string(),
            output_sha256: "b".repeat(64),
            output_bytes: 42,
            width: 1280,
            height: 720,
            fps: 24,
            duration_ms: 20_000,
            silent: true,
            audio_streams: 0,
            caption_cues: 1,
            input_kinds: BTreeMap::from([
                ("captions".to_string(), 1),
                ("manifest".to_string(), 1),
                ("visual".to_string(), 1),
            ]),
            motion_backend: "ffmpeg-perspective".to_string(),
            motion_quality: "smooth".to_string(),
            motion_interpolation: "cubic".to_string(),
            motion_curve: "ease-in-out".to_string(),
            motion_shots: 1,
            motion_safety_passed: true,
            render_transport: Some("wsl".to_string()),
            render_environment_fingerprint: Some("c".repeat(64)),
            verified: true,
        };
        let json = serde_json::to_string(&receipt).unwrap();

        assert!(!json.contains("C:\\"));
        assert!(!json.contains("/home/"));
        assert!(!json.contains("\"path\""));
        assert!(!json.contains("artifact_manifest"));
        assert!(!json.contains("\"inputs\""));
        validate_animatic_receipt(&receipt).expect("receipt validates");

        let mut with_path: serde_json::Value = serde_json::from_str(&json).unwrap();
        with_path["path"] = serde_json::Value::String(r"C:\private\frame.png".to_string());
        assert!(
            serde_json::from_value::<AnimaticReceipt>(with_path).is_err(),
            "unknown path field must be rejected"
        );

        let mut bad_hash = receipt;
        bad_hash.output_sha256 = "NOT-A-HASH".to_string();
        assert!(validate_animatic_receipt(&bad_hash).is_err());
    }
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
    fn punch_treatments_use_a_deliberate_twenty_percent_scale_change() {
        let punch_in = motion_filter(
            "punch-in",
            0.6,
            24,
            1280,
            720,
            MotionQuality::Smooth,
            MotionCurve::EaseInOut,
        );
        let punch_out = motion_filter(
            "punch-out",
            0.6,
            24,
            1280,
            720,
            MotionQuality::Legacy,
            MotionCurve::EaseInOut,
        );
        assert!(punch_in.contains("1+0.20*"));
        assert!(punch_out.contains("1.20"));

        let edge = Shot {
            id: "unsafe-punch".to_string(),
            motion: "punch-in".to_string(),
            focal_point: Some(FocalPoint { x: 0.01, y: 0.5 }),
            ..Shot::default()
        };
        assert!(!safety_report(&edge).passed);
    }

    #[test]
    fn trailer_motion_treatments_are_fast_and_crop_safe() {
        let slam = motion_filter(
            "slam-in",
            0.7,
            24,
            1280,
            720,
            MotionQuality::Smooth,
            MotionCurve::EaseOut,
        );
        let whip = motion_filter(
            "whip-right",
            0.7,
            24,
            1280,
            720,
            MotionQuality::Smooth,
            MotionCurve::EaseOut,
        );
        assert!(slam.contains("1+0.28*"));
        assert!(slam.contains("pow(1-"));
        assert!(whip.contains("1.12"));
        assert!(whip.contains("pow(1-"));

        let edge = Shot {
            id: "unsafe-whip".to_string(),
            motion: "whip-right".to_string(),
            focal_point: Some(FocalPoint { x: 0.01, y: 0.5 }),
            ..Shot::default()
        };
        assert!(!safety_report(&edge).passed);
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
