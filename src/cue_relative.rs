//! Cue-relative assembly timing for REEL production manifests.
//!
//! This is a compiler sidecar for `reel.manifest.v0.2`: it binds semantic
//! anchors to the manifest's stable cue/shot/beat/audio/camera/effect IDs and
//! emits an integer-sample conform. It deliberately does not introduce media
//! or rendering objects of its own.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::production;

pub const CONTRACT_SCHEMA: &str = "reel.cue-relative-assembly.v0.1";
pub const COMPILED_SCHEMA: &str = "reel.cue-relative-conform.v0.1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub schema: String,
    pub id: String,
    pub production_manifest: PathBuf,
    pub sample_rate: u32,
    pub frame_rate: Rational,
    pub cues: Vec<CueClock>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rational {
    pub numerator: u64,
    pub denominator: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CueClock {
    pub cue_id: String,
    pub duration_samples: u64,
    #[serde(default)]
    pub markers: Vec<CueMarker>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CueMarker {
    pub id: String,
    pub kind: MarkerKind,
    pub start_sample: u64,
    pub end_sample: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MarkerKind {
    Word,
    Phrase,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    pub id: String,
    pub target: Target,
    pub start: Anchor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<Anchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_boundary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_boundary: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Target {
    Shot {
        shot_id: String,
    },
    Cel {
        shot_id: String,
        cel_id: String,
    },
    Beat {
        beat_marker_id: String,
    },
    Camera {
        shot_id: String,
        camera_track_id: String,
    },
    Effect {
        shot_id: String,
        effect_pass_id: String,
    },
    Audio {
        audio_event_id: String,
    },
    Sonic {
        audio_event_id: String,
    },
    Caption {
        cue_id: String,
        caption_id: String,
    },
    Title {
        title_id: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Anchor {
    CueStart {
        cue_id: String,
        #[serde(default)]
        offset_samples: i64,
    },
    CueEnd {
        cue_id: String,
        #[serde(default)]
        offset_samples: i64,
    },
    CueProgress {
        cue_id: String,
        numerator: u64,
        denominator: u64,
        #[serde(default)]
        offset_samples: i64,
        #[serde(default)]
        rounding: FractionRounding,
    },
    Marker {
        cue_id: String,
        marker_id: String,
        edge: MarkerEdge,
        #[serde(default)]
        offset_samples: i64,
    },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FractionRounding {
    #[default]
    NearestHalfUp,
    Floor,
    Ceiling,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MarkerEdge {
    Start,
    End,
}

#[derive(Clone, Debug, Serialize)]
pub struct CompiledConform {
    pub schema: String,
    pub contract_id: String,
    pub production_manifest_sha256: String,
    pub sample_rate: u32,
    pub frame_rate: Rational,
    pub frame_rounding: FrameRounding,
    pub duration_samples: u64,
    pub cues: Vec<CompiledCue>,
    pub attachments: Vec<CompiledAttachment>,
    pub shared_boundaries: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FrameRounding {
    pub starts: String,
    pub ends: String,
    pub intervals: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CompiledCue {
    pub cue_id: String,
    pub start_sample: u64,
    pub end_sample: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CompiledAttachment {
    pub id: String,
    pub target: Target,
    pub start_sample: u64,
    pub end_sample: u64,
    pub start_frame: u64,
    pub end_frame_exclusive: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_boundary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_boundary: Option<String>,
}

pub fn load(path: impl AsRef<Path>) -> Result<Contract> {
    let path = path.as_ref();
    serde_yaml::from_slice(
        &fs::read(path).with_context(|| format!("failed to read {}", path.display()))?,
    )
    .with_context(|| format!("failed to parse {}", path.display()))
}

pub fn compile_file(input: impl AsRef<Path>, output: impl AsRef<Path>) -> Result<CompiledConform> {
    let input = input.as_ref();
    let contract = load(input)?;
    let base = input.parent().unwrap_or_else(|| Path::new("."));
    let compiled = compile(&contract, base)?;
    let bytes = serde_json::to_vec_pretty(&compiled)?;
    fs::write(output.as_ref(), bytes)
        .with_context(|| format!("failed to write {}", output.as_ref().display()))?;
    Ok(compiled)
}

pub fn compile(contract: &Contract, base: &Path) -> Result<CompiledConform> {
    if contract.schema != CONTRACT_SCHEMA {
        bail!(
            "unsupported schema {}; expected {CONTRACT_SCHEMA}",
            contract.schema
        );
    }
    valid_id("contract", &contract.id)?;
    if contract.sample_rate == 0 {
        bail!("sample_rate must be positive");
    }
    if contract.frame_rate.numerator == 0 || contract.frame_rate.denominator == 0 {
        bail!("frame_rate terms must be positive");
    }
    if contract.cues.is_empty() {
        bail!("cues must not be empty");
    }

    let manifest_path = base.join(&contract.production_manifest);
    let loaded = production::load(&manifest_path)?;
    let manifest = &loaded.manifest;
    let manifest_cues: BTreeSet<_> = manifest
        .narration_cues
        .iter()
        .map(|x| x.id.as_str())
        .collect();
    let shots: BTreeMap<_, _> = manifest.shots.iter().map(|x| (x.id.as_str(), x)).collect();
    let beats: BTreeSet<_> = manifest
        .beat_markers
        .iter()
        .map(|x| x.id.as_str())
        .collect();
    let audio: BTreeSet<_> = manifest
        .audio_events
        .iter()
        .map(|x| x.id.as_str())
        .collect();

    let mut cue_ranges = BTreeMap::new();
    let mut cue_markers = BTreeMap::new();
    let mut cursor = 0u64;
    let mut compiled_cues = Vec::new();
    for cue in &contract.cues {
        valid_id("cue", &cue.cue_id)?;
        if !manifest_cues.contains(cue.cue_id.as_str()) {
            bail!("cue {} is missing from production manifest", cue.cue_id);
        }
        if cue.duration_samples == 0 {
            bail!("cue {} duration_samples must be positive", cue.cue_id);
        }
        let end = cursor
            .checked_add(cue.duration_samples)
            .ok_or_else(|| anyhow!("cue clock overflow"))?;
        if cue_ranges
            .insert(cue.cue_id.as_str(), (cursor, end))
            .is_some()
        {
            bail!("duplicate cue {}", cue.cue_id);
        }
        let mut ids = BTreeSet::new();
        for marker in &cue.markers {
            valid_id("marker", &marker.id)?;
            if !ids.insert(marker.id.as_str()) {
                bail!("duplicate marker {} in cue {}", marker.id, cue.cue_id);
            }
            if marker.start_sample > marker.end_sample || marker.end_sample > cue.duration_samples {
                bail!(
                    "marker {} is out of range for cue {}",
                    marker.id,
                    cue.cue_id
                );
            }
            cue_markers.insert((cue.cue_id.as_str(), marker.id.as_str()), marker);
        }
        compiled_cues.push(CompiledCue {
            cue_id: cue.cue_id.clone(),
            start_sample: cursor,
            end_sample: end,
        });
        cursor = end;
    }

    let mut attachment_ids = BTreeSet::new();
    let mut boundary_samples = BTreeMap::<String, u64>::new();
    let mut compiled = Vec::new();
    for attachment in &contract.attachments {
        valid_id("attachment", &attachment.id)?;
        if !attachment_ids.insert(attachment.id.as_str()) {
            bail!("duplicate attachment {}", attachment.id);
        }
        validate_target(&attachment.target, &shots, &beats, &audio, &manifest_cues)?;
        let start = resolve_anchor(&attachment.start, &cue_ranges, &cue_markers)?;
        let end = match &attachment.end {
            Some(anchor) => resolve_anchor(anchor, &cue_ranges, &cue_markers)?,
            None => start,
        };
        if start > cursor || end > cursor {
            bail!("attachment {} resolves outside sequence", attachment.id);
        }
        if end < start {
            bail!("attachment {} ends before it starts", attachment.id);
        }
        bind_boundary(
            &mut boundary_samples,
            attachment.start_boundary.as_deref(),
            start,
            &attachment.id,
        )?;
        bind_boundary(
            &mut boundary_samples,
            attachment.end_boundary.as_deref(),
            end,
            &attachment.id,
        )?;
        compiled.push(CompiledAttachment {
            id: attachment.id.clone(),
            target: attachment.target.clone(),
            start_sample: start,
            end_sample: end,
            start_frame: sample_to_frame(start, contract.sample_rate, contract.frame_rate, false)?,
            end_frame_exclusive: sample_to_frame(
                end,
                contract.sample_rate,
                contract.frame_rate,
                true,
            )?,
            start_boundary: attachment.start_boundary.clone(),
            end_boundary: attachment.end_boundary.clone(),
        });
    }
    compiled.sort_by(|a, b| (a.start_sample, &a.id).cmp(&(b.start_sample, &b.id)));
    Ok(CompiledConform {
        schema: COMPILED_SCHEMA.into(),
        contract_id: contract.id.clone(),
        production_manifest_sha256: production::sha256_bytes(&loaded.bytes),
        sample_rate: contract.sample_rate,
        frame_rate: contract.frame_rate,
        frame_rounding: FrameRounding {
            starts: "floor(sample * fps_numerator / (sample_rate * fps_denominator))".into(),
            ends: "ceiling(sample * fps_numerator / (sample_rate * fps_denominator))".into(),
            intervals: "half-open [start_frame, end_frame_exclusive)".into(),
        },
        duration_samples: cursor,
        cues: compiled_cues,
        attachments: compiled,
        shared_boundaries: boundary_samples,
    })
}

fn validate_target<'a>(
    target: &Target,
    shots: &BTreeMap<&'a str, &'a production::Shot>,
    beats: &BTreeSet<&str>,
    audio: &BTreeSet<&str>,
    cues: &BTreeSet<&str>,
) -> Result<()> {
    match target {
        Target::Shot { shot_id } | Target::Cel { shot_id, .. } => {
            require(shots.contains_key(shot_id.as_str()), "shot", shot_id)
        }
        Target::Beat { beat_marker_id } => require(
            beats.contains(beat_marker_id.as_str()),
            "beat marker",
            beat_marker_id,
        ),
        Target::Audio { audio_event_id } | Target::Sonic { audio_event_id } => require(
            audio.contains(audio_event_id.as_str()),
            "audio event",
            audio_event_id,
        ),
        Target::Caption { cue_id, .. } => {
            require(cues.contains(cue_id.as_str()), "caption cue", cue_id)
        }
        Target::Title { title_id } => valid_id("title", title_id),
        Target::Camera {
            shot_id,
            camera_track_id,
        } => {
            let shot = shots
                .get(shot_id.as_str())
                .ok_or_else(|| anyhow!("camera target shot {} is missing", shot_id))?;
            if shot.camera_track.is_none() {
                bail!("camera target shot {} has no camera_track", shot_id);
            }
            valid_id("camera track", camera_track_id)
        }
        Target::Effect {
            shot_id,
            effect_pass_id,
        } => {
            let shot = shots
                .get(shot_id.as_str())
                .ok_or_else(|| anyhow!("effect target shot {} is missing", shot_id))?;
            if !shot.effect_passes.iter().any(|x| x.id == *effect_pass_id) {
                bail!(
                    "effect pass {} is missing from shot {}",
                    effect_pass_id,
                    shot_id
                );
            }
            Ok(())
        }
    }
}

fn resolve_anchor<'a>(
    anchor: &Anchor,
    cues: &BTreeMap<&'a str, (u64, u64)>,
    markers: &BTreeMap<(&'a str, &'a str), &'a CueMarker>,
) -> Result<u64> {
    let (cue_id, base, offset) = match anchor {
        Anchor::CueStart {
            cue_id,
            offset_samples,
        } => (cue_id, cue(cues, cue_id)?.0, *offset_samples),
        Anchor::CueEnd {
            cue_id,
            offset_samples,
        } => (cue_id, cue(cues, cue_id)?.1, *offset_samples),
        Anchor::CueProgress {
            cue_id,
            numerator,
            denominator,
            offset_samples,
            rounding,
        } => {
            if *denominator == 0 || numerator > denominator {
                bail!(
                    "cue progress for {} must be within 0/denominator..1",
                    cue_id
                );
            }
            let (start, end) = cue(cues, cue_id)?;
            let duration = end - start;
            let product = u128::from(duration) * u128::from(*numerator);
            let den = u128::from(*denominator);
            let relative = match rounding {
                FractionRounding::Floor => (product / den) as u64,
                FractionRounding::Ceiling => product.div_ceil(den) as u64,
                FractionRounding::NearestHalfUp => ((product + den / 2) / den) as u64,
            };
            (cue_id, start + relative, *offset_samples)
        }
        Anchor::Marker {
            cue_id,
            marker_id,
            edge,
            offset_samples,
        } => {
            let (start, _) = cue(cues, cue_id)?;
            let marker = markers
                .get(&(cue_id.as_str(), marker_id.as_str()))
                .ok_or_else(|| {
                    anyhow!(
                        "marker {} is missing or ambiguous in cue {}",
                        marker_id,
                        cue_id
                    )
                })?;
            (
                cue_id,
                start
                    + match edge {
                        MarkerEdge::Start => marker.start_sample,
                        MarkerEdge::End => marker.end_sample,
                    },
                *offset_samples,
            )
        }
    };
    let value = i128::from(base) + i128::from(offset);
    if value < 0 || value > i128::from(u64::MAX) {
        bail!(
            "anchor with offset resolves outside sequence near cue {}",
            cue_id
        );
    }
    Ok(value as u64)
}

fn sample_to_frame(sample: u64, sample_rate: u32, fps: Rational, ceil: bool) -> Result<u64> {
    let n = u128::from(sample) * u128::from(fps.numerator);
    let d = u128::from(sample_rate) * u128::from(fps.denominator);
    let v = if ceil { n.div_ceil(d) } else { n / d };
    u64::try_from(v).map_err(|_| anyhow!("frame index overflow"))
}
fn cue(cues: &BTreeMap<&str, (u64, u64)>, id: &str) -> Result<(u64, u64)> {
    cues.get(id)
        .copied()
        .ok_or_else(|| anyhow!("cue {} is missing or ambiguous", id))
}
fn bind_boundary(
    map: &mut BTreeMap<String, u64>,
    id: Option<&str>,
    sample: u64,
    owner: &str,
) -> Result<()> {
    if let Some(id) = id {
        valid_id("shared boundary", id)?;
        if let Some(old) = map.insert(id.into(), sample) {
            if old != sample {
                bail!(
                    "shared boundary {} disagrees: {} vs {} samples at {}",
                    id,
                    old,
                    sample,
                    owner
                );
            }
        }
    }
    Ok(())
}
fn require(ok: bool, kind: &str, id: &str) -> Result<()> {
    if !ok {
        bail!("{} {} is missing from production manifest", kind, id)
    }
    Ok(())
}
fn valid_id(kind: &str, id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        bail!("invalid {} id: {}", kind, id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn anchor(cue: &str) -> Anchor {
        Anchor::CueEnd {
            cue_id: cue.into(),
            offset_samples: 0,
        }
    }
    #[test]
    fn shared_boundary_disagreement_fails() {
        let contract = Contract {
            schema: CONTRACT_SCHEMA.into(),
            id: "x".into(),
            production_manifest: "unused".into(),
            sample_rate: 48_000,
            frame_rate: Rational {
                numerator: 24,
                denominator: 1,
            },
            cues: vec![],
            attachments: vec![],
        };
        assert!(compile(&contract, Path::new(".")).is_err());
        let _ = anchor("a");
    }
}
