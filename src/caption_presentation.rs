use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, anyhow, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    caption::{self, CaptionCheckReport, CaptionThresholdReport, CaptionThresholds},
    production::LoadedProductionManifest,
    series::parse_srt,
};

pub const CAPTION_PRESENTATION_SCHEMA: &str = "reel.caption-presentation.v0.1";
pub const CAPTION_LINEAGE_SCHEMA: &str = "reel.caption-lineage.v0.1";

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum CaptionProfile {
    #[default]
    YoutubeReview,
    PhoneReview,
    PrivateReview,
}

impl CaptionProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::YoutubeReview => "youtube-review",
            Self::PhoneReview => "phone-review",
            Self::PrivateReview => "private-review",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "youtube-review" => Ok(Self::YoutubeReview),
            "phone-review" => Ok(Self::PhoneReview),
            "private-review" => Ok(Self::PrivateReview),
            _ => bail!("unsupported caption profile {value}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum SpeakerLabelPolicy {
    #[default]
    None,
    FirstEntrance,
    Persistent,
    ReintroduceAfterMs,
}

impl SpeakerLabelPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::FirstEntrance => "first-entrance",
            Self::Persistent => "persistent",
            Self::ReintroduceAfterMs => "reintroduce-after-ms",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "none" => Ok(Self::None),
            "first-entrance" => Ok(Self::FirstEntrance),
            "persistent" => Ok(Self::Persistent),
            "reintroduce-after-ms" => Ok(Self::ReintroduceAfterMs),
            _ => bail!("unsupported speaker label policy {value}"),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptionPresentationInput {
    pub schema: String,
    pub speakers: Vec<SpeakerLabel>,
    pub cues: Vec<DeliveryCueAssignment>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeakerLabel {
    pub speaker_id: String,
    pub audience_label: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryCueAssignment {
    pub srt_index: usize,
    pub narration_cue_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    fn right(&self) -> u64 {
        u64::from(self.x) + u64::from(self.width)
    }

    fn bottom(&self) -> u64 {
        u64::from(self.y) + u64::from(self.height)
    }

    fn intersects(&self, other: &Self) -> bool {
        u64::from(self.x) < other.right()
            && self.right() > u64::from(other.x)
            && u64::from(self.y) < other.bottom()
            && self.bottom() > u64::from(other.y)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CaptionStyleReport {
    pub profile: String,
    pub orientation: String,
    pub font_family: String,
    pub caption_font_size: u32,
    pub caption_text_color: String,
    pub caption_outline_px: u32,
    pub caption_region: PixelRect,
    pub badge_font_size_px: u32,
    pub badge_text_color: String,
    pub badge_background: String,
    pub badge_padding_px: u32,
    pub badge_region: PixelRect,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SpeakerLabelEvent {
    pub srt_index: usize,
    pub narration_cue_id: String,
    pub speaker_id: String,
    pub audience_label: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CaptionLineage {
    pub schema: String,
    pub captions_sha256: String,
    pub caption_check_report_sha256: String,
    pub thresholds: CaptionThresholdReport,
    pub threshold_policy_note: Option<String>,
    pub profile: String,
    pub speaker_label_policy: String,
    pub speaker_reintroduce_after_ms: Option<u64>,
    pub presentation_input_sha256: Option<String>,
    pub presentation_sha256: String,
    pub style: CaptionStyleReport,
    pub label_events: Vec<SpeakerLabelEvent>,
    pub check: CaptionCheckReport,
    pub passed: bool,
}

#[derive(Clone, Debug)]
pub struct CaptionPresentationOptions<'a> {
    pub captions: &'a Path,
    pub presentation: Option<&'a Path>,
    pub profile: CaptionProfile,
    pub policy: SpeakerLabelPolicy,
    pub reintroduce_after_ms: Option<u64>,
    pub thresholds: CaptionThresholds,
    pub threshold_policy_note: Option<&'a str>,
    pub width: u32,
    pub height: u32,
}

pub fn prepare(
    loaded: &LoadedProductionManifest,
    options: CaptionPresentationOptions<'_>,
) -> Result<CaptionLineage> {
    let default_thresholds = CaptionThresholds::default();
    if options.thresholds != default_thresholds
        && options
            .threshold_policy_note
            .is_none_or(|note| note.trim().is_empty())
    {
        bail!("caption threshold overrides require --caption-policy-note");
    }
    if options.policy == SpeakerLabelPolicy::ReintroduceAfterMs
        && options.reintroduce_after_ms.is_none_or(|value| value == 0)
    {
        bail!("reintroduce-after-ms policy requires a positive --speaker-reintroduce-after-ms");
    }
    if options.policy != SpeakerLabelPolicy::ReintroduceAfterMs
        && options.reintroduce_after_ms.is_some()
    {
        bail!("--speaker-reintroduce-after-ms requires reintroduce-after-ms policy");
    }
    let check = caption::check(options.captions, options.thresholds)?;
    if !check.passed {
        bail!(
            "caption accessibility preflight failed with {} violation(s)",
            check.violations.len()
        );
    }
    let check_bytes = serde_json::to_vec(&check)?;
    let style = style_for(options.profile, options.width, options.height)?;
    validate_style(&style, options.width, options.height)?;

    let (presentation_input_sha256, label_events) = if options.policy == SpeakerLabelPolicy::None {
        if options.presentation.is_some() {
            bail!("speaker label policy none does not accept a presentation input");
        }
        (None, Vec::new())
    } else {
        let path = options
            .presentation
            .ok_or_else(|| anyhow!("speaker label policy requires --caption-presentation"))?;
        let bytes = fs::read(path)
            .with_context(|| format!("failed to read caption presentation {}", path.display()))?;
        let input: CaptionPresentationInput =
            serde_yaml::from_slice(&bytes).context("caption presentation is not valid YAML")?;
        if input.schema != CAPTION_PRESENTATION_SCHEMA {
            bail!("caption presentation schema must be {CAPTION_PRESENTATION_SCHEMA}");
        }
        let events = label_events(loaded, options.captions, &input, &options)?;
        validate_label_fit(&style, &events)?;
        (Some(sha256(&bytes)), events)
    };

    let presentation_sha256 = sha256(&serde_json::to_vec(&serde_json::json!({
        "profile": options.profile.as_str(),
        "policy": options.policy.as_str(),
        "reintroduce_after_ms": options.reintroduce_after_ms,
        "style": &style,
        "events": &label_events,
    }))?);
    Ok(CaptionLineage {
        schema: CAPTION_LINEAGE_SCHEMA.to_string(),
        captions_sha256: check.captions_sha256.clone(),
        caption_check_report_sha256: sha256(&check_bytes),
        thresholds: check.thresholds.clone(),
        threshold_policy_note: options
            .threshold_policy_note
            .map(str::trim)
            .filter(|note| !note.is_empty())
            .map(str::to_string),
        profile: options.profile.as_str().to_string(),
        speaker_label_policy: options.policy.as_str().to_string(),
        speaker_reintroduce_after_ms: options.reintroduce_after_ms,
        presentation_input_sha256,
        presentation_sha256,
        style,
        label_events,
        check,
        passed: true,
    })
}

fn label_events(
    loaded: &LoadedProductionManifest,
    captions: &Path,
    input: &CaptionPresentationInput,
    options: &CaptionPresentationOptions<'_>,
) -> Result<Vec<SpeakerLabelEvent>> {
    let entries = parse_srt(&fs::read_to_string(captions)?)?;
    let speakers = loaded
        .manifest
        .speakers
        .iter()
        .map(|speaker| (speaker.id.as_str(), speaker))
        .collect::<BTreeMap<_, _>>();
    let cues = loaded
        .manifest
        .narration_cues
        .iter()
        .map(|cue| (cue.id.as_str(), cue))
        .collect::<BTreeMap<_, _>>();
    let mut labels = BTreeMap::new();
    for label in &input.speakers {
        if !speakers.contains_key(label.speaker_id.as_str()) {
            bail!(
                "caption presentation references unknown speaker {}",
                label.speaker_id
            );
        }
        validate_label(&label.audience_label)?;
        if labels
            .insert(label.speaker_id.as_str(), label.audience_label.as_str())
            .is_some()
        {
            bail!("caption presentation repeats speaker {}", label.speaker_id);
        }
    }
    if input.cues.len() != entries.len() {
        bail!("caption presentation must assign every SRT cue exactly once");
    }
    let mut assignments = BTreeMap::new();
    for assignment in &input.cues {
        if assignments
            .insert(assignment.srt_index, assignment.narration_cue_id.as_str())
            .is_some()
        {
            bail!(
                "caption presentation repeats SRT cue {}",
                assignment.srt_index
            );
        }
    }

    let mut candidates = Vec::new();
    for entry in &entries {
        let cue_id = assignments.get(&entry.index).ok_or_else(|| {
            anyhow!(
                "caption presentation does not assign SRT cue {}",
                entry.index
            )
        })?;
        let cue = cues.get(cue_id).ok_or_else(|| {
            anyhow!("caption presentation references unknown narration cue {cue_id}")
        })?;
        let cue_start = (cue
            .start_seconds
            .ok_or_else(|| anyhow!("narration cue {cue_id} has no conformed start"))?
            * 1000.0)
            .round() as u64;
        let cue_end = cue_start
            + (cue
                .duration_seconds
                .ok_or_else(|| anyhow!("narration cue {cue_id} has no conformed duration"))?
                * 1000.0)
                .round() as u64;
        if entry.start_ms < cue_start || entry.end_ms > cue_end {
            bail!(
                "SRT cue {} timing is outside narration cue {cue_id}",
                entry.index
            );
        }
        let label = labels.get(cue.speaker_id.as_str()).ok_or_else(|| {
            anyhow!(
                "caption presentation has no audience label for speaker {}",
                cue.speaker_id
            )
        })?;
        candidates.push(SpeakerLabelEvent {
            srt_index: entry.index,
            narration_cue_id: cue.id.clone(),
            speaker_id: cue.speaker_id.clone(),
            audience_label: (*label).to_string(),
            start_ms: entry.start_ms,
            end_ms: entry.end_ms,
        });
    }

    let mut seen = BTreeSet::new();
    let mut last_end = BTreeMap::<String, u64>::new();
    candidates.retain(|event| {
        let show = match options.policy {
            SpeakerLabelPolicy::None => false,
            SpeakerLabelPolicy::Persistent => true,
            SpeakerLabelPolicy::FirstEntrance => seen.insert(event.speaker_id.clone()),
            SpeakerLabelPolicy::ReintroduceAfterMs => {
                let first = seen.insert(event.speaker_id.clone());
                first
                    || last_end.get(&event.speaker_id).is_some_and(|previous_end| {
                        event.start_ms.saturating_sub(*previous_end)
                            >= options.reintroduce_after_ms.unwrap_or_default()
                    })
            }
        };
        last_end.insert(event.speaker_id.clone(), event.end_ms);
        show
    });
    Ok(candidates)
}

fn validate_label(label: &str) -> Result<()> {
    let length = label.trim().chars().count();
    if length == 0
        || length > 48
        || label != label.trim()
        || label.chars().any(|character| {
            !(character.is_alphanumeric()
                || character.is_whitespace()
                || matches!(character, '-' | '.' | '\'' | '’'))
        })
    {
        bail!(
            "audience speaker labels must contain 1..48 safe name characters without outer whitespace"
        );
    }
    Ok(())
}

fn style_for(profile: CaptionProfile, width: u32, height: u32) -> Result<CaptionStyleReport> {
    if width < 640 || height < 360 {
        bail!("caption presentation requires at least 640x360 output");
    }
    if profile == CaptionProfile::YoutubeReview && u64::from(width) * 9 != u64::from(height) * 16 {
        bail!("youtube-review caption profile requires a 16:9 output");
    }
    if profile == CaptionProfile::PhoneReview && u64::from(width) * 16 != u64::from(height) * 9 {
        bail!("phone-review caption profile requires a 9:16 output");
    }
    let portrait = height > width;
    let caption_font_size = match (profile, portrait) {
        (CaptionProfile::PrivateReview, true) => 20,
        (CaptionProfile::PrivateReview, false) => 18,
        (CaptionProfile::PhoneReview, _) => 44,
        (CaptionProfile::YoutubeReview, true) => 42,
        (CaptionProfile::YoutubeReview, false) => 36,
    };
    let (badge_font_size_px, caption_y, caption_height) = if portrait {
        (32, 900_u32 * height / 1280, 260_u32 * height / 1280)
    } else {
        (28, 520_u32 * height / 720, 150_u32 * height / 720)
    };
    let horizontal_margin = match profile {
        CaptionProfile::PhoneReview => width * 8 / 100,
        CaptionProfile::YoutubeReview | CaptionProfile::PrivateReview => width * 5 / 100,
    };
    let badge_y = match profile {
        CaptionProfile::PrivateReview => height * 10 / 100,
        CaptionProfile::YoutubeReview | CaptionProfile::PhoneReview => height * 6 / 100,
    };
    Ok(CaptionStyleReport {
        profile: profile.as_str().to_string(),
        orientation: if portrait { "9:16" } else { "16:9" }.to_string(),
        font_family: "sans-serif".to_string(),
        caption_font_size,
        caption_text_color: "white".to_string(),
        caption_outline_px: 2,
        caption_region: PixelRect {
            x: horizontal_margin,
            y: caption_y,
            width: width - horizontal_margin * 2,
            height: caption_height,
        },
        badge_font_size_px,
        badge_text_color: "white".to_string(),
        badge_background: "black@0.82".to_string(),
        badge_padding_px: if portrait { 12 } else { 10 },
        badge_region: PixelRect {
            x: horizontal_margin,
            y: badge_y,
            width: width * 55 / 100,
            height: height * 10 / 100,
        },
    })
}

fn validate_style(style: &CaptionStyleReport, width: u32, height: u32) -> Result<()> {
    for (name, region) in [
        ("caption", &style.caption_region),
        ("speaker badge", &style.badge_region),
    ] {
        if region.width == 0
            || region.height == 0
            || region.right() > u64::from(width)
            || region.bottom() > u64::from(height)
        {
            bail!("{name} safe region is outside the output frame");
        }
    }
    if style.caption_region.intersects(&style.badge_region) {
        bail!("speaker badge and caption safe regions overlap");
    }
    if style.badge_font_size_px == 0
        || style.caption_font_size == 0
        || style.badge_font_size_px + style.badge_padding_px * 2 > style.badge_region.height
    {
        bail!("caption presentation font or margin profile is infeasible");
    }
    Ok(())
}

fn validate_label_fit(style: &CaptionStyleReport, events: &[SpeakerLabelEvent]) -> Result<()> {
    for event in events {
        let estimated_width = (event.audience_label.chars().count() as f64
            * f64::from(style.badge_font_size_px)
            * 0.62)
            .ceil() as u64
            + u64::from(style.badge_padding_px) * 2;
        if estimated_width > u64::from(style.badge_region.width) {
            bail!(
                "speaker label for {} is infeasible in the selected caption profile",
                event.speaker_id
            );
        }
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = "manifests/fixtures/speaker-captions/manifest.yaml";
    const CAPTIONS: &str = "manifests/fixtures/speaker-captions/captions.srt";
    const PRESENTATION: &str = "manifests/fixtures/speaker-captions/presentation.yaml";

    fn prepare_fixture(
        policy: SpeakerLabelPolicy,
        reintroduce_after_ms: Option<u64>,
    ) -> CaptionLineage {
        let loaded = crate::production::load(MANIFEST).unwrap();
        prepare(
            &loaded,
            CaptionPresentationOptions {
                captions: Path::new(CAPTIONS),
                presentation: (policy != SpeakerLabelPolicy::None)
                    .then_some(Path::new(PRESENTATION)),
                profile: CaptionProfile::YoutubeReview,
                policy,
                reintroduce_after_ms,
                thresholds: CaptionThresholds::default(),
                threshold_policy_note: None,
                width: 1280,
                height: 720,
            },
        )
        .unwrap()
    }

    #[test]
    fn style_profiles_keep_badges_and_captions_separate() {
        for (width, height, profile) in [
            (1280, 720, CaptionProfile::YoutubeReview),
            (720, 1280, CaptionProfile::PhoneReview),
        ] {
            let style = style_for(profile, width, height).unwrap();
            validate_style(&style, width, height).unwrap();
            assert!(!style.caption_region.intersects(&style.badge_region));
        }
    }

    #[test]
    fn overlapping_regions_are_rejected() {
        let mut style = style_for(CaptionProfile::YoutubeReview, 1280, 720).unwrap();
        style.badge_region = style.caption_region.clone();
        assert!(validate_style(&style, 1280, 720).is_err());
    }

    #[test]
    fn policies_select_only_explicitly_mapped_speaker_entrances() {
        let none = prepare_fixture(SpeakerLabelPolicy::None, None);
        let first = prepare_fixture(SpeakerLabelPolicy::FirstEntrance, None);
        let persistent = prepare_fixture(SpeakerLabelPolicy::Persistent, None);
        let reintroduced = prepare_fixture(SpeakerLabelPolicy::ReintroduceAfterMs, Some(10_000));
        assert!(none.label_events.is_empty());
        assert_eq!(first.label_events.len(), 3);
        assert_eq!(persistent.label_events.len(), 11);
        assert_eq!(reintroduced.label_events.len(), 4);
        assert_eq!(reintroduced.label_events[3].srt_index, 10);
        assert_eq!(none.captions_sha256, first.captions_sha256);
        assert_eq!(first.captions_sha256, persistent.captions_sha256);
        assert_ne!(first.presentation_sha256, persistent.presentation_sha256);
    }
}
