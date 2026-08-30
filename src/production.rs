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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MediaKind {
    #[default]
    Still,
    Video,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioRole {
    Music,
    Ambience,
    Effect,
    Narration,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_mastering: Option<AudioMastering>,
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
    pub audio_events: usize,
    pub beat_markers: usize,
    pub narration_ducking: bool,
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
        audio_events: manifest.audio_events.len(),
        beat_markers: manifest.beat_markers.len(),
        narration_ducking: manifest.narration_ducking.is_some(),
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
        let has_asset = shot
            .visual_asset
            .as_deref()
            .is_some_and(|asset| !asset.trim().is_empty());
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
                "shot {} declares selected media without visual_asset",
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
        if shot.media_kind == MediaKind::Still && shot.source_in_seconds != 0.0 {
            bail!("still shot {} cannot declare source_in_seconds", shot.id);
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
        if let Some(event_duration) = event.duration_seconds {
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
        }
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
        expected += duration;
        *shot_by_scene.entry(&shot.scene_id).or_default() += duration;
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
        ) && duration > max
            && matches!(shot.motion.as_str(), "" | "hold" | "hold-dark")
        {
            warnings.push(QualityWarning {
                shot_id: shot.id.clone(),
                code: "long-low-motion-hold".to_string(),
                message: format!("{duration:.3}s hold exceeds configured {max:.3}s maximum"),
            });
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
        if let Some(point) = &shot.focal_point
            && (!(0.0..=1.0).contains(&point.x) || !(0.0..=1.0).contains(&point.y))
        {
            warnings.push(QualityWarning {
                shot_id: shot.id.clone(),
                code: "invalid-focal-point".to_string(),
                message: "focal point coordinates must remain within 0..1".to_string(),
            });
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
        if let (Some(left), Some(right)) = (&pair[0].screen_position, &pair[1].screen_position)
            && left != right
            && pair[0].eye_line == pair[1].eye_line
        {
            warnings.push(QualityWarning {
                shot_id: pair[1].id.clone(),
                code: "screen-direction-review".to_string(),
                message: "screen position changes while eye-line direction is unchanged; review continuity".to_string(),
            });
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

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
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
