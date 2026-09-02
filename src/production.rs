use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use sha2::{Digest, Sha256};

pub const MANIFEST_VERSION: &str = "reel.manifest.v0.2";
pub const CONFORM_SCHEMA: &str = "reel.conform.v0.1";
pub const PROVIDER_PACKAGE_SCHEMA: &str = "reel.provider-package.v0.1";

fn default_manifest_version() -> String {
    MANIFEST_VERSION.to_string()
}

fn default_profile() -> String {
    "animatic".to_string()
}

fn default_timing_status() -> TimingStatus {
    TimingStatus::Conformed
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TimingStatus {
    Untimed,
    Guide,
    #[default]
    Conformed,
    Locked,
}

impl TimingStatus {
    pub fn is_timed(self) -> bool {
        self != Self::Untimed
    }

    pub fn allows_preview(self) -> bool {
        self != Self::Untimed
    }

    pub fn allows_delivery(self) -> bool {
        matches!(self, Self::Conformed | Self::Locked)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Untimed => "untimed",
            Self::Guide => "guide",
            Self::Conformed => "conformed",
            Self::Locked => "locked",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VisualAssetStatus {
    PlannedUnrendered,
    #[serde(alias = "candidate-unreviewed")]
    Candidate,
    Selected,
    Approved,
    Missing,
}

impl VisualAssetStatus {
    fn is_selected(self) -> bool {
        matches!(self, Self::Selected | Self::Approved)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::PlannedUnrendered => "planned-unrendered",
            Self::Candidate => "candidate",
            Self::Selected => "selected",
            Self::Approved => "approved",
            Self::Missing => "missing",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SourceScenario {
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub source_commit: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Platform {
    pub name: String,
    pub aspect_ratio: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_duration_seconds: Option<f64>,
    #[serde(default)]
    pub sound_optional: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Scene {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub purpose: String,
    #[serde(default)]
    pub story_beat: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub characters: Vec<String>,
    #[serde(default)]
    pub continuity_notes: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct FocalPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ProtectedRegion {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Shot {
    pub id: String,
    pub scene_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub camera: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub visual_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_asset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_asset_status: Option<VisualAssetStatus>,
    #[serde(default)]
    pub render_from_prompt: bool,
    #[serde(default)]
    pub media_kind: MediaKind,
    #[serde(default)]
    pub visual_fit: VisualFit,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<AnimationSequence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite_animation: Option<SpriteAnimation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_track: Option<StillCameraTrack>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effect_passes: Vec<EffectPass>,
    #[serde(default)]
    pub source_in_seconds: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat_marker_id: Option<String>,
    #[serde(default)]
    pub motion: String,
    #[serde(default)]
    pub style_constraints: Vec<String>,
    #[serde(default)]
    pub transition_out: String,
    #[serde(default)]
    pub narration_cue_ids: Vec<String>,
    #[serde(default)]
    pub allocation_weight: f64,
    #[serde(default)]
    pub fixed_hold_ms: u64,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focal_point: Option<FocalPoint>,
    #[serde(default)]
    pub protected_regions: Vec<ProtectedRegion>,
    #[serde(default)]
    pub depth_layers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_position: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eye_line: Option<String>,
    #[serde(default)]
    pub audio: Value,
    #[serde(default)]
    pub captions: Value,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EffectPass {
    pub id: String,
    pub color: EffectAsset,
    pub matte: EffectAsset,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occlusion_matte: Option<EffectAsset>,
    pub alpha_mode: String,
    pub composite_operator: String,
    pub color_space: String,
    pub alpha_mode_detail: String,
    pub timing_fps: u32,
    pub duration_frames: u32,
    pub placement: EffectPlacement,
    pub visible_start_frame: u32,
    pub visible_end_frame: u32,
    #[serde(default)]
    pub z_index: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EffectAsset {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EffectPlacement {
    pub space: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MediaKind {
    #[default]
    Still,
    Video,
    Animation,
    SpriteAnimation,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VisualFit {
    #[default]
    Cover,
    Contain,
}

impl VisualFit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cover => "cover",
            Self::Contain => "contain",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AnimationSequence {
    pub timing_fps: u32,
    pub frames: Vec<AnimationFrame>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AnimationFrame {
    pub asset: String,
    pub hold_frames: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pose: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SpriteAnimation {
    pub background: String,
    pub timing_fps: u32,
    pub sprites: Vec<SpriteTrack>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub camera: Vec<SpriteCameraKeyframe>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intentional_holds: Vec<SpriteIntentionalHold>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emissions: Vec<SpriteEmission>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SpriteIntentionalHold {
    pub start_frame: u32,
    pub end_frame: u32,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SpriteEmission {
    pub id: String,
    pub asset: String,
    pub parent: String,
    pub frame: u32,
    pub duration_frames: u32,
    pub offset_x: f64,
    pub offset_y: f64,
    pub width: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_width: Option<f64>,
    #[serde(default)]
    pub drift_x: f64,
    #[serde(default)]
    pub drift_y: f64,
    #[serde(default)]
    pub rotation_degrees: f64,
    #[serde(default)]
    pub end_rotation_degrees: f64,
    #[serde(default)]
    pub fade_out_frames: u32,
    #[serde(default)]
    pub z_index: i32,
    #[serde(default = "default_sprite_anchor")]
    pub anchor_x: f64,
    #[serde(default = "default_sprite_anchor")]
    pub anchor_y: f64,
}

fn default_sprite_anchor() -> f64 {
    0.5
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SpriteCameraKeyframe {
    pub frame: u32,
    pub center_x: f64,
    pub center_y: f64,
    pub zoom: f64,
    #[serde(default)]
    pub curve_to_next: SpriteCameraCurve,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StillCameraTrack {
    pub timing_fps: u32,
    pub keyframes: Vec<SpriteCameraKeyframe>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<StillCameraGeometry>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StillCameraGeometry {
    pub source_width: u32,
    pub source_height: u32,
    pub canvas_width: u32,
    pub canvas_height: u32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpriteCameraCurve {
    #[default]
    Linear,
    EaseInOut,
    EaseOut,
    HoldThenBurst,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SpriteTrack {
    pub id: String,
    #[serde(default)]
    pub z_index: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_start_frame: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_end_frame: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_y: Option<f64>,
    #[serde(default)]
    pub movement: SpriteMovement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub movement_steps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default)]
    pub position_space: SpritePositionSpace,
    pub keyframes: Vec<SpriteKeyframe>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpritePositionSpace {
    #[default]
    Canvas,
    ParentWidth,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpriteMovement {
    #[default]
    Linear,
    Stepped,
    Hold,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SpriteKeyframe {
    pub frame: u32,
    pub asset: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z_index: Option<i32>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioRole {
    Music,
    Ambience,
    Effect,
    Narration,
    Dialogue,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GainCurve {
    Hold,
    Linear,
    Smooth,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GainAutomationPoint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat_marker_id: Option<String>,
    pub gain_db: f64,
    pub curve: GainCurve,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AudioEvent {
    pub id: String,
    pub role: AudioRole,
    pub source: String,
    pub start_seconds: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub source_in_seconds: f64,
    #[serde(default)]
    pub gain_db: f64,
    #[serde(default)]
    pub loop_source: bool,
    #[serde(default)]
    pub fade_in_ms: u64,
    #[serde(default)]
    pub fade_out_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat_marker_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gain_automation: Vec<GainAutomationPoint>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BeatMarker {
    pub id: String,
    pub time_seconds: f64,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub accent: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScoreOriginalityPolicy {
    #[default]
    OriginalOnly,
    Licensed,
    TempReview,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ScoreInstrument {
    pub family: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub timbre: String,
    #[serde(default)]
    pub articulations: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ScoreMotif {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub instruments: Vec<String>,
    #[serde(default)]
    pub recurrence_notes: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScoreSyncKind {
    Downbeat,
    Accent,
    Break,
    Swell,
    Cadence,
    Transition,
    PictureHit,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ScoreSyncPoint {
    pub id: String,
    pub time_seconds: f64,
    pub kind: ScoreSyncKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat_marker_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emphasis: Option<f64>,
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ScoreCue {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub chapter: String,
    pub narrative_function: String,
    #[serde(default)]
    pub mood_from: String,
    #[serde(default)]
    pub mood_to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub energy_from: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub energy_to: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tempo_bpm: Option<f64>,
    #[serde(default)]
    pub meter: String,
    #[serde(default)]
    pub style_tags: Vec<String>,
    #[serde(default)]
    pub instruments: Vec<ScoreInstrument>,
    #[serde(default)]
    pub motif_ids: Vec<String>,
    #[serde(default)]
    pub transition_in: String,
    #[serde(default)]
    pub transition_out: String,
    #[serde(default)]
    pub montage_intent: String,
    #[serde(default)]
    pub picture_notes: Vec<String>,
    #[serde(default)]
    pub sync_points: Vec<ScoreSyncPoint>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ScoreDirection {
    #[serde(default)]
    pub originality_policy: ScoreOriginalityPolicy,
    pub creative_brief: String,
    #[serde(default)]
    pub global_instruments: Vec<ScoreInstrument>,
    #[serde(default)]
    pub motifs: Vec<ScoreMotif>,
    #[serde(default)]
    pub avoid: Vec<String>,
    #[serde(default)]
    pub cues: Vec<ScoreCue>,
}

fn default_ducking_threshold() -> f64 {
    0.03
}

fn default_ducking_ratio() -> f64 {
    8.0
}

fn default_ducking_attack_ms() -> u64 {
    20
}

fn default_ducking_release_ms() -> u64 {
    300
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NarrationDucking {
    #[serde(default = "default_ducking_threshold")]
    pub threshold: f64,
    #[serde(default = "default_ducking_ratio")]
    pub ratio: f64,
    #[serde(default = "default_ducking_attack_ms")]
    pub attack_ms: u64,
    #[serde(default = "default_ducking_release_ms")]
    pub release_ms: u64,
}

fn default_ducking_max_reduction_db() -> f64 {
    6.0
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DynamicEqPolicy {
    pub frequency_hz: f64,
    pub q: f64,
    pub max_cut_db: f64,
    pub attack_ms: u64,
    pub release_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AudioDuckingPolicy {
    pub id: String,
    pub detector_roles: Vec<AudioRole>,
    pub target_roles: Vec<AudioRole>,
    #[serde(default = "default_ducking_threshold")]
    pub threshold: f64,
    #[serde(default = "default_ducking_ratio")]
    pub ratio: f64,
    #[serde(default = "default_ducking_max_reduction_db")]
    pub max_reduction_db: f64,
    #[serde(default = "default_ducking_attack_ms")]
    pub attack_ms: u64,
    #[serde(default = "default_ducking_release_ms")]
    pub release_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_eq: Option<DynamicEqPolicy>,
}

fn default_master_lufs() -> f64 {
    -18.0
}

fn default_master_lra() -> f64 {
    11.0
}

fn default_master_true_peak() -> f64 {
    -2.0
}

fn default_master_limiter() -> f64 {
    0.88
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AudioMastering {
    #[serde(default = "default_master_lufs")]
    pub integrated_lufs: f64,
    #[serde(default = "default_master_lra")]
    pub loudness_range_lu: f64,
    #[serde(default = "default_master_true_peak")]
    pub true_peak_dbfs: f64,
    #[serde(default = "default_master_limiter")]
    pub limiter: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AudioReviewPolicy {
    pub id: String,
    pub dialogue_loudness_target_lufs: f64,
    pub dialogue_loudness_tolerance_lu: f64,
    pub minimum_speech_to_background_margin_db: f64,
    pub speech_activity_threshold_dbfs: f64,
    pub maximum_mono_loss_db: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Export {
    pub id: String,
    pub filename: String,
    pub aspect_ratio: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Speaker {
    pub id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub pronunciation_profile: String,
    #[serde(default)]
    pub performance_direction: String,
    #[serde(default)]
    pub approval_reference: String,
    #[serde(default)]
    pub asset_kind: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct NarrationCue {
    pub id: String,
    pub speaker_id: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub text_reference: String,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub shot_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_reference: Option<String>,
    #[serde(default)]
    pub pause_policy: String,
    #[serde(default)]
    pub invented: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ProtectedPause {
    pub id: String,
    pub after_cue_id: String,
    pub duration_ms: u64,
    #[serde(default = "default_true")]
    pub locked: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SourceRange {
    pub id: String,
    pub start: u64,
    pub end: u64,
    #[serde(default)]
    pub label: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Omission {
    pub id: String,
    pub start: u64,
    pub end: u64,
    pub bridge: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ReferenceAsset {
    pub id: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub local_path: String,
    pub provider_transfer: TransferPolicy,
    #[serde(default)]
    pub approval_reference: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferPolicy {
    #[default]
    Forbidden,
    ApprovalRequired,
    Approved,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ContinuityEntity {
    pub id: String,
    #[serde(default)]
    pub age_at_scene: String,
    #[serde(default)]
    pub observations: Vec<String>,
    #[serde(default)]
    pub confidence: String,
    #[serde(default)]
    pub provenance: String,
    #[serde(default)]
    pub human_confirmation_status: String,
    #[serde(default)]
    pub reference_assets: Vec<ReferenceAsset>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ContinuityRegistry {
    #[serde(default)]
    pub entities: Vec<ContinuityEntity>,
    #[serde(default)]
    pub canon_constraints: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct VariantLineage {
    #[serde(default)]
    pub parent_manifest: String,
    #[serde(default)]
    pub root_work: String,
    #[serde(default)]
    pub scene_key: String,
    #[serde(default)]
    pub transformation_reason: String,
    #[serde(default)]
    pub changed_dimensions: Vec<String>,
    #[serde(default)]
    pub review_candidate: bool,
    #[serde(default)]
    pub principal_approved: bool,
    #[serde(default)]
    pub created_unix: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PrincipalFinding {
    pub principal: String,
    pub artifact: String,
    pub finding: String,
    #[serde(default)]
    pub decision_reference: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ReviewState {
    #[serde(default)]
    pub required_roles: Vec<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub principal_findings: Vec<PrincipalFinding>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct QualityControls {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_uninterrupted_hold_seconds: Option<f64>,
    #[serde(default)]
    pub require_focal_points: bool,
    #[serde(default)]
    pub require_protected_regions: bool,
    #[serde(default)]
    pub no_lip_sync: bool,
    #[serde(default)]
    pub ab_outputs: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ProviderHandoff {
    #[serde(default)]
    pub asset_ids: Vec<String>,
    #[serde(default)]
    pub text_fields: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProductionManifest {
    #[serde(default = "default_manifest_version", alias = "schema")]
    pub manifest_version: String,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default = "default_timing_status")]
    pub timing_status: TimingStatus,
    pub work: String,
    pub title: String,
    #[serde(default)]
    pub source_scenario: SourceScenario,
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub audience: Value,
    #[serde(default)]
    pub platforms: Vec<Platform>,
    #[serde(default)]
    pub continuity: ContinuityRegistry,
    #[serde(default)]
    pub scenes: Vec<Scene>,
    #[serde(default)]
    pub shots: Vec<Shot>,
    #[serde(default)]
    pub speakers: Vec<Speaker>,
    #[serde(default)]
    pub narration_cues: Vec<NarrationCue>,
    #[serde(default)]
    pub protected_pauses: Vec<ProtectedPause>,
    #[serde(default)]
    pub audio_events: Vec<AudioEvent>,
    #[serde(default)]
    pub beat_markers: Vec<BeatMarker>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narration_ducking: Option<NarrationDucking>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_ducking: Vec<AudioDuckingPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_mastering: Option<AudioMastering>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_review_policy: Option<AudioReviewPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<ScoreDirection>,
    #[serde(default)]
    pub source_ranges: Vec<SourceRange>,
    #[serde(default)]
    pub omissions: Vec<Omission>,
    #[serde(default)]
    pub audio: Value,
    #[serde(default)]
    pub captions: Value,
    #[serde(default)]
    pub renderer_assumptions: Value,
    #[serde(default)]
    pub exports: Vec<Export>,
    #[serde(default)]
    pub review: ReviewState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<VariantLineage>,
    #[serde(default)]
    pub quality_controls: QualityControls,
    #[serde(default)]
    pub provider_handoff: ProviderHandoff,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug)]
pub struct LoadedProductionManifest {
    pub path: PathBuf,
    pub manifest: ProductionManifest,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProductionValidationReport {
    pub manifest: String,
    pub version: String,
    pub profile: String,
    pub timing_status: String,
    pub scenes: usize,
    pub shots: usize,
    pub speakers: usize,
    pub narration_cues: usize,
    pub still_events: usize,
    pub video_events: usize,
    pub animation_events: usize,
    pub sprite_animation_events: usize,
    pub audio_events: usize,
    pub beat_markers: usize,
    pub score_cues: usize,
    pub narration_ducking: bool,
    pub audio_ducking: usize,
    pub audio_mastering: bool,
    pub duration_ms: Option<u64>,
    pub timing_ready: bool,
    pub generation_ready: bool,
    pub asset_ready: bool,
    pub preview_ready: bool,
    pub delivery_ready: bool,
    pub asset_status_counts: BTreeMap<String, usize>,
    pub semantic_blockers: Vec<String>,
    pub gated_commands: Vec<String>,
    pub warnings: Vec<String>,
}

struct AssetReadiness {
    generation_ready: bool,
    asset_ready: bool,
    status_counts: BTreeMap<String, usize>,
    semantic_blockers: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProductionPlan {
    pub work: String,
    pub timing_status: String,
    pub scenes: Vec<PlanScene>,
    pub gated_commands: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScorePlan {
    pub schema: String,
    pub work: String,
    pub timing_status: String,
    pub duration_ms: Option<u64>,
    pub originality_policy: ScoreOriginalityPolicy,
    pub creative_brief: String,
    pub global_instruments: Vec<ScoreInstrument>,
    pub motifs: Vec<ScoreMotif>,
    pub avoid: Vec<String>,
    pub cues: Vec<ScoreCue>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlanScene {
    pub id: String,
    pub purpose: String,
    pub story_beat: String,
    pub duration_ms: Option<u64>,
    pub shots: Vec<PlanShot>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlanShot {
    pub id: String,
    pub order: usize,
    pub camera: String,
    pub action: String,
    pub speaker_ids: Vec<String>,
    pub source_refs: Vec<String>,
    pub start_ms: Option<u64>,
    pub duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CueMeasurements {
    pub cues: Vec<CueMeasurement>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CueMeasurement {
    pub cue_id: String,
    pub duration_ms: u64,
    #[serde(default)]
    pub head_silence_ms: u64,
    #[serde(default)]
    pub tail_silence_ms: u64,
    #[serde(default)]
    pub audio_path: String,
    #[serde(default)]
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConformLineage {
    pub schema: String,
    pub generated_unix: u64,
    pub tool_version: String,
    pub input_manifest: String,
    pub input_manifest_sha256: String,
    pub cue_measurements: String,
    pub cue_measurements_sha256: String,
    pub audio_inputs: Vec<HashedInput>,
    pub speaker_tempos_percent: BTreeMap<String, u32>,
    pub output_manifest: String,
    pub output_manifest_sha256: String,
    pub captions: String,
    pub captions_sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct HashedInput {
    pub id: String,
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConformReport {
    pub schema: String,
    pub work: String,
    pub timing_status: String,
    pub duration_ms: u64,
    pub scene_count: usize,
    pub shot_count: usize,
    pub cue_count: usize,
    pub protected_pause_count: usize,
    pub packet: String,
    pub manifest: String,
    pub captions: String,
    pub lineage: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CoverageReport {
    pub work: String,
    pub selected_ranges: Vec<SourceRange>,
    pub omissions: Vec<Omission>,
    pub spoken_cues: usize,
    pub attributed_cues: usize,
    pub invented_cues: Vec<String>,
    pub unattributed_cues: Vec<String>,
    pub invalid_references: Vec<String>,
    pub uncovered_units: Vec<u64>,
    pub complete: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProviderPackage {
    pub schema: String,
    pub work: String,
    pub generated_unix: u64,
    pub approved_text_observations: BTreeMap<String, Vec<String>>,
    pub prompts: BTreeMap<String, String>,
    pub requested_assets: Vec<ProviderAsset>,
    pub outbound_text_fields: Vec<String>,
    pub blocked: bool,
    pub blockers: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProviderAsset {
    pub id: String,
    pub sha256: String,
    pub provider_transfer: TransferPolicy,
    pub approval_reference: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewSelectionReport {
    pub root: String,
    pub groups: BTreeMap<String, ReviewSelection>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewSelection {
    pub latest_review_candidate: Option<String>,
    pub principal_approved: Vec<String>,
    pub candidates: Vec<String>,
    pub findings: Vec<PrincipalFinding>,
}

#[derive(Clone, Debug, Serialize)]
pub struct QualityReport {
    pub work: String,
    pub warnings: Vec<QualityWarning>,
    pub narration_only_output: bool,
    pub effects_music_output: bool,
    pub passed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct QualityWarning {
    pub shot_id: String,
    pub code: String,
    pub message: String,
}

pub fn is_production_manifest(path: impl AsRef<Path>) -> Result<bool> {
    let text = fs::read_to_string(path.as_ref())?;
    let raw: Value = serde_yaml::from_str(&text)?;
    let Some(top) = raw.as_mapping() else {
        return Ok(false);
    };
    let timing = top.contains_key(Value::String("timing_status".to_string()));
    let profile = top.contains_key(Value::String("profile".to_string()));
    let version = top
        .get(Value::String("manifest_version".to_string()))
        .or_else(|| top.get(Value::String("schema".to_string())))
        .and_then(Value::as_str)
        .unwrap_or_default();
    Ok(timing || profile || version == MANIFEST_VERSION)
}

pub fn load(path: impl AsRef<Path>) -> Result<LoadedProductionManifest> {
    let path = path.as_ref();
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let manifest: ProductionManifest = serde_yaml::from_slice(&bytes)
        .with_context(|| format!("failed to parse production manifest: {}", path.display()))?;
    Ok(LoadedProductionManifest {
        path: path.to_path_buf(),
        manifest,
        bytes,
    })
}

pub fn validate(loaded: &LoadedProductionManifest) -> Result<ProductionValidationReport> {
    let manifest = &loaded.manifest;
    if manifest.manifest_version != MANIFEST_VERSION {
        bail!(
            "production manifest version must be {MANIFEST_VERSION}, got {} (run `reel migrate`)",
            manifest.manifest_version
        );
    }
    if !matches!(
        manifest.profile.as_str(),
        "animatic" | "voice-audition" | "production-package"
    ) {
        bail!(
            "unsupported production profile {}; expected animatic, voice-audition, or production-package",
            manifest.profile
        );
    }
    require_nonempty("work", &manifest.work)?;
    require_nonempty("title", &manifest.title)?;
    crate::continuity::resolve_for_manifest(&loaded.path, manifest)?;
    if manifest.scenes.is_empty() || manifest.shots.is_empty() {
        bail!("production manifest requires at least one scene and one ordered shot");
    }
    unique("scene", manifest.scenes.iter().map(|item| item.id.as_str()))?;
    unique("shot", manifest.shots.iter().map(|item| item.id.as_str()))?;
    unique(
        "speaker",
        manifest.speakers.iter().map(|item| item.id.as_str()),
    )?;
    unique(
        "narration cue",
        manifest.narration_cues.iter().map(|item| item.id.as_str()),
    )?;
    unique(
        "source range",
        manifest.source_ranges.iter().map(|item| item.id.as_str()),
    )?;
    unique(
        "audio event",
        manifest.audio_events.iter().map(|item| item.id.as_str()),
    )?;
    unique(
        "beat marker",
        manifest.beat_markers.iter().map(|item| item.id.as_str()),
    )?;
    if let Some(score) = &manifest.score {
        unique("score cue", score.cues.iter().map(|item| item.id.as_str()))?;
        unique(
            "score motif",
            score.motifs.iter().map(|item| item.id.as_str()),
        )?;
    }
    let scene_ids = manifest
        .scenes
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let shot_ids = manifest
        .shots
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let speaker_ids = manifest
        .speakers
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let cue_ids = manifest
        .narration_cues
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    for shot in &manifest.shots {
        if !scene_ids.contains(shot.scene_id.as_str()) {
            bail!(
                "shot {} references unknown scene {}",
                shot.id,
                shot.scene_id
            );
        }
        for cue in &shot.narration_cue_ids {
            if !cue_ids.contains(cue.as_str()) {
                bail!("shot {} references unknown narration cue {cue}", shot.id);
            }
        }
        if shot.source_in_seconds < 0.0 || !shot.source_in_seconds.is_finite() {
            bail!(
                "shot {} source_in_seconds must be finite and non-negative",
                shot.id
            );
        }
    }
    for cue in &manifest.narration_cues {
        if !speaker_ids.contains(cue.speaker_id.as_str()) {
            bail!(
                "cue {} references unknown speaker {}",
                cue.id,
                cue.speaker_id
            );
        }
        for shot in &cue.shot_ids {
            if !shot_ids.contains(shot.as_str()) {
                bail!("cue {} references unknown shot {shot}", cue.id);
            }
        }
    }
    for pause in &manifest.protected_pauses {
        if !cue_ids.contains(pause.after_cue_id.as_str()) {
            bail!(
                "protected pause {} references unknown cue {}",
                pause.id,
                pause.after_cue_id
            );
        }
        if pause.duration_ms == 0 {
            bail!(
                "protected pause {} must have positive duration_ms",
                pause.id
            );
        }
    }

    let mut warnings = Vec::new();
    let duration_ms = if manifest.timing_status.is_timed() {
        Some(validate_timeline(manifest)?)
    } else {
        for scene in &manifest.scenes {
            if scene.duration_seconds.is_some() {
                warnings.push(format!(
                    "untimed scene {} carries provisional timing",
                    scene.id
                ));
            }
        }
        for shot in &manifest.shots {
            if shot.start_seconds.is_some() || shot.duration_seconds.is_some() {
                warnings.push(format!(
                    "untimed shot {} carries provisional timing",
                    shot.id
                ));
            }
        }
        None
    };
    validate_mixed_media(manifest, duration_ms)?;
    validate_score_direction(manifest, duration_ms)?;
    let timing_ready = manifest.timing_status.allows_preview();
    let asset_readiness = evaluate_asset_readiness(manifest)?;
    let generation_ready = asset_readiness.generation_ready;
    let asset_ready = asset_readiness.asset_ready;
    let asset_status_counts = asset_readiness.status_counts;
    let semantic_blockers = asset_readiness.semantic_blockers;
    let preview_ready = timing_ready && asset_ready;
    let delivery_ready = manifest.timing_status.allows_delivery() && asset_ready;
    let mut gated_commands = if manifest.timing_status == TimingStatus::Untimed {
        vec![
            "scene-preview".to_string(),
            "scene-previews".to_string(),
            "work-preview".to_string(),
            "animatic-render".to_string(),
            "caption-export".to_string(),
            "artifact-manifest".to_string(),
            "delivery".to_string(),
        ]
    } else if manifest.timing_status == TimingStatus::Guide {
        vec!["delivery".to_string()]
    } else {
        Vec::new()
    };
    if !asset_ready {
        for command in [
            "scene-preview",
            "scene-previews",
            "work-preview",
            "animatic-render",
            "artifact-manifest",
            "delivery",
        ] {
            if !gated_commands.iter().any(|gated| gated == command) {
                gated_commands.push(command.to_string());
            }
        }
    }
    Ok(ProductionValidationReport {
        manifest: loaded.path.display().to_string(),
        version: manifest.manifest_version.clone(),
        profile: manifest.profile.clone(),
        timing_status: manifest.timing_status.as_str().to_string(),
        scenes: manifest.scenes.len(),
        shots: manifest.shots.len(),
        speakers: manifest.speakers.len(),
        narration_cues: manifest.narration_cues.len(),
        still_events: manifest
            .shots
            .iter()
            .filter(|shot| shot.media_kind == MediaKind::Still)
            .count(),
        video_events: manifest
            .shots
            .iter()
            .filter(|shot| shot.media_kind == MediaKind::Video)
            .count(),
        animation_events: manifest
            .shots
            .iter()
            .filter(|shot| shot.media_kind == MediaKind::Animation)
            .count(),
        sprite_animation_events: manifest
            .shots
            .iter()
            .filter(|shot| shot.media_kind == MediaKind::SpriteAnimation)
            .count(),
        audio_events: manifest.audio_events.len(),
        beat_markers: manifest.beat_markers.len(),
        score_cues: manifest.score.as_ref().map_or(0, |score| score.cues.len()),
        narration_ducking: manifest.narration_ducking.is_some(),
        audio_ducking: manifest.audio_ducking.len(),
        audio_mastering: manifest.audio_mastering.is_some(),
        duration_ms,
        timing_ready,
        generation_ready,
        asset_ready,
        preview_ready,
        delivery_ready,
        asset_status_counts,
        semantic_blockers,
        gated_commands,
        warnings,
    })
}

fn evaluate_asset_readiness(manifest: &ProductionManifest) -> Result<AssetReadiness> {
    let mut counts = BTreeMap::new();
    let mut blockers = Vec::new();
    let mut generation_blockers = 0_usize;
    let mut asset_blockers = 0_usize;

    for shot in &manifest.shots {
        let has_asset = shot_has_materialized_picture(shot);
        if shot.render_from_prompt && shot.visual_prompt.trim().is_empty() {
            bail!(
                "shot {} enables render_from_prompt but has no visual_prompt",
                shot.id
            );
        }
        if shot
            .visual_asset_status
            .is_some_and(VisualAssetStatus::is_selected)
            && !has_asset
        {
            bail!(
                "shot {} declares selected media without a materialized picture source",
                shot.id
            );
        }

        let status = shot.visual_asset_status.map_or_else(
            || {
                if has_asset {
                    "selected"
                } else if shot.render_from_prompt {
                    "prompt-renderable"
                } else {
                    "missing"
                }
            },
            VisualAssetStatus::as_str,
        );
        *counts.entry(status.to_string()).or_insert(0) += 1;

        let selected = shot
            .visual_asset_status
            .map_or(has_asset, VisualAssetStatus::is_selected);
        if !selected {
            asset_blockers += 1;
            if !shot.render_from_prompt {
                generation_blockers += 1;
            }
        }
    }

    for (status, count) in &counts {
        if matches!(
            status.as_str(),
            "planned-unrendered" | "candidate" | "missing"
        ) {
            blockers.push(format!("{count} shot(s) have asset status {status}"));
        }
    }
    let prompt_only = asset_blockers.saturating_sub(generation_blockers);
    if prompt_only > 0 {
        blockers.push(format!(
            "{prompt_only} shot(s) require prompt rendering before picture preview"
        ));
    }
    Ok(AssetReadiness {
        generation_ready: generation_blockers == 0,
        asset_ready: asset_blockers == 0,
        status_counts: counts,
        semantic_blockers: blockers,
    })
}

fn shot_has_materialized_picture(shot: &Shot) -> bool {
    match shot.media_kind {
        MediaKind::Still | MediaKind::Video => shot
            .visual_asset
            .as_deref()
            .is_some_and(|asset| !asset.trim().is_empty()),
        MediaKind::Animation => shot
            .animation
            .as_ref()
            .is_some_and(|animation| !animation.frames.is_empty()),
        MediaKind::SpriteAnimation => shot.sprite_animation.is_some(),
    }
}

pub fn score_plan(loaded: &LoadedProductionManifest) -> Result<ScorePlan> {
    let report = validate(loaded)?;
    let score = loaded
        .manifest
        .score
        .as_ref()
        .ok_or_else(|| anyhow!("score-plan requires manifest score direction"))?;
    Ok(ScorePlan {
        schema: "reel.score-plan.v0.1".to_string(),
        work: loaded.manifest.work.clone(),
        timing_status: loaded.manifest.timing_status.as_str().to_string(),
        duration_ms: report.duration_ms,
        originality_policy: score.originality_policy,
        creative_brief: score.creative_brief.clone(),
        global_instruments: score.global_instruments.clone(),
        motifs: score.motifs.clone(),
        avoid: score.avoid.clone(),
        cues: score.cues.clone(),
    })
}

fn validate_score_instrument(instrument: &ScoreInstrument, context: &str) -> Result<()> {
    require_nonempty(&format!("{context} instrument family"), &instrument.family)?;
    if instrument.role.trim().is_empty() && instrument.timbre.trim().is_empty() {
        bail!(
            "{context} instrument {} requires role or timbre",
            instrument.family
        );
    }
    for articulation in &instrument.articulations {
        require_nonempty(&format!("{context} articulation"), articulation)?;
    }
    Ok(())
}

fn validate_score_direction(manifest: &ProductionManifest, duration_ms: Option<u64>) -> Result<()> {
    let Some(score) = &manifest.score else {
        return Ok(());
    };
    require_nonempty("score creative_brief", &score.creative_brief)?;
    if score.cues.is_empty() {
        bail!("score requires at least one cue");
    }
    if score.global_instruments.is_empty()
        && score.cues.iter().all(|cue| cue.instruments.is_empty())
    {
        bail!("score requires at least one global or cue instrument direction");
    }
    for instrument in &score.global_instruments {
        validate_score_instrument(instrument, "global score")?;
    }
    for item in &score.avoid {
        require_nonempty("score avoid note", item)?;
    }

    let motif_ids = score
        .motifs
        .iter()
        .map(|motif| motif.id.as_str())
        .collect::<HashSet<_>>();
    for motif in &score.motifs {
        require_nonempty("score motif description", &motif.description)?;
        for instrument in &motif.instruments {
            require_nonempty(&format!("score motif {} instrument", motif.id), instrument)?;
        }
    }

    let marker_times = manifest
        .beat_markers
        .iter()
        .map(|marker| (marker.id.as_str(), seconds_to_ms(marker.time_seconds)))
        .collect::<HashMap<_, _>>();
    for cue in &score.cues {
        require_nonempty(
            &format!("score cue {} narrative_function", cue.id),
            &cue.narrative_function,
        )?;
        if cue.start_seconds.is_some() != cue.duration_seconds.is_some() {
            bail!(
                "score cue {} must declare start_seconds and duration_seconds together",
                cue.id
            );
        }
        if manifest.timing_status.is_timed()
            && (cue.start_seconds.is_none() || cue.duration_seconds.is_none())
        {
            bail!(
                "timed score cue {} requires start_seconds and duration_seconds",
                cue.id
            );
        }
        let cue_range = cue
            .start_seconds
            .zip(cue.duration_seconds)
            .map(|(start, duration)| -> Result<(u64, u64)> {
                if start < 0.0 || !start.is_finite() {
                    bail!(
                        "score cue {} start_seconds must be finite and non-negative",
                        cue.id
                    );
                }
                let cue_duration_ms =
                    required_ms(Some(duration), &format!("score cue {} duration", cue.id))?;
                let start_ms = seconds_to_ms(start);
                if cue_duration_ms == 0 {
                    bail!("score cue {} duration must be positive", cue.id);
                }
                if cue_duration_ms.checked_add(start_ms).is_none()
                    || cue_duration_ms
                        .checked_add(start_ms)
                        .is_some_and(|end| duration_ms.is_some_and(|timeline| end > timeline + 1))
                {
                    bail!(
                        "score cue {} extends beyond the production timeline",
                        cue.id
                    );
                }
                Ok((start_ms, cue_duration_ms))
            })
            .transpose()?;

        for (label, energy) in [
            ("energy_from", cue.energy_from),
            ("energy_to", cue.energy_to),
        ] {
            if energy.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
                bail!("score cue {} {label} must be between 0 and 1", cue.id);
            }
        }
        if cue
            .tempo_bpm
            .is_some_and(|tempo| !tempo.is_finite() || !(20.0..=320.0).contains(&tempo))
        {
            bail!("score cue {} tempo_bpm must be between 20 and 320", cue.id);
        }
        for instrument in &cue.instruments {
            validate_score_instrument(instrument, &format!("score cue {}", cue.id))?;
        }
        for motif_id in &cue.motif_ids {
            if !motif_ids.contains(motif_id.as_str()) {
                bail!("score cue {} references unknown motif {}", cue.id, motif_id);
            }
        }
        for note in &cue.picture_notes {
            require_nonempty(&format!("score cue {} picture note", cue.id), note)?;
        }
        unique(
            &format!("score cue {} sync point", cue.id),
            cue.sync_points.iter().map(|point| point.id.as_str()),
        )?;
        for point in &cue.sync_points {
            if point.time_seconds < 0.0 || !point.time_seconds.is_finite() {
                bail!(
                    "score sync point {} time_seconds must be finite and non-negative",
                    point.id
                );
            }
            let point_ms = seconds_to_ms(point.time_seconds);
            if duration_ms.is_some_and(|timeline| point_ms > timeline) {
                bail!(
                    "score sync point {} falls outside the production timeline",
                    point.id
                );
            }
            if let Some((start_ms, cue_duration_ms)) = cue_range {
                if point_ms < start_ms || point_ms > start_ms + cue_duration_ms {
                    bail!("score sync point {} falls outside cue {}", point.id, cue.id);
                }
            }
            if point
                .emphasis
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            {
                bail!(
                    "score sync point {} emphasis must be between 0 and 1",
                    point.id
                );
            }
            if let Some(marker_id) = &point.beat_marker_id {
                let marker_ms = marker_times.get(marker_id.as_str()).ok_or_else(|| {
                    anyhow!(
                        "score sync point {} references unknown beat marker {}",
                        point.id,
                        marker_id
                    )
                })?;
                if point_ms.abs_diff(*marker_ms) > 1 {
                    bail!(
                        "score sync point {} time does not align with beat marker {}",
                        point.id,
                        marker_id
                    );
                }
            }
        }
    }
    Ok(())
}

fn camera_keyframes_have_motion(keyframes: &[SpriteCameraKeyframe]) -> bool {
    keyframes.windows(2).any(|pair| {
        let from = &pair[0];
        let to = &pair[1];
        (from.center_x - to.center_x).abs() > f64::EPSILON
            || (from.center_y - to.center_y).abs() > f64::EPSILON
            || (from.zoom - to.zoom).abs() > f64::EPSILON
    })
}

fn validate_camera_keyframes(
    shot_kind: &str,
    shot_id: &str,
    field: &str,
    keyframes: &[SpriteCameraKeyframe],
    total_frames: Option<u64>,
) -> Result<()> {
    if keyframes[0].frame != 0 {
        bail!("{shot_kind} shot {shot_id} {field} must begin at frame 0");
    }
    let mut prior = None;
    for keyframe in keyframes {
        if prior.is_some_and(|frame| keyframe.frame <= frame) {
            bail!("{shot_kind} shot {shot_id} {field} keyframe frames must increase");
        }
        if total_frames.is_some_and(|frames| u64::from(keyframe.frame) >= frames) {
            bail!(
                "{shot_kind} shot {shot_id} {field} keyframe {} falls outside the shot",
                keyframe.frame
            );
        }
        if !keyframe.center_x.is_finite()
            || !keyframe.center_y.is_finite()
            || !keyframe.zoom.is_finite()
            || !(0.0..=1.0).contains(&keyframe.center_x)
            || !(0.0..=1.0).contains(&keyframe.center_y)
            || !(1.0..=4.0).contains(&keyframe.zoom)
        {
            bail!("{shot_kind} shot {shot_id} {field} keyframe geometry is invalid");
        }
        prior = Some(keyframe.frame);
    }
    Ok(())
}

fn validate_mixed_media(manifest: &ProductionManifest, duration_ms: Option<u64>) -> Result<()> {
    let marker_times = manifest
        .beat_markers
        .iter()
        .map(|marker| {
            if marker.time_seconds < 0.0 || !marker.time_seconds.is_finite() {
                bail!(
                    "beat marker {} time_seconds must be finite and non-negative",
                    marker.id
                );
            }
            let time_ms = seconds_to_ms(marker.time_seconds);
            if duration_ms.is_some_and(|duration| time_ms > duration) {
                bail!(
                    "beat marker {} falls outside the production timeline",
                    marker.id
                );
            }
            Ok((marker.id.as_str(), time_ms))
        })
        .collect::<Result<HashMap<_, _>>>()?;

    for shot in &manifest.shots {
        unique(
            "effect pass id",
            shot.effect_passes.iter().map(|effect| effect.id.as_str()),
        )?;
        for effect in &shot.effect_passes {
            require_nonempty("effect pass id", &effect.id)?;
            for (role, asset) in [("color", &effect.color), ("matte", &effect.matte)] {
                require_nonempty(&format!("effect pass {role} path"), &asset.path)?;
                if !is_sha256(&asset.sha256) {
                    bail!("effect pass {} {role} sha256 is invalid", effect.id);
                }
            }
            if let Some(asset) = &effect.occlusion_matte {
                require_nonempty("effect pass occlusion matte path", &asset.path)?;
                if !is_sha256(&asset.sha256) {
                    bail!(
                        "effect pass {} occlusion matte sha256 is invalid",
                        effect.id
                    );
                }
            }
            if effect.alpha_mode != "separate-matte"
                || effect.composite_operator != "over"
                || effect.color_space != "srgb"
                || effect.alpha_mode_detail != "straight"
            {
                bail!(
                    "effect pass {} uses an unsupported compositing contract",
                    effect.id
                );
            }
            if effect.timing_fps == 0
                || effect.timing_fps > 60
                || effect.duration_frames == 0
                || effect.visible_start_frame > effect.visible_end_frame
                || effect.visible_end_frame >= effect.duration_frames
            {
                bail!("effect pass {} has invalid timing or visibility", effect.id);
            }
            let expected_frames = shot
                .duration_seconds
                .map(|seconds| (seconds * f64::from(effect.timing_fps)).round() as u32);
            if expected_frames.is_some_and(|frames| frames != effect.duration_frames) {
                bail!(
                    "effect pass {} duration differs from shot {}",
                    effect.id,
                    shot.id
                );
            }
            if effect.placement.space != "normalized"
                || !effect.placement.x.is_finite()
                || !effect.placement.y.is_finite()
                || !effect.placement.width.is_finite()
                || !effect.placement.height.is_finite()
                || effect.placement.x < 0.0
                || effect.placement.y < 0.0
                || effect.placement.width <= 0.0
                || effect.placement.height <= 0.0
                || effect.placement.x + effect.placement.width > 1.0
                || effect.placement.y + effect.placement.height > 1.0
            {
                bail!("effect pass {} normalized placement is invalid", effect.id);
            }
        }
        if !shot.effect_passes.is_empty()
            && !matches!(shot.media_kind, MediaKind::Still | MediaKind::Video)
        {
            bail!(
                "effect passes currently require a still or video shot: {}",
                shot.id
            );
        }
        if shot.visual_fit == VisualFit::Contain {
            match shot.media_kind {
                MediaKind::Still
                    if shot.camera_track.is_none()
                        && !matches!(shot.motion.as_str(), "hold" | "hold-dark") =>
                {
                    bail!(
                        "still shot {} visual_fit contain requires camera_track, motion hold, or motion hold-dark",
                        shot.id
                    );
                }
                MediaKind::Still
                    if shot.camera_track.as_ref().is_some_and(|track| {
                        track.geometry.is_none()
                            && (shot.focal_point.is_some() || !shot.protected_regions.is_empty())
                    }) =>
                {
                    bail!(
                        "still shot {} contained camera_track requires geometry to map source-space focal_point or protected_regions",
                        shot.id
                    );
                }
                MediaKind::SpriteAnimation => {
                    bail!(
                        "sprite-animation shot {} cannot declare visual_fit contain",
                        shot.id
                    );
                }
                _ => {}
            }
        }
        if shot.media_kind != MediaKind::Video && shot.source_in_seconds != 0.0 {
            bail!(
                "{} shot {} cannot declare source_in_seconds",
                match shot.media_kind {
                    MediaKind::Still => "still",
                    MediaKind::Video => "video",
                    MediaKind::Animation => "animation",
                    MediaKind::SpriteAnimation => "sprite-animation",
                },
                shot.id
            );
        }
        match (&shot.media_kind, &shot.camera_track) {
            (MediaKind::Still, Some(track)) => {
                if !shot.motion.is_empty() {
                    bail!(
                        "still shot {} cannot combine camera_track with motion",
                        shot.id
                    );
                }
                if track.timing_fps == 0 || track.timing_fps > 60 {
                    bail!(
                        "still shot {} camera_track timing_fps must be between 1 and 60",
                        shot.id
                    );
                }
                if track.keyframes.len() < 2 {
                    bail!(
                        "still shot {} camera_track must declare at least two keyframes",
                        shot.id
                    );
                }
                if let Some(geometry) = &track.geometry {
                    if shot.visual_fit != VisualFit::Contain {
                        bail!(
                            "still shot {} camera_track geometry requires visual_fit contain",
                            shot.id
                        );
                    }
                    if geometry.source_width == 0
                        || geometry.source_height == 0
                        || geometry.canvas_width == 0
                        || geometry.canvas_height == 0
                    {
                        bail!(
                            "still shot {} camera_track geometry dimensions must be positive",
                            shot.id
                        );
                    }
                }
                let total_frames = shot
                    .duration_seconds
                    .map(|duration| (duration * f64::from(track.timing_fps)).round() as u64);
                validate_camera_keyframes(
                    "still",
                    &shot.id,
                    "camera_track",
                    &track.keyframes,
                    total_frames,
                )?;
                if !camera_keyframes_have_motion(&track.keyframes) {
                    bail!(
                        "still shot {} camera_track must change center or zoom",
                        shot.id
                    );
                }
            }
            (_, Some(_)) => bail!(
                "{} shot {} cannot declare camera_track",
                match shot.media_kind {
                    MediaKind::Still => "still",
                    MediaKind::Video => "video",
                    MediaKind::Animation => "animation",
                    MediaKind::SpriteAnimation => "sprite-animation",
                },
                shot.id
            ),
            (_, None) => {}
        }
        match (&shot.media_kind, &shot.animation) {
            (MediaKind::Animation, Some(animation)) => {
                if shot.visual_asset.is_some() {
                    bail!(
                        "animation shot {} uses animation.frames instead of visual_asset",
                        shot.id
                    );
                }
                if animation.timing_fps == 0 || animation.timing_fps > 60 {
                    bail!(
                        "animation shot {} timing_fps must be between 1 and 60",
                        shot.id
                    );
                }
                if animation.frames.is_empty() {
                    bail!("animation shot {} must declare at least one frame", shot.id);
                }
                let mut held_frames = 0_u64;
                for (index, frame) in animation.frames.iter().enumerate() {
                    require_nonempty("animation frame asset", &frame.asset)?;
                    if frame.hold_frames == 0 {
                        bail!(
                            "animation shot {} frame {} hold_frames must be positive",
                            shot.id,
                            index + 1
                        );
                    }
                    held_frames = held_frames
                        .checked_add(u64::from(frame.hold_frames))
                        .ok_or_else(|| {
                            anyhow!("animation shot {} frame holds overflow", shot.id)
                        })?;
                }
                if let Some(duration) = shot.duration_seconds {
                    let sequence_duration = held_frames as f64 / animation.timing_fps as f64;
                    let tolerance = 1.0 / animation.timing_fps as f64 / 2.0;
                    if (sequence_duration - duration).abs() > tolerance {
                        bail!(
                            "animation shot {} frame holds total {:.3}s but shot duration is {:.3}s",
                            shot.id,
                            sequence_duration,
                            duration
                        );
                    }
                }
            }
            (MediaKind::Animation, None) => {
                bail!("animation shot {} has no animation sequence", shot.id)
            }
            (_, Some(_)) => bail!(
                "{} shot {} cannot declare an animation sequence",
                match shot.media_kind {
                    MediaKind::Still => "still",
                    MediaKind::Video => "video",
                    MediaKind::Animation => "animation",
                    MediaKind::SpriteAnimation => "sprite-animation",
                },
                shot.id
            ),
            (_, None) => {}
        }
        match (&shot.media_kind, &shot.sprite_animation) {
            (MediaKind::SpriteAnimation, Some(animation)) => {
                if shot.visual_asset.is_some() || shot.animation.is_some() {
                    bail!(
                        "sprite-animation shot {} uses sprite_animation instead of visual_asset or animation",
                        shot.id
                    );
                }
                require_nonempty("sprite-animation background", &animation.background)?;
                if animation.timing_fps == 0 || animation.timing_fps > 60 {
                    bail!(
                        "sprite-animation shot {} timing_fps must be between 1 and 60",
                        shot.id
                    );
                }
                if animation.sprites.is_empty() {
                    bail!(
                        "sprite-animation shot {} must declare at least one sprite track",
                        shot.id
                    );
                }
                unique(
                    "sprite track id",
                    animation.sprites.iter().map(|track| track.id.as_str()),
                )?;
                let sprite_ids = animation
                    .sprites
                    .iter()
                    .map(|track| track.id.as_str())
                    .collect::<BTreeSet<_>>();
                unique(
                    "sprite emission id",
                    animation
                        .emissions
                        .iter()
                        .map(|emission| emission.id.as_str()),
                )?;
                let total_frames = shot
                    .duration_seconds
                    .map(|duration| (duration * animation.timing_fps as f64).round() as u64);
                let mut prior_hold_end = None;
                let mut intentional_hold_transitions = 0_u64;
                for hold in &animation.intentional_holds {
                    require_nonempty("sprite intentional hold reason", &hold.reason)?;
                    if hold.start_frame >= hold.end_frame {
                        bail!(
                            "sprite-animation shot {} intentional hold must span at least one transition",
                            shot.id
                        );
                    }
                    if prior_hold_end.is_some_and(|end| hold.start_frame < end) {
                        bail!(
                            "sprite-animation shot {} intentional holds must be ordered and non-overlapping",
                            shot.id
                        );
                    }
                    if total_frames.is_some_and(|frames| u64::from(hold.end_frame) >= frames) {
                        bail!(
                            "sprite-animation shot {} intentional hold end frame {} falls outside the shot",
                            shot.id,
                            hold.end_frame
                        );
                    }
                    intentional_hold_transitions += u64::from(hold.end_frame - hold.start_frame);
                    prior_hold_end = Some(hold.end_frame);
                }
                if total_frames.is_some_and(|frames| {
                    let transitions = frames.saturating_sub(1);
                    transitions > 0 && intentional_hold_transitions * 2 > transitions
                }) {
                    bail!(
                        "sprite-animation shot {} intentional holds cannot exceed half of shot transitions",
                        shot.id
                    );
                }
                if animation.emissions.len() > 64 {
                    bail!(
                        "sprite-animation shot {} cannot declare more than 64 emissions",
                        shot.id
                    );
                }
                for emission in &animation.emissions {
                    require_nonempty("sprite emission asset", &emission.asset)?;
                    if !sprite_ids.contains(emission.parent.as_str()) {
                        bail!(
                            "sprite-animation shot {} emission {} has unknown parent {}",
                            shot.id,
                            emission.id,
                            emission.parent
                        );
                    }
                    let parent = animation
                        .sprites
                        .iter()
                        .find(|track| track.id == emission.parent)
                        .expect("validated emission parent");
                    if parent
                        .visible_start_frame
                        .is_some_and(|start| emission.frame < start)
                        || parent
                            .visible_end_frame
                            .is_some_and(|end| emission.frame > end)
                    {
                        bail!(
                            "sprite-animation shot {} emission {} starts outside parent {} visibility",
                            shot.id,
                            emission.id,
                            emission.parent
                        );
                    }
                    if parent.position_space != SpritePositionSpace::Canvas {
                        bail!(
                            "sprite-animation shot {} emission {} parent must use canvas position space",
                            shot.id,
                            emission.id
                        );
                    }
                    let end_frame = emission.frame.saturating_add(emission.duration_frames);
                    if emission.duration_frames == 0
                        || total_frames.is_some_and(|frames| u64::from(end_frame) > frames)
                    {
                        bail!(
                            "sprite-animation shot {} emission {} duration falls outside the shot",
                            shot.id,
                            emission.id
                        );
                    }
                    if emission.fade_out_frames > emission.duration_frames {
                        bail!(
                            "sprite-animation shot {} emission {} fade exceeds its duration",
                            shot.id,
                            emission.id
                        );
                    }
                    let end_width = emission.end_width.unwrap_or(emission.width);
                    if !emission.offset_x.is_finite()
                        || !emission.offset_y.is_finite()
                        || !emission.drift_x.is_finite()
                        || !emission.drift_y.is_finite()
                        || !emission.width.is_finite()
                        || !end_width.is_finite()
                        || !emission.rotation_degrees.is_finite()
                        || !emission.end_rotation_degrees.is_finite()
                        || !emission.anchor_x.is_finite()
                        || !emission.anchor_y.is_finite()
                        || !(-4.0..=4.0).contains(&emission.offset_x)
                        || !(-4.0..=4.0).contains(&emission.offset_y)
                        || !(-2.0..=2.0).contains(&emission.drift_x)
                        || !(-2.0..=2.0).contains(&emission.drift_y)
                        || !(0.0..=2.0).contains(&emission.width)
                        || emission.width == 0.0
                        || !(0.0..=2.0).contains(&end_width)
                        || end_width == 0.0
                        || !(-720.0..=720.0).contains(&emission.rotation_degrees)
                        || !(-720.0..=720.0).contains(&emission.end_rotation_degrees)
                        || !(0.0..=1.0).contains(&emission.anchor_x)
                        || !(0.0..=1.0).contains(&emission.anchor_y)
                    {
                        bail!(
                            "sprite-animation shot {} emission {} geometry is invalid",
                            shot.id,
                            emission.id
                        );
                    }
                }
                if !animation.camera.is_empty() {
                    validate_camera_keyframes(
                        "sprite-animation",
                        &shot.id,
                        "camera",
                        &animation.camera,
                        total_frames,
                    )?;
                }
                for track in &animation.sprites {
                    match (track.visible_start_frame, track.visible_end_frame) {
                        (None, None) => {}
                        (Some(start), Some(end)) => {
                            if start > end
                                || total_frames.is_some_and(|frames| u64::from(end) >= frames)
                            {
                                bail!(
                                    "sprite-animation shot {} track {} visibility window is invalid",
                                    shot.id,
                                    track.id
                                );
                            }
                        }
                        _ => bail!(
                            "sprite-animation shot {} track {} must declare both visibility frames or neither",
                            shot.id,
                            track.id
                        ),
                    }
                    match (track.position_space, track.parent.as_deref()) {
                        (SpritePositionSpace::Canvas, None) => {}
                        (SpritePositionSpace::Canvas, Some(_)) => bail!(
                            "sprite-animation shot {} track {} declares a parent but uses canvas position space",
                            shot.id,
                            track.id
                        ),
                        (SpritePositionSpace::ParentWidth, None) => bail!(
                            "sprite-animation shot {} track {} uses parent-width position space without a parent",
                            shot.id,
                            track.id
                        ),
                        (SpritePositionSpace::ParentWidth, Some(parent)) => {
                            if parent == track.id || !sprite_ids.contains(parent) {
                                bail!(
                                    "sprite-animation shot {} track {} has invalid parent {}",
                                    shot.id,
                                    track.id,
                                    parent
                                );
                            }
                            let parent_track = animation
                                .sprites
                                .iter()
                                .find(|candidate| candidate.id == parent)
                                .expect("validated sprite parent");
                            if parent_track.parent.is_some() {
                                bail!(
                                    "sprite-animation shot {} track {} cannot attach to nested parent {}",
                                    shot.id,
                                    track.id,
                                    parent
                                );
                            }
                            if track.movement != parent_track.movement
                                || track.movement_steps != parent_track.movement_steps
                            {
                                bail!(
                                    "sprite-animation shot {} track {} must share movement cadence with parent {}",
                                    shot.id,
                                    track.id,
                                    parent
                                );
                            }
                            let child_frames = track
                                .keyframes
                                .iter()
                                .map(|keyframe| keyframe.frame)
                                .collect::<BTreeSet<_>>();
                            if let Some(missing) = parent_track
                                .keyframes
                                .iter()
                                .map(|keyframe| keyframe.frame)
                                .find(|frame| !child_frames.contains(frame))
                            {
                                bail!(
                                    "sprite-animation shot {} track {} must key parent {} frame {}",
                                    shot.id,
                                    track.id,
                                    parent,
                                    missing
                                );
                            }
                        }
                    }
                    if track
                        .anchor_x
                        .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
                        || track.anchor_y.is_some_and(|value| {
                            !value.is_finite() || !(0.0..=1.0).contains(&value)
                        })
                    {
                        bail!(
                            "sprite-animation shot {} track {} anchors must be between 0 and 1",
                            shot.id,
                            track.id
                        );
                    }
                    match (track.movement, track.movement_steps) {
                        (SpriteMovement::Stepped, Some(steps)) if !(2..=12).contains(&steps) => {
                            bail!(
                                "sprite-animation shot {} track {} movement_steps must be between 2 and 12",
                                shot.id,
                                track.id
                            );
                        }
                        (SpriteMovement::Linear | SpriteMovement::Hold, Some(_)) => {
                            bail!(
                                "sprite-animation shot {} track {} movement_steps requires stepped movement",
                                shot.id,
                                track.id
                            );
                        }
                        _ => {}
                    }
                    if track.keyframes.is_empty() {
                        bail!(
                            "sprite-animation shot {} track {} has no keyframes",
                            shot.id,
                            track.id
                        );
                    }
                    if track.keyframes[0].frame != 0 {
                        bail!(
                            "sprite-animation shot {} track {} must begin at frame 0",
                            shot.id,
                            track.id
                        );
                    }
                    let mut prior = None;
                    for keyframe in &track.keyframes {
                        require_nonempty("sprite keyframe asset", &keyframe.asset)?;
                        if prior.is_some_and(|frame| keyframe.frame <= frame) {
                            bail!(
                                "sprite-animation shot {} track {} keyframe frames must increase",
                                shot.id,
                                track.id
                            );
                        }
                        if total_frames.is_some_and(|frames| u64::from(keyframe.frame) >= frames) {
                            bail!(
                                "sprite-animation shot {} track {} keyframe {} falls outside the shot",
                                shot.id,
                                track.id,
                                keyframe.frame
                            );
                        }
                        let valid_position = match track.position_space {
                            SpritePositionSpace::Canvas => {
                                (-1.0..=2.0).contains(&keyframe.x)
                                    && (-1.0..=2.0).contains(&keyframe.y)
                            }
                            SpritePositionSpace::ParentWidth => {
                                (-4.0..=4.0).contains(&keyframe.x)
                                    && (-4.0..=4.0).contains(&keyframe.y)
                            }
                        };
                        if !keyframe.x.is_finite()
                            || !keyframe.y.is_finite()
                            || !keyframe.width.is_finite()
                            || !valid_position
                            || !(0.0..=2.0).contains(&keyframe.width)
                            || keyframe.width == 0.0
                        {
                            bail!(
                                "sprite-animation shot {} track {} keyframe geometry is invalid",
                                shot.id,
                                track.id
                            );
                        }
                        prior = Some(keyframe.frame);
                    }
                }
            }
            (MediaKind::SpriteAnimation, None) => bail!(
                "sprite-animation shot {} has no sprite_animation sequence",
                shot.id
            ),
            (_, Some(_)) => bail!(
                "non-sprite shot {} cannot declare a sprite_animation sequence",
                shot.id
            ),
            (_, None) => {}
        }
        if let Some(marker_id) = &shot.beat_marker_id {
            let marker_ms = marker_times.get(marker_id.as_str()).ok_or_else(|| {
                anyhow!(
                    "shot {} references unknown beat marker {}",
                    shot.id,
                    marker_id
                )
            })?;
            if let Some(start) = shot.start_seconds {
                let start_ms = seconds_to_ms(start);
                if start_ms.abs_diff(*marker_ms) > 1 {
                    bail!(
                        "shot {} start does not align with beat marker {}",
                        shot.id,
                        marker_id
                    );
                }
            }
        }
    }

    for event in &manifest.audio_events {
        require_nonempty("audio event source", &event.source)?;
        if event.start_seconds < 0.0
            || event.source_in_seconds < 0.0
            || !event.start_seconds.is_finite()
            || !event.source_in_seconds.is_finite()
            || !event.gain_db.is_finite()
        {
            bail!(
                "audio event {} timing and gain must be finite and non-negative where applicable",
                event.id
            );
        }
        let start_ms = seconds_to_ms(event.start_seconds);
        if duration_ms.is_some_and(|duration| start_ms >= duration) {
            bail!(
                "audio event {} starts outside the production timeline",
                event.id
            );
        }
        let resolved_event_duration_ms = if let Some(event_duration) = event.duration_seconds {
            let event_duration_ms = required_ms(
                Some(event_duration),
                &format!("audio event {} duration", event.id),
            )?;
            if event_duration_ms == 0 {
                bail!("audio event {} duration must be positive", event.id);
            }
            if duration_ms.is_some_and(|duration| start_ms + event_duration_ms > duration + 1) {
                bail!(
                    "audio event {} extends beyond the production timeline",
                    event.id
                );
            }
            if event.fade_in_ms + event.fade_out_ms > event_duration_ms {
                bail!("audio event {} fades exceed its duration", event.id);
            }
            Some(event_duration_ms)
        } else {
            duration_ms.map(|duration| duration - start_ms)
        };
        if let Some(marker_id) = &event.beat_marker_id {
            let marker_ms = marker_times.get(marker_id.as_str()).ok_or_else(|| {
                anyhow!(
                    "audio event {} references unknown beat marker {}",
                    event.id,
                    marker_id
                )
            })?;
            if start_ms.abs_diff(*marker_ms) > 1 {
                bail!(
                    "audio event {} start does not align with beat marker {}",
                    event.id,
                    marker_id
                );
            }
        }
        let mut previous_automation_ms = None;
        for point in &event.gain_automation {
            if !point.gain_db.is_finite()
                || point.time_seconds.is_some() == point.beat_marker_id.is_some()
            {
                bail!(
                    "audio event {} automation points require finite gain and exactly one time_seconds or beat_marker_id anchor",
                    event.id
                );
            }
            let local_ms = if let Some(seconds) = point.time_seconds {
                if !seconds.is_finite() || seconds < 0.0 {
                    bail!(
                        "audio event {} automation time must be finite and non-negative",
                        event.id
                    );
                }
                seconds_to_ms(seconds)
            } else {
                let marker_id = point.beat_marker_id.as_deref().expect("one anchor");
                let marker_ms = *marker_times.get(marker_id).ok_or_else(|| {
                    anyhow!(
                        "audio event {} automation references unknown beat marker {}",
                        event.id,
                        marker_id
                    )
                })?;
                marker_ms.checked_sub(start_ms).ok_or_else(|| {
                    anyhow!(
                        "audio event {} automation marker {} precedes the event",
                        event.id,
                        marker_id
                    )
                })?
            };
            let event_duration_ms = resolved_event_duration_ms.ok_or_else(|| {
                anyhow!(
                    "audio event {} automation requires a timed event or timeline",
                    event.id
                )
            })?;
            if local_ms > event_duration_ms {
                bail!(
                    "audio event {} automation point is outside the event",
                    event.id
                );
            }
            if previous_automation_ms.is_some_and(|previous| local_ms <= previous) {
                bail!(
                    "audio event {} automation points must resolve to unique ascending times",
                    event.id
                );
            }
            previous_automation_ms = Some(local_ms);
        }
    }

    if let Some(ducking) = &manifest.narration_ducking {
        if !(0.000_001..=1.0).contains(&ducking.threshold)
            || !(1.0..=20.0).contains(&ducking.ratio)
            || !(1..=2_000).contains(&ducking.attack_ms)
            || !(1..=10_000).contains(&ducking.release_ms)
        {
            bail!(
                "narration_ducking must use threshold 0..1, ratio 1..20, attack 1..2000ms, and release 1..10000ms"
            );
        }
        if !manifest
            .audio_events
            .iter()
            .any(|event| event.role == AudioRole::Narration)
            || !manifest
                .audio_events
                .iter()
                .any(|event| event.role != AudioRole::Narration)
        {
            bail!("narration_ducking requires both narration and background audio events");
        }
    }
    if manifest.narration_ducking.is_some() && !manifest.audio_ducking.is_empty() {
        bail!("narration_ducking and audio_ducking cannot both be declared");
    }
    let present_roles = manifest
        .audio_events
        .iter()
        .map(|event| event.role)
        .collect::<BTreeSet<_>>();
    let mut policy_ids = BTreeSet::new();
    let mut targeted_roles = BTreeSet::new();
    for policy in &manifest.audio_ducking {
        require_nonempty("audio ducking policy id", &policy.id)?;
        if !policy_ids.insert(policy.id.as_str()) {
            bail!("audio_ducking policy ids must be unique");
        }
        let detectors = policy
            .detector_roles
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let targets = policy.target_roles.iter().copied().collect::<BTreeSet<_>>();
        if detectors.is_empty()
            || targets.is_empty()
            || detectors.len() != policy.detector_roles.len()
            || targets.len() != policy.target_roles.len()
            || !detectors.is_disjoint(&targets)
        {
            bail!(
                "audio_ducking {} requires unique, non-empty, disjoint detector_roles and target_roles",
                policy.id
            );
        }
        if !detectors.is_subset(&present_roles) || !targets.is_subset(&present_roles) {
            bail!(
                "audio_ducking {} references a role with no audio event",
                policy.id
            );
        }
        if targets.iter().any(|role| !targeted_roles.insert(*role)) {
            bail!("audio_ducking target roles may appear in only one ordered policy");
        }
        let target_stem_groups = targets
            .iter()
            .map(|role| match role {
                AudioRole::Narration | AudioRole::Dialogue => 0,
                AudioRole::Music => 1,
                AudioRole::Ambience | AudioRole::Effect => 2,
            })
            .collect::<BTreeSet<_>>();
        if target_stem_groups.len() != 1 {
            bail!(
                "audio_ducking {} target roles must remain within one D, M, or E stem",
                policy.id
            );
        }
        if !policy.threshold.is_finite()
            || !policy.ratio.is_finite()
            || !policy.max_reduction_db.is_finite()
            || !(0.000_001..=1.0).contains(&policy.threshold)
            || !(1.0..=20.0).contains(&policy.ratio)
            || !(0.1..=60.0).contains(&policy.max_reduction_db)
            || !(1..=2_000).contains(&policy.attack_ms)
            || !(1..=10_000).contains(&policy.release_ms)
        {
            bail!(
                "audio_ducking {} has invalid threshold, ratio, max reduction, attack, or release",
                policy.id
            );
        }
        if let Some(eq) = &policy.dynamic_eq {
            if !eq.frequency_hz.is_finite()
                || !eq.q.is_finite()
                || !eq.max_cut_db.is_finite()
                || !(20.0..=20_000.0).contains(&eq.frequency_hz)
                || !(0.1..=30.0).contains(&eq.q)
                || !(0.1..=24.0).contains(&eq.max_cut_db)
                || !(1..=2_000).contains(&eq.attack_ms)
                || !(1..=10_000).contains(&eq.release_ms)
            {
                bail!("audio_ducking {} has invalid dynamic_eq values", policy.id);
            }
        }
    }
    if let Some(mastering) = &manifest.audio_mastering {
        if manifest.audio_events.is_empty() {
            bail!("audio_mastering requires manifest audio_events");
        }
        if !(-36.0..=-5.0).contains(&mastering.integrated_lufs)
            || !(1.0..=20.0).contains(&mastering.loudness_range_lu)
            || !(-12.0..=-0.1).contains(&mastering.true_peak_dbfs)
            || !(0.1..=1.0).contains(&mastering.limiter)
        {
            bail!(
                "audio_mastering must use integrated_lufs -36..-5, loudness_range_lu 1..20, true_peak_dbfs -12..-0.1, and limiter 0.1..1"
            );
        }
    }
    if let Some(policy) = &manifest.audio_review_policy {
        require_nonempty("audio review policy id", &policy.id)?;
        if manifest.audio_events.is_empty()
            || !manifest
                .audio_events
                .iter()
                .any(|event| matches!(event.role, AudioRole::Narration | AudioRole::Dialogue))
        {
            bail!("audio_review_policy requires narration or dialogue audio events");
        }
        if !policy.dialogue_loudness_target_lufs.is_finite()
            || !policy.dialogue_loudness_tolerance_lu.is_finite()
            || !policy.minimum_speech_to_background_margin_db.is_finite()
            || !policy.speech_activity_threshold_dbfs.is_finite()
            || !policy.maximum_mono_loss_db.is_finite()
            || !(-70.0..=-5.0).contains(&policy.dialogue_loudness_target_lufs)
            || !(0.1..=20.0).contains(&policy.dialogue_loudness_tolerance_lu)
            || !(-20.0..=40.0).contains(&policy.minimum_speech_to_background_margin_db)
            || !(-80.0..=-1.0).contains(&policy.speech_activity_threshold_dbfs)
            || !(0.0..=30.0).contains(&policy.maximum_mono_loss_db)
        {
            bail!(
                "audio_review_policy contains invalid loudness, margin, activity, or mono-loss values"
            );
        }
    }
    Ok(())
}

fn validate_timeline(manifest: &ProductionManifest) -> Result<u64> {
    let mut expected = 0u64;
    let mut scene_durations = HashMap::new();
    for scene in &manifest.scenes {
        let duration = required_ms(
            scene.duration_seconds,
            &format!("scene {} duration", scene.id),
        )?;
        if duration == 0 {
            bail!("scene {} duration must be positive", scene.id);
        }
        scene_durations.insert(scene.id.as_str(), duration);
    }
    let mut shot_by_scene: HashMap<&str, u64> = HashMap::new();
    for shot in &manifest.shots {
        let start = required_ms(shot.start_seconds, &format!("shot {} start", shot.id))?;
        let duration = required_ms(shot.duration_seconds, &format!("shot {} duration", shot.id))?;
        if duration == 0 {
            bail!("shot {} duration must be positive", shot.id);
        }
        if start != expected {
            bail!(
                "shot {} starts at {}ms, expected {}ms; run `reel migrate --normalize-timing` or reconform",
                shot.id,
                start,
                expected
            );
        }
        expected = expected
            .checked_add(duration)
            .ok_or_else(|| anyhow!("production timeline duration exceeds supported range"))?;
        let scene_total = shot_by_scene.entry(&shot.scene_id).or_default();
        *scene_total = scene_total
            .checked_add(duration)
            .ok_or_else(|| anyhow!("scene {} duration exceeds supported range", shot.scene_id))?;
    }
    for scene in &manifest.scenes {
        let actual = shot_by_scene.get(scene.id.as_str()).copied().unwrap_or(0);
        if actual != scene_durations[scene.id.as_str()] {
            bail!(
                "scene {} is {}ms but its shots total {}ms",
                scene.id,
                scene_durations[scene.id.as_str()],
                actual
            );
        }
    }
    for platform in &manifest.platforms {
        if required_ms(
            platform.target_duration_seconds,
            &format!("platform {} target duration", platform.name),
        )? != expected
        {
            bail!(
                "platform {} duration does not match timeline",
                platform.name
            );
        }
    }
    for export in &manifest.exports {
        if required_ms(
            export.duration_seconds,
            &format!("export {} duration", export.id),
        )? != expected
        {
            bail!("export {} duration does not match timeline", export.id);
        }
    }
    Ok(expected)
}

pub fn plan(loaded: &LoadedProductionManifest) -> Result<ProductionPlan> {
    let report = validate(loaded)?;
    let cue_speakers = loaded
        .manifest
        .narration_cues
        .iter()
        .map(|cue| (cue.id.as_str(), cue.speaker_id.as_str()))
        .collect::<HashMap<_, _>>();
    let scenes = loaded
        .manifest
        .scenes
        .iter()
        .map(|scene| PlanScene {
            id: scene.id.clone(),
            purpose: scene.purpose.clone(),
            story_beat: scene.story_beat.clone(),
            duration_ms: scene.duration_seconds.map(seconds_to_ms),
            shots: loaded
                .manifest
                .shots
                .iter()
                .enumerate()
                .filter(|(_, shot)| shot.scene_id == scene.id)
                .map(|(order, shot)| PlanShot {
                    id: shot.id.clone(),
                    order: order + 1,
                    camera: shot.camera.clone(),
                    action: shot.action.clone(),
                    speaker_ids: shot
                        .narration_cue_ids
                        .iter()
                        .filter_map(|id| {
                            cue_speakers
                                .get(id.as_str())
                                .map(|value| (*value).to_string())
                        })
                        .collect(),
                    source_refs: shot.source_refs.clone(),
                    start_ms: shot.start_seconds.map(seconds_to_ms),
                    duration_ms: shot.duration_seconds.map(seconds_to_ms),
                })
                .collect(),
        })
        .collect();
    Ok(ProductionPlan {
        work: loaded.manifest.work.clone(),
        timing_status: report.timing_status,
        scenes,
        gated_commands: report.gated_commands,
    })
}

pub fn require_preview_ready(path: impl AsRef<Path>) -> Result<LoadedProductionManifest> {
    let loaded = load(path)?;
    let report = validate(&loaded)?;
    if !report.preview_ready {
        if !report.timing_ready {
            bail!(
                "timing not conformed: preview and render commands are gated for untimed manifests"
            );
        }
        bail!(
            "visual assets not ready: {}",
            report.semantic_blockers.join("; ")
        );
    }
    Ok(loaded)
}

pub fn require_timing_ready(path: impl AsRef<Path>) -> Result<LoadedProductionManifest> {
    let loaded = load(path)?;
    let report = validate(&loaded)?;
    if !report.timing_ready {
        bail!("timing not conformed: preview and render commands are gated for untimed manifests");
    }
    Ok(loaded)
}

pub fn conform(
    manifest_path: impl AsRef<Path>,
    measurements_path: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
    tempos: &BTreeMap<String, u32>,
) -> Result<ConformReport> {
    let loaded = load(manifest_path.as_ref())?;
    validate(&loaded)?;
    if loaded.manifest.timing_status == TimingStatus::Locked {
        bail!(
            "locked manifests cannot be conformed without creating an unlocked planning derivative"
        );
    }
    let measurement_bytes = fs::read(measurements_path.as_ref())?;
    let measurements: CueMeasurements = serde_yaml::from_slice(&measurement_bytes)?;
    let by_id = measurements
        .cues
        .iter()
        .map(|cue| (cue.cue_id.as_str(), cue))
        .collect::<HashMap<_, _>>();
    let speakers = loaded
        .manifest
        .speakers
        .iter()
        .map(|speaker| speaker.id.as_str())
        .collect::<HashSet<_>>();
    for (speaker, tempo) in tempos {
        if !speakers.contains(speaker.as_str()) {
            bail!("tempo references unknown speaker {speaker}");
        }
        if *tempo == 0 || *tempo > 200 {
            bail!("speaker tempo must be between 1 and 200 percent: {speaker}={tempo}");
        }
    }
    let mut cue_ms = HashMap::new();
    let mut audio_inputs = Vec::new();
    for cue in &loaded.manifest.narration_cues {
        let measured = by_id
            .get(cue.id.as_str())
            .ok_or_else(|| anyhow!("missing measurement for narration cue {}", cue.id))?;
        let tempo = tempos.get(&cue.speaker_id).copied().unwrap_or(100) as u64;
        let speech =
            ((measured.duration_ms as u128 * 100u128 + tempo as u128 / 2) / tempo as u128) as u64;
        cue_ms.insert(
            cue.id.as_str(),
            measured.head_silence_ms + speech + measured.tail_silence_ms,
        );
        if !measured.audio_path.is_empty() {
            let hash = if !measured.sha256.is_empty() {
                measured.sha256.clone()
            } else {
                sha256_path(resolve_relative(
                    measurements_path.as_ref(),
                    &measured.audio_path,
                ))?
            };
            audio_inputs.push(HashedInput {
                id: cue.id.clone(),
                path: measured.audio_path.clone(),
                sha256: hash,
            });
        }
    }
    if by_id.len() != loaded.manifest.narration_cues.len() {
        let known = loaded
            .manifest
            .narration_cues
            .iter()
            .map(|cue| cue.id.as_str())
            .collect::<HashSet<_>>();
        let extra = by_id.keys().find(|id| !known.contains(**id));
        if let Some(extra) = extra {
            bail!("measurement references unknown cue {extra}");
        }
    }
    let pause_by_cue = loaded
        .manifest
        .protected_pauses
        .iter()
        .map(|pause| (pause.after_cue_id.as_str(), pause.duration_ms))
        .collect::<HashMap<_, _>>();
    let mut manifest = loaded.manifest.clone();
    let shot_index_by_id = manifest
        .shots
        .iter()
        .enumerate()
        .map(|(index, shot)| (shot.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut cue_shots: HashMap<String, Vec<usize>> = HashMap::new();
    let mut shot_cues: Vec<Vec<String>> = vec![Vec::new(); manifest.shots.len()];
    let mut allocations: HashMap<(String, usize), u64> = HashMap::new();
    let shot_weights = manifest
        .shots
        .iter()
        .map(|shot| {
            if shot.allocation_weight > 0.0 {
                shot.allocation_weight
            } else {
                1.0
            }
        })
        .collect::<Vec<_>>();
    for cue in &manifest.narration_cues {
        let mut indices = if cue.shot_ids.is_empty() {
            manifest
                .shots
                .iter()
                .enumerate()
                .filter(|(_, shot)| shot.narration_cue_ids.iter().any(|id| id == &cue.id))
                .map(|(index, _)| index)
                .collect::<Vec<_>>()
        } else {
            cue.shot_ids
                .iter()
                .map(|id| {
                    shot_index_by_id
                        .get(id)
                        .copied()
                        .ok_or_else(|| anyhow!("cue {} references unknown shot {id}", cue.id))
                })
                .collect::<Result<Vec<_>>>()?
        };
        indices.sort_unstable();
        indices.dedup();
        if indices.is_empty() {
            bail!("narration cue {} is not allocated to any shot", cue.id);
        }
        if indices.windows(2).any(|pair| pair[1] != pair[0] + 1) {
            bail!(
                "narration cue {} must span contiguous ordered shots",
                cue.id
            );
        }
        let total_weight = indices
            .iter()
            .map(|index| shot_weights[*index])
            .sum::<f64>();
        let measured = cue_ms[cue.id.as_str()];
        let mut assigned = 0u64;
        for (position, index) in indices.iter().enumerate() {
            let allocation = if position + 1 == indices.len() {
                measured - assigned
            } else {
                let weight = shot_weights[*index];
                ((measured as f64 * weight / total_weight).round() as u64).min(measured - assigned)
            };
            assigned += allocation;
            allocations.insert((cue.id.clone(), *index), allocation);
            shot_cues[*index].push(cue.id.clone());
            if !manifest.shots[*index]
                .narration_cue_ids
                .iter()
                .any(|id| id == &cue.id)
            {
                manifest.shots[*index]
                    .narration_cue_ids
                    .push(cue.id.clone());
            }
        }
        cue_shots.insert(cue.id.clone(), indices);
    }
    let mut cursor = 0u64;
    let mut cue_timelines = BTreeMap::new();
    let mut active_cue: Option<String> = None;
    let mut closed_cues = HashSet::new();
    for (shot_index, shot) in manifest.shots.iter_mut().enumerate() {
        let mut duration = shot.fixed_hold_ms;
        for cue_id in &shot_cues[shot_index] {
            if active_cue.as_deref() != Some(cue_id.as_str()) {
                if let Some(previous) = active_cue.replace(cue_id.clone()) {
                    closed_cues.insert(previous);
                }
                if closed_cues.contains(cue_id) {
                    bail!("narration cue {cue_id} allocations are interleaved with another cue");
                }
            }
            let allocation = allocations[&(cue_id.clone(), shot_index)];
            let cue_start = cue_timelines
                .get(cue_id)
                .map(|(start, _)| *start)
                .unwrap_or(cursor + duration);
            duration += allocation;
            cue_timelines.insert(cue_id.clone(), (cue_start, cursor + duration));
            let last_shot = cue_shots[cue_id].last().copied() == Some(shot_index);
            if last_shot {
                duration += pause_by_cue.get(cue_id.as_str()).copied().unwrap_or(0);
            }
        }
        if duration == 0 {
            bail!(
                "shot {} has no cue duration or fixed_hold_ms; conform cannot invent timing",
                shot.id
            );
        }
        shot.start_seconds = Some(ms_to_seconds(cursor));
        shot.duration_seconds = Some(ms_to_seconds(duration));
        cursor += duration;
    }
    for cue in &mut manifest.narration_cues {
        let (start, end) = cue_timelines
            .get(&cue.id)
            .copied()
            .ok_or_else(|| anyhow!("conform produced no timeline for cue {}", cue.id))?;
        cue.start_seconds = Some(ms_to_seconds(start));
        cue.duration_seconds = Some(ms_to_seconds(end - start));
    }
    for scene in &mut manifest.scenes {
        let duration = manifest
            .shots
            .iter()
            .filter(|shot| shot.scene_id == scene.id)
            .map(|shot| seconds_to_ms(shot.duration_seconds.expect("conformed")))
            .sum();
        scene.duration_seconds = Some(ms_to_seconds(duration));
    }
    for platform in &mut manifest.platforms {
        platform.target_duration_seconds = Some(ms_to_seconds(cursor));
    }
    for export in &mut manifest.exports {
        export.duration_seconds = Some(ms_to_seconds(cursor));
    }
    manifest.timing_status = TimingStatus::Conformed;
    manifest.lineage = Some(VariantLineage {
        parent_manifest: manifest_path.as_ref().display().to_string(),
        root_work: manifest
            .lineage
            .as_ref()
            .map(|lineage| lineage.root_work.clone())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| manifest.work.clone()),
        scene_key: manifest
            .scenes
            .iter()
            .map(|scene| scene.id.as_str())
            .collect::<Vec<_>>()
            .join("+"),
        transformation_reason: "voice-driven atomic conform".to_string(),
        changed_dimensions: vec![
            "voice".to_string(),
            "pace".to_string(),
            "captions".to_string(),
            "edit".to_string(),
        ],
        review_candidate: true,
        principal_approved: false,
        created_unix: unix_now()?,
    });
    validate(&LoadedProductionManifest {
        path: output_dir.as_ref().join("manifest.yaml"),
        manifest: manifest.clone(),
        bytes: Vec::new(),
    })?;

    let packet = output_dir.as_ref();
    if packet.exists() && fs::read_dir(packet)?.next().is_some() {
        bail!(
            "refusing partial overwrite: output packet is not empty: {}",
            packet.display()
        );
    }
    let parent = packet.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".reel-conform-{}", std::process::id()));
    if staging.exists() {
        bail!(
            "staging path already exists; inspect and remove it before retrying: {}",
            staging.display()
        );
    }
    fs::create_dir_all(&staging)?;
    let manifest_out = staging.join("manifest.yaml");
    let captions_out = staging.join("captions.srt");
    let lineage_out = staging.join("lineage.json");
    let report_out = staging.join("conform-report.json");
    let manifest_bytes = serde_yaml::to_string(&manifest)?.into_bytes();
    fs::write(&manifest_out, &manifest_bytes)?;
    let captions = render_srt(&manifest.narration_cues, &cue_timelines);
    fs::write(&captions_out, captions.as_bytes())?;
    let final_manifest = packet.join("manifest.yaml");
    let final_captions = packet.join("captions.srt");
    let final_lineage = packet.join("lineage.json");
    let lineage = ConformLineage {
        schema: CONFORM_SCHEMA.to_string(),
        generated_unix: unix_now()?,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        input_manifest: manifest_path.as_ref().display().to_string(),
        input_manifest_sha256: sha256_bytes(&loaded.bytes),
        cue_measurements: measurements_path.as_ref().display().to_string(),
        cue_measurements_sha256: sha256_bytes(&measurement_bytes),
        audio_inputs,
        speaker_tempos_percent: tempos.clone(),
        output_manifest: final_manifest.display().to_string(),
        output_manifest_sha256: sha256_bytes(&manifest_bytes),
        captions: final_captions.display().to_string(),
        captions_sha256: sha256_bytes(captions.as_bytes()),
    };
    fs::write(&lineage_out, serde_json::to_vec_pretty(&lineage)?)?;
    let report = ConformReport {
        schema: CONFORM_SCHEMA.to_string(),
        work: manifest.work.clone(),
        timing_status: manifest.timing_status.as_str().to_string(),
        duration_ms: cursor,
        scene_count: manifest.scenes.len(),
        shot_count: manifest.shots.len(),
        cue_count: manifest.narration_cues.len(),
        protected_pause_count: manifest.protected_pauses.len(),
        packet: packet.display().to_string(),
        manifest: final_manifest.display().to_string(),
        captions: final_captions.display().to_string(),
        lineage: final_lineage.display().to_string(),
    };
    fs::write(&report_out, serde_json::to_vec_pretty(&report)?)?;
    fs::rename(&staging, packet).with_context(|| {
        format!(
            "failed to atomically publish conform packet {}",
            packet.display()
        )
    })?;
    Ok(report)
}

fn render_srt(cues: &[NarrationCue], timeline: &BTreeMap<String, (u64, u64)>) -> String {
    let mut output = String::new();
    let rendered = cues
        .iter()
        .filter_map(|cue| {
            let (start, end) = timeline.get(&cue.id)?;
            (!cue.text.trim().is_empty()).then_some((*start, *end, cue.text.as_str()))
        })
        .collect::<Vec<_>>();
    for (position, (start, end, text)) in rendered.iter().enumerate() {
        output.push_str(&format!(
            "{}\n{} --> {}\n{}",
            position + 1,
            srt_time(*start),
            srt_time(*end),
            text
        ));
        output.push('\n');
        if position + 1 < rendered.len() {
            output.push('\n');
        }
    }
    output
}

pub fn caption_export(
    manifest_path: impl AsRef<Path>,
    output: impl AsRef<Path>,
) -> Result<PathBuf> {
    let loaded = require_timing_ready(manifest_path)?;
    let mut timeline = BTreeMap::new();
    for cue in &loaded.manifest.narration_cues {
        let start = required_ms(cue.start_seconds, &format!("cue {} start", cue.id))?;
        let duration = required_ms(cue.duration_seconds, &format!("cue {} duration", cue.id))?;
        if duration == 0 {
            bail!("cue {} duration must be positive", cue.id);
        }
        timeline.insert(cue.id.clone(), (start, start + duration));
    }
    if let Some(parent) = output.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }
    let mut captions = render_srt(&loaded.manifest.narration_cues, &timeline);
    let preserve_crlf = loaded
        .manifest
        .extra
        .get("cue_import")
        .and_then(Value::as_mapping)
        .and_then(|mapping| mapping.get(Value::String("srt_line_ending".to_string())))
        .and_then(Value::as_str)
        == Some("crlf");
    if preserve_crlf {
        captions = captions.replace('\n', "\r\n");
    }
    fs::write(output.as_ref(), captions)?;
    Ok(output.as_ref().to_path_buf())
}

fn srt_time(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let minutes = (ms % 3_600_000) / 60_000;
    let seconds = (ms % 60_000) / 1_000;
    let millis = ms % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

pub fn migrate(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    normalize_timing: bool,
) -> Result<PathBuf> {
    let input = input.as_ref();
    let output = output.as_ref();
    if input == output {
        bail!("migration must write a new derivative; input and output cannot match");
    }
    let mut raw: Value = serde_yaml::from_slice(&fs::read(input)?)?;
    let top = raw
        .as_mapping_mut()
        .ok_or_else(|| anyhow!("manifest root must be a mapping"))?;
    let schema_key = Value::String("schema".to_string());
    let version_key = Value::String("manifest_version".to_string());
    if !top.contains_key(&version_key) {
        top.remove(&schema_key);
    }
    top.insert(version_key, Value::String(MANIFEST_VERSION.to_string()));
    let inferred_profile = top
        .get(Value::String("work".to_string()))
        .and_then(Value::as_str)
        .map(|work| {
            if work.contains("voice-audition") {
                "voice-audition"
            } else {
                "animatic"
            }
        })
        .unwrap_or("animatic");
    top.entry(Value::String("profile".to_string()))
        .or_insert(Value::String(inferred_profile.to_string()));
    top.entry(Value::String("timing_status".to_string()))
        .or_insert(Value::String("conformed".to_string()));
    top.entry(Value::String("speakers".to_string()))
        .or_insert(Value::Sequence(Vec::new()));
    top.entry(Value::String("narration_cues".to_string()))
        .or_insert(Value::Sequence(Vec::new()));
    top.entry(Value::String("protected_pauses".to_string()))
        .or_insert(Value::Sequence(Vec::new()));
    top.entry(Value::String("source_ranges".to_string()))
        .or_insert(Value::Sequence(Vec::new()));
    top.entry(Value::String("omissions".to_string()))
        .or_insert(Value::Sequence(Vec::new()));
    lift_legacy_shot_narration(top)?;
    if normalize_timing {
        normalize_raw_timing(top)?;
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, serde_yaml::to_string(&raw)?)?;
    Ok(output.to_path_buf())
}

fn lift_legacy_shot_narration(top: &mut serde_yaml::Mapping) -> Result<()> {
    let cue_key = Value::String("narration_cues".to_string());
    let speaker_key = Value::String("speakers".to_string());
    let existing_cues = top
        .get(&cue_key)
        .and_then(Value::as_sequence)
        .map(|items| !items.is_empty())
        .unwrap_or(false);
    if existing_cues {
        return Ok(());
    }
    let mut cues = Vec::new();
    if let Some(shots) = top
        .get_mut(Value::String("shots".to_string()))
        .and_then(Value::as_sequence_mut)
    {
        for shot in shots {
            let Some(mapping) = shot.as_mapping_mut() else {
                continue;
            };
            let shot_id = mapping
                .get(Value::String("id".to_string()))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let narration = mapping
                .get(Value::String("audio".to_string()))
                .and_then(Value::as_mapping)
                .and_then(|audio| audio.get(Value::String("narration".to_string())))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            if narration.is_empty() {
                continue;
            }
            let cue_id = format!("cue-{shot_id}");
            mapping.insert(
                Value::String("narration_cue_ids".to_string()),
                Value::Sequence(vec![Value::String(cue_id.clone())]),
            );
            let mut cue = serde_yaml::Mapping::new();
            cue.insert(Value::String("id".to_string()), Value::String(cue_id));
            cue.insert(
                Value::String("speaker_id".to_string()),
                Value::String("legacy-narrator".to_string()),
            );
            cue.insert(Value::String("text".to_string()), Value::String(narration));
            cue.insert(
                Value::String("shot_ids".to_string()),
                Value::Sequence(vec![Value::String(shot_id)]),
            );
            cues.push(Value::Mapping(cue));
        }
    }
    if !cues.is_empty() {
        top.insert(cue_key, Value::Sequence(cues));
        let speakers = top
            .entry(speaker_key)
            .or_insert(Value::Sequence(Vec::new()))
            .as_sequence_mut()
            .ok_or_else(|| anyhow!("speakers must be a sequence"))?;
        if speakers.is_empty() {
            let mut speaker = serde_yaml::Mapping::new();
            speaker.insert(
                Value::String("id".to_string()),
                Value::String("legacy-narrator".to_string()),
            );
            speaker.insert(
                Value::String("display_name".to_string()),
                Value::String("Legacy narrator — identity requires review".to_string()),
            );
            speaker.insert(
                Value::String("asset_kind".to_string()),
                Value::String("guide".to_string()),
            );
            speakers.push(Value::Mapping(speaker));
        }
    }
    Ok(())
}

fn normalize_raw_timing(top: &mut serde_yaml::Mapping) -> Result<()> {
    let shots = top
        .get_mut(Value::String("shots".to_string()))
        .and_then(Value::as_sequence_mut)
        .ok_or_else(|| anyhow!("shots must be a sequence to normalize timing"))?;
    let mut cursor = 0u64;
    let mut scene_totals: BTreeMap<String, u64> = BTreeMap::new();
    for shot in shots {
        let mapping = shot
            .as_mapping_mut()
            .ok_or_else(|| anyhow!("shot must be a mapping"))?;
        let duration = mapping
            .get(Value::String("duration_seconds".to_string()))
            .and_then(Value::as_f64)
            .ok_or_else(|| anyhow!("timed shot is missing duration_seconds"))?;
        let duration_ms = seconds_to_ms(duration);
        mapping.insert(
            Value::String("start_seconds".to_string()),
            serde_yaml::to_value(ms_to_seconds(cursor))?,
        );
        let scene = mapping
            .get(Value::String("scene_id".to_string()))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        *scene_totals.entry(scene).or_default() += duration_ms;
        cursor += duration_ms;
    }
    if let Some(scenes) = top
        .get_mut(Value::String("scenes".to_string()))
        .and_then(Value::as_sequence_mut)
    {
        for scene in scenes {
            let mapping = scene
                .as_mapping_mut()
                .ok_or_else(|| anyhow!("scene must be mapping"))?;
            let id = mapping
                .get(Value::String("id".to_string()))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if let Some(total) = scene_totals.get(id) {
                mapping.insert(
                    Value::String("duration_seconds".to_string()),
                    serde_yaml::to_value(ms_to_seconds(*total))?,
                );
            }
        }
    }
    for key in ["platforms", "exports"] {
        if let Some(items) = top
            .get_mut(Value::String(key.to_string()))
            .and_then(Value::as_sequence_mut)
        {
            let duration_key = if key == "platforms" {
                "target_duration_seconds"
            } else {
                "duration_seconds"
            };
            for item in items {
                if let Some(mapping) = item.as_mapping_mut() {
                    mapping.insert(
                        Value::String(duration_key.to_string()),
                        serde_yaml::to_value(ms_to_seconds(cursor))?,
                    );
                }
            }
        }
    }
    Ok(())
}

pub fn source_coverage(path: impl AsRef<Path>) -> Result<CoverageReport> {
    let loaded = load(path)?;
    validate(&loaded)?;
    let range_ids = loaded
        .manifest
        .source_ranges
        .iter()
        .map(|range| range.id.as_str())
        .collect::<HashSet<_>>();
    let mut invented = Vec::new();
    let mut unattributed = Vec::new();
    let mut invalid = Vec::new();
    let mut attributed = 0;
    for cue in &loaded.manifest.narration_cues {
        if cue.invented {
            invented.push(cue.id.clone());
        }
        if cue.source_refs.is_empty() && !cue.invented {
            unattributed.push(cue.id.clone());
        } else if !cue.invented {
            attributed += 1;
        }
        for reference in &cue.source_refs {
            if !range_ids.contains(reference.as_str()) {
                invalid.push(format!("cue {} -> {reference}", cue.id));
            }
        }
    }
    for shot in &loaded.manifest.shots {
        for reference in &shot.source_refs {
            if !range_ids.contains(reference.as_str()) {
                invalid.push(format!("shot {} -> {reference}", shot.id));
            }
        }
    }
    let mut selected = BTreeSet::new();
    for range in &loaded.manifest.source_ranges {
        if range.start > range.end {
            bail!("source range {} starts after it ends", range.id);
        }
        selected.extend(range.start..=range.end);
    }
    let mut omitted = BTreeSet::new();
    for omission in &loaded.manifest.omissions {
        if !matches!(
            omission.bridge.as_str(),
            "silence" | "title-card" | "archival-image" | "approved-adaptation"
        ) {
            bail!(
                "omission {} has unsupported bridge {}",
                omission.id,
                omission.bridge
            );
        }
        omitted.extend(omission.start..=omission.end);
    }
    let uncovered_units = match (selected.first(), selected.last()) {
        (Some(first), Some(last)) => (*first..=*last)
            .filter(|unit| !selected.contains(unit) && !omitted.contains(unit))
            .collect(),
        _ => Vec::new(),
    };
    let complete = invented.is_empty()
        && unattributed.is_empty()
        && invalid.is_empty()
        && uncovered_units.is_empty();
    Ok(CoverageReport {
        work: loaded.manifest.work,
        selected_ranges: loaded.manifest.source_ranges,
        omissions: loaded.manifest.omissions,
        spoken_cues: loaded.manifest.narration_cues.len(),
        attributed_cues: attributed,
        invented_cues: invented,
        unattributed_cues: unattributed,
        invalid_references: invalid,
        uncovered_units,
        complete,
    })
}

pub fn provider_package(path: impl AsRef<Path>) -> Result<ProviderPackage> {
    let loaded = load(&path)?;
    validate(&loaded)?;
    let mut observations = BTreeMap::new();
    let mut assets = HashMap::new();
    for entity in crate::continuity::resolve_for_manifest(path.as_ref(), &loaded.manifest)? {
        observations.insert(entity.id.clone(), entity.observations.clone());
        for asset in entity.reference_assets {
            assets.insert(asset.id.clone(), asset);
        }
    }
    let mut requested_assets = Vec::new();
    let mut blockers = Vec::new();
    for id in &loaded.manifest.provider_handoff.asset_ids {
        let asset = assets
            .get(id)
            .ok_or_else(|| anyhow!("provider handoff references unknown local asset {id}"))?;
        if asset.provider_transfer != TransferPolicy::Approved
            || asset.approval_reference.is_empty()
        {
            blockers.push(format!(
                "asset {id} transfer is {:?} and lacks an approved transfer reference",
                asset.provider_transfer
            ));
        }
        requested_assets.push(ProviderAsset {
            id: asset.id.clone(),
            sha256: asset.sha256.clone(),
            provider_transfer: asset.provider_transfer,
            approval_reference: asset.approval_reference.clone(),
        });
    }
    let prompts = loaded
        .manifest
        .shots
        .iter()
        .filter(|shot| !shot.visual_prompt.is_empty())
        .map(|shot| (shot.id.clone(), shot.visual_prompt.clone()))
        .collect();
    Ok(ProviderPackage {
        schema: PROVIDER_PACKAGE_SCHEMA.to_string(),
        work: loaded.manifest.work,
        generated_unix: unix_now()?,
        approved_text_observations: observations,
        prompts,
        requested_assets,
        outbound_text_fields: loaded.manifest.provider_handoff.text_fields,
        blocked: !blockers.is_empty(),
        blockers,
    })
}

pub fn write_provider_package(
    manifest: impl AsRef<Path>,
    output: impl AsRef<Path>,
) -> Result<ProviderPackage> {
    let package = provider_package(manifest)?;
    if let Some(parent) = output.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, serde_json::to_vec_pretty(&package)?)?;
    if package.blocked {
        bail!(
            "provider package is blocked: {}",
            package.blockers.join("; ")
        );
    }
    Ok(package)
}

pub fn review_select(root: impl AsRef<Path>) -> Result<ReviewSelectionReport> {
    let root = root.as_ref();
    let mut manifests = Vec::new();
    discover_yaml(root, &mut manifests)?;
    manifests.sort();
    let mut groups: BTreeMap<String, ReviewSelection> = BTreeMap::new();
    let mut latest: BTreeMap<String, (u64, String)> = BTreeMap::new();
    for path in manifests {
        if !is_production_manifest(&path).unwrap_or(false) {
            continue;
        }
        let loaded = load(&path)?;
        validate(&loaded)?;
        let lineage = loaded.manifest.lineage.clone().unwrap_or_default();
        let root_key = if lineage.root_work.is_empty() {
            loaded.manifest.work.clone()
        } else {
            lineage.root_work.clone()
        };
        let scene_key = if lineage.scene_key.is_empty() {
            loaded
                .manifest
                .scenes
                .iter()
                .map(|scene| scene.id.as_str())
                .collect::<Vec<_>>()
                .join("+")
        } else {
            lineage.scene_key.clone()
        };
        let key = format!("{root_key}:{scene_key}");
        let group = groups
            .entry(key.clone())
            .or_insert_with(|| ReviewSelection {
                latest_review_candidate: None,
                principal_approved: Vec::new(),
                candidates: Vec::new(),
                findings: Vec::new(),
            });
        let display = path.display().to_string();
        if lineage.review_candidate {
            group.candidates.push(display.clone());
            let candidate = (lineage.created_unix, display.clone());
            let replace = latest
                .get(&key)
                .map(|current| candidate > *current)
                .unwrap_or(true);
            if replace {
                latest.insert(key.clone(), candidate);
                group.latest_review_candidate = Some(display.clone());
            }
        }
        if lineage.principal_approved {
            group.principal_approved.push(display);
        }
        group
            .findings
            .extend(loaded.manifest.review.principal_findings);
    }
    Ok(ReviewSelectionReport {
        root: root.display().to_string(),
        groups,
    })
}

pub fn quality_check(path: impl AsRef<Path>) -> Result<QualityReport> {
    let loaded = load(path)?;
    validate(&loaded)?;
    let controls = &loaded.manifest.quality_controls;
    let mut warnings = Vec::new();
    for shot in &loaded.manifest.shots {
        if let (Some(max), Some(duration)) = (
            controls.max_uninterrupted_hold_seconds,
            shot.duration_seconds,
        ) {
            if duration > max && matches!(shot.motion.as_str(), "" | "hold" | "hold-dark") {
                warnings.push(QualityWarning {
                    shot_id: shot.id.clone(),
                    code: "long-low-motion-hold".to_string(),
                    message: format!("{duration:.3}s hold exceeds configured {max:.3}s maximum"),
                });
            }
        }
        if controls.require_focal_points && shot.focal_point.is_none() {
            warnings.push(QualityWarning {
                shot_id: shot.id.clone(),
                code: "missing-focal-point".to_string(),
                message: "pan/zoom safety cannot be checked without a normalized focal point"
                    .to_string(),
            });
        }
        if controls.require_protected_regions && shot.protected_regions.is_empty() {
            warnings.push(QualityWarning {
                shot_id: shot.id.clone(),
                code: "missing-protected-region".to_string(),
                message: "face/caption crop safety has no protected region".to_string(),
            });
        }
        if let Some(point) = &shot.focal_point {
            if !(0.0..=1.0).contains(&point.x) || !(0.0..=1.0).contains(&point.y) {
                warnings.push(QualityWarning {
                    shot_id: shot.id.clone(),
                    code: "invalid-focal-point".to_string(),
                    message: "focal point coordinates must remain within 0..1".to_string(),
                });
            }
        }
        for region in &shot.protected_regions {
            if region.x < 0.0
                || region.y < 0.0
                || region.width <= 0.0
                || region.height <= 0.0
                || region.x + region.width > 1.0
                || region.y + region.height > 1.0
            {
                warnings.push(QualityWarning {
                    shot_id: shot.id.clone(),
                    code: "invalid-protected-region".to_string(),
                    message: format!(
                        "protected region {} extends outside normalized canvas",
                        region.id
                    ),
                });
            }
        }
    }
    for pair in loaded.manifest.shots.windows(2) {
        if let (Some(left), Some(right)) = (&pair[0].screen_position, &pair[1].screen_position) {
            if left != right && pair[0].eye_line == pair[1].eye_line {
                warnings.push(QualityWarning {
                    shot_id: pair[1].id.clone(),
                    code: "screen-direction-review".to_string(),
                    message: "screen position changes while eye-line direction is unchanged; review continuity".to_string(),
                });
            }
        }
    }
    let narration_only = controls
        .ab_outputs
        .iter()
        .any(|item| item == "narration-only");
    let effects_music = controls
        .ab_outputs
        .iter()
        .any(|item| item == "effects-music");
    Ok(QualityReport {
        work: loaded.manifest.work,
        passed: warnings.is_empty(),
        warnings,
        narration_only_output: narration_only,
        effects_music_output: effects_music,
    })
}

pub fn parse_tempos(values: &[String]) -> Result<BTreeMap<String, u32>> {
    let mut result = BTreeMap::new();
    for value in values {
        let (speaker, percent) = value
            .split_once('=')
            .ok_or_else(|| anyhow!("speaker tempo must use SPEAKER=PERCENT"))?;
        let percent = percent.parse::<u32>()?;
        if result.insert(speaker.to_string(), percent).is_some() {
            bail!("duplicate speaker tempo: {speaker}");
        }
    }
    Ok(result)
}

fn discover_yaml(root: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    if root.is_file() {
        output.push(root.to_path_buf());
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            discover_yaml(&path, output)?;
        } else if matches!(
            path.extension().and_then(|item| item.to_str()),
            Some("yaml" | "yml")
        ) {
            output.push(path);
        }
    }
    Ok(())
}

fn required_ms(value: Option<f64>, label: &str) -> Result<u64> {
    let value = value.ok_or_else(|| anyhow!("timing not conformed: {label} is missing"))?;
    if value < 0.0 || !value.is_finite() {
        bail!("{label} must be a finite non-negative duration");
    }
    Ok(seconds_to_ms(value))
}

fn seconds_to_ms(seconds: f64) -> u64 {
    (seconds * 1000.0).round() as u64
}

fn ms_to_seconds(ms: u64) -> f64 {
    ms as f64 / 1000.0
}

fn require_nonempty(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} must not be empty");
    }
    Ok(())
}

fn unique<'a>(label: &str, values: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut seen = HashSet::new();
    for value in values {
        require_nonempty(label, value)?;
        if !seen.insert(value) {
            bail!("duplicate {label}: {value}");
        }
    }
    Ok(())
}

fn resolve_relative(base_file: &Path, referenced: &str) -> PathBuf {
    let path = Path::new(referenced);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_file
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(path)
    }
}

pub fn sha256_path(path: impl AsRef<Path>) -> Result<String> {
    Ok(sha256_bytes(&fs::read(path.as_ref())?))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn unix_now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const UNTITLED: &str = r#"
manifest_version: reel.manifest.v0.2
profile: animatic
timing_status: untimed
work: fixture-work
title: Fixture
source_scenario: { repo: FIXTURE, path: source.txt, id: scene, source_commit: abc }
format: short-film
style: illustrated-2d
platforms:
  - { name: private-review, aspect_ratio: '16:9', sound_optional: false }
continuity: {}
scenes:
  - { id: scene-01, purpose: test, story_beat: change, location: room }
shots:
  - id: shot-01
    scene_id: scene-01
    camera: hold
    action: listen
    visual_prompt: safe fixture
    narration_cue_ids: [poem]
  - id: shot-02
    scene_id: scene-01
    camera: push
    action: answer
    visual_prompt: safe fixture two
    narration_cue_ids: [prose]
speakers:
  - { id: poet, display_name: Poet, language: es, asset_kind: guide }
  - { id: narrator, display_name: Narrator, language: es, asset_kind: guide }
narration_cues:
  - { id: poem, speaker_id: poet, text: 'A test line.', source_refs: [selected-a], shot_ids: [shot-01] }
  - { id: prose, speaker_id: narrator, text: 'Another test line.', source_refs: [selected-b], shot_ids: [shot-02] }
protected_pauses:
  - { id: poem-to-prose, after_cue_id: poem, duration_ms: 1500, locked: true }
source_ranges:
  - { id: selected-a, start: 1, end: 3 }
  - { id: selected-b, start: 6, end: 8 }
omissions:
  - { id: omitted, start: 4, end: 5, bridge: silence }
exports:
  - { id: private-review, filename: fixture.mp4, aspect_ratio: '16:9' }
review: { required_roles: [editor], status: planning }
"#;

    fn write_fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("planning.yaml");
        fs::write(&path, UNTITLED).unwrap();
        (dir, path)
    }

    #[test]
    fn untimed_manifest_validates_and_plans_but_is_gated() {
        let (_dir, path) = write_fixture();
        let loaded = load(&path).unwrap();
        let report = validate(&loaded).unwrap();
        assert_eq!(report.timing_status, "untimed");
        assert!(!report.preview_ready);
        assert!(
            report
                .gated_commands
                .contains(&"animatic-render".to_string())
        );
        let plan = plan(&loaded).unwrap();
        assert_eq!(plan.scenes[0].shots.len(), 2);
        assert!(plan.scenes[0].shots[0].duration_ms.is_none());
    }

    #[test]
    fn mixed_media_timeline_validates_named_beat_anchors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mixed-media.yaml");
        fs::write(
            &path,
            r#"
manifest_version: reel.manifest.v0.2
profile: animatic
timing_status: conformed
work: mixed-media-fixture
title: Mixed Media Fixture
scenes:
  - { id: scene-01, duration_seconds: 2.0 }
shots:
  - { id: still-01, scene_id: scene-01, start_seconds: 0.0, duration_seconds: 1.0, visual_asset: still.png, media_kind: still, beat_marker_id: downbeat }
  - { id: clip-01, scene_id: scene-01, start_seconds: 1.0, duration_seconds: 1.0, visual_asset: clip.mp4, media_kind: video, source_in_seconds: 3.5, beat_marker_id: cut }
beat_markers:
  - { id: downbeat, time_seconds: 0.0, label: Downbeat, accent: true }
  - { id: cut, time_seconds: 1.0, label: Cut }
audio_events:
  - { id: room, role: ambience, source: room.wav, start_seconds: 0.0, duration_seconds: 2.0, loop_source: true, gain_db: -8 }
  - { id: hit, role: effect, source: hit.wav, start_seconds: 1.0, duration_seconds: 0.25, beat_marker_id: cut }
  - { id: voice, role: narration, source: voice.wav, start_seconds: 0.25, duration_seconds: 1.0 }
narration_ducking: { threshold: 0.03, ratio: 8, attack_ms: 20, release_ms: 300 }
"#,
        )
        .unwrap();

        let loaded = load(&path).unwrap();
        let report = validate(&loaded).unwrap();
        assert_eq!(report.duration_ms, Some(2_000));
        assert_eq!(loaded.manifest.shots[1].media_kind, MediaKind::Video);
        assert_eq!(loaded.manifest.audio_events[0].role, AudioRole::Ambience);

        let invalid = fs::read_to_string(&path).unwrap().replace(
            "start_seconds: 1.0, duration_seconds: 0.25",
            "start_seconds: 1.1, duration_seconds: 0.25",
        );
        fs::write(&path, invalid).unwrap();
        let error = validate(&load(&path).unwrap()).unwrap_err().to_string();
        assert!(error.contains("does not align with beat marker cut"));
    }

    #[test]
    fn score_direction_validates_and_compiles_chapter_plan() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("score.yaml");
        fs::write(
            &path,
            r#"
manifest_version: reel.manifest.v0.2
profile: animatic
timing_status: conformed
work: score-fixture
title: Chapter Score Fixture
scenes:
  - { id: career, duration_seconds: 10.0 }
shots:
  - { id: first, scene_id: career, start_seconds: 0.0, duration_seconds: 4.0 }
  - { id: second, scene_id: career, start_seconds: 4.0, duration_seconds: 6.0 }
beat_markers:
  - { id: opening, time_seconds: 0.0 }
  - { id: desert, time_seconds: 4.0, accent: true }
score:
  originality_policy: original-only
  creative_brief: Carry one motif through changing city palettes without imitating a song or artist.
  global_instruments:
    - { family: brass, role: recurring identity, timbre: warm and human, articulations: [swell, short accent] }
  motifs:
    - { id: next-shift, description: rising three-note persistence idea, instruments: [brass], recurrence_notes: return in every chapter }
  avoid: [copyrighted melodies, artist imitation]
  cues:
    - id: ontario
      start_seconds: 0.0
      duration_seconds: 4.0
      chapter: Ontario
      narrative_function: reflective setup before the move
      mood_from: uncertain
      mood_to: determined
      energy_from: 0.2
      energy_to: 0.4
      tempo_bpm: 72
      meter: 4/4
      style_tags: [movie-montage, reflective]
      motif_ids: [next-shift]
      transition_out: descend into the desert on one unmistakable down-note phrase
      montage_intent: let the longer game calls breathe
      picture_notes: [do not accent every cut]
      sync_points:
        - { id: opening-note, time_seconds: 0.0, kind: downbeat, beat_marker_id: opening, emphasis: 0.5 }
    - id: palm-desert
      start_seconds: 4.0
      duration_seconds: 6.0
      chapter: Palm Desert
      narrative_function: first professional lift
      mood_from: displaced
      mood_to: joyful
      energy_from: 0.35
      energy_to: 0.8
      tempo_bpm: 108
      style_tags: [desert, festival-color]
      instruments:
        - { family: hand-percussion, role: pulse, timbre: dry and sunlit }
        - { family: plucked-strings, role: hook, timbre: airy }
      motif_ids: [next-shift]
      transition_in: start exactly on the geographic arrival
      sync_points:
        - { id: desert-arrival, time_seconds: 4.0, kind: transition, beat_marker_id: desert, emphasis: 0.9 }
"#,
        )
        .unwrap();

        let loaded = load(&path).unwrap();
        let report = validate(&loaded).unwrap();
        assert_eq!(report.score_cues, 2);
        let plan = score_plan(&loaded).unwrap();
        assert_eq!(plan.schema, "reel.score-plan.v0.1");
        assert_eq!(plan.cues[1].chapter, "Palm Desert");
        assert_eq!(plan.cues[1].instruments.len(), 2);

        let invalid = fs::read_to_string(&path)
            .unwrap()
            .replace("tempo_bpm: 108", "tempo_bpm: 400");
        fs::write(&path, invalid).unwrap();
        let error = validate(&load(&path).unwrap()).unwrap_err().to_string();
        assert!(error.contains("tempo_bpm must be between 20 and 320"));
    }

    #[test]
    fn conform_preserves_pause_and_applies_per_speaker_tempo_atomically() {
        let (dir, path) = write_fixture();
        let measurements = dir.path().join("measurements.yaml");
        fs::write(
            &measurements,
            "cues:\n  - { cue_id: poem, duration_ms: 2000 }\n  - { cue_id: prose, duration_ms: 3400 }\n",
        )
        .unwrap();
        let output = dir.path().join("packet");
        let tempos = BTreeMap::from([("narrator".to_string(), 85)]);
        let report = conform(&path, &measurements, &output, &tempos).unwrap();
        assert_eq!(report.duration_ms, 7500);
        let result = load(output.join("manifest.yaml")).unwrap();
        assert_eq!(result.manifest.shots[0].duration_seconds, Some(3.5));
        assert_eq!(result.manifest.shots[1].duration_seconds, Some(4.0));
        assert_eq!(validate(&result).unwrap().duration_ms, Some(7500));
        assert!(output.join("captions.srt").is_file());
        assert!(output.join("lineage.json").is_file());
    }

    #[test]
    fn conform_allocates_one_cue_across_weighted_contiguous_shots_once() {
        let (dir, path) = write_fixture();
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace(
                "narration_cue_ids: [poem]",
                "narration_cue_ids: [poem]\n    allocation_weight: 1",
            )
            .replace(
                "narration_cue_ids: [prose]",
                "narration_cue_ids: [poem, prose]\n    allocation_weight: 3",
            )
            .replace("shot_ids: [shot-01]", "shot_ids: [shot-01, shot-02]");
        fs::write(&path, text).unwrap();
        let measurements = dir.path().join("measurements.yaml");
        fs::write(
            &measurements,
            "cues:\n  - { cue_id: poem, duration_ms: 2000 }\n  - { cue_id: prose, duration_ms: 3400 }\n",
        )
        .unwrap();
        let output = dir.path().join("packet");
        let tempos = BTreeMap::from([("narrator".to_string(), 85)]);
        conform(&path, &measurements, &output, &tempos).unwrap();
        let result = load(output.join("manifest.yaml")).unwrap();
        assert_eq!(result.manifest.shots[0].duration_seconds, Some(0.5));
        assert_eq!(result.manifest.shots[1].duration_seconds, Some(7.0));
        assert_eq!(validate(&result).unwrap().duration_ms, Some(7500));
        let captions = fs::read_to_string(output.join("captions.srt")).unwrap();
        assert!(captions.contains("00:00:00,000 --> 00:00:02,000"));
        assert!(captions.contains("00:00:03,500 --> 00:00:07,500"));
    }

    #[test]
    fn source_coverage_accounts_for_explicit_omission() {
        let (_dir, path) = write_fixture();
        let report = source_coverage(path).unwrap();
        assert!(report.complete);
        assert!(report.uncovered_units.is_empty());
    }

    #[test]
    fn provider_package_never_includes_local_reference_path() {
        let (dir, path) = write_fixture();
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace(
                "continuity: {}",
                "continuity:\n  entities:\n    - id: person-a\n      observations: [thin build, dark hair]\n      reference_assets:\n        - { id: private-photo, sha256: abc, local_path: C:/private/photo.jpg, provider_transfer: forbidden }",
            );
        fs::write(&path, text).unwrap();
        let package = provider_package(&path).unwrap();
        let json = serde_json::to_string(&package).unwrap();
        assert!(!json.contains("C:/private"));
        assert!(!package.blocked);
        drop(dir);
    }

    #[test]
    fn provider_package_blocks_requested_forbidden_asset_but_writes_audit() {
        let (dir, path) = write_fixture();
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace(
                "continuity: {}",
                "continuity:\n  entities:\n    - id: person-a\n      observations: [approved description]\n      reference_assets:\n        - { id: private-photo, sha256: abc, local_path: C:/private/photo.jpg, provider_transfer: forbidden }",
            )
            .replace(
                "review: { required_roles: [editor], status: planning }",
                "review: { required_roles: [editor], status: planning }\nprovider_handoff: { asset_ids: [private-photo], text_fields: [continuity.entities.observations] }",
            );
        fs::write(&path, text).unwrap();
        let package = provider_package(&path).unwrap();
        assert!(package.blocked);
        assert!(
            !serde_json::to_string(&package)
                .unwrap()
                .contains("C:/private")
        );
        let audit = dir.path().join("provider.json");
        assert!(write_provider_package(&path, &audit).is_err());
        assert!(audit.is_file());
    }

    #[test]
    fn migration_writes_derivative_normalizes_timing_and_lifts_legacy_narration() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("legacy.yaml");
        let output = dir.path().join("migrated.yaml");
        let legacy = r#"
schema: reel.scenario-video-manifest.v0.1
work: legacy-work
title: Legacy
scenes:
  - { id: scene-01, duration_seconds: 2.01 }
shots:
  - id: shot-01
    scene_id: scene-01
    start_seconds: 0
    duration_seconds: 1.005
    audio: { narration: First line. }
  - id: shot-02
    scene_id: scene-01
    start_seconds: 1.0
    duration_seconds: 1.005
    audio: { narration: Second line. }
platforms:
  - { name: review, aspect_ratio: '16:9', target_duration_seconds: 2.01 }
exports:
  - { id: review, filename: legacy.mp4, aspect_ratio: '16:9', duration_seconds: 2.01 }
"#;
        fs::write(&input, legacy).unwrap();
        migrate(&input, &output, true).unwrap();
        let loaded = load(&output).unwrap();
        assert_eq!(loaded.manifest.manifest_version, MANIFEST_VERSION);
        assert_eq!(loaded.manifest.speakers[0].id, "legacy-narrator");
        assert_eq!(loaded.manifest.narration_cues.len(), 2);
        assert_eq!(loaded.manifest.shots[1].start_seconds, Some(1.005));
        assert_eq!(validate(&loaded).unwrap().duration_ms, Some(2010));
        assert!(migrate(&input, &input, false).is_err());
    }

    #[test]
    fn review_selection_uses_scene_and_created_time_without_inventing_approval() {
        let dir = tempdir().unwrap();
        for (name, created) in [("z-old.yaml", 10), ("a-new.yaml", 20)] {
            let text = UNTITLED
                .replace("timing_status: untimed", "timing_status: untimed\nlineage:\n  root_work: root\n  scene_key: threshold\n  review_candidate: true")
                .replace("review_candidate: true", &format!("review_candidate: true\n  created_unix: {created}"));
            fs::write(dir.path().join(name), text).unwrap();
        }
        let report = review_select(dir.path()).unwrap();
        let selection = &report.groups["root:threshold"];
        assert!(
            selection
                .latest_review_candidate
                .as_ref()
                .unwrap()
                .ends_with("a-new.yaml")
        );
        assert!(selection.principal_approved.is_empty());
    }

    #[test]
    fn quality_controls_warn_on_long_unprotected_hold() {
        let (dir, path) = write_fixture();
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace("timing_status: untimed", "timing_status: conformed")
            .replace("purpose: test", "duration_seconds: 21, purpose: test")
            .replace("camera: hold", "start_seconds: 0\n    duration_seconds: 20\n    camera: hold")
            .replace("camera: push", "start_seconds: 20\n    duration_seconds: 1\n    camera: push")
            .replace("  - { name: private-review, aspect_ratio: '16:9', sound_optional: false }", "  - { name: private-review, aspect_ratio: '16:9', target_duration_seconds: 21, sound_optional: false }")
            .replace("  - { id: private-review, filename: fixture.mp4, aspect_ratio: '16:9' }", "  - { id: private-review, filename: fixture.mp4, aspect_ratio: '16:9', duration_seconds: 21 }")
            .replace("review: { required_roles: [editor], status: planning }", "review: { required_roles: [editor], status: planning }\nquality_controls: { max_uninterrupted_hold_seconds: 10, require_focal_points: true, require_protected_regions: true }");
        fs::write(&path, text).unwrap();
        let report = quality_check(&path).unwrap();
        assert!(!report.passed);
        assert!(
            report
                .warnings
                .iter()
                .any(|item| item.code == "long-low-motion-hold")
        );
        drop(dir);
    }
}
