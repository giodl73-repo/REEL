use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use tempfile::{Builder, NamedTempFile};

use crate::{
    adapters::ffmpeg::FfmpegAdapter,
    production,
    production_binding::{self, ProductionBinding, ResolvedProductionBinding},
};

pub const CHOREOGRAPHY_SCHEMA: &str = "reel.choreography.v0.1";
pub const PLAN_SCHEMA: &str = "reel.choreography-plan.v0.1";
pub const PREVIEW_SCHEMA: &str = "reel.choreography-preview.v0.1";
pub const ASSET_BINDING_SCHEMA: &str = "reel.choreography-assets.v0.1";
pub const SPRITE_MANIFEST_REPORT_SCHEMA: &str = "reel.choreography-sprite-manifest.v0.1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Choreography {
    pub schema: String,
    pub fps: u32,
    pub duration_frames: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shot_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub production_binding: Option<ProductionBinding>,
    #[serde(default = "default_canvas")]
    pub canvas: CanvasSpec,
    pub stage: Stage,
    pub beats: Vec<Beat>,
    pub performers: BTreeMap<String, Performer>,
    #[serde(default)]
    pub props: BTreeMap<String, Prop>,
    #[serde(default)]
    pub camera: CameraChoreography,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CameraChoreography {
    #[serde(default)]
    pub phrases: Vec<CameraPhrase>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CameraPhrase {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub action: CameraAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub between: [String; 2],
    #[serde(default = "default_camera_zoom")]
    pub zoom_from: f64,
    #[serde(default = "default_camera_zoom")]
    pub zoom_to: f64,
}

fn default_camera_zoom() -> f64 {
    1.0
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CameraAction {
    Hold,
    Follow,
    Whip,
    Settle,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasSpec {
    pub width: u32,
    pub height: u32,
}

fn default_canvas() -> CanvasSpec {
    CanvasSpec {
        width: 640,
        height: 360,
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub marks: BTreeMap<String, Mark>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mark {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Beat {
    pub id: String,
    pub frame: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Performer {
    pub start: String,
    #[serde(default = "default_performer_color")]
    pub color: String,
    #[serde(default = "default_performer_radius")]
    pub radius: f64,
    #[serde(default)]
    pub phrases: Vec<Phrase>,
}

fn default_performer_color() -> String {
    "#4da3ff".to_string()
}

fn default_performer_radius() -> f64 {
    0.035
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Prop {
    pub owner: String,
    #[serde(default = "default_prop_color")]
    pub color: String,
    #[serde(default = "default_prop_radius")]
    pub radius: f64,
}

fn default_prop_color() -> String {
    "#ffd23f".to_string()
}

fn default_prop_radius() -> f64 {
    0.012
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Phrase {
    Approach {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        to: String,
        between: [String; 2],
        #[serde(default)]
        path: SpatialPath,
        #[serde(default)]
        timing: TimingCurve,
    },
    Handoff {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        prop: String,
        target: String,
        between: [String; 2],
        #[serde(default = "default_handoff_path")]
        path: SpatialPath,
        #[serde(default = "default_handoff_timing")]
        timing: TimingCurve,
    },
    React {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        at: String,
        pose: String,
    },
}

fn default_handoff_path() -> SpatialPath {
    SpatialPath::ArcRight
}

fn default_handoff_timing() -> TimingCurve {
    TimingCurve::EaseInOut
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpatialPath {
    #[default]
    Linear,
    ArcLeft,
    ArcRight,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TimingCurve {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    HoldThenBurst,
}

#[derive(Clone, Debug)]
pub struct LoadedChoreography {
    pub path: PathBuf,
    pub source_sha256: String,
    pub choreography: Choreography,
}

#[derive(Clone, Debug, Serialize)]
pub struct ValidationReport {
    pub schema: String,
    pub source_sha256: String,
    pub fps: u32,
    pub duration_frames: u32,
    pub duration_seconds: f64,
    pub marks: usize,
    pub beats: usize,
    pub performers: usize,
    pub props: usize,
    pub approach_phrases: usize,
    pub handoff_phrases: usize,
    pub react_phrases: usize,
    pub production_bound: bool,
    pub camera_phrases: usize,
    pub passed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChoreographyPlan {
    pub schema: String,
    pub source_schema: String,
    pub source_sha256: String,
    pub fps: u32,
    pub duration_frames: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub production_binding: Option<ResolvedProductionBinding>,
    pub canvas: CanvasSpec,
    pub marks: BTreeMap<String, Mark>,
    pub beats: Vec<Beat>,
    pub performers: Vec<ResolvedPerformer>,
    pub props: Vec<ResolvedProp>,
    pub reactions: Vec<ResolvedReaction>,
    pub camera: Vec<ResolvedCameraSegment>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResolvedCameraSegment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phrase_id: Option<String>,
    pub action: CameraAction,
    pub target: Option<String>,
    pub start_frame: u32,
    pub end_frame: u32,
    pub from: Mark,
    pub to: Mark,
    pub zoom_from: f64,
    pub zoom_to: f64,
    pub timing: TimingCurve,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResolvedPerformer {
    pub id: String,
    pub color: String,
    pub radius: f64,
    pub start: Mark,
    pub segments: Vec<MotionSegment>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MotionSegment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phrase_id: Option<String>,
    pub action: String,
    pub start_frame: u32,
    pub end_frame: u32,
    pub from: Mark,
    pub to: Mark,
    pub path: SpatialPath,
    pub timing: TimingCurve,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResolvedProp {
    pub id: String,
    pub color: String,
    pub radius: f64,
    pub initial_owner: String,
    pub handoffs: Vec<ResolvedHandoff>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResolvedHandoff {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phrase_id: Option<String>,
    pub start_frame: u32,
    pub end_frame: u32,
    pub from_owner: String,
    pub to_owner: String,
    pub path: SpatialPath,
    pub timing: TimingCurve,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResolvedReaction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phrase_id: Option<String>,
    pub performer: String,
    pub frame: u32,
    pub pose: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PreviewFile {
    pub role: String,
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PreviewReport {
    pub schema: String,
    pub source_sha256: String,
    pub plan_sha256: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_frames: u32,
    pub duration_ms: u64,
    pub ffmpeg_version: String,
    pub files: Vec<PreviewFile>,
    pub passed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChoreographyAssets {
    pub schema: String,
    pub choreography_sha256: String,
    pub background: String,
    pub performers: BTreeMap<String, PerformerAssets>,
    pub props: BTreeMap<String, PropAssets>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PerformerAssets {
    pub default_asset: String,
    #[serde(default)]
    pub poses: BTreeMap<String, String>,
    pub width: f64,
    #[serde(default)]
    pub z_index: i32,
    #[serde(default = "default_anchor_x")]
    pub anchor_x: f64,
    #[serde(default = "default_anchor_y")]
    pub anchor_y: f64,
}

fn default_anchor_x() -> f64 {
    0.5
}

fn default_anchor_y() -> f64 {
    0.5
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PropAssets {
    pub asset: String,
    pub width: f64,
    #[serde(default)]
    pub z_index: i32,
}

#[derive(Clone, Debug, Serialize)]
pub struct SpriteManifestReport {
    pub schema: String,
    pub choreography_sha256: String,
    pub asset_binding_sha256: String,
    pub production_manifest_sha256: String,
    pub bound_shot_id: String,
    pub output_sha256: String,
    pub performers: usize,
    pub props: usize,
    pub camera_phrases: usize,
    pub passed: bool,
}

pub fn load(path: impl AsRef<Path>) -> Result<LoadedChoreography> {
    let path = path.as_ref();
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read choreography {}", path.display()))?;
    let choreography = serde_yaml::from_str::<Choreography>(&source)
        .with_context(|| format!("failed to parse choreography {}", path.display()))?;
    Ok(LoadedChoreography {
        path: path.to_path_buf(),
        source_sha256: production::sha256_path(path)?,
        choreography,
    })
}

fn validate_contract(loaded: &LoadedChoreography) -> Result<ValidationReport> {
    let choreography = &loaded.choreography;
    if choreography.schema != CHOREOGRAPHY_SCHEMA {
        bail!(
            "unsupported choreography schema {}; expected {CHOREOGRAPHY_SCHEMA}",
            choreography.schema
        );
    }
    if !(1..=120).contains(&choreography.fps) {
        bail!("choreography fps must be between 1 and 120");
    }
    if choreography.duration_frames < 2 {
        bail!("choreography duration_frames must be at least 2");
    }
    if !(160..=3840).contains(&choreography.canvas.width)
        || !(90..=2160).contains(&choreography.canvas.height)
    {
        bail!("choreography canvas must be between 160x90 and 3840x2160");
    }
    if choreography.stage.marks.is_empty() {
        bail!("choreography stage must declare at least one mark");
    }
    for (id, mark) in &choreography.stage.marks {
        validate_id("mark", id)?;
        if !in_unit(mark.x) || !in_unit(mark.y) {
            bail!("mark {id} coordinates must be finite values from 0 to 1");
        }
    }

    let mut beat_ids = BTreeSet::new();
    let mut previous_frame = None;
    for beat in &choreography.beats {
        validate_id("beat", &beat.id)?;
        if !beat_ids.insert(beat.id.as_str()) {
            bail!("duplicate choreography beat {}", beat.id);
        }
        if beat.frame >= choreography.duration_frames {
            bail!("beat {} falls outside choreography duration", beat.id);
        }
        if previous_frame.is_some_and(|frame| beat.frame <= frame) {
            bail!("choreography beats must use strictly increasing frames");
        }
        previous_frame = Some(beat.frame);
    }
    if choreography.beats.is_empty() {
        bail!("choreography must declare at least one beat");
    }

    let beat_frames = choreography
        .beats
        .iter()
        .map(|beat| (beat.id.as_str(), beat.frame))
        .collect::<BTreeMap<_, _>>();
    let performer_ids = choreography
        .performers
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if performer_ids.is_empty() {
        bail!("choreography must declare at least one performer");
    }
    let mut approaches = 0;
    let mut handoffs = 0;
    let mut reactions = 0;
    for (id, performer) in &choreography.performers {
        validate_id("performer", id)?;
        require_mark(choreography, &performer.start, "performer start")?;
        parse_color(&performer.color)
            .with_context(|| format!("performer {id} has invalid color"))?;
        validate_radius("performer", id, performer.radius)?;
        let mut motion_ranges = Vec::new();
        let mut phrase_ids = BTreeSet::new();
        for phrase in &performer.phrases {
            if let Some(phrase_id) = phrase.id() {
                validate_id("phrase", phrase_id)?;
                if !phrase_ids.insert(phrase_id) {
                    bail!("performer {id} has duplicate phrase id {phrase_id}");
                }
            }
            match phrase {
                Phrase::Approach {
                    id: _,
                    to,
                    between,
                    path: _,
                    timing: _,
                } => {
                    approaches += 1;
                    require_mark(choreography, to, "approach destination")?;
                    let range = beat_range(&beat_frames, between)?;
                    motion_ranges.push(range);
                }
                Phrase::Handoff {
                    id: _,
                    prop,
                    target,
                    between,
                    path: _,
                    timing: _,
                } => {
                    handoffs += 1;
                    if !choreography.props.contains_key(prop) {
                        bail!("performer {id} hands off unknown prop {prop}");
                    }
                    if !performer_ids.contains(target.as_str()) {
                        bail!("performer {id} hands off to unknown performer {target}");
                    }
                    if target == id {
                        bail!("performer {id} cannot hand a prop to itself");
                    }
                    beat_range(&beat_frames, between)?;
                }
                Phrase::React { id: _, at, pose } => {
                    reactions += 1;
                    require_beat(&beat_frames, at)?;
                    if pose.trim().is_empty() {
                        bail!("performer {id} reaction pose cannot be empty");
                    }
                }
            }
        }
        motion_ranges.sort_unstable();
        for pair in motion_ranges.windows(2) {
            if pair[1].0 < pair[0].1 {
                bail!("performer {id} has overlapping approach phrases");
            }
        }
    }
    for (id, prop) in &choreography.props {
        validate_id("prop", id)?;
        if !performer_ids.contains(prop.owner.as_str()) {
            bail!("prop {id} has unknown initial owner {}", prop.owner);
        }
        parse_color(&prop.color).with_context(|| format!("prop {id} has invalid color"))?;
        validate_radius("prop", id, prop.radius)?;
    }

    validate_handoff_ownership(choreography, &beat_frames)?;
    validate_camera(choreography, &beat_frames, &performer_ids)?;
    let resolved_binding = resolve_production_binding(loaded)?;

    Ok(ValidationReport {
        schema: choreography.schema.clone(),
        source_sha256: loaded.source_sha256.clone(),
        fps: choreography.fps,
        duration_frames: choreography.duration_frames,
        duration_seconds: f64::from(choreography.duration_frames) / f64::from(choreography.fps),
        marks: choreography.stage.marks.len(),
        beats: choreography.beats.len(),
        performers: choreography.performers.len(),
        props: choreography.props.len(),
        approach_phrases: approaches,
        handoff_phrases: handoffs,
        react_phrases: reactions,
        production_bound: resolved_binding.is_some(),
        camera_phrases: choreography.camera.phrases.len(),
        passed: true,
    })
}

pub fn validate(loaded: &LoadedChoreography) -> Result<ValidationReport> {
    let report = validate_contract(loaded)?;
    let plan = compile_unchecked(loaded);
    validate_resolved_bounds(&plan)?;
    Ok(report)
}

pub fn compile(loaded: &LoadedChoreography) -> Result<ChoreographyPlan> {
    validate_contract(loaded)?;
    let plan = compile_unchecked(loaded);
    validate_resolved_bounds(&plan)?;
    Ok(plan)
}

fn compile_unchecked(loaded: &LoadedChoreography) -> ChoreographyPlan {
    let choreography = &loaded.choreography;
    let beat_frames = choreography
        .beats
        .iter()
        .map(|beat| (beat.id.as_str(), beat.frame))
        .collect::<BTreeMap<_, _>>();
    let mut performers = Vec::new();
    let mut reactions = Vec::new();
    for (id, performer) in &choreography.performers {
        let start = *choreography
            .stage
            .marks
            .get(&performer.start)
            .expect("validated performer start");
        let mut approaches = performer
            .phrases
            .iter()
            .filter_map(|phrase| match phrase {
                Phrase::Approach {
                    id,
                    to,
                    between,
                    path,
                    timing,
                } => Some((
                    beat_frames[between[0].as_str()],
                    beat_frames[between[1].as_str()],
                    *choreography.stage.marks.get(to).expect("validated mark"),
                    *path,
                    *timing,
                    id.clone(),
                )),
                _ => None,
            })
            .collect::<Vec<_>>();
        approaches.sort_by_key(|approach| approach.0);
        let mut from = start;
        let segments = approaches
            .into_iter()
            .map(|(start_frame, end_frame, to, path, timing, phrase_id)| {
                let segment = MotionSegment {
                    phrase_id,
                    action: "approach".to_string(),
                    start_frame,
                    end_frame,
                    from,
                    to,
                    path,
                    timing,
                };
                from = to;
                segment
            })
            .collect();
        for phrase in &performer.phrases {
            if let Phrase::React {
                id: phrase_id,
                at,
                pose,
            } = phrase
            {
                reactions.push(ResolvedReaction {
                    phrase_id: phrase_id.clone(),
                    performer: id.clone(),
                    frame: beat_frames[at.as_str()],
                    pose: pose.clone(),
                });
            }
        }
        performers.push(ResolvedPerformer {
            id: id.clone(),
            color: performer.color.clone(),
            radius: performer.radius,
            start,
            segments,
        });
    }
    reactions.sort_by_key(|reaction| reaction.frame);

    let mut props = Vec::new();
    for (id, prop) in &choreography.props {
        let mut handoffs = Vec::new();
        for (performer_id, performer) in &choreography.performers {
            for phrase in &performer.phrases {
                if let Phrase::Handoff {
                    id: phrase_id,
                    prop: phrase_prop,
                    target,
                    between,
                    path,
                    timing,
                } = phrase
                {
                    if phrase_prop == id {
                        handoffs.push(ResolvedHandoff {
                            phrase_id: phrase_id.clone(),
                            start_frame: beat_frames[between[0].as_str()],
                            end_frame: beat_frames[between[1].as_str()],
                            from_owner: performer_id.clone(),
                            to_owner: target.clone(),
                            path: *path,
                            timing: *timing,
                        });
                    }
                }
            }
        }
        handoffs.sort_by_key(|handoff| handoff.start_frame);
        props.push(ResolvedProp {
            id: id.clone(),
            color: prop.color.clone(),
            radius: prop.radius,
            initial_owner: prop.owner.clone(),
            handoffs,
        });
    }

    let camera = compile_camera(choreography, &beat_frames, &performers);
    ChoreographyPlan {
        schema: PLAN_SCHEMA.to_string(),
        source_schema: choreography.schema.clone(),
        source_sha256: loaded.source_sha256.clone(),
        fps: choreography.fps,
        duration_frames: choreography.duration_frames,
        production_binding: resolve_production_binding(loaded)
            .expect("production binding validated"),
        canvas: choreography.canvas.clone(),
        marks: choreography.stage.marks.clone(),
        beats: choreography.beats.clone(),
        performers,
        props,
        reactions,
        camera,
    }
}

fn validate_camera(
    choreography: &Choreography,
    beats: &BTreeMap<&str, u32>,
    performers: &BTreeSet<&str>,
) -> Result<()> {
    let mut ids = BTreeSet::new();
    let mut ranges = Vec::new();
    for phrase in &choreography.camera.phrases {
        if let Some(id) = phrase.id.as_deref() {
            validate_id("camera phrase", id)?;
            if !ids.insert(id) {
                bail!("duplicate camera phrase id {id}");
            }
        }
        let range = beat_range(beats, &phrase.between)?;
        ranges.push(range);
        if !phrase.zoom_from.is_finite()
            || !phrase.zoom_to.is_finite()
            || !(1.0..=4.0).contains(&phrase.zoom_from)
            || !(1.0..=4.0).contains(&phrase.zoom_to)
        {
            bail!("camera phrase zoom values must be between 1 and 4");
        }
        if matches!(phrase.action, CameraAction::Follow | CameraAction::Whip)
            && phrase.target.is_none()
        {
            bail!("camera {:?} phrase requires a target", phrase.action);
        }
        if let Some(target) = phrase.target.as_deref() {
            if !performers.contains(target) && !choreography.stage.marks.contains_key(target) {
                bail!("camera phrase references unknown target {target}");
            }
        }
    }
    ranges.sort_unstable();
    for pair in ranges.windows(2) {
        if pair[1].0 < pair[0].1 {
            bail!("camera phrases cannot overlap");
        }
    }
    Ok(())
}

fn compile_camera(
    choreography: &Choreography,
    beats: &BTreeMap<&str, u32>,
    performers: &[ResolvedPerformer],
) -> Vec<ResolvedCameraSegment> {
    let mut phrases = choreography.camera.phrases.iter().collect::<Vec<_>>();
    phrases.sort_by_key(|phrase| beats[phrase.between[0].as_str()]);
    let mut center = Mark { x: 0.5, y: 0.5 };
    let mut segments = Vec::new();
    for phrase in phrases {
        let start_frame = beats[phrase.between[0].as_str()];
        let end_frame = beats[phrase.between[1].as_str()];
        let target_at = |frame| {
            phrase.target.as_deref().map_or(center, |target| {
                if let Some(performer) = performers.iter().find(|item| item.id == target) {
                    track_position(performer.start, &performer.segments, frame)
                } else {
                    *choreography
                        .stage
                        .marks
                        .get(target)
                        .expect("validated camera target")
                }
            })
        };
        let (from, to, timing) = match phrase.action {
            CameraAction::Hold => (center, center, TimingCurve::Linear),
            CameraAction::Follow => (center, target_at(end_frame), TimingCurve::EaseInOut),
            CameraAction::Whip => (center, target_at(end_frame), TimingCurve::HoldThenBurst),
            CameraAction::Settle => (center, target_at(end_frame), TimingCurve::EaseOut),
        };
        center = to;
        segments.push(ResolvedCameraSegment {
            phrase_id: phrase.id.clone(),
            action: phrase.action,
            target: phrase.target.clone(),
            start_frame,
            end_frame,
            from,
            to,
            zoom_from: phrase.zoom_from,
            zoom_to: phrase.zoom_to,
            timing,
        });
    }
    segments
}

fn resolve_production_binding(
    loaded: &LoadedChoreography,
) -> Result<Option<ResolvedProductionBinding>> {
    let Some(binding) = &loaded.choreography.production_binding else {
        if loaded.choreography.shot_ref.is_some() {
            bail!("choreography shot_ref requires production_binding");
        }
        return Ok(None);
    };
    let shot_ref = loaded
        .choreography
        .shot_ref
        .as_deref()
        .ok_or_else(|| anyhow!("production-bound choreography requires shot_ref"))?;
    let bound = production_binding::resolve(&loaded.path, binding)?;
    let shot = production_binding::require_shot(&bound.resolved, shot_ref)?;
    let duration_ms =
        u64::from(loaded.choreography.duration_frames) * 1_000 / u64::from(loaded.choreography.fps);
    let half_frame_ms = (500.0 / f64::from(loaded.choreography.fps)).ceil() as u64;
    if duration_ms.abs_diff(shot.duration_ms) > half_frame_ms {
        bail!(
            "choreography duration {}ms does not match bound shot {} duration {}ms",
            duration_ms,
            shot.shot_id,
            shot.duration_ms
        );
    }
    for beat in &loaded.choreography.beats {
        let resolved = production_binding::require_beat(&bound.resolved, &beat.id)?;
        let local_ms = u64::from(beat.frame) * 1_000 / u64::from(loaded.choreography.fps);
        let expected_ms = shot.start_ms + local_ms;
        if expected_ms.abs_diff(resolved.time_ms) > half_frame_ms {
            bail!(
                "choreography beat {} at {}ms does not align with bound beat {} at {}ms",
                beat.id,
                expected_ms,
                resolved.beat_id,
                resolved.time_ms
            );
        }
    }
    Ok(Some(bound.resolved))
}

pub fn write_plan(plan: &ChoreographyPlan, output: impl AsRef<Path>) -> Result<()> {
    let output = output.as_ref();
    if output.exists() {
        bail!(
            "refusing to overwrite choreography plan {}",
            output.display()
        );
    }
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(format!("{}\n", serde_json::to_string_pretty(plan)?).as_bytes())?;
    temporary
        .persist(output)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish choreography plan {}", output.display()))?;
    Ok(())
}

pub fn write_sprite_manifest(
    loaded: &LoadedChoreography,
    asset_binding_path: impl AsRef<Path>,
    output: impl AsRef<Path>,
) -> Result<SpriteManifestReport> {
    let plan = compile(loaded)?;
    let resolved_binding = plan
        .production_binding
        .as_ref()
        .ok_or_else(|| anyhow!("sprite manifest execution requires production_binding"))?;
    let shot_ref = loaded
        .choreography
        .shot_ref
        .as_deref()
        .ok_or_else(|| anyhow!("sprite manifest execution requires shot_ref"))?;
    let bound_shot = production_binding::require_shot(resolved_binding, shot_ref)?;
    let asset_binding_path = asset_binding_path.as_ref();
    let assets: ChoreographyAssets = serde_yaml::from_slice(&fs::read(asset_binding_path)?)
        .with_context(|| {
            format!(
                "failed to parse choreography asset binding {}",
                asset_binding_path.display()
            )
        })?;
    validate_assets(loaded, &plan, &assets)?;
    let asset_binding_sha256 = production::sha256_path(asset_binding_path)?;
    let binding = loaded
        .choreography
        .production_binding
        .as_ref()
        .expect("compiled plan is production bound");
    let bound = production_binding::resolve(&loaded.path, binding)?;
    let source_shot = bound
        .loaded
        .manifest
        .shots
        .iter()
        .find(|shot| shot.id == bound_shot.shot_id)
        .ok_or_else(|| anyhow!("bound production lost shot {}", bound_shot.shot_id))?;
    let source_scene = bound
        .loaded
        .manifest
        .scenes
        .iter()
        .find(|scene| scene.id == source_shot.scene_id)
        .ok_or_else(|| anyhow!("bound production lost scene {}", source_shot.scene_id))?;
    let duration_seconds = f64::from(plan.duration_frames) / f64::from(plan.fps);
    let mut manifest = bound.loaded.manifest.clone();
    manifest.work = format!("{}-{}-choreography", manifest.work, bound_shot.shot_id);
    manifest.title = format!("{} — choreography execution", manifest.title);
    let mut scene = source_scene.clone();
    scene.duration_seconds = Some(duration_seconds);
    manifest.scenes = vec![scene];
    let mut shot = source_shot.clone();
    shot.start_seconds = Some(0.0);
    shot.duration_seconds = Some(duration_seconds);
    shot.visual_asset = None;
    shot.animation = None;
    shot.media_kind = production::MediaKind::SpriteAnimation;
    shot.motion = "hold".to_string();
    shot.beat_marker_id = None;
    shot.sprite_animation = Some(build_sprite_animation(&plan, &assets)?);
    shot.extra.insert(
        "camera_choreography".to_string(),
        serde_yaml::to_value(&plan.camera)?,
    );
    manifest.shots = vec![shot];
    manifest.beat_markers = loaded
        .choreography
        .beats
        .iter()
        .map(|beat| production::BeatMarker {
            id: beat.id.clone(),
            time_seconds: f64::from(beat.frame) / f64::from(plan.fps),
            label: String::new(),
            accent: false,
        })
        .collect();
    manifest.audio_events.clear();
    manifest.score = None;
    manifest.speakers.clear();
    manifest.narration_cues.clear();
    manifest.protected_pauses.clear();
    for platform in &mut manifest.platforms {
        platform.target_duration_seconds = Some(duration_seconds);
    }
    manifest.exports.truncate(1);
    if let Some(export) = manifest.exports.first_mut() {
        export.duration_seconds = Some(duration_seconds);
        export.filename = "choreography-execution.mp4".to_string();
    }
    manifest.extra.insert(
        "choreography_execution".to_string(),
        serde_yaml::to_value(serde_json::json!({
            "schema": SPRITE_MANIFEST_REPORT_SCHEMA,
            "choreography_sha256": loaded.source_sha256,
            "asset_binding_sha256": asset_binding_sha256,
            "production_manifest_sha256": resolved_binding.manifest_sha256,
            "bound_shot_id": bound_shot.shot_id,
        }))?,
    );
    let serialized = serde_yaml::to_string(&manifest)?;
    let verification = production::LoadedProductionManifest {
        path: output.as_ref().to_path_buf(),
        manifest: manifest.clone(),
        bytes: serialized.as_bytes().to_vec(),
    };
    production::validate(&verification)?;
    write_atomic_new(output.as_ref(), serialized.as_bytes())?;
    Ok(SpriteManifestReport {
        schema: SPRITE_MANIFEST_REPORT_SCHEMA.to_string(),
        choreography_sha256: loaded.source_sha256.clone(),
        asset_binding_sha256,
        production_manifest_sha256: resolved_binding.manifest_sha256.clone(),
        bound_shot_id: bound_shot.shot_id.clone(),
        output_sha256: production::sha256_path(output.as_ref())?,
        performers: plan.performers.len(),
        props: plan.props.len(),
        camera_phrases: plan.camera.len(),
        passed: true,
    })
}

fn validate_assets(
    loaded: &LoadedChoreography,
    plan: &ChoreographyPlan,
    assets: &ChoreographyAssets,
) -> Result<()> {
    if assets.schema != ASSET_BINDING_SCHEMA {
        bail!(
            "unsupported choreography asset schema {}; expected {ASSET_BINDING_SCHEMA}",
            assets.schema
        );
    }
    if assets.choreography_sha256 != loaded.source_sha256 {
        bail!("choreography asset binding hash does not match choreography sidecar");
    }
    if assets.background.trim().is_empty() {
        bail!("choreography asset binding requires background");
    }
    let performer_ids = plan
        .performers
        .iter()
        .map(|performer| performer.id.as_str())
        .collect::<BTreeSet<_>>();
    let asset_performers = assets
        .performers
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if performer_ids != asset_performers {
        bail!("choreography asset binding performer set does not match resolved plan");
    }
    let prop_ids = plan
        .props
        .iter()
        .map(|prop| prop.id.as_str())
        .collect::<BTreeSet<_>>();
    let asset_props = assets
        .props
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if prop_ids != asset_props {
        bail!("choreography asset binding prop set does not match resolved plan");
    }
    for performer in &plan.performers {
        let binding = &assets.performers[&performer.id];
        if binding.default_asset.trim().is_empty()
            || !binding.width.is_finite()
            || !(0.001..=2.0).contains(&binding.width)
            || !in_unit(binding.anchor_x)
            || !in_unit(binding.anchor_y)
        {
            bail!("performer {} asset geometry is invalid", performer.id);
        }
        for reaction in plan
            .reactions
            .iter()
            .filter(|reaction| reaction.performer == performer.id)
        {
            if !binding.poses.contains_key(&reaction.pose) {
                bail!(
                    "performer {} has no asset for reaction pose {}",
                    performer.id,
                    reaction.pose
                );
            }
        }
    }
    for prop in &plan.props {
        let binding = &assets.props[&prop.id];
        if binding.asset.trim().is_empty()
            || !binding.width.is_finite()
            || !(0.001..=2.0).contains(&binding.width)
        {
            bail!("prop {} asset geometry is invalid", prop.id);
        }
    }
    Ok(())
}

fn build_sprite_animation(
    plan: &ChoreographyPlan,
    assets: &ChoreographyAssets,
) -> Result<production::SpriteAnimation> {
    let mut sprites = Vec::new();
    for performer in &plan.performers {
        let binding = &assets.performers[&performer.id];
        let mut frames = BTreeSet::from([0, plan.duration_frames - 1]);
        for segment in &performer.segments {
            for step in 0..=6 {
                frames.insert(
                    segment.start_frame + (segment.end_frame - segment.start_frame) * step / 6,
                );
            }
        }
        for reaction in plan
            .reactions
            .iter()
            .filter(|reaction| reaction.performer == performer.id)
        {
            frames.insert(reaction.frame);
            frames.insert((reaction.frame + 8).min(plan.duration_frames - 1));
        }
        let keyframes = frames
            .into_iter()
            .map(|frame| {
                let position = performer_position(plan, &performer.id, frame)?;
                let active_pose = plan.reactions.iter().find(|reaction| {
                    reaction.performer == performer.id
                        && frame >= reaction.frame
                        && frame < reaction.frame.saturating_add(8)
                });
                let asset = active_pose.map_or_else(
                    || binding.default_asset.clone(),
                    |reaction| binding.poses[&reaction.pose].clone(),
                );
                Ok(production::SpriteKeyframe {
                    frame,
                    asset,
                    z_index: None,
                    x: position.x,
                    y: position.y,
                    width: binding.width,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        sprites.push(production::SpriteTrack {
            id: performer.id.clone(),
            z_index: binding.z_index,
            anchor_x: Some(binding.anchor_x),
            anchor_y: Some(binding.anchor_y),
            movement: production::SpriteMovement::Linear,
            movement_steps: None,
            keyframes,
        });
    }
    for prop in &plan.props {
        let binding = &assets.props[&prop.id];
        let mut frames = BTreeSet::from([0, plan.duration_frames - 1]);
        for handoff in &prop.handoffs {
            for step in 0..=8 {
                frames.insert(
                    handoff.start_frame + (handoff.end_frame - handoff.start_frame) * step / 8,
                );
            }
        }
        let keyframes = frames
            .into_iter()
            .map(|frame| {
                let position = prop_position(plan, prop, frame)?;
                Ok(production::SpriteKeyframe {
                    frame,
                    asset: binding.asset.clone(),
                    z_index: None,
                    x: position.x,
                    y: position.y,
                    width: binding.width,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        sprites.push(production::SpriteTrack {
            id: prop.id.clone(),
            z_index: binding.z_index,
            anchor_x: Some(0.5),
            anchor_y: Some(0.5),
            movement: production::SpriteMovement::Linear,
            movement_steps: None,
            keyframes,
        });
    }
    let mut camera = BTreeMap::new();
    for segment in &plan.camera {
        let curve = match segment.timing {
            TimingCurve::Linear | TimingCurve::EaseIn => production::SpriteCameraCurve::Linear,
            TimingCurve::EaseInOut => production::SpriteCameraCurve::EaseInOut,
            TimingCurve::EaseOut => production::SpriteCameraCurve::EaseOut,
            TimingCurve::HoldThenBurst => production::SpriteCameraCurve::HoldThenBurst,
        };
        let safe = |value: f64, zoom: f64| value.clamp(0.5 / zoom, 1.0 - 0.5 / zoom);
        camera
            .entry(segment.start_frame)
            .and_modify(|keyframe: &mut production::SpriteCameraKeyframe| {
                keyframe.curve_to_next = curve;
            })
            .or_insert_with(|| production::SpriteCameraKeyframe {
                frame: segment.start_frame,
                center_x: safe(segment.from.x, segment.zoom_from),
                center_y: safe(segment.from.y, segment.zoom_from),
                zoom: segment.zoom_from,
                curve_to_next: curve,
            });
        camera
            .entry(segment.end_frame)
            .or_insert_with(|| production::SpriteCameraKeyframe {
                frame: segment.end_frame,
                center_x: safe(segment.to.x, segment.zoom_to),
                center_y: safe(segment.to.y, segment.zoom_to),
                zoom: segment.zoom_to,
                curve_to_next: production::SpriteCameraCurve::Linear,
            });
    }
    if let Some(last) = camera.values().next_back().cloned()
        && last.frame < plan.duration_frames - 1
    {
        camera.insert(
            plan.duration_frames - 1,
            production::SpriteCameraKeyframe {
                frame: plan.duration_frames - 1,
                ..last
            },
        );
    }
    Ok(production::SpriteAnimation {
        background: assets.background.clone(),
        timing_fps: plan.fps,
        sprites,
        camera: camera.into_values().collect(),
    })
}

fn write_atomic_new(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        bail!("refusing to overwrite {}", path.display());
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish {}", path.display()))?;
    Ok(())
}

pub fn render_preview(
    loaded: &LoadedChoreography,
    output_dir: impl AsRef<Path>,
) -> Result<PreviewReport> {
    let plan = compile(loaded)?;
    let output_dir = output_dir.as_ref();
    if output_dir.exists() && fs::read_dir(output_dir)?.next().is_some() {
        bail!(
            "choreography preview output directory must be absent or empty: {}",
            output_dir.display()
        );
    }
    let parent = output_dir.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = Builder::new()
        .prefix(".reel-choreography-")
        .tempdir_in(parent)
        .context("failed to create choreography preview staging directory")?;
    let frames_dir = staging.path().join("frames");
    fs::create_dir(&frames_dir)?;

    let plan_path = staging.path().join("resolved-plan.json");
    fs::write(
        &plan_path,
        format!("{}\n", serde_json::to_string_pretty(&plan)?),
    )?;
    for frame in 0..plan.duration_frames {
        let canvas = render_frame(&plan, frame, false)?;
        canvas.write_ppm(&frames_dir.join(format!("frame-{frame:06}.ppm")))?;
    }
    let path_ppm = staging.path().join("paths.ppm");
    render_frame(&plan, plan.duration_frames - 1, true)?.write_ppm(&path_ppm)?;

    let adapter = FfmpegAdapter;
    let ffmpeg_version = adapter.run_ffmpeg(&["-version".to_string()], &[])?;
    let video_path = staging.path().join("blocking-preview.mp4");
    adapter.run_ffmpeg(
        &[
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-y".to_string(),
            "-framerate".to_string(),
            plan.fps.to_string(),
            "-start_number".to_string(),
            "0".to_string(),
            "-i".to_string(),
        ],
        &[
            adapter.path_argument(&frames_dir.join("frame-%06d.ppm"))?,
            "-frames:v".to_string(),
            plan.duration_frames.to_string(),
            "-c:v".to_string(),
            "libx264".to_string(),
            "-preset".to_string(),
            "veryfast".to_string(),
            "-pix_fmt".to_string(),
            "yuv420p".to_string(),
            "-movflags".to_string(),
            "+faststart".to_string(),
            adapter.path_argument(&video_path)?,
        ],
    )?;

    let path_png = staging.path().join("path-overlay.png");
    convert_image(&adapter, &path_ppm, &path_png)?;
    let contact_path = staging.path().join("contact-sheet.png");
    let samples = [0, (plan.duration_frames - 1) / 2, plan.duration_frames - 1];
    let sample_paths = samples
        .iter()
        .map(|frame| frames_dir.join(format!("frame-{frame:06}.ppm")))
        .collect::<Vec<_>>();
    render_contact_sheet(&adapter, &sample_paths, &contact_path)?;
    fs::remove_file(path_ppm)?;
    fs::remove_dir_all(frames_dir)?;

    let plan_sha256 = production::sha256_path(&plan_path)?;
    let mut files = Vec::new();
    for (role, file, path) in [
        ("resolved-plan", "resolved-plan.json", &plan_path),
        ("blocking-preview", "blocking-preview.mp4", &video_path),
        ("path-overlay", "path-overlay.png", &path_png),
        ("contact-sheet", "contact-sheet.png", &contact_path),
    ] {
        files.push(PreviewFile {
            role: role.to_string(),
            file: file.to_string(),
            bytes: fs::metadata(path)?.len(),
            sha256: production::sha256_path(path)?,
        });
    }
    let report = PreviewReport {
        schema: PREVIEW_SCHEMA.to_string(),
        source_sha256: loaded.source_sha256.clone(),
        plan_sha256,
        width: plan.canvas.width,
        height: plan.canvas.height,
        fps: plan.fps,
        duration_frames: plan.duration_frames,
        duration_ms: u64::from(plan.duration_frames) * 1_000 / u64::from(plan.fps),
        ffmpeg_version: ffmpeg_version
            .lines()
            .next()
            .unwrap_or("unknown")
            .trim()
            .to_string(),
        files,
        passed: true,
    };
    fs::write(
        staging.path().join("preview-report.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    if output_dir.exists() {
        fs::remove_dir(output_dir).with_context(|| {
            format!(
                "failed to replace empty output directory {}",
                output_dir.display()
            )
        })?;
    }
    fs::rename(staging.path(), output_dir).with_context(|| {
        format!(
            "failed to publish choreography preview {}",
            output_dir.display()
        )
    })?;
    Ok(report)
}

fn validate_handoff_ownership(
    choreography: &Choreography,
    beats: &BTreeMap<&str, u32>,
) -> Result<()> {
    let mut by_prop: BTreeMap<&str, Vec<(u32, u32, &str, &str)>> = BTreeMap::new();
    for (performer_id, performer) in &choreography.performers {
        for phrase in &performer.phrases {
            if let Phrase::Handoff {
                id: _,
                prop,
                target,
                between,
                ..
            } = phrase
            {
                let (start, end) = beat_range(beats, between)?;
                by_prop.entry(prop).or_default().push((
                    start,
                    end,
                    performer_id.as_str(),
                    target.as_str(),
                ));
            }
        }
    }
    for (prop_id, mut handoffs) in by_prop {
        handoffs.sort_by_key(|handoff| handoff.0);
        let prop = &choreography.props[prop_id];
        let mut owner = prop.owner.as_str();
        let mut previous_end = None;
        for (start, end, from, to) in handoffs {
            if previous_end.is_some_and(|prior| start < prior) {
                bail!("prop {prop_id} has overlapping handoffs");
            }
            if from != owner {
                bail!(
                    "prop {prop_id} is owned by {owner} at frame {start}, so {from} cannot hand it off"
                );
            }
            owner = to;
            previous_end = Some(end);
        }
    }
    Ok(())
}

fn validate_resolved_bounds(plan: &ChoreographyPlan) -> Result<()> {
    for frame in 0..plan.duration_frames {
        for performer in &plan.performers {
            let position = performer_position(plan, &performer.id, frame)?;
            if !in_unit(position.x) || !in_unit(position.y) {
                bail!(
                    "performer {} leaves the normalized stage at frame {} ({:.4}, {:.4})",
                    performer.id,
                    frame,
                    position.x,
                    position.y
                );
            }
        }
        for prop in &plan.props {
            let position = prop_position(plan, prop, frame)?;
            if !in_unit(position.x) || !in_unit(position.y) {
                bail!(
                    "prop {} leaves the normalized stage at frame {} ({:.4}, {:.4})",
                    prop.id,
                    frame,
                    position.x,
                    position.y
                );
            }
        }
    }
    Ok(())
}

fn validate_id(kind: &str, id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        bail!("{kind} id {id:?} must use ASCII letters, numbers, hyphens, or underscores");
    }
    Ok(())
}

fn validate_radius(kind: &str, id: &str, radius: f64) -> Result<()> {
    if !radius.is_finite() || !(0.004..=0.15).contains(&radius) {
        bail!("{kind} {id} radius must be between 0.004 and 0.15");
    }
    Ok(())
}

fn in_unit(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn require_mark<'a>(choreography: &'a Choreography, id: &str, role: &str) -> Result<&'a Mark> {
    choreography
        .stage
        .marks
        .get(id)
        .ok_or_else(|| anyhow!("{role} references unknown mark {id}"))
}

fn require_beat(beats: &BTreeMap<&str, u32>, id: &str) -> Result<u32> {
    beats
        .get(id)
        .copied()
        .ok_or_else(|| anyhow!("phrase references unknown beat {id}"))
}

fn beat_range(beats: &BTreeMap<&str, u32>, between: &[String; 2]) -> Result<(u32, u32)> {
    let start = require_beat(beats, &between[0])?;
    let end = require_beat(beats, &between[1])?;
    if start >= end {
        bail!(
            "phrase beat range {} -> {} must move forward in time",
            between[0],
            between[1]
        );
    }
    Ok((start, end))
}

fn parse_color(value: &str) -> Result<[u8; 3]> {
    let hex = value
        .strip_prefix('#')
        .ok_or_else(|| anyhow!("color must use #RRGGBB"))?;
    if hex.len() != 6 || !hex.chars().all(|character| character.is_ascii_hexdigit()) {
        bail!("color must use #RRGGBB");
    }
    Ok([
        u8::from_str_radix(&hex[0..2], 16)?,
        u8::from_str_radix(&hex[2..4], 16)?,
        u8::from_str_radix(&hex[4..6], 16)?,
    ])
}

fn performer_position(plan: &ChoreographyPlan, id: &str, frame: u32) -> Result<Mark> {
    let performer = plan
        .performers
        .iter()
        .find(|performer| performer.id == id)
        .ok_or_else(|| anyhow!("resolved plan has no performer {id}"))?;
    Ok(track_position(performer.start, &performer.segments, frame))
}

fn track_position(start: Mark, segments: &[MotionSegment], frame: u32) -> Mark {
    let mut position = start;
    for segment in segments {
        if frame < segment.start_frame {
            break;
        }
        if frame <= segment.end_frame {
            let progress = f64::from(frame - segment.start_frame)
                / f64::from(segment.end_frame - segment.start_frame);
            return interpolate(
                segment.from,
                segment.to,
                progress,
                segment.path,
                segment.timing,
            );
        }
        position = segment.to;
    }
    position
}

fn prop_position(plan: &ChoreographyPlan, prop: &ResolvedProp, frame: u32) -> Result<Mark> {
    let mut owner = prop.initial_owner.as_str();
    for handoff in &prop.handoffs {
        if frame < handoff.start_frame {
            break;
        }
        if frame <= handoff.end_frame {
            let from = performer_position(plan, &handoff.from_owner, frame)?;
            let to = performer_position(plan, &handoff.to_owner, frame)?;
            let progress = f64::from(frame - handoff.start_frame)
                / f64::from(handoff.end_frame - handoff.start_frame);
            return Ok(interpolate(
                from,
                to,
                progress,
                handoff.path,
                handoff.timing,
            ));
        }
        owner = &handoff.to_owner;
    }
    performer_position(plan, owner, frame)
}

fn interpolate(
    from: Mark,
    to: Mark,
    progress: f64,
    path: SpatialPath,
    timing: TimingCurve,
) -> Mark {
    let t = apply_timing(progress.clamp(0.0, 1.0), timing);
    let mut x = from.x + (to.x - from.x) * t;
    let mut y = from.y + (to.y - from.y) * t;
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let distance = (dx * dx + dy * dy).sqrt();
    if distance > f64::EPSILON {
        let direction = match path {
            SpatialPath::Linear => 0.0,
            SpatialPath::ArcLeft => -1.0,
            SpatialPath::ArcRight => 1.0,
        };
        let arc = direction * distance * 0.28 * 4.0 * t * (1.0 - t);
        x += -dy / distance * arc;
        y += dx / distance * arc;
    }
    Mark { x, y }
}

fn apply_timing(t: f64, timing: TimingCurve) -> f64 {
    match timing {
        TimingCurve::Linear => t,
        TimingCurve::EaseIn => t * t,
        TimingCurve::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
        TimingCurve::EaseInOut => t * t * (3.0 - 2.0 * t),
        TimingCurve::HoldThenBurst => {
            if t < 0.45 {
                0.0
            } else {
                let shifted = (t - 0.45) / 0.55;
                1.0 - (1.0 - shifted) * (1.0 - shifted)
            }
        }
    }
}

fn render_frame(plan: &ChoreographyPlan, frame: u32, path_overlay: bool) -> Result<Raster> {
    let mut raster = Raster::new(plan.canvas.width, plan.canvas.height, [10, 20, 34]);
    draw_stage(&mut raster);
    for (id, mark) in &plan.marks {
        let (x, y) = raster.point(*mark);
        raster.circle(x, y, 3, [86, 108, 130]);
        raster.text(x + 6, y - 4, id, [126, 147, 168], 1);
    }
    if path_overlay {
        for performer in &plan.performers {
            let color = parse_color(&performer.color)?;
            for segment in &performer.segments {
                draw_segment(&mut raster, segment, color);
            }
        }
    }
    for (performer_index, performer) in plan.performers.iter().enumerate() {
        let position = performer_position(plan, &performer.id, frame)?;
        let (x, y) = raster.point(position);
        let color = parse_color(&performer.color)?;
        let radius = (performer.radius * f64::from(plan.canvas.width)).round() as i32;
        raster.circle(x, y, radius.max(3), color);
        raster.circle_outline(x, y, radius.max(3), [235, 244, 255]);
        raster.text(
            x - radius,
            y + radius + 5 + i32::try_from(performer_index % 3).unwrap_or(0) * 9,
            &performer.id,
            [235, 244, 255],
            1,
        );
        for reaction in &plan.reactions {
            if reaction.performer == performer.id
                && frame >= reaction.frame
                && frame < reaction.frame.saturating_add(10)
            {
                let pulse = radius + 4 + i32::try_from(frame - reaction.frame).unwrap_or(0) * 2;
                raster.circle_outline(x, y, pulse, [255, 91, 91]);
                raster.text(
                    x - radius,
                    y - radius - 14,
                    &reaction.pose,
                    [255, 160, 96],
                    1,
                );
            }
        }
    }
    for prop in &plan.props {
        let position = prop_position(plan, prop, frame)?;
        let (x, y) = raster.point(position);
        let radius = (prop.radius * f64::from(plan.canvas.width)).round() as i32;
        raster.circle(x, y, radius.max(2), parse_color(&prop.color)?);
        raster.circle_outline(x, y, radius.max(2), [255, 255, 255]);
    }
    if let Some((center, zoom, action)) = camera_state(plan, frame) {
        let (cx, cy) = raster.point(center);
        let viewport_width = (f64::from(plan.canvas.width) / zoom).round() as i32;
        let viewport_height = (f64::from(plan.canvas.height) / zoom).round() as i32;
        let left = cx - viewport_width / 2;
        let top = cy - viewport_height / 2;
        let right = left + viewport_width;
        let bottom = top + viewport_height;
        raster.line(left, top, right, top, [255, 160, 96]);
        raster.line(right, top, right, bottom, [255, 160, 96]);
        raster.line(right, bottom, left, bottom, [255, 160, 96]);
        raster.line(left, bottom, left, top, [255, 160, 96]);
        raster.text(
            12,
            i32::try_from(plan.canvas.height).unwrap_or(0) - 20,
            &format!("CAM {:?} {:.2}X", action, zoom),
            [255, 160, 96],
            1,
        );
    }
    if let Some(beat) = plan.beats.iter().rev().find(|beat| beat.frame <= frame) {
        raster.text(
            12,
            12,
            &format!("BEAT {}  F{}", beat.id, frame),
            [235, 244, 255],
            2,
        );
    } else {
        raster.text(12, 12, &format!("BLOCKING  F{frame}"), [235, 244, 255], 2);
    }
    Ok(raster)
}

fn camera_state(plan: &ChoreographyPlan, frame: u32) -> Option<(Mark, f64, CameraAction)> {
    let mut last = None;
    for segment in &plan.camera {
        if frame < segment.start_frame {
            break;
        }
        if frame <= segment.end_frame {
            let progress = f64::from(frame - segment.start_frame)
                / f64::from(segment.end_frame - segment.start_frame);
            let timed = apply_timing(progress, segment.timing);
            return Some((
                interpolate(
                    segment.from,
                    segment.to,
                    progress,
                    SpatialPath::Linear,
                    segment.timing,
                ),
                segment.zoom_from + (segment.zoom_to - segment.zoom_from) * timed,
                segment.action,
            ));
        }
        last = Some((segment.to, segment.zoom_to, segment.action));
    }
    last
}

impl Phrase {
    fn id(&self) -> Option<&str> {
        match self {
            Self::Approach { id, .. } | Self::Handoff { id, .. } | Self::React { id, .. } => {
                id.as_deref()
            }
        }
    }
}

fn draw_stage(raster: &mut Raster) {
    let margin_x = i32::try_from(raster.width / 16).unwrap_or(0);
    let margin_y = i32::try_from(raster.height / 10).unwrap_or(0);
    let right = i32::try_from(raster.width).unwrap_or(0) - margin_x;
    let bottom = i32::try_from(raster.height).unwrap_or(0) - margin_y;
    raster.line(margin_x, margin_y, right, margin_y, [35, 65, 88]);
    raster.line(right, margin_y, right, bottom, [35, 65, 88]);
    raster.line(right, bottom, margin_x, bottom, [35, 65, 88]);
    raster.line(margin_x, bottom, margin_x, margin_y, [35, 65, 88]);
    raster.line(
        i32::try_from(raster.width / 2).unwrap_or(0),
        margin_y,
        i32::try_from(raster.width / 2).unwrap_or(0),
        bottom,
        [25, 50, 71],
    );
    raster.circle_outline(
        i32::try_from(raster.width / 2).unwrap_or(0),
        i32::try_from(raster.height / 2).unwrap_or(0),
        i32::try_from(raster.height / 7).unwrap_or(0),
        [25, 50, 71],
    );
}

fn draw_segment(raster: &mut Raster, segment: &MotionSegment, color: [u8; 3]) {
    let mut previous = segment.from;
    for index in 1..=24 {
        let current = interpolate(
            segment.from,
            segment.to,
            f64::from(index) / 24.0,
            segment.path,
            TimingCurve::Linear,
        );
        let (x0, y0) = raster.point(previous);
        let (x1, y1) = raster.point(current);
        raster.line(x0, y0, x1, y1, dim(color));
        previous = current;
    }
}

fn dim(color: [u8; 3]) -> [u8; 3] {
    [color[0] / 2, color[1] / 2, color[2] / 2]
}

fn convert_image(adapter: &FfmpegAdapter, input: &Path, output: &Path) -> Result<()> {
    adapter.run_ffmpeg(
        &[
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-y".to_string(),
            "-i".to_string(),
        ],
        &[
            adapter.path_argument(input)?,
            "-frames:v".to_string(),
            "1".to_string(),
            adapter.path_argument(output)?,
        ],
    )?;
    Ok(())
}

fn render_contact_sheet(adapter: &FfmpegAdapter, inputs: &[PathBuf], output: &Path) -> Result<()> {
    let mut args = vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-y".to_string(),
    ];
    for input in inputs {
        args.push("-i".to_string());
        args.push(adapter.path_argument(input)?);
    }
    args.extend([
        "-filter_complex".to_string(),
        format!("hstack=inputs={}", inputs.len()),
        "-frames:v".to_string(),
        "1".to_string(),
        adapter.path_argument(output)?,
    ]);
    adapter.run_ffmpeg(&args, &[])?;
    Ok(())
}

struct Raster {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Raster {
    fn new(width: u32, height: u32, color: [u8; 3]) -> Self {
        let mut pixels = vec![0; width as usize * height as usize * 3];
        for pixel in pixels.chunks_exact_mut(3) {
            pixel.copy_from_slice(&color);
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    fn point(&self, mark: Mark) -> (i32, i32) {
        (
            (mark.x * f64::from(self.width - 1)).round() as i32,
            (mark.y * f64::from(self.height - 1)).round() as i32,
        )
    }

    fn set(&mut self, x: i32, y: i32, color: [u8; 3]) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let index = (y as usize * self.width as usize + x as usize) * 3;
        self.pixels[index..index + 3].copy_from_slice(&color);
    }

    fn line(&mut self, mut x0: i32, mut y0: i32, x1: i32, y1: i32, color: [u8; 3]) {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut error = dx + dy;
        loop {
            self.set(x0, y0, color);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let twice = 2 * error;
            if twice >= dy {
                error += dy;
                x0 += sx;
            }
            if twice <= dx {
                error += dx;
                y0 += sy;
            }
        }
    }

    fn circle(&mut self, cx: i32, cy: i32, radius: i32, color: [u8; 3]) {
        for y in -radius..=radius {
            for x in -radius..=radius {
                if x * x + y * y <= radius * radius {
                    self.set(cx + x, cy + y, color);
                }
            }
        }
    }

    fn circle_outline(&mut self, cx: i32, cy: i32, radius: i32, color: [u8; 3]) {
        let mut x = radius;
        let mut y = 0;
        let mut error = 1 - x;
        while x >= y {
            for (dx, dy) in [
                (x, y),
                (y, x),
                (-y, x),
                (-x, y),
                (-x, -y),
                (-y, -x),
                (y, -x),
                (x, -y),
            ] {
                self.set(cx + dx, cy + dy, color);
            }
            y += 1;
            if error < 0 {
                error += 2 * y + 1;
            } else {
                x -= 1;
                error += 2 * (y - x) + 1;
            }
        }
    }

    fn text(&mut self, x: i32, y: i32, text: &str, color: [u8; 3], scale: i32) {
        let mut cursor = x;
        for character in text.chars().take(42) {
            let glyph = glyph(character.to_ascii_uppercase());
            for (row, bits) in glyph.iter().enumerate() {
                for column in 0..5 {
                    if bits & (1 << (4 - column)) != 0 {
                        for sy in 0..scale {
                            for sx in 0..scale {
                                self.set(
                                    cursor + column * scale + sx,
                                    y + row as i32 * scale + sy,
                                    color,
                                );
                            }
                        }
                    }
                }
            }
            cursor += 6 * scale;
        }
    }

    fn write_ppm(&self, path: &Path) -> Result<()> {
        let mut file = fs::File::create(path)?;
        write!(file, "P6\n{} {}\n255\n", self.width, self.height)?;
        file.write_all(&self.pixels)?;
        Ok(())
    }
}

fn glyph(character: char) -> [u8; 7] {
    match character {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 31],
        ':' => [0, 4, 4, 0, 4, 4, 0],
        ' ' => [0; 7],
        _ => [14, 17, 2, 4, 4, 0, 4],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fixture() -> LoadedChoreography {
        let source = r##"
schema: reel.choreography.v0.1
fps: 24
duration_frames: 72
stage:
  marks:
    left: { x: 0.15, y: 0.65 }
    slot: { x: 0.45, y: 0.50 }
    back: { x: 0.75, y: 0.55 }
beats:
  - { id: read, frame: 0 }
  - { id: commit, frame: 20 }
  - { id: release, frame: 28 }
  - { id: receive, frame: 44 }
  - { id: finish, frame: 60 }
performers:
  passer:
    start: left
    color: "#2d7ff9"
    phrases:
      - { action: approach, to: slot, between: [read, commit], path: arc-right, timing: ease-in-out }
      - { action: handoff, prop: token, target: scorer, between: [release, receive] }
  scorer:
    start: back
    color: "#f14f5a"
    phrases:
      - { action: react, at: finish, pose: celebrate }
props:
  token: { owner: passer }
"##;
        let directory = tempdir().unwrap();
        let path = directory.path().join("choreography.yaml");
        fs::write(&path, source).unwrap();
        load(&path).unwrap()
    }

    #[test]
    fn validates_and_compiles_three_action_vocabulary() {
        let loaded = fixture();
        let report = validate(&loaded).unwrap();
        assert_eq!(report.approach_phrases, 1);
        assert_eq!(report.handoff_phrases, 1);
        assert_eq!(report.react_phrases, 1);
        let plan = compile(&loaded).unwrap();
        assert_eq!(plan.performers.len(), 2);
        assert_eq!(plan.props[0].handoffs.len(), 1);
        assert_eq!(plan.reactions[0].pose, "celebrate");
    }

    #[test]
    fn rejects_handoff_by_non_owner() {
        let mut loaded = fixture();
        loaded.choreography.props.get_mut("token").unwrap().owner = "scorer".to_string();
        let error = validate(&loaded).unwrap_err().to_string();
        assert!(error.contains("cannot hand it off"));
    }

    #[test]
    fn arc_and_timing_are_not_linear_midpoints() {
        let from = Mark { x: 0.1, y: 0.5 };
        let to = Mark { x: 0.9, y: 0.5 };
        let arc = interpolate(from, to, 0.5, SpatialPath::ArcRight, TimingCurve::Linear);
        assert!(arc.y > 0.5);
        let eased = apply_timing(0.25, TimingCurve::EaseInOut);
        assert!(eased < 0.25);
    }

    #[test]
    fn rejects_a_resolved_arc_that_leaves_the_stage() {
        let mut loaded = fixture();
        let marks = &mut loaded.choreography.stage.marks;
        marks.get_mut("left").unwrap().y = 0.99;
        marks.get_mut("slot").unwrap().y = 0.99;
        let error = validate(&loaded).unwrap_err().to_string();
        assert!(error.contains("leaves the normalized stage"));
    }
}
