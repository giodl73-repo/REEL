//! Hash-bound scene delivery directly from the cue-relative compiler.
//! Media interpretation and creative selection remain with the consumer.
use crate::cue_relative::{self, Target};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileRef {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub schema: String,
    pub id: String,
    pub contract: FileRef,
    pub production_manifest_sha256: String,
    pub width: u32,
    pub height: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption_picture_layout: Option<crate::caption_presentation::CaptionPictureLayoutConfig>,
    /// Explicit output-space viewport for side panels or portrait reflow.
    /// Source/protection coordinates remain local to the fitted picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture_region: Option<crate::caption_presentation::PixelRect>,
    #[serde(default)]
    pub still_sequence_encoding: Option<String>,
    /// Optional temporal lossless encoding for overlay and post-camera stages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition_encoding: Option<String>,
    pub max_composition_samples: u64,
    pub pictures: Vec<Picture>,
    pub audio: Vec<Audio>,
    pub buses: BTreeMap<String, BusPolicy>,
    /// Exact render attachments intentionally owned by another delivery layer.
    #[serde(default)]
    pub external_layers: Vec<ExternalLayer>,
    /// Camera applied to the composed scene picture after selected overlays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_compose_camera: Option<PostComposeCamera>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PostComposeCamera {
    pub evidence: FileRef,
    pub zoom_step: f64,
    pub zoom_max: f64,
    pub windows: Vec<CameraWindow>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CameraWindow {
    pub start_frame: u64,
    pub end_frame: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalLayer {
    pub attachment_id: String,
    pub reason: String,
    pub evidence: FileRef,
    #[serde(default)]
    pub render_mode: ExternalLayerRenderMode,
    #[serde(default)]
    pub font: Option<FileRef>,
    /// A video conformed from selected evidence; the derivation receipt binds
    /// the source recipe and every component input to these delivered bytes.
    #[serde(default)]
    pub render_source: Option<FileRef>,
    #[serde(default)]
    pub derivation_receipt: Option<FileRef>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TimedOverlayDerivation {
    pub schema: String,
    pub selected_evidence: FileRef,
    pub output: FileRef,
    pub inputs: Vec<FileRef>,
    pub recipe: serde_json::Value,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExternalLayerRenderMode {
    #[default]
    EvidenceOnly,
    AssOverlay,
    TimedVideoOverlay,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BusPolicy {
    pub state: BusState,
    pub reason: String,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BusState {
    Present,
    IntentionalSilence,
    Held,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Picture {
    pub attachment_id: String,
    pub source: FileRef,
    pub kind: PictureKind,
    /// Offset after resampling the source to the declared delivery frame rate.
    #[serde(default)]
    pub source_start_frame: u64,
    pub attention: String,
    #[serde(default)]
    pub crop: Option<Crop>,
    #[serde(default)]
    pub motion: Option<PictureMotion>,
    /// Adjacent identical stills with this ID render one continuous motion run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_group_id: Option<String>,
    /// Optional exact delivery-frame allocation for recorded cut replay.
    /// If any picture supplies this, every picture in the job must supply it.
    #[serde(default)]
    pub delivery_frame_count: Option<u64>,
    #[serde(default)]
    pub stillness_exception: Option<Exception>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PictureKind {
    Still,
    Video,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
// Keep the public construction API stable: plans are shot-level metadata, not
// a per-frame collection, and callers already construct this variant directly.
#[allow(clippy::large_enum_variant)]
pub enum PictureMotion {
    PhasedCamera {
        plan: reel_assembly::motioncraft::FramePlan,
    },
    Zoompan {
        scale_width: u32,
        scale_height: u32,
        crop_width: u32,
        crop_height: u32,
        zoom_step: f64,
        zoom_max: f64,
    },
    CenteredZoompan {
        scale_width: u32,
        scale_height: u32,
        crop_width: u32,
        crop_height: u32,
        zoom_step: f64,
        zoom_max: f64,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Exception {
    pub reason: String,
    pub decision: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Audio {
    pub attachment_id: String,
    pub source: FileRef,
    pub bus: String,
    #[serde(default)]
    pub cue_id: Option<String>,
    #[serde(default)]
    pub source_start_sample: u64,
    #[serde(default)]
    pub gain_db: f64,
    #[serde(default)]
    pub fade_in_samples: u64,
    #[serde(default)]
    pub fade_out_samples: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_mapping: Option<AudioChannelMapping>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement_offset: Option<AudioPlacementOffset>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioChannelMapping {
    DuplicateMono,
    /// Default FFmpeg stereo-to-mono conversion, then exact mono duplication.
    /// This is not a 0.5 + 0.5 average or a loudness normalization.
    DownmixMonoDuplicate,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudioPlacementOffset {
    pub samples: i64,
    pub evidence: FileRef,
    pub reason: String,
}

pub(crate) fn audio_placement_samples(
    start: u64,
    end: u64,
    duration: u64,
    offset: i64,
) -> Result<(u64, u64)> {
    let placed_start = start
        .checked_add_signed(offset)
        .context("audio placement offset starts before the scene or overflows")?;
    let placed_end = end
        .checked_add_signed(offset)
        .context("audio placement offset ends before the scene or overflows")?;
    if placed_start >= placed_end || placed_end > duration {
        bail!("audio placement offset exceeds the scene end or has no positive span");
    }
    Ok((placed_start, placed_end))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub attachment_id: String,
    pub start_sample: u64,
    pub end_sample: u64,
    pub start_frame: u64,
    pub end_frame: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: String,
    pub id: String,
    pub job_sha256: String,
    pub contract_sha256: String,
    pub production_sha256: String,
    pub compiled_sha256: String,
    pub sample_rate: u32,
    pub fps_numerator: u64,
    pub fps_denominator: u64,
    pub duration_samples: u64,
    pub frame_count: u64,
    pub pictures: Vec<Span>,
    pub audio: Vec<Span>,
    /// Original contract anchors when explicit E placement is requested.
    /// `audio` always reports actual rendered sample/frame positions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_anchor_spans: Vec<Span>,
    pub external_layers: Vec<String>,
    #[serde(default)]
    pub external_layer_spans: Vec<Span>,
    #[serde(default)]
    pub rendered_external_layers: Vec<String>,
    pub buses: BTreeMap<String, BusPolicy>,
    pub creative_authority: String,
}

pub(crate) fn checked_file(root: &Path, item: &FileRef) -> Result<PathBuf> {
    if item.path.is_absolute()
        || item
            .path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        bail!("asset path must be a relative local path");
    }
    let root = root.canonicalize()?;
    // Windows may accept mismatched case that later fails on Linux. Verify the
    // exact directory entries as well as resolved bytes on every platform.
    let mut exact = root.clone();
    for component in item.path.components() {
        let name = component.as_os_str();
        if !fs::read_dir(&exact)?.any(|entry| entry.is_ok_and(|entry| entry.file_name() == name)) {
            bail!(
                "missing or case-mismatched asset path: {}",
                item.path.display()
            );
        }
        exact.push(name);
    }
    let path = root
        .join(&item.path)
        .canonicalize()
        .context("missing declared file")?;
    if !path.starts_with(&root) || !path.is_file() {
        bail!("asset escapes its root");
    }
    if fs::metadata(&path)?.len() != item.bytes || crate::sha256_file(&path)? != item.sha256 {
        bail!("file hash/byte mismatch: {}", item.path.display());
    }
    Ok(path)
}

fn timed_overlay_source<'a>(root: &Path, layer: &'a ExternalLayer) -> Result<&'a FileRef> {
    match (&layer.render_source, &layer.derivation_receipt) {
        (None, None) => Ok(&layer.evidence),
        (Some(source), Some(receipt_ref)) => {
            checked_file(root, source)?;
            let receipt_path = checked_file(root, receipt_ref)?;
            let receipt: TimedOverlayDerivation = serde_json::from_slice(&fs::read(receipt_path)?)?;
            if receipt.schema != "reel.timed-overlay-derivation.v1"
                || receipt.selected_evidence != layer.evidence
                || receipt.output != *source
                || receipt.inputs.is_empty()
                || !receipt.recipe.is_object()
            {
                bail!("timed overlay derivation does not bind selected evidence and source");
            }
            for input in &receipt.inputs {
                checked_file(root, input)?;
            }
            Ok(source)
        }
        _ => bail!("timed overlay source and derivation receipt must be paired"),
    }
}

fn nonempty(s: &str) -> bool {
    !s.trim().is_empty()
}

pub fn picture_layout(
    job: &Job,
) -> Result<Option<crate::caption_presentation::CaptionPictureLayoutReport>> {
    crate::caption_presentation::resolve_scene_picture_layout(
        job.caption_picture_layout.as_ref(),
        job.picture_region.as_ref(),
        job.width,
        job.height,
    )
}

pub fn plan(job_path: &Path, asset_root: &Path) -> Result<(Job, Plan)> {
    let job_bytes = fs::read(job_path)?;
    let job: Job = serde_yaml::from_slice(&job_bytes)?;
    if job.schema != "reel.scene-delivery.v0.1" || !nonempty(&job.id) {
        bail!("invalid scene-delivery schema/id");
    }
    if job
        .composition_encoding
        .as_deref()
        .is_some_and(|value| value != "h264-lossless")
    {
        bail!("unsupported composition encoding");
    }
    if job.width == 0
        || job.height == 0
        || job.width > 8192
        || job.height > 8192
        || job.width % 2 != 0
        || job.height % 2 != 0
        || job.max_composition_samples == 0
    {
        bail!("invalid delivery dimensions/composition limit");
    }
    if picture_layout(&job)?.is_some()
        && (job.post_compose_camera.is_some()
            || job.pictures.iter().any(|picture| {
                matches!(
                    picture.motion,
                    Some(PictureMotion::Zoompan { .. } | PictureMotion::CenteredZoompan { .. })
                )
            }))
    {
        bail!(
            "reserved caption band or explicit picture region requires contained picture motion; legacy or post-compose cameras are unsupported"
        );
    }
    let base = job_path.parent().unwrap_or(Path::new("."));
    let contract_path = checked_file(base, &job.contract)?;
    let contract = cue_relative::load(&contract_path)?;
    let contract_base = contract_path.parent().unwrap();
    checked_file(
        contract_base,
        &FileRef {
            path: contract.production_manifest.clone(),
            sha256: job.production_manifest_sha256.clone(),
            bytes: fs::metadata(contract_base.join(&contract.production_manifest))?.len(),
        },
    )?;
    let compiled = cue_relative::compile(&contract, contract_path.parent().unwrap())?;
    if compiled.production_manifest_sha256 != job.production_manifest_sha256 {
        bail!("production manifest differs from the delivery selection");
    }
    let mut attached = BTreeMap::new();
    for a in &compiled.attachments {
        attached.insert(a.id.as_str(), a);
    }
    let mut used = BTreeSet::new();
    let mut take = |id: &str| -> Result<&cue_relative::CompiledAttachment> {
        if !used.insert(id.to_string()) {
            bail!("attachment consumed more than once: {id}");
        }
        attached
            .get(id)
            .copied()
            .with_context(|| format!("unknown attachment {id}"))
    };
    let frame = |sample: u64, ceil: bool| -> Result<u64> {
        let n = u128::from(sample) * u128::from(compiled.frame_rate.numerator);
        let d = u128::from(compiled.sample_rate) * u128::from(compiled.frame_rate.denominator);
        Ok(u64::try_from(if ceil { n.div_ceil(d) } else { n / d })?)
    };
    let span = |a: &cue_relative::CompiledAttachment| -> Result<Span> {
        Ok(Span {
            attachment_id: a.id.clone(),
            start_sample: a.start_sample,
            end_sample: a.end_sample,
            start_frame: frame(a.start_sample, false)?,
            end_frame: frame(a.end_sample, a.end_sample == compiled.duration_samples)?,
        })
    };
    let mut pictures = Vec::new();
    let explicit_picture_frames = job
        .pictures
        .iter()
        .any(|picture| picture.delivery_frame_count.is_some());
    if explicit_picture_frames
        && job
            .pictures
            .iter()
            .any(|picture| picture.delivery_frame_count.is_none())
    {
        bail!("declare delivery_frame_count for every picture or none");
    }
    let mut picture_frame_cursor = 0_u64;
    let mut prior: Option<&Picture> = None;
    let mut unchanged_start = 0;
    let mut cursor = 0;
    let mut motion_group_last = BTreeMap::<String, usize>::new();
    for (index, picture) in job.pictures.iter().enumerate() {
        if let Some(group) = &picture.motion_group_id {
            if group.trim().is_empty()
                || picture.kind != PictureKind::Still
                || picture.motion.is_none()
                || picture.crop.is_some()
            {
                bail!("motion group requires a named moving still without crop");
            }
            if matches!(picture.motion, Some(PictureMotion::PhasedCamera { .. })) {
                bail!("phased camera cannot share a legacy motion group");
            }
            if let Some(previous_index) = motion_group_last.insert(group.clone(), index) {
                let previous = &job.pictures[previous_index];
                if previous_index + 1 != index
                    || previous.source != picture.source
                    || previous.motion != picture.motion
                    || previous.source_start_frame != picture.source_start_frame
                {
                    bail!("motion group must be adjacent with identical picture and motion");
                }
            }
        }
    }
    for p in &job.pictures {
        let a = take(&p.attachment_id)?;
        if !matches!(a.target, Target::Shot { .. } | Target::Cel { .. }) {
            bail!("picture must bind a shot or cel");
        }
        if !nonempty(&p.attention) || a.start_sample != cursor || a.end_sample <= a.start_sample {
            bail!("picture attention missing or coverage gap/overlap");
        }
        checked_file(asset_root, &p.source)?;
        if p.kind == PictureKind::Still && p.source_start_frame != 0 {
            bail!("still pictures cannot have a source frame offset");
        }
        if let Some(c) = &p.crop {
            if c.width == 0 || c.height == 0 {
                bail!("empty crop");
            }
        }
        if let Some(
            PictureMotion::Zoompan {
                scale_width,
                scale_height,
                crop_width,
                crop_height,
                zoom_step,
                zoom_max,
            }
            | PictureMotion::CenteredZoompan {
                scale_width,
                scale_height,
                crop_width,
                crop_height,
                zoom_step,
                zoom_max,
            },
        ) = &p.motion
        {
            if p.kind != PictureKind::Still
                || p.crop.is_some()
                || *scale_width < job.width
                || *scale_height < job.height
                || *crop_width != job.width
                || *crop_height != job.height
                || !zoom_step.is_finite()
                || *zoom_step <= 0.0
                || !zoom_max.is_finite()
                || *zoom_max < 1.0
                || *zoom_max > 4.0
            {
                bail!("invalid still-picture zoompan motion");
            }
        }
        if prior.is_none_or(|old| {
            old.source.sha256 != p.source.sha256 || old.crop != p.crop || old.motion != p.motion
        }) {
            unchanged_start = a.start_sample;
        }
        if a.end_sample - unchanged_start > job.max_composition_samples
            && !p
                .stillness_exception
                .as_ref()
                .is_some_and(|e| nonempty(&e.reason) && nonempty(&e.decision))
        {
            bail!("unchanged composition exceeds limit: {}", p.attachment_id);
        }
        let mut s = span(a)?;
        if explicit_picture_frames {
            let count = p
                .delivery_frame_count
                .context("missing delivery frame count")?;
            if count == 0 {
                bail!(
                    "picture delivery frame count must be positive: {}",
                    p.attachment_id
                );
            }
            s.start_frame = picture_frame_cursor;
            s.end_frame = picture_frame_cursor
                .checked_add(count)
                .context("picture delivery frame count overflow")?;
            picture_frame_cursor = s.end_frame;
        }
        if s.end_frame <= s.start_frame {
            bail!("composition has no delivery frame: {}", p.attachment_id);
        }
        if let Some(PictureMotion::PhasedCamera { plan }) = &p.motion {
            if p.kind != PictureKind::Still || p.crop.is_some() || p.source_start_frame != 0 {
                bail!("phased camera requires an uncropped still at its first frame");
            }
            reel_assembly::motioncraft::validate_camera(plan)?;
            reel_assembly::motioncraft::camera_geometry(plan)?;
            let mut expected = reel_assembly::motioncraft::compile(
                &plan.direction,
                &plan.safe_area,
                s.end_sample - s.start_sample,
                compiled.sample_rate,
                compiled.frame_rate.numerator,
                compiled.frame_rate.denominator,
            )?;
            reel_assembly::motioncraft::allocate_delivery_frames(
                &mut expected,
                s.end_frame - s.start_frame,
            )?;
            if expected != *plan {
                bail!("phased camera compiled plan differs from native selected span");
            }
        }
        pictures.push(s);
        cursor = a.end_sample;
        prior = Some(p);
    }
    if cursor != compiled.duration_samples || pictures.is_empty() {
        bail!("picture does not cover the complete scene");
    }
    if job
        .buses
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != BTreeSet::from(["D", "E", "M"])
    {
        bail!("declare exactly D, M and E bus policies");
    }
    let mut audio = Vec::new();
    let mut audio_anchor_spans = Vec::new();
    let explicit_audio_placement = job
        .audio
        .iter()
        .any(|event| event.placement_offset.is_some());
    // A normal D cue is one complete native take.  A declared handoff may use
    // two distinct takes to cover one cue (for example, dialogue into a
    // preserved narrator tail); those spans must remain contiguous/overlapped
    // and collectively cover the cue without replaying the same source.
    let mut dialogue = BTreeMap::<String, (u64, u64, String)>::new();
    for event in &job.audio {
        let a = take(&event.attachment_id)?;
        if !matches!(a.target, Target::Audio { .. } | Target::Sonic { .. }) {
            bail!("audio must bind an audio/sonic attachment");
        }
        if !job.buses.contains_key(&event.bus)
            || !event.gain_db.is_finite()
            || event.gain_db.abs() > 60.0
        {
            bail!("invalid audio bus/gain");
        }
        let duration = a.end_sample - a.start_sample;
        if duration == 0
            || event.fade_in_samples > duration
            || event.fade_out_samples > duration
            || event.fade_in_samples.saturating_add(event.fade_out_samples) > duration
        {
            bail!("invalid audio span/fades");
        }
        checked_file(asset_root, &event.source)?;
        if matches!(
            event.channel_mapping,
            Some(AudioChannelMapping::DownmixMonoDuplicate)
        ) && event.bus != "E"
        {
            bail!("downmix-mono-duplicate is an explicit E-only channel mapping");
        }
        if event.bus == "D" {
            let id = event.cue_id.as_deref().context("D event needs cue_id")?;
            let c = compiled
                .cues
                .iter()
                .find(|c| c.cue_id == id)
                .context("D cue is unknown")?;
            if let Some((covered_end, parts, first_source)) = dialogue.get_mut(id) {
                if a.start_sample > *covered_end
                    || a.end_sample <= *covered_end
                    || event.source.sha256 == *first_source
                {
                    bail!("D handoff must extend one cue with a distinct source");
                }
                *covered_end = a.end_sample;
                *parts += 1;
            } else {
                if c.start_sample != a.start_sample || event.source_start_sample != 0 {
                    bail!("D event must begin with a complete native cue start");
                }
                dialogue.insert(
                    id.to_string(),
                    (a.end_sample, 1, event.source.sha256.clone()),
                );
            }
        } else if event.cue_id.is_some() {
            bail!("only D events declare cue_id");
        }
        let anchor = span(a)?;
        let mut rendered = anchor.clone();
        if let Some(offset) = &event.placement_offset {
            if event.bus != "E" || !nonempty(&offset.reason) {
                bail!("audio placement offsets require an E event and an explicit reason");
            }
            checked_file(asset_root, &offset.evidence)?;
            (rendered.start_sample, rendered.end_sample) = audio_placement_samples(
                anchor.start_sample,
                anchor.end_sample,
                compiled.duration_samples,
                offset.samples,
            )?;
            rendered.start_frame = frame(rendered.start_sample, false)?;
            rendered.end_frame = frame(rendered.end_sample, true)?;
        }
        if explicit_audio_placement {
            audio_anchor_spans.push(anchor);
        }
        audio.push(rendered);
    }
    for (id, (covered_end, parts, _)) in &dialogue {
        let cue = compiled
            .cues
            .iter()
            .find(|cue| &cue.cue_id == id)
            .expect("validated D cue");
        if *covered_end != cue.end_sample {
            bail!("D event does not cover its cue through the native end");
        }
        if *parts == 1 {
            let event = job
                .audio
                .iter()
                .find(|event| event.bus == "D" && event.cue_id.as_deref() == Some(id))
                .expect("validated D event");
            if event.source_start_sample != 0 {
                bail!("one-part D event must consume one complete native cue");
            }
        }
    }
    for (bus, policy) in &job.buses {
        if !nonempty(&policy.reason) || policy.state == BusState::Held {
            bail!("bus {bus} is held or lacks a reason");
        }
        let present = job.audio.iter().any(|a| a.bus == *bus);
        if present != (policy.state == BusState::Present) {
            bail!("bus {bus} policy and consumed audio disagree");
        }
    }
    if job.buses["D"].state == BusState::Present && dialogue.len() != compiled.cues.len() {
        bail!("D bus must cover every native cue");
    }
    let mut external_layers = Vec::new();
    let mut external_layer_spans = Vec::new();
    let mut rendered_external_layers = Vec::new();
    for layer in &job.external_layers {
        let a = take(&layer.attachment_id)?;
        let mut layer_span = span(a)?;
        if !matches!(
            a.target,
            Target::Beat { .. }
                | Target::Camera { .. }
                | Target::Effect { .. }
                | Target::Overlay { .. }
                | Target::Caption { .. }
                | Target::Title { .. }
        ) || !nonempty(&layer.reason)
        {
            bail!("invalid external layer; primary picture/audio cannot be omitted");
        }
        checked_file(asset_root, &layer.evidence)?;
        if let Some(font) = &layer.font {
            checked_file(asset_root, font)?;
        }
        match layer.render_mode {
            ExternalLayerRenderMode::AssOverlay => {
                // Content-addressed cache objects have a hash for a filename,
                // so the ASS format cannot be inferred from the extension.
                let ass = fs::read_to_string(checked_file(asset_root, &layer.evidence)?)?;
                if !ass
                    .trim_start_matches('\u{feff}')
                    .starts_with("[Script Info]")
                    || !ass.contains("[Events]")
                    || a.start_sample != 0
                    || a.end_sample != compiled.duration_samples
                    || layer.render_source.is_some()
                    || layer.derivation_receipt.is_some()
                {
                    bail!("ASS overlay must be a full-scene .ass attachment");
                }
                // Full-scene ASS covers the delivered picture, including an
                // explicitly retained local frame partition. Its semantic
                // sample end may ceil to a frame that is not in that picture.
                if explicit_picture_frames {
                    layer_span.end_frame = picture_frame_cursor;
                }
                rendered_external_layers.push(layer.attachment_id.clone());
            }
            ExternalLayerRenderMode::TimedVideoOverlay => {
                if !matches!(a.target, Target::Effect { .. } | Target::Overlay { .. })
                    || layer.font.is_some()
                {
                    bail!(
                        "timed video overlay requires an effect or overlay attachment without a font"
                    );
                }
                // Display a frame while its start tick is inside the semantic
                // interval. A fractional end therefore includes its last frame.
                layer_span.start_frame = frame(a.start_sample, true)?;
                layer_span.end_frame = frame(a.end_sample, true)?;
                let delivery_frames = if explicit_picture_frames {
                    picture_frame_cursor
                } else {
                    frame(compiled.duration_samples, true)?
                };
                if explicit_picture_frames {
                    if let Some(picture) = pictures.iter().find(|picture| {
                        picture.start_sample == a.start_sample
                            && picture.start_frame > layer_span.start_frame
                            && picture.start_frame - layer_span.start_frame <= 1
                    }) {
                        layer_span.start_frame = picture.start_frame;
                    }
                }
                if explicit_picture_frames
                    && a.end_sample == compiled.duration_samples
                    && layer_span.end_frame > delivery_frames
                    && layer_span.end_frame - delivery_frames <= 1
                {
                    layer_span.end_frame = delivery_frames;
                }
                if layer_span.start_frame >= layer_span.end_frame
                    || layer_span.end_frame > delivery_frames
                {
                    bail!("timed overlay needs a positive span inside delivered picture frames");
                }
                let source = checked_file(asset_root, timed_overlay_source(asset_root, layer)?)?;
                let info = probe(&source)?;
                let video = info["streams"]
                    .as_array()
                    .and_then(|streams| streams.iter().find(|s| s["codec_type"] == "video"))
                    .context("timed overlay has no video stream")?;
                let pixel_format = video["pix_fmt"].as_str().unwrap_or_default();
                if !pixel_format.contains('a')
                    || video["width"].as_u64() != Some(u64::from(job.width))
                    || video["height"].as_u64() != Some(u64::from(job.height))
                {
                    bail!("timed overlay needs exact scene geometry and an alpha pixel format");
                }
                let rate = video["avg_frame_rate"]
                    .as_str()
                    .and_then(|rate| rate.split_once('/'))
                    .and_then(|(num, den)| {
                        Some((num.parse::<u64>().ok()?, den.parse::<u64>().ok()?))
                    })
                    .filter(|(_, den)| *den > 0)
                    .context("timed overlay frame rate unavailable")?;
                if u128::from(rate.0) * u128::from(compiled.frame_rate.denominator)
                    != u128::from(compiled.frame_rate.numerator) * u128::from(rate.1)
                {
                    bail!("timed overlay frame rate differs from scene delivery rate");
                }
                rendered_external_layers.push(layer.attachment_id.clone());
            }
            ExternalLayerRenderMode::EvidenceOnly => {
                if layer.font.is_some()
                    || layer.render_source.is_some()
                    || layer.derivation_receipt.is_some()
                {
                    bail!("evidence-only layer cannot bind render sources or fonts");
                }
            }
        }
        external_layers.push(layer.attachment_id.clone());
        external_layer_spans.push(layer_span);
    }
    let rendered: Vec<_> = job
        .external_layers
        .iter()
        .filter(|layer| layer.render_mode != ExternalLayerRenderMode::EvidenceOnly)
        .collect();
    let ass_indices: Vec<_> = rendered
        .iter()
        .enumerate()
        .filter(|(_, layer)| layer.render_mode == ExternalLayerRenderMode::AssOverlay)
        .map(|(index, _)| index)
        .collect();
    if ass_indices.len() > 1 {
        bail!("combine text presentations into one selected ASS layer");
    }
    if let Some(&index) = ass_indices.first()
        && index + 1 != rendered.len()
    {
        bail!("ASS presentation must follow all timed overlays");
    }
    if used.len() != attached.len() {
        bail!("unconsumed compiled attachments; declare external layers explicitly");
    }
    let plan = Plan {
        schema: "reel.scene-delivery-plan.v0.1".into(),
        id: job.id.clone(),
        job_sha256: <sha2::Sha256 as sha2::Digest>::digest(&job_bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        contract_sha256: job.contract.sha256.clone(),
        production_sha256: compiled.production_manifest_sha256.clone(),
        compiled_sha256: <sha2::Sha256 as sha2::Digest>::digest(serde_json::to_vec(&compiled)?)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        sample_rate: compiled.sample_rate,
        fps_numerator: compiled.frame_rate.numerator,
        fps_denominator: compiled.frame_rate.denominator,
        duration_samples: compiled.duration_samples,
        frame_count: if explicit_picture_frames {
            picture_frame_cursor
        } else {
            frame(compiled.duration_samples, true)?
        },
        pictures,
        audio,
        audio_anchor_spans,
        external_layers,
        external_layer_spans,
        rendered_external_layers,
        buses: job.buses.clone(),
        creative_authority: "not-granted; external layers are not certified as rendered".into(),
    };
    if let Some(camera) = &job.post_compose_camera {
        checked_file(asset_root, &camera.evidence)?;
        if !camera.zoom_step.is_finite()
            || camera.zoom_step <= 0.0
            || !camera.zoom_max.is_finite()
            || !(1.0..=4.0).contains(&camera.zoom_max)
            || camera.zoom_max == 1.0
            || camera.windows.is_empty()
        {
            bail!("invalid post-composition camera");
        }
        let mut prior_end = 0;
        for window in &camera.windows {
            if window.start_frame < prior_end
                || window.start_frame >= window.end_frame
                || window.end_frame > plan.frame_count
            {
                bail!("post-composition camera windows overlap or exceed scene picture");
            }
            prior_end = window.end_frame;
        }
    }
    crate::motioncraft_review::samples(&job, &plan)?;
    Ok((job, plan))
}

pub(crate) fn ffmpeg(args: &[String]) -> Result<()> {
    let mut local_args = Vec::new();
    for argument in args {
        if argument == "-i" {
            local_args.extend(["-protocol_whitelist".to_string(), "file,pipe".to_string()]);
        }
        local_args.push(argument.clone());
    }
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-v", "error", "-nostdin", "-n"])
        .args(local_args)
        .output()?;
    if !output.status.success() {
        bail!("FFmpeg failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}
pub(crate) fn probe(path: &Path) -> Result<serde_json::Value> {
    let o = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-show_streams",
            "-of",
            "json",
        ])
        .arg(path)
        .output()?;
    if !o.status.success() {
        bail!("ffprobe failed");
    }
    Ok(serde_json::from_slice(&o.stdout)?)
}
fn arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn indexed_layer_source_name(layer: &ExternalLayer, index: usize) -> String {
    match layer.render_mode {
        ExternalLayerRenderMode::AssOverlay => format!("presentation-{index:03}.ass"),
        ExternalLayerRenderMode::TimedVideoOverlay => format!("selected-overlay-{index:03}.mkv"),
        ExternalLayerRenderMode::EvidenceOnly => unreachable!(),
    }
}

fn copy_ass_layer(root: &Path, asset_root: &Path, layer: &ExternalLayer, name: &str) -> Result<()> {
    fs::copy(checked_file(asset_root, &layer.evidence)?, root.join(name))?;
    if let Some(font) = &layer.font {
        fs::create_dir(root.join("fonts"))?;
        let name = font
            .path
            .file_name()
            .context("selected font has no file name")?;
        fs::copy(
            checked_file(asset_root, font)?,
            root.join("fonts").join(name),
        )?;
    }
    Ok(())
}

fn composition_encoder_args(encoding: Option<&str>) -> Vec<&'static str> {
    if encoding == Some("h264-lossless") {
        vec!["-c:v", "libx264", "-crf", "0", "-preset", "veryfast"]
    } else {
        vec!["-c:v", "ffv1"]
    }
}

fn render_ass_overlay(
    root: &Path,
    fps: &str,
    font_bound: bool,
    input: &str,
    ass: &str,
    output_name: &str,
    encoding: Option<&str>,
) -> Result<()> {
    let filter = if font_bound {
        format!("ass={ass}:fontsdir=fonts")
    } else {
        format!("ass={ass}")
    };
    let output = Command::new("ffmpeg")
        .current_dir(root)
        .args(["-hide_banner", "-v", "error", "-nostdin", "-n", "-i", input])
        .args(["-vf", &filter, "-an"])
        .args(composition_encoder_args(encoding))
        .args(["-pix_fmt", "yuv444p", "-r", fps, output_name])
        .output()?;
    if !output.status.success() {
        bail!(
            "ASS layer render failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn render_timed_video_overlay(
    root: &Path,
    plan: &Plan,
    span: &Span,
    picture_input: &str,
    overlay_input: &str,
    picture_output: &str,
    encoding: Option<&str>,
) -> Result<()> {
    let count = span.end_frame - span.start_frame;
    // Matroska timestamps may round native frame positions to milliseconds.
    // Put both decoded streams on the same exact frame-index timebase before
    // framesync, or EOF can hide the carrier's final selected frame at 30 fps.
    let graph = format!(
        "[0:v]settb=expr={}/{},setpts=N[base];[1:v]settb=expr={}/{},trim=start_frame=0:end_frame={count},setpts=N+{}[effect];[base][effect]overlay=eof_action=pass:repeatlast=0:shortest=0:format=auto[v]",
        plan.fps_denominator,
        plan.fps_numerator,
        plan.fps_denominator,
        plan.fps_numerator,
        span.start_frame
    );
    let output = Command::new("ffmpeg")
        .current_dir(root)
        .args(["-hide_banner", "-v", "error", "-nostdin", "-n"])
        .args(["-i", picture_input, "-i", overlay_input])
        .args(["-filter_complex", &graph, "-map", "[v]", "-an"])
        .args(composition_encoder_args(encoding))
        .args([
            "-pix_fmt",
            "yuv444p",
            "-frames:v",
            &plan.frame_count.to_string(),
        ])
        .args([picture_output])
        .output()?;
    if !output.status.success() {
        bail!(
            "timed overlay render failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn render_post_compose_camera(
    root: &Path,
    plan: &Plan,
    camera: &PostComposeCamera,
    encoding: Option<&str>,
) -> Result<()> {
    let before = root.join("pre-camera-picture.mkv");
    fs::rename(root.join("picture.mkv"), &before)?;
    let mut zoom = "1".to_string();
    for window in camera.windows.iter().rev() {
        zoom = format!(
            "if(between(on,{},{}),min({},1+{}*(on-{})),{})",
            window.start_frame,
            window.end_frame - 1,
            camera.zoom_max,
            camera.zoom_step,
            window.start_frame,
            zoom
        );
    }
    let fps = format!("{}/{}", plan.fps_numerator, plan.fps_denominator);
    // Geometry comes from the decoded composed picture, not a source still.
    let picture = probe(&before)?;
    let stream = picture["streams"]
        .as_array()
        .and_then(|streams| {
            streams
                .iter()
                .find(|stream| stream["codec_type"] == "video")
        })
        .context("composed picture has no video stream")?;
    let width = stream["width"]
        .as_u64()
        .context("composed picture width unavailable")?;
    let height = stream["height"]
        .as_u64()
        .context("composed picture height unavailable")?;
    let filter = format!(
        "zoompan=z='{zoom}':x='iw/2-iw/zoom/2':y='ih/2-ih/zoom/2':d=1:s={width}x{height}:fps={fps},format=yuv444p"
    );
    let mut args = vec![
        "-i".into(),
        arg(&before),
        "-vf".into(),
        filter,
        "-frames:v".into(),
        plan.frame_count.to_string(),
        "-an".into(),
    ];
    args.extend(
        composition_encoder_args(encoding)
            .into_iter()
            .map(str::to_string),
    );
    args.push(arg(&root.join("picture.mkv")));
    ffmpeg(&args)?;
    Ok(())
}

fn decoded_video_frames(path: &Path) -> Result<u64> {
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-count_frames", "-select_streams", "v:0"])
        .args([
            "-show_entries",
            "stream=nb_read_frames",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()?;
    if !output.status.success() {
        bail!("could not count timed overlay frames");
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .context("timed overlay frame count unavailable")
}

fn rgb_frame_at_index(path: &Path, plan: &Plan, index: u64) -> Result<Vec<u8>> {
    let seconds = index as f64 * plan.fps_denominator as f64 / plan.fps_numerator as f64;
    let output = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-v",
            "error",
            "-nostdin",
            "-ss",
            &format!("{seconds:.9}"),
            "-i",
        ])
        .arg(path)
        .args(["-frames:v", "1", "-pix_fmt", "rgb24", "-f", "rawvideo", "-"])
        .output()?;
    if !output.status.success() || output.stdout.is_empty() {
        bail!("could not decode presentation frame {index}");
    }
    Ok(output.stdout)
}

fn ass_visibility_frames(
    ass: &str,
    fps_numerator: u64,
    fps_denominator: u64,
    frame_count: u64,
) -> Result<Vec<u64>> {
    fn centiseconds(value: &str) -> Result<u64> {
        let fields = value.trim().split(':').collect::<Vec<_>>();
        if fields.len() != 3 {
            bail!("invalid ASS dialogue clock");
        }
        let (seconds, fraction) = fields[2].split_once('.').context("invalid ASS seconds")?;
        let hours: u64 = fields[0].parse()?;
        let minutes: u64 = fields[1].parse()?;
        let seconds: u64 = seconds.parse()?;
        let fraction: u64 = fraction.parse()?;
        if hours > 999 || minutes >= 60 || seconds >= 60 || fraction >= 100 {
            bail!("invalid ASS time range");
        }
        Ok(((hours * 60 + minutes) * 60 + seconds) * 100 + fraction)
    }
    if fps_numerator == 0 || fps_denominator == 0 {
        bail!("invalid ASS probe frame rate");
    }
    let mut frames = BTreeSet::new();
    for line in ass
        .lines()
        .filter_map(|line| line.trim().strip_prefix("Dialogue:"))
    {
        let fields = line.splitn(10, ',').collect::<Vec<_>>();
        if fields.len() != 10 {
            bail!("invalid ASS dialogue fields");
        }
        let start = centiseconds(fields[1])?;
        let end = centiseconds(fields[2])?;
        if end <= start {
            bail!("empty ASS dialogue interval");
        }
        let frame = ((u128::from(start) + u128::from(end)) * u128::from(fps_numerator)
            / (200 * u128::from(fps_denominator))) as u64;
        if frame < frame_count {
            frames.insert(frame);
        }
    }
    if frames.is_empty() {
        bail!("ASS has no dialogue visible within scene");
    }
    Ok(frames.into_iter().collect())
}

#[cfg(test)]
mod ass_visibility_tests {
    use super::*;
    #[test]
    fn short_title_is_checked_while_visible_in_long_scene() {
        let ass = "Dialogue: 0,0:00:00.00,0:00:04.00,Chapter,,0,0,0,,Lourdes";
        assert_eq!(ass_visibility_frames(ass, 24, 1, 2400).unwrap(), vec![48]);
    }
    #[test]
    fn rejects_invisible_and_malformed_intervals() {
        assert!(
            ass_visibility_frames(
                "Dialogue: 0,0:02:00.00,0:02:04.00,X,,0,0,0,,Title",
                24,
                1,
                2400
            )
            .is_err()
        );
        assert!(
            ass_visibility_frames(
                "Dialogue: 0,0:00:04.00,0:00:00.00,X,,0,0,0,,Title",
                24,
                1,
                2400
            )
            .is_err()
        );
        assert!(ass_visibility_frames("", 24, 1, 2400).is_err());
    }
}

pub(crate) fn finish_pcm(float_path: &Path, output: &Path) -> Result<()> {
    // Reject overload before PCM24 quantization can hide clipping. This is not
    // a loudness/true-peak or intelligibility approval; audio-quality still owns it.
    let result = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-nostdin",
            "-protocol_whitelist",
            "file,pipe",
            "-i",
        ])
        .arg(float_path)
        .args(["-af", "astats=metadata=0:reset=0", "-f", "null", "-"])
        .output()?;
    if !result.status.success() {
        bail!("float mix inspection failed");
    }
    let text = String::from_utf8_lossy(&result.stderr);
    let peaks: Vec<f64> = text
        .lines()
        .filter_map(|line| line.split_once("Peak level dB: "))
        .map(|(_, value)| value.trim().parse::<f64>())
        .collect::<std::result::Result<_, _>>()?;
    if peaks.is_empty() || peaks.iter().any(|p| p.is_nan() || *p >= 0.0) {
        bail!("audio overload or missing peak evidence; revise explicit gains");
    }
    ffmpeg(&[
        "-i".into(),
        arg(float_path),
        "-c:a".into(),
        "pcm_s24le".into(),
        arg(output),
    ])?;
    fs::remove_file(float_path)?;
    Ok(())
}
fn pcm_samples(path: &Path, sr: u32) -> Result<u64> {
    let mut child = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "file,pipe",
            "-i",
        ])
        .arg(path)
        .args([
            "-map",
            "0:a:0",
            "-ar",
            &sr.to_string(),
            "-ac",
            "2",
            "-f",
            "s24le",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut out = child.stdout.take().context("PCM pipe missing")?;
    let mut total = 0u64;
    let mut buf = [0u8; 65536];
    loop {
        let n = out.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
    }
    let status = child.wait_with_output()?;
    if !status.status.success() || total % 6 != 0 {
        bail!("PCM decode failed");
    }
    Ok(total / 6)
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub tool_version: String,
    pub ffmpeg_version: String,
    pub plan: Plan,
    pub outputs: BTreeMap<String, FileRef>,
    pub content_samples: u64,
    pub delivery_frames: u64,
    pub publication: String,
}

/// Sound-only evidence shares the full scene's mixer and native plan. It makes
/// no claim that pictures, captions, effects or complete episodes were rendered.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudioReceipt {
    pub schema: String,
    pub tool_version: String,
    pub ffmpeg_version: String,
    pub plan: Plan,
    pub outputs: BTreeMap<String, FileRef>,
    pub content_samples: u64,
    pub publication: String,
}

const AUDIO_OUTPUTS: [&str; 4] = ["D.wav", "M.wav", "E.wav", "mix.wav"];

/// Render native D/M/E/mix for independent soundtrack qualification without
/// producing scene pictures. All job/source bindings are still planned.
pub fn render_audio(job_path: &Path, asset_root: &Path, output: &Path) -> Result<AudioReceipt> {
    let (job, plan) = plan(job_path, asset_root)?;
    if output.exists() {
        bail!("audio output already exists; use a new directory");
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".scene-audio-")
        .tempdir_in(parent)?;
    let root = stage.path();
    render_audio_stems(&job, &plan, asset_root, root)?;
    let mut outputs = BTreeMap::new();
    for name in AUDIO_OUTPUTS {
        let path = root.join(name);
        outputs.insert(
            name.into(),
            FileRef {
                path: name.into(),
                sha256: crate::sha256_file(&path)?,
                bytes: fs::metadata(path)?.len(),
            },
        );
    }
    let version = Command::new("ffmpeg").arg("-version").output()?;
    if !version.status.success() {
        bail!("FFmpeg version inspection failed");
    }
    let receipt = AudioReceipt {
        schema: "reel.scene-audio-receipt.v0.1".into(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
        ffmpeg_version: String::from_utf8_lossy(&version.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .into(),
        content_samples: plan.duration_samples,
        plan,
        outputs,
        publication: "not-authorized-by-tool".into(),
    };
    fs::write(
        root.join("audio-receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    let verified = check_audio(job_path, asset_root, root)?;
    fs::rename(root, output)?;
    Ok(verified)
}

/// Verify an audio-only receipt, its exact job/plan and four decoded PCM clocks.
/// This receipt cannot be consumed as a complete scene-delivery receipt.
pub fn check_audio(job_path: &Path, asset_root: &Path, output: &Path) -> Result<AudioReceipt> {
    let (_, plan) = plan(job_path, asset_root)?;
    let receipt: AudioReceipt =
        serde_json::from_slice(&fs::read(output.join("audio-receipt.json"))?)?;
    if receipt.schema != "reel.scene-audio-receipt.v0.1"
        || serde_json::to_value(&receipt.plan)? != serde_json::to_value(&plan)?
        || receipt.content_samples != plan.duration_samples
        || receipt.publication != "not-authorized-by-tool"
        || receipt
            .outputs
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != AUDIO_OUTPUTS.into_iter().collect()
    {
        bail!("audio receipt does not match current compiled scene");
    }
    for name in AUDIO_OUTPUTS {
        let item = &receipt.outputs[name];
        if item.path != Path::new(name) {
            bail!("audio output name mismatch");
        }
        let path = checked_file(output, item)?;
        let info = probe(&path)?;
        let audio = info["streams"]
            .as_array()
            .and_then(|xs| xs.iter().find(|s| s["codec_type"] == "audio"))
            .context("delivery audio stream missing")?;
        if audio["codec_name"] != "pcm_s24le"
            || audio["sample_rate"].as_str() != Some(&plan.sample_rate.to_string())
            || audio["channels"].as_u64() != Some(2)
            || pcm_samples(&path, plan.sample_rate)? != plan.duration_samples
        {
            bail!("audio PCM format or decoded sample count mismatch: {name}");
        }
    }
    Ok(receipt)
}

pub fn render(job_path: &Path, asset_root: &Path, output: &Path) -> Result<Receipt> {
    let (job, plan) = plan(job_path, asset_root)?;
    if job
        .still_sequence_encoding
        .as_deref()
        .is_some_and(|value| value != "h264-lossless")
    {
        bail!("unsupported still sequence encoding");
    }
    if output.exists() {
        bail!("output already exists; use a new delivery directory");
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".scene-delivery-")
        .tempdir_in(parent)?;
    let root = stage.path();
    let fps = format!("{}/{}", plan.fps_numerator, plan.fps_denominator);
    let overlays: Vec<_> = job
        .external_layers
        .iter()
        .filter(|layer| layer.render_mode != ExternalLayerRenderMode::EvidenceOnly)
        .collect();
    let overlay = overlays.first().copied();
    let mut inputs = Vec::new();
    let mut filters = Vec::new();
    let mut groups = Vec::new();
    let mut start = 0;
    while start < job.pictures.len() {
        let mut end = start + 1;
        if let Some(group) = &job.pictures[start].motion_group_id {
            while end < job.pictures.len()
                && job.pictures[end].motion_group_id.as_ref() == Some(group)
            {
                end += 1;
            }
        }
        groups.push((start, end));
        start = end;
    }
    for (i, &(start, end)) in groups.iter().enumerate() {
        let p = &job.pictures[start];
        let s = &plan.pictures[start];
        let group_frames = plan.pictures[end - 1].end_frame - s.start_frame;
        let path = checked_file(asset_root, &p.source)?;
        let info = probe(&path)?;
        let stream = info["streams"]
            .as_array()
            .and_then(|xs| xs.iter().find(|s| s["codec_type"] == "video"))
            .context("picture has no video stream")?;
        if let Some(c) = &p.crop {
            if u64::from(c.x) + u64::from(c.width) > stream["width"].as_u64().unwrap_or(0)
                || u64::from(c.y) + u64::from(c.height) > stream["height"].as_u64().unwrap_or(0)
            {
                bail!("crop outside source");
            }
        }
        if p.kind == PictureKind::Still {
            inputs.extend(["-loop".into(), "1".into(), "-framerate".into(), fps.clone()]);
        }
        inputs.extend(["-i".into(), arg(&path)]);
        let crop = p
            .crop
            .as_ref()
            .map(|c| format!("crop={}:{}:{}:{},", c.width, c.height, c.x, c.y))
            .unwrap_or_default();
        let source_end = p
            .source_start_frame
            .checked_add(group_frames)
            .context("source frame offset overflow")?;
        let region = picture_layout(&job)?
            .map(|layout| layout.picture_region)
            .unwrap_or(crate::caption_presentation::PixelRect {
                x: 0,
                y: 0,
                width: job.width,
                height: job.height,
            });
        let output_pad = if region.width != job.width || region.height != job.height {
            format!(
                "pad={}:{}:{}:{},",
                job.width, job.height, region.x, region.y
            )
        } else {
            String::new()
        };
        // RGB preserves odd-sized reserved regions exactly before final chroma
        // conversion; YUV padding may otherwise round a caption boundary.
        let layout_format = if output_pad.is_empty() {
            ""
        } else {
            "format=rgb24,"
        };
        let visual = match &p.motion {
            Some(PictureMotion::PhasedCamera { plan: motion }) => {
                let [left, top, right, bottom] =
                    reel_assembly::motioncraft::camera_geometry(motion)?;
                format!(
                    "{layout_format}scale={}:{}:force_original_aspect_ratio=decrease,pad={}:{}:(ow-iw)/2:(oh-ih)/2,perspective=x0='{left}':y0='{top}':x1='{right}':y1='{top}':x2='{left}':y2='{bottom}':x3='{right}':y3='{bottom}':interpolation=cubic:sense=source:eval=frame,{output_pad}",
                    region.width, region.height, region.width, region.height
                )
            }
            Some(PictureMotion::Zoompan {
                scale_width,
                scale_height,
                crop_width,
                crop_height,
                zoom_step,
                zoom_max,
            }) => format!(
                "scale={scale_width}:{scale_height}:force_original_aspect_ratio=increase,crop={crop_width}:{crop_height},zoompan=z='min(zoom+{zoom_step},{zoom_max})':d={}:s={}x{}:fps={fps},",
                group_frames, job.width, job.height
            ),
            Some(PictureMotion::CenteredZoompan {
                scale_width,
                scale_height,
                crop_width,
                crop_height,
                zoom_step,
                zoom_max,
            }) => format!(
                "scale={scale_width}:{scale_height}:force_original_aspect_ratio=increase,crop={crop_width}:{crop_height},zoompan=z='min(zoom+{zoom_step},{zoom_max})':x='(iw-iw/zoom)/2':y='(ih-ih/zoom)/2':d={}:s={}x{}:fps={fps},",
                group_frames, job.width, job.height
            ),
            None => format!(
                "{crop}{layout_format}scale={}:{}:force_original_aspect_ratio=decrease,pad={}:{}:(ow-iw)/2:(oh-ih)/2,{output_pad}",
                region.width, region.height, region.width, region.height
            ),
        };
        filters.push(format!("[{i}:v]{visual}setsar=1,fps={fps},trim=start_frame={}:end_frame={source_end},setpts=PTS-STARTPTS[v{i}]",p.source_start_frame));
    }
    filters.push(format!(
        "{}concat=n={}:v=1:a=0[v]",
        (0..groups.len())
            .map(|i| format!("[v{i}]"))
            .collect::<String>(),
        groups.len()
    ));
    if job.still_sequence_encoding.as_deref() == Some("h264-lossless") {
        inputs.extend([
            "-crf".into(),
            "0".into(),
            "-preset".into(),
            "veryfast".into(),
        ]);
    }
    inputs.extend([
        "-filter_complex".into(),
        filters.join(";"),
        "-map".into(),
        "[v]".into(),
        "-an".into(),
        "-c:v".into(),
        if job.still_sequence_encoding.as_deref() == Some("h264-lossless") {
            "libx264"
        } else {
            "ffv1"
        }
        .into(),
        "-pix_fmt".into(),
        "yuv444p".into(),
        "-r".into(),
        fps.clone(),
        arg(&root.join(if overlay.is_some() {
            "clean-picture.mkv"
        } else {
            "picture.mkv"
        })),
    ]);
    ffmpeg(&inputs)?;
    if overlays.len() > 1 {
        for (index, layer) in overlays.iter().enumerate() {
            let selected = indexed_layer_source_name(layer, index);
            let input = if index == 0 {
                "clean-picture.mkv".to_string()
            } else {
                format!("layered-picture-{:03}.mkv", index - 1)
            };
            let output = if index + 1 == overlays.len() {
                "picture.mkv".to_string()
            } else {
                format!("layered-picture-{index:03}.mkv")
            };
            match layer.render_mode {
                ExternalLayerRenderMode::AssOverlay => {
                    copy_ass_layer(root, asset_root, layer, &selected)?;
                    render_ass_overlay(
                        root,
                        &fps,
                        layer.font.is_some(),
                        &input,
                        &selected,
                        &output,
                        job.composition_encoding.as_deref(),
                    )?;
                }
                ExternalLayerRenderMode::TimedVideoOverlay => {
                    let span = plan
                        .external_layer_spans
                        .iter()
                        .find(|span| span.attachment_id == layer.attachment_id)
                        .context("timed overlay span missing")?;
                    let source =
                        checked_file(asset_root, timed_overlay_source(asset_root, layer)?)?;
                    if decoded_video_frames(&source)? < span.end_frame - span.start_frame {
                        bail!("timed overlay lacks frames for its selected span");
                    }
                    fs::copy(source, root.join(&selected))?;
                    render_timed_video_overlay(
                        root,
                        &plan,
                        span,
                        &input,
                        &selected,
                        &output,
                        job.composition_encoding.as_deref(),
                    )?;
                }
                ExternalLayerRenderMode::EvidenceOnly => unreachable!(),
            }
        }
    } else if let Some(layer) = overlay {
        match layer.render_mode {
            ExternalLayerRenderMode::AssOverlay => {
                copy_ass_layer(root, asset_root, layer, "presentation.ass")?;
                render_ass_overlay(
                    root,
                    &fps,
                    layer.font.is_some(),
                    "clean-picture.mkv",
                    "presentation.ass",
                    "picture.mkv",
                    job.composition_encoding.as_deref(),
                )?;
            }
            ExternalLayerRenderMode::TimedVideoOverlay => {
                let span = plan
                    .external_layer_spans
                    .iter()
                    .find(|span| span.attachment_id == layer.attachment_id)
                    .context("timed overlay span missing")?;
                let source = checked_file(asset_root, timed_overlay_source(asset_root, layer)?)?;
                if decoded_video_frames(&source)? < span.end_frame - span.start_frame {
                    bail!("timed overlay lacks frames for its selected span");
                }
                fs::copy(source, root.join("selected-overlay.mkv"))?;
                render_timed_video_overlay(
                    root,
                    &plan,
                    span,
                    "clean-picture.mkv",
                    "selected-overlay.mkv",
                    "picture.mkv",
                    job.composition_encoding.as_deref(),
                )?;
            }
            ExternalLayerRenderMode::EvidenceOnly => unreachable!(),
        }
    }
    if let Some(camera) = &job.post_compose_camera {
        render_post_compose_camera(root, &plan, camera, job.composition_encoding.as_deref())?;
    }
    render_audio_stems(&job, &plan, asset_root, root)?;
    ffmpeg(&[
        "-i".into(),
        arg(&root.join("picture.mkv")),
        "-i".into(),
        arg(&root.join("mix.wav")),
        "-map".into(),
        "0:v:0".into(),
        "-map".into(),
        "1:a:0".into(),
        "-c".into(),
        "copy".into(),
        arg(&root.join("master.mkv")),
    ])?;
    ffmpeg(&[
        "-i".into(),
        arg(&root.join("master.mkv")),
        "-map".into(),
        "0:v:0".into(),
        "-map".into(),
        "0:a:0".into(),
        "-c:v".into(),
        "libx264".into(),
        "-crf".into(),
        "18".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "192k".into(),
        "-movflags".into(),
        "+faststart".into(),
        arg(&root.join("review.mp4")),
    ])?;
    let mut names: Vec<String> = vec![
        "picture.mkv",
        "D.wav",
        "M.wav",
        "E.wav",
        "mix.wav",
        "master.mkv",
        "review.mp4",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    if overlays.len() > 1 {
        names.push("clean-picture.mkv".into());
        for (index, layer) in overlays.iter().enumerate() {
            names.push(indexed_layer_source_name(layer, index));
            if index + 1 < overlays.len() {
                names.push(format!("layered-picture-{index:03}.mkv"));
            }
        }
    } else if let Some(overlay) = overlay {
        names.push("clean-picture.mkv".into());
        names.push(
            match overlay.render_mode {
                ExternalLayerRenderMode::AssOverlay => "presentation.ass",
                ExternalLayerRenderMode::TimedVideoOverlay => "selected-overlay.mkv",
                ExternalLayerRenderMode::EvidenceOnly => unreachable!(),
            }
            .into(),
        );
    }
    if job.post_compose_camera.is_some() {
        names.push("pre-camera-picture.mkv".into());
    }
    crate::motioncraft_review::render(&job, &plan, root)?;
    names.extend(crate::motioncraft_review::output_names(&job, &plan)?);
    let mut outputs = BTreeMap::new();
    for name in names {
        let p = root.join(&name);
        outputs.insert(
            name.clone(),
            FileRef {
                path: name.into(),
                sha256: crate::sha256_file(&p)?,
                bytes: fs::metadata(p)?.len(),
            },
        );
    }
    if let Some(layer) = overlays.iter().find_map(|layer| layer.font.as_ref()) {
        let name = layer
            .path
            .file_name()
            .context("selected font has no file name")?;
        let relative = Path::new("fonts").join(name);
        let p = root.join(&relative);
        outputs.insert(
            relative.to_string_lossy().replace('\\', "/"),
            FileRef {
                path: relative,
                sha256: crate::sha256_file(&p)?,
                bytes: fs::metadata(p)?.len(),
            },
        );
    }
    let version = Command::new("ffmpeg").arg("-version").output()?;
    let receipt = Receipt {
        schema: "reel.scene-delivery-receipt.v0.1".into(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
        ffmpeg_version: String::from_utf8_lossy(&version.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .into(),
        content_samples: plan.duration_samples,
        delivery_frames: plan.frame_count,
        plan,
        outputs,
        publication: "not-authorized-by-tool".into(),
    };
    fs::write(
        root.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    let verified = check(job_path, asset_root, root)?;
    if serde_json::to_vec(&receipt.outputs)? != serde_json::to_vec(&verified.outputs)? {
        bail!("render and independent check disagree on output bytes");
    }
    // No result directory is published until all streams and hashes recheck.
    fs::rename(root, output)?;
    Ok(verified)
}

fn render_audio_stems(job: &Job, plan: &Plan, asset_root: &Path, root: &Path) -> Result<()> {
    for bus in ["D", "M", "E"] {
        let mut inputs = Vec::new();
        let mut filters = Vec::new();
        let mut labels = String::new();
        let mut count = 0;
        for (event, s) in job
            .audio
            .iter()
            .zip(&plan.audio)
            .filter(|(a, _)| a.bus == bus)
        {
            let path = checked_file(asset_root, &event.source)?;
            let info = probe(&path)?;
            let streams = info["streams"]
                .as_array()
                .context("audio streams missing")?;
            let aud = streams
                .iter()
                .find(|s| s["codec_type"] == "audio")
                .context("audio stream missing")?;
            if aud["sample_rate"].as_str() != Some(&plan.sample_rate.to_string()) {
                bail!("audio must be preconformed to the scene sample rate");
            }
            let samples = pcm_samples(&path, plan.sample_rate)?;
            let n = s.end_sample - s.start_sample;
            if event
                .source_start_sample
                .checked_add(n)
                .is_none_or(|end| end > samples)
                || (bus == "D" && event.source_start_sample == 0 && samples != n)
            {
                bail!("native audio duration mismatch or insufficient source samples");
            }
            inputs.extend(["-i".into(), arg(&path)]);
            let mapping = match event.channel_mapping {
                None => "aformat=sample_fmts=fltp:channel_layouts=stereo",
                Some(AudioChannelMapping::DuplicateMono) => {
                    if aud["channels"].as_u64() != Some(1) {
                        bail!("duplicate-mono channel mapping requires a mono source");
                    }
                    "pan=stereo|c0=c0|c1=c0,aformat=sample_fmts=fltp:channel_layouts=stereo"
                }
                Some(AudioChannelMapping::DownmixMonoDuplicate) => {
                    if aud["channels"].as_u64() != Some(2) {
                        bail!("downmix-mono-duplicate channel mapping requires a stereo source");
                    }
                    "aformat=sample_fmts=fltp:channel_layouts=mono,pan=stereo|c0=c0|c1=c0,aformat=sample_fmts=fltp:channel_layouts=stereo"
                }
            };
            let mut f = format!(
                "[{count}:a]{mapping},atrim=start_sample={}:end_sample={},asetpts=PTS-STARTPTS,volume={}dB",
                event.source_start_sample,
                event.source_start_sample + n,
                event.gain_db
            );
            if event.fade_in_samples > 0 {
                f.push_str(&format!(",afade=t=in:ss=0:ns={}", event.fade_in_samples));
            }
            if event.fade_out_samples > 0 {
                f.push_str(&format!(
                    ",afade=t=out:ss={}:ns={}",
                    n - event.fade_out_samples,
                    event.fade_out_samples
                ));
            }
            f.push_str(&format!(
                ",adelay={}S:all=1,apad=whole_len={},atrim=end_sample={}[a{count}]",
                s.start_sample, plan.duration_samples, plan.duration_samples
            ));
            filters.push(f);
            labels.push_str(&format!("[a{count}]"));
            count += 1;
        }
        if count == 0 {
            inputs.extend([
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                format!("anullsrc=r={}:cl=stereo", plan.sample_rate),
            ]);
            filters.push(format!(
                "[0:a]atrim=end_sample={}[out]",
                plan.duration_samples
            ));
        } else {
            filters.push(format!(
                "{labels}amix=inputs={count}:normalize=0:duration=longest,atrim=end_sample={}[out]",
                plan.duration_samples
            ));
        }
        inputs.extend([
            "-filter_complex".into(),
            filters.join(";"),
            "-map".into(),
            "[out]".into(),
            "-ar".into(),
            plan.sample_rate.to_string(),
            "-c:a".into(),
            "pcm_f32le".into(),
            arg(&root.join(format!("{bus}-float.wav"))),
        ]);
        ffmpeg(&inputs)?;
        finish_pcm(
            &root.join(format!("{bus}-float.wav")),
            &root.join(format!("{bus}.wav")),
        )?;
    }
    let mut mix = Vec::new();
    for bus in ["D", "M", "E"] {
        mix.extend(["-i".into(), arg(&root.join(format!("{bus}.wav")))]);
    }
    mix.extend([
        "-filter_complex".into(),
        format!(
            "[0:a][1:a][2:a]amix=inputs=3:normalize=0,atrim=end_sample={}[a]",
            plan.duration_samples
        ),
        "-map".into(),
        "[a]".into(),
        "-c:a".into(),
        "pcm_f32le".into(),
        arg(&root.join("mix-float.wav")),
    ]);
    ffmpeg(&mix)?;
    finish_pcm(&root.join("mix-float.wav"), &root.join("mix.wav"))?;
    Ok(())
}

pub fn check(job_path: &Path, asset_root: &Path, output: &Path) -> Result<Receipt> {
    check_impl(job_path, asset_root, output, true)
}

/// Recheck immutable bytes after the caller verifies the exact native scene
/// build proof's job and delivery-receipt hashes. That native proof is issued
/// only after `check` succeeds. Standalone checks always decode all media.
pub(crate) fn recheck_native_receipt(
    job_path: &Path,
    asset_root: &Path,
    output: &Path,
) -> Result<Receipt> {
    check_impl(job_path, asset_root, output, false)
}

fn check_impl(
    job_path: &Path,
    asset_root: &Path,
    output: &Path,
    decode_media: bool,
) -> Result<Receipt> {
    let (job, plan) = plan(job_path, asset_root)?;
    let receipt: Receipt = serde_json::from_slice(&fs::read(output.join("receipt.json"))?)?;
    if receipt.schema != "reel.scene-delivery-receipt.v0.1"
        || serde_json::to_value(&receipt.plan)? != serde_json::to_value(&plan)?
        || receipt.content_samples != plan.duration_samples
        || receipt.delivery_frames != plan.frame_count
    {
        bail!("receipt does not match current compiled scene");
    }
    let mut expected: BTreeSet<String> = [
        "picture.mkv",
        "D.wav",
        "M.wav",
        "E.wav",
        "mix.wav",
        "master.mkv",
        "review.mp4",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    let overlays: Vec<_> = job
        .external_layers
        .iter()
        .filter(|layer| layer.render_mode != ExternalLayerRenderMode::EvidenceOnly)
        .collect();
    let overlay = overlays.first().copied();
    if overlays.len() > 1 {
        expected.insert("clean-picture.mkv".into());
        for (index, layer) in overlays.iter().enumerate() {
            expected.insert(indexed_layer_source_name(layer, index));
            if index + 1 < overlays.len() {
                expected.insert(format!("layered-picture-{index:03}.mkv"));
            }
        }
    } else if let Some(overlay) = overlay {
        expected.insert("clean-picture.mkv".into());
        expected.insert(
            match overlay.render_mode {
                ExternalLayerRenderMode::AssOverlay => "presentation.ass",
                ExternalLayerRenderMode::TimedVideoOverlay => "selected-overlay.mkv",
                ExternalLayerRenderMode::EvidenceOnly => unreachable!(),
            }
            .into(),
        );
    }
    if job.post_compose_camera.is_some() {
        expected.insert("pre-camera-picture.mkv".into());
    }
    expected.extend(crate::motioncraft_review::output_names(&job, &plan)?);
    let font_name = overlays
        .iter()
        .find_map(|layer| layer.font.as_ref())
        .map(|font| {
            format!(
                "fonts/{}",
                font.path.file_name().unwrap_or_default().to_string_lossy()
            )
        });
    if let Some(name) = font_name.as_deref() {
        expected.insert(name.to_string());
    }
    if receipt.outputs.keys().cloned().collect::<BTreeSet<_>>() != expected {
        bail!("receipt output set mismatch");
    }
    for (name, item) in &receipt.outputs {
        if item.path != Path::new(name) {
            bail!("output name mismatch");
        }
        checked_file(output, item)?;
    }
    crate::motioncraft_review::check(&job, &plan, output, decode_media)?;
    if !decode_media {
        return Ok(receipt);
    }
    if let Some(camera) = &job.post_compose_camera {
        let before = output.join("pre-camera-picture.mkv");
        let after = output.join("picture.mkv");
        if decoded_video_frames(&before)? != plan.frame_count
            || decoded_video_frames(&after)? != plan.frame_count
        {
            bail!("post-composition camera changed scene frame count");
        }
        let window = &camera.windows[0];
        let active_frame = window.end_frame - 1;
        if active_frame > window.start_frame
            && rgb_frame_at_index(&before, &plan, active_frame)?
                == rgb_frame_at_index(&after, &plan, active_frame)?
        {
            bail!("post-composition camera made no visible change");
        }
    }
    if overlays.len() > 1 {
        for (index, layer) in overlays.iter().enumerate() {
            let selected = indexed_layer_source_name(layer, index);
            let source = if layer.render_mode == ExternalLayerRenderMode::AssOverlay {
                &layer.evidence
            } else {
                timed_overlay_source(asset_root, layer)?
            };
            if receipt.outputs[&selected].sha256 != source.sha256 {
                bail!("rendered layer source differs from selected evidence");
            }
            let before = output.join(if index == 0 {
                "clean-picture.mkv".to_string()
            } else {
                format!("layered-picture-{:03}.mkv", index - 1)
            });
            let after = output.join(if index + 1 == overlays.len() {
                if job.post_compose_camera.is_some() {
                    "pre-camera-picture.mkv".to_string()
                } else {
                    "picture.mkv".to_string()
                }
            } else {
                format!("layered-picture-{index:03}.mkv")
            });
            if decoded_video_frames(&before)? != plan.frame_count
                || decoded_video_frames(&after)? != plan.frame_count
            {
                bail!("rendered layer changed scene frame count");
            }
            if layer.render_mode == ExternalLayerRenderMode::AssOverlay {
                if let (Some(font), Some(name)) = (&layer.font, font_name.as_deref())
                    && receipt.outputs[name].sha256 != font.sha256
                {
                    bail!("rendered presentation font differs from selected font");
                }
                let ass = fs::read_to_string(checked_file(asset_root, &layer.evidence)?)?;
                let frames = ass_visibility_frames(
                    &ass,
                    plan.fps_numerator,
                    plan.fps_denominator,
                    plan.frame_count,
                )?;
                let mut visible = false;
                for frame in frames {
                    let before_pixels = rgb_frame_at_index(&before, &plan, frame)?;
                    let after_pixels = rgb_frame_at_index(&after, &plan, frame)?;
                    if before_pixels.len() != after_pixels.len() {
                        bail!("ASS layer changed picture geometry");
                    }
                    visible |= before_pixels != after_pixels;
                }
                if !visible {
                    bail!(
                        "selected ASS layer made no visible change during its dialogue intervals"
                    );
                }
                continue;
            }
            let span = plan
                .external_layer_spans
                .iter()
                .find(|span| span.attachment_id == layer.attachment_id)
                .context("timed overlay span missing")?;
            let active_frames = span.end_frame - span.start_frame;
            let mut visible = false;
            for frame in [
                span.start_frame,
                span.start_frame + active_frames / 4,
                span.start_frame + active_frames / 2,
                span.start_frame + active_frames * 3 / 4,
                span.end_frame - 1,
            ] {
                visible |= rgb_frame_at_index(&before, &plan, frame)?
                    != rgb_frame_at_index(&after, &plan, frame)?;
            }
            if !visible {
                bail!("timed overlay made no visible change in its selected span");
            }
            for frame in [
                span.start_frame.checked_sub(1),
                (span.end_frame < plan.frame_count).then_some(span.end_frame),
            ]
            .into_iter()
            .flatten()
            {
                if rgb_frame_at_index(&before, &plan, frame)?
                    != rgb_frame_at_index(&after, &plan, frame)?
                {
                    bail!("timed overlay changed picture outside its selected span");
                }
            }
        }
    } else if let Some(layer) = overlay {
        let layered_picture = output.join(if job.post_compose_camera.is_some() {
            "pre-camera-picture.mkv"
        } else {
            "picture.mkv"
        });
        match layer.render_mode {
            ExternalLayerRenderMode::AssOverlay => {
                if receipt.outputs["presentation.ass"].sha256 != layer.evidence.sha256 {
                    bail!("rendered presentation source differs from selected ASS layer");
                }
                if let (Some(font), Some(name)) = (&layer.font, font_name.as_deref()) {
                    if receipt.outputs[name].sha256 != font.sha256 {
                        bail!("rendered presentation font differs from selected font");
                    }
                }
                let ass = fs::read_to_string(checked_file(asset_root, &layer.evidence)?)?;
                let frames = ass_visibility_frames(
                    &ass,
                    plan.fps_numerator,
                    plan.fps_denominator,
                    plan.frame_count,
                )?;
                let mut visible = false;
                for frame in frames {
                    let clean =
                        rgb_frame_at_index(&output.join("clean-picture.mkv"), &plan, frame)?;
                    let rendered = rgb_frame_at_index(&layered_picture, &plan, frame)?;
                    if clean.len() != rendered.len() {
                        bail!("ASS layer changed picture geometry");
                    }
                    if clean != rendered {
                        visible = true;
                        break;
                    }
                }
                if !visible {
                    bail!(
                        "selected ASS layer made no visible change during its dialogue intervals"
                    );
                }
            }
            ExternalLayerRenderMode::TimedVideoOverlay => {
                if receipt.outputs["selected-overlay.mkv"].sha256
                    != timed_overlay_source(asset_root, layer)?.sha256
                {
                    bail!("rendered timed-overlay source differs from selected evidence");
                }
                let span = plan
                    .external_layer_spans
                    .iter()
                    .find(|span| span.attachment_id == layer.attachment_id)
                    .context("timed overlay span missing")?;
                let active_frames = span.end_frame - span.start_frame;
                let mut visible = false;
                for frame in [
                    span.start_frame,
                    span.start_frame + active_frames / 4,
                    span.start_frame + active_frames / 2,
                    span.start_frame + active_frames * 3 / 4,
                    span.end_frame - 1,
                ] {
                    let clean =
                        rgb_frame_at_index(&output.join("clean-picture.mkv"), &plan, frame)?;
                    let rendered = rgb_frame_at_index(&layered_picture, &plan, frame)?;
                    if clean.len() != rendered.len() {
                        bail!("timed overlay changed picture dimensions at frame {frame}");
                    }
                    visible |= clean != rendered;
                }
                if !visible {
                    bail!("timed overlay made no visible change in its selected span");
                }
                for frame in [
                    span.start_frame.checked_sub(1),
                    (span.end_frame < plan.frame_count).then_some(span.end_frame),
                ]
                .into_iter()
                .flatten()
                {
                    let clean =
                        rgb_frame_at_index(&output.join("clean-picture.mkv"), &plan, frame)?;
                    let rendered = rgb_frame_at_index(&layered_picture, &plan, frame)?;
                    if clean != rendered {
                        bail!("timed overlay changed clean picture outside its selected span");
                    }
                }
            }
            ExternalLayerRenderMode::EvidenceOnly => unreachable!(),
        }
    }
    for name in ["D.wav", "M.wav", "E.wav", "mix.wav", "master.mkv"] {
        let info = probe(&output.join(name))?;
        let audio = info["streams"]
            .as_array()
            .and_then(|xs| xs.iter().find(|s| s["codec_type"] == "audio"))
            .context("delivery audio stream missing")?;
        if audio["codec_name"] != "pcm_s24le"
            || audio["sample_rate"].as_str() != Some(&plan.sample_rate.to_string())
            || audio["channels"].as_u64() != Some(2)
        {
            bail!("delivery PCM format mismatch: {name}");
        }
        if pcm_samples(&output.join(name), plan.sample_rate)? != plan.duration_samples {
            bail!("decoded sample count mismatch: {name}");
        }
    }
    for name in ["picture.mkv", "master.mkv", "review.mp4"] {
        let o = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-count_frames",
                "-show_streams",
                "-of",
                "json",
            ])
            .arg(output.join(name))
            .output()?;
        if !o.status.success() {
            bail!("frame decode failed");
        }
        let d: serde_json::Value = serde_json::from_slice(&o.stdout)?;
        let stream = d["streams"]
            .as_array()
            .and_then(|xs| xs.iter().find(|s| s["codec_type"] == "video"))
            .context("delivery video missing")?;
        let rate = stream["r_frame_rate"]
            .as_str()
            .context("video frame rate missing")?;
        let (rn, rd) = rate.split_once('/').context("invalid video frame rate")?;
        let (rn, rd) = (rn.parse::<u64>()?, rd.parse::<u64>()?);
        let codec = if name == "review.mp4"
            || ((!overlays.is_empty() || job.post_compose_camera.is_some())
                && job.composition_encoding.as_deref() == Some("h264-lossless"))
            || (overlays.is_empty()
                && job.post_compose_camera.is_none()
                && job.still_sequence_encoding.as_deref() == Some("h264-lossless"))
        {
            "h264"
        } else {
            "ffv1"
        };
        if stream["codec_name"] != codec
            || rd == 0
            || u128::from(rn) * u128::from(plan.fps_denominator)
                != u128::from(rd) * u128::from(plan.fps_numerator)
        {
            bail!("delivery codec/frame rate mismatch: {name}");
        }
        if stream["nb_read_frames"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            != Some(plan.frame_count)
            || stream["width"].as_u64() != Some(u64::from(job.width))
            || stream["height"].as_u64() != Some(u64::from(job.height))
        {
            bail!("delivery frame geometry mismatch: {name}");
        }
    }
    Ok(receipt)
}
