//! Hash-bound, language-local lossless episode conform from selected scene and
//! presentation masters. Source selection belongs to the authoring bindings.

use crate::{episode_delivery, presentation_adopt, scene_delivery};
use anyhow::{Context, Result, bail};
use reel_assembly::scene_authoring::{
    Episode, ScopedBindings, TemplateCatalog, resolve_episode_presentation,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
};

pub const SCHEMA: &str = "reel.episode-conform.v1";

fn verify_native_delivery_proof(
    receipt: &serde_json::Value,
    job: &scene_delivery::FileRef,
    delivery: &scene_delivery::FileRef,
) -> Result<()> {
    if receipt["scene_delivery_receipt_sha256"] != delivery.sha256 {
        bail!("scene build receipt differs from selected delivery receipt");
    }
    if receipt["selected_delivery_job_sha256"] != job.sha256 {
        bail!("scene build receipt differs from selected delivery job");
    }
    Ok(())
}

#[cfg(test)]
mod native_proof_tests {
    use super::*;
    #[test]
    fn repinned_job_or_receipt_cannot_reuse_an_old_native_proof() {
        let mut job = scene_delivery::FileRef {
            path: "job.json".into(),
            sha256: "a".repeat(64),
            bytes: 1,
        };
        let mut delivery = scene_delivery::FileRef {
            path: "receipt.json".into(),
            sha256: "b".repeat(64),
            bytes: 1,
        };
        let proof = serde_json::json!({"selected_delivery_job_sha256":job.sha256,"scene_delivery_receipt_sha256":delivery.sha256});
        verify_native_delivery_proof(&proof, &job, &delivery).unwrap();
        job.sha256 = "c".repeat(64);
        assert!(verify_native_delivery_proof(&proof, &job, &delivery).is_err());
        job.sha256 = "a".repeat(64);
        delivery.sha256 = "d".repeat(64);
        assert!(verify_native_delivery_proof(&proof, &job, &delivery).is_err());
        assert!(verify_native_delivery_proof(&serde_json::json!({}), &job, &delivery).is_err());
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub episode_id: String,
    pub language: String,
    pub output_sample_rate: u32,
    pub catalog: scene_delivery::FileRef,
    pub master_template_definition: scene_delivery::FileRef,
    #[serde(default)]
    pub source_master_template: Option<scene_delivery::FileRef>,
    pub episode: scene_delivery::FileRef,
    pub season_bindings: scene_delivery::FileRef,
    pub episode_bindings: scene_delivery::FileRef,
    pub segments: Vec<Segment>,
    /// Optional compact delivery after the lossless conform has been verified.
    #[serde(default)]
    pub compact_delivery: Option<CompactDelivery>,
    #[serde(default)]
    pub audio_frame_conform: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactDelivery {
    pub crf: u8,
    pub audio_bitrate_kbps: u16,
    pub retain_lossless_master: bool,
    #[serde(default)]
    pub intermediate_video_encoding: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EncodedDelivery {
    pub filename: String,
    pub sha256: String,
    pub bytes: u64,
    pub video_codec: String,
    pub audio_codec: String,
    pub crf: u8,
    pub audio_bitrate_kbps: u16,
    pub decoded_frames: u64,
    pub full_decode_verified: bool,
    pub lossless_master_retained: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub kind: SegmentKind,
    /// Scene ID or episode presentation role.
    pub id: String,
    #[serde(default)]
    pub role: Option<String>,
    pub master: scene_delivery::FileRef,
    pub source_receipt: scene_delivery::FileRef,
    /// Selected scene-delivery job in the authoring tree. Supplying this and
    /// `delivery_receipt` rechecks the upstream render before conform.
    #[serde(default)]
    pub delivery_job: Option<scene_delivery::FileRef>,
    /// Receipt beside the hydrated scene-delivery output files.
    #[serde(default)]
    pub delivery_receipt: Option<scene_delivery::FileRef>,
    /// Exact source-adoption manifest for an inherited presentation master.
    #[serde(default)]
    pub adoption_manifest: Option<scene_delivery::FileRef>,
    /// V2 source-text selection evidence for an editable episode display.
    #[serde(default)]
    pub source_text_evidence: Option<scene_delivery::FileRef>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MasterTemplate {
    schema: String,
    template_id: String,
    #[serde(default)]
    ordered_roles: Vec<String>,
    #[serde(default)]
    optional_roles: Vec<String>,
    #[serde(default)]
    ordered_units: Vec<MasterUnit>,
    #[serde(default)]
    source_template_sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct MasterUnit {
    kind: SegmentKind,
    id: String,
    /// Stable source IDs in display order; empty for scenes or non-text presentation.
    #[serde(default)]
    source_ids: Vec<String>,
    /// Exact selected source-text hashes for each language, in `source_ids` order.
    #[serde(default)]
    source_text_sha256_by_language: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceTextEvidence {
    schema: String,
    episode_id: String,
    role: String,
    language: String,
    picture_binding: String,
    picture_sha256: String,
    #[serde(default)]
    rendered_source_master_sha256: Option<String>,
    caption_template_id: String,
    units: Vec<SourceTextUnit>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceTextUnit {
    source_id: String,
    text: String,
    sha256: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SegmentKind {
    Scene,
    EpisodePresentation,
}

#[derive(Debug, Serialize)]
pub struct SegmentReceipt {
    pub kind: String,
    pub id: String,
    pub selected_master_sha256: String,
    pub selected_master_bytes: u64,
    pub source_receipt_sha256: String,
    pub upstream_delivery_verified: bool,
    pub upstream_presentation_verified: bool,
    pub input_sample_rate: u32,
    pub output_sample_rate: u32,
    pub input_frames: u64,
    pub input_samples: u64,
    pub frames: u64,
    pub samples: u64,
    pub audio_padding_samples: u64,
    pub start_frame: u64,
    pub start_sample: u64,
    pub decoded_picture_sha256: String,
    pub decoded_audio_sha256: String,
}

#[derive(Debug, Serialize)]
pub struct Receipt {
    pub schema: String,
    pub episode_id: String,
    pub language: String,
    pub manifest_sha256: String,
    pub episode_presentation_fingerprint_sha256: String,
    pub ffmpeg_version: String,
    pub output_sample_rate: u32,
    pub total_frames: u64,
    pub total_samples: u64,
    pub master_sha256: String,
    pub master_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoded_delivery: Option<EncodedDelivery>,
    pub decoded_master_matches_ordered_segments: bool,
    pub timestamps_verified: bool,
    pub boundary_findings: Vec<episode_delivery::BoundaryFinding>,
    pub boundary_review_state: String,
    pub external_layer_review_state: String,
    pub upstream_delivery_recheck_state: String,
    pub upstream_presentation_recheck_state: String,
    pub creative_review_state: String,
    pub segments: Vec<SegmentReceipt>,
    pub publication: String,
}

#[derive(Clone)]
pub(crate) struct MediaFacts {
    pub width: u64,
    pub height: u64,
    pub fps: String,
    pub sample_rate: u32,
    pub video_codec: String,
    pub reordered_video_frames: u64,
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn fps_parts(fps: &str) -> Result<(u64, u64)> {
    let (numerator, denominator) = fps.split_once('/').context("frame rate must be rational")?;
    let (numerator, denominator): (u64, u64) = (numerator.parse()?, denominator.parse()?);
    if numerator == 0 || denominator == 0 {
        bail!("invalid frame rate");
    }
    Ok((numerator, denominator))
}

pub(crate) fn file_sha(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

pub(crate) fn command(name: &str) -> Command {
    #[allow(unused_mut)] // Windows adds CREATE_NO_WINDOW to this command.
    let mut cmd = Command::new(name);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW for desktop callers.
    }
    cmd
}

pub(crate) fn probe(path: &Path) -> Result<MediaFacts> {
    let output = command("ffprobe")
        .args(["-v", "error", "-show_streams", "-of", "json"])
        .arg(path)
        .output()?;
    if !output.status.success() {
        bail!("FFprobe rejected {}", path.display());
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let streams = value["streams"]
        .as_array()
        .context("FFprobe lacks streams")?;
    let video = streams
        .iter()
        .filter(|s| s["codec_type"] == "video")
        .collect::<Vec<_>>();
    let audio = streams
        .iter()
        .filter(|s| s["codec_type"] == "audio")
        .collect::<Vec<_>>();
    if video.len() != 1 || audio.len() != 1 {
        bail!("episode segment must have one picture and one audio stream");
    }
    let (v, a) = (video[0], audio[0]);
    if !matches!(v["codec_name"].as_str(), Some("ffv1" | "h264"))
        || v["pix_fmt"] != "yuv444p"
        || a["codec_name"] != "pcm_s24le"
        || a["channels"] != 2
    {
        bail!("episode segment must be FFV1 or H264/yuv444p and stereo PCM24");
    }
    let width = v["width"].as_u64().context("missing picture width")?;
    let height = v["height"].as_u64().context("missing picture height")?;
    let fps = v["r_frame_rate"]
        .as_str()
        .context("missing picture frame rate")?
        .to_owned();
    let sample_rate = a["sample_rate"]
        .as_str()
        .context("missing audio sample rate")?
        .parse()?;
    if width == 0 || height == 0 || fps == "0/0" || sample_rate == 0 {
        bail!("invalid episode media geometry or clock");
    }
    Ok(MediaFacts {
        reordered_video_frames: v["has_b_frames"].as_u64().unwrap_or(0),
        video_codec: v["codec_name"]
            .as_str()
            .context("video codec missing")?
            .to_owned(),
        width,
        height,
        fps,
        sample_rate,
    })
}

pub(crate) fn decoded_digest(path: &Path, video: bool) -> Result<(String, u64)> {
    decoded_digest_with_padding(path, video, 0)
}

fn decoded_digest_with_padding(
    path: &Path,
    video: bool,
    padding_samples: u64,
) -> Result<(String, u64)> {
    let mut cmd = command("ffmpeg");
    cmd.args(["-v", "error", "-nostdin", "-i"]).arg(path);
    if video {
        cmd.args([
            "-map",
            "0:v:0",
            "-fps_mode",
            "passthrough",
            "-pix_fmt",
            "yuv444p",
            "-f",
            "rawvideo",
        ]);
    } else {
        cmd.args(["-map", "0:a:0", "-c:a", "pcm_s24le", "-f", "s24le"]);
    }
    let errors = tempfile::tempfile()?;
    let mut child = cmd.arg("-").stdout(Stdio::piped()).stderr(errors).spawn()?;
    let mut pipe = child.stdout.take().context("decoder pipe")?;
    let mut digest = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let n = pipe.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
        count += n as u64;
    }
    if !child.wait()?.success() {
        bail!("segment decode failed: {}", path.display());
    }
    let mut padding_bytes = padding_samples
        .checked_mul(6)
        .context("audio padding overflow")?;
    let zeroes = [0u8; 65536];
    while padding_bytes > 0 {
        let n = padding_bytes.min(zeroes.len() as u64) as usize;
        digest.update(&zeroes[..n]);
        count = count
            .checked_add(n as u64)
            .context("audio byte count overflow")?;
        padding_bytes -= n as u64;
    }
    Ok((
        digest
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        count,
    ))
}

fn selected_receipt(segment: &Segment, language: &str, receipt: &serde_json::Value) -> Result<()> {
    let expected = match segment.kind {
        SegmentKind::Scene => "reel.scene-build-receipt.v1",
        SegmentKind::EpisodePresentation => "reel.presentation-master-receipt.v1",
    };
    if receipt["schema"] != expected
        || receipt["language"] != language
        || receipt["master_sha256"] != segment.master.sha256
        || receipt["master_bytes"] != segment.master.bytes
    {
        bail!("segment source receipt does not bind selected master/language");
    }
    let id_key = if segment.kind == SegmentKind::Scene {
        "scene_id"
    } else {
        "role"
    };
    if receipt[id_key] != segment.id {
        bail!("segment source receipt identity mismatch");
    }
    if segment.kind == SegmentKind::Scene {
        if receipt["presentation_role"].as_str() != segment.role.as_deref() {
            bail!("scene presentation role differs from source receipt");
        }
    } else if segment.role.is_some() {
        bail!("episode presentation role is the segment ID");
    }
    Ok(())
}

fn valid_sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn validate_v2_order(template: &MasterTemplate, manifest: &Manifest) -> Result<()> {
    if !template.ordered_roles.is_empty()
        || !template.optional_roles.is_empty()
        || template.ordered_units.is_empty()
        || template.ordered_units.len() != manifest.segments.len()
    {
        bail!("invalid ordered-unit episode master grammar");
    }
    let mut scene_ids = BTreeSet::new();
    let mut presentation_ids = BTreeSet::new();
    let mut display_source_ids = BTreeSet::new();
    for (unit, segment) in template.ordered_units.iter().zip(&manifest.segments) {
        if unit.id.is_empty() || unit.kind != segment.kind || unit.id != segment.id {
            bail!("episode segment differs from selected ordered unit");
        }
        match unit.kind {
            SegmentKind::Scene => {
                if !scene_ids.insert(unit.id.as_str())
                    || !unit.source_ids.is_empty()
                    || !unit.source_text_sha256_by_language.is_empty()
                    || segment.source_text_evidence.is_some()
                {
                    bail!("invalid ordered scene unit");
                }
            }
            SegmentKind::EpisodePresentation => {
                if !presentation_ids.insert(unit.id.as_str()) {
                    bail!("duplicate ordered presentation unit");
                }
                if unit.source_ids.is_empty() {
                    if !unit.source_text_sha256_by_language.is_empty()
                        || segment.source_text_evidence.is_some()
                    {
                        bail!("non-text presentation has source text evidence");
                    }
                } else {
                    let unique = unit.source_ids.iter().collect::<BTreeSet<_>>();
                    if unique.len() != unit.source_ids.len()
                        || unit.source_ids.iter().any(String::is_empty)
                        || unit
                            .source_ids
                            .iter()
                            .any(|source_id| !display_source_ids.insert(source_id.as_str()))
                        || unit.source_text_sha256_by_language.len() != 2
                        || !unit.source_text_sha256_by_language.contains_key("es")
                        || !unit.source_text_sha256_by_language.contains_key("en")
                        || segment.source_text_evidence.is_none()
                    {
                        bail!("editable display lacks exact bilingual source identity");
                    }
                    for hashes in unit.source_text_sha256_by_language.values() {
                        if hashes.len() != unit.source_ids.len()
                            || hashes.iter().any(|hash| !valid_sha(hash))
                        {
                            bail!("editable display source hashes are invalid");
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn verify_source_text_evidence(
    unit: &MasterUnit,
    segment: &Segment,
    manifest: &Manifest,
    receipt: &serde_json::Value,
    invocation: &reel_assembly::scene_authoring::PresentationUse,
    bindings: [&ScopedBindings; 2],
    input_root: &Path,
) -> Result<()> {
    if unit.source_ids.is_empty() {
        return Ok(());
    }
    let reference = segment
        .source_text_evidence
        .as_ref()
        .context("editable display lacks source-text evidence")?;
    let path = scene_delivery::checked_file(input_root, reference)?;
    let evidence: SourceTextEvidence = serde_json::from_slice(&fs::read(path)?)?;
    if let Some(adoption_reference) = &segment.adoption_manifest {
        let adoption_path = scene_delivery::checked_file(input_root, adoption_reference)?;
        let adoption: presentation_adopt::Manifest =
            serde_json::from_slice(&fs::read(adoption_path)?)?;
        verify_adopted_display_source(
            evidence.rendered_source_master_sha256.as_deref(),
            receipt,
            &adoption,
            reference,
        )?;
    }
    if evidence.schema != "reel.presentation-source-text.v1"
        || evidence.episode_id != manifest.episode_id
        || evidence.role != segment.id
        || evidence.language != manifest.language
        || evidence.caption_template_id != invocation.template_id
        || receipt["selection_evidence_sha256"] != reference.sha256
        || invocation.content["source_text_evidence_sha256_by_language"][&manifest.language]
            != reference.sha256
        || invocation.content["picture_binding"] != evidence.picture_binding
        || evidence.units.len() != unit.source_ids.len()
    {
        bail!("editable display evidence differs from selected source or template");
    }
    let expected_hashes = unit
        .source_text_sha256_by_language
        .get(&manifest.language)
        .context("selected language lacks display source hashes")?;
    for (index, source) in evidence.units.iter().enumerate() {
        if source.source_id != unit.source_ids[index]
            || source.sha256 != expected_hashes[index]
            || sha(source.text.as_bytes()) != source.sha256
        {
            bail!("editable display text differs from selected source hash");
        }
    }
    let candidates = bindings
        .into_iter()
        .filter_map(|scope| scope.assets.get(&evidence.picture_binding))
        .collect::<Vec<_>>();
    if candidates.len() != 1 {
        bail!("editable display picture binding must resolve exactly once");
    }
    let picture = candidates[0];
    if picture.sha256 != evidence.picture_sha256
        || !valid_sha(&picture.sha256)
        || picture.bytes == 0
        || picture.cache_uri != format!("cache://sha256/{}", picture.sha256)
        || !matches!(
            picture.selection_state.as_str(),
            "selected-private-production" | "principal-approved" | "release-cleared"
        )
    {
        bail!("editable display picture differs from selected clean-picture binding");
    }
    Ok(())
}

fn verify_adopted_display_source(
    source_hash: Option<&str>,
    receipt: &serde_json::Value,
    adoption: &presentation_adopt::Manifest,
    reference: &scene_delivery::FileRef,
) -> Result<()> {
    if source_hash.is_none_or(|hash| !valid_sha(hash))
        || receipt["source_sha256"].as_str() != source_hash
        || source_hash != Some(adoption.source.sha256.as_str())
        || adoption.evidence_hash_pointer.as_deref() != Some("/rendered_source_master_sha256")
        || adoption.selection_evidence.as_ref().is_none_or(|selected| {
            selected.sha256 != reference.sha256 || selected.bytes != reference.bytes
        })
    {
        bail!("editable display evidence does not bind its adopted rendered source master");
    }
    Ok(())
}

#[cfg(test)]
mod adopted_display_source_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_internally_consistent_but_unbound_source_proofs() {
        let source_hash = "a".repeat(64);
        let file = json!({"path":"evidence.json","sha256":"b".repeat(64),"bytes":42});
        let reference: scene_delivery::FileRef = serde_json::from_value(file.clone()).unwrap();
        let mut source = file.clone();
        source["sha256"] = source_hash.clone().into();
        let mut adoption: presentation_adopt::Manifest = serde_json::from_value(json!({
            "schema":"reel.presentation-adopt.v1","role":"photo","language":"es",
            "season_id":"season","episode_id":"episode","template_id":"caption",
            "catalog":file,"template_definition":file,"season_bindings":file,
            "episode_bindings":file,"source_binding":"rendered.photo","source":source,
            "selection_evidence":file,"evidence_hash_pointer":"/rendered_source_master_sha256"
        }))
        .unwrap();
        let receipt = json!({"source_sha256":source_hash});
        assert!(
            verify_adopted_display_source(Some(&source_hash), &receipt, &adoption, &reference)
                .is_ok()
        );
        let wrong_hash = "c".repeat(64);
        for bad in [None, Some("invalid"), Some(wrong_hash.as_str())] {
            assert!(verify_adopted_display_source(bad, &receipt, &adoption, &reference).is_err());
        }
        adoption.evidence_hash_pointer = Some("/picture_sha256".into());
        assert!(
            verify_adopted_display_source(Some(&source_hash), &receipt, &adoption, &reference)
                .is_err()
        );
        adoption.evidence_hash_pointer = Some("/rendered_source_master_sha256".into());
        adoption.selection_evidence.as_mut().unwrap().sha256 = "d".repeat(64);
        assert!(
            verify_adopted_display_source(Some(&source_hash), &receipt, &adoption, &reference)
                .is_err()
        );
        adoption.selection_evidence.as_mut().unwrap().sha256 = reference.sha256.clone();
        adoption.selection_evidence.as_mut().unwrap().bytes += 1;
        assert!(
            verify_adopted_display_source(Some(&source_hash), &receipt, &adoption, &reference)
                .is_err()
        );
        adoption.selection_evidence = None;
        assert!(
            verify_adopted_display_source(Some(&source_hash), &receipt, &adoption, &reference)
                .is_err()
        );
    }
}

pub fn build(
    manifest_path: &Path,
    input_root: &Path,
    asset_root: &Path,
    output: &Path,
) -> Result<Receipt> {
    if output.exists() {
        bail!("episode output must be a new directory");
    }
    let manifest_bytes = fs::read(manifest_path)?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    if manifest.compact_delivery.as_ref().is_some_and(|profile| {
        profile.crf > 51
            || !(32..=512).contains(&profile.audio_bitrate_kbps)
            || profile
                .intermediate_video_encoding
                .as_deref()
                .is_some_and(|value| value != "h264-lossless")
    }) {
        bail!("invalid compact episode encoding profile");
    }
    if manifest
        .audio_frame_conform
        .as_deref()
        .is_some_and(|value| value != "pad-silence-to-picture-boundaries")
    {
        bail!("unsupported episode audio frame conform policy");
    }
    if manifest.schema != SCHEMA
        || manifest.episode_id.is_empty()
        || !matches!(manifest.language.as_str(), "es" | "en")
        || manifest.output_sample_rate == 0
        || manifest.segments.is_empty()
    {
        bail!("invalid episode conform manifest");
    }
    let read_input = |item: &scene_delivery::FileRef| -> Result<Vec<u8>> {
        Ok(fs::read(scene_delivery::checked_file(input_root, item)?)?)
    };
    let catalog: TemplateCatalog = serde_json::from_slice(&read_input(&manifest.catalog)?)?;
    let master_bytes = read_input(&manifest.master_template_definition)?;
    let master_template: MasterTemplate = serde_json::from_slice(&master_bytes)?;
    match (
        &master_template.source_template_sha256,
        &manifest.source_master_template,
    ) {
        (Some(expected), Some(source)) => {
            let bytes = read_input(source)?;
            if sha(&bytes) != *expected {
                bail!("master template source differs from selected original");
            }
            let original: serde_json::Value = serde_json::from_slice(&bytes)?;
            if master_template.schema == "reel.episode-master-template.v2" {
                let original_units: Vec<MasterUnit> =
                    serde_json::from_value(original["ordered_units"].clone())
                        .context("source master lacks ordered units")?;
                if original_units != master_template.ordered_units {
                    bail!("generic master unit order differs from source master");
                }
            } else {
                let original_roles = original["ordered_roles"]
                    .as_array()
                    .context("source master lacks ordered roles")?
                    .iter()
                    .map(|value| value.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
                    .context("source master has non-text ordered role")?;
                if original_roles != master_template.ordered_roles {
                    bail!("generic master role order differs from source master");
                }
            }
        }
        (None, None) => {}
        _ => bail!("master template source binding is incomplete"),
    }
    let episode: Episode = serde_json::from_slice(&read_input(&manifest.episode)?)?;
    let season: ScopedBindings = serde_json::from_slice(&read_input(&manifest.season_bindings)?)?;
    let episode_bindings: ScopedBindings =
        serde_json::from_slice(&read_input(&manifest.episode_bindings)?)?;
    if episode.episode_id != manifest.episode_id {
        bail!("episode conform identity mismatch");
    }
    let presentation =
        resolve_episode_presentation(&catalog, &episode, &season, &episode_bindings)?;
    let selected_master_template = catalog
        .templates
        .iter()
        .find(|item| {
            item.template_id == episode.master_template_id && item.kind == "episode-master"
        })
        .context("episode master template is not selected")?;
    let ordered_units_v2 = master_template.schema == "reel.episode-master-template.v2";
    if !ordered_units_v2 && master_template.schema != "reel.episode-master-template.v1"
        || master_template.template_id != episode.master_template_id
        || selected_master_template.definition_sha256 != sha(&master_bytes)
        || master_template
            .source_template_sha256
            .as_ref()
            .is_some_and(|value| !valid_sha(value))
    {
        bail!("episode master order definition differs from selected template");
    }
    let role_set = master_template
        .ordered_roles
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let optional_set = master_template
        .optional_roles
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if ordered_units_v2 {
        validate_v2_order(&master_template, &manifest)?;
    } else if role_set.len() != master_template.ordered_roles.len()
        || !role_set.contains("chapter-scenes")
        || !optional_set.is_subset(&role_set)
        || optional_set.contains("chapter-scenes")
        || !master_template.ordered_units.is_empty()
    {
        bail!("invalid episode master role grammar");
    }
    let expected_scenes = &episode.scene_ids;
    let mut seen_scenes = Vec::new();
    let mut seen_roles = BTreeSet::new();
    let mut observed_roles: Vec<String> = Vec::new();
    let mut sources = Vec::new();
    let mut upstream_verified = Vec::new();
    let mut presentation_verified = Vec::new();
    let mut presentation_counts = Vec::new();
    for (segment_index, segment) in manifest.segments.iter().enumerate() {
        if !ordered_units_v2 && segment.source_text_evidence.is_some() {
            bail!("v1 episode segment cannot carry v2 source-text evidence");
        }
        let master = scene_delivery::checked_file(asset_root, &segment.master)?;
        let receipt_path = scene_delivery::checked_file(asset_root, &segment.source_receipt)?;
        let receipt: serde_json::Value = serde_json::from_slice(&fs::read(&receipt_path)?)?;
        selected_receipt(segment, &manifest.language, &receipt)?;
        let presentation_rechecked = match (segment.kind, &segment.adoption_manifest) {
            (SegmentKind::Scene, None) => false,
            (SegmentKind::Scene, Some(_)) => {
                bail!("scene cannot supply a presentation adoption manifest")
            }
            (SegmentKind::EpisodePresentation, Some(adoption)) => {
                if receipt["technical_validation_state"] != "decoded-source-equivalent" {
                    bail!("adoption manifest requires decoded-source-equivalent receipt");
                }
                let adoption_path = scene_delivery::checked_file(input_root, adoption)?;
                let stage_parent = output
                    .parent()
                    .filter(|path| !path.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                presentation_adopt::check(
                    &adoption_path,
                    input_root,
                    asset_root,
                    &master,
                    &receipt_path,
                    stage_parent,
                )?;
                true
            }
            (SegmentKind::EpisodePresentation, None) => {
                if receipt["technical_validation_state"] == "decoded-source-equivalent" {
                    bail!("adopted presentation needs its source manifest recheck");
                }
                false
            }
        };
        let rechecked = match (
            segment.kind,
            &segment.delivery_job,
            &segment.delivery_receipt,
        ) {
            (SegmentKind::Scene, Some(job_ref), Some(delivery_ref)) => {
                let job_path = scene_delivery::checked_file(input_root, job_ref)?;
                let delivery_path = scene_delivery::checked_file(asset_root, delivery_ref)?;
                if delivery_path
                    .file_name()
                    .is_none_or(|name| name != "receipt.json")
                {
                    bail!("scene delivery receipt must be named receipt.json");
                }
                let delivery_root = delivery_path
                    .parent()
                    .context("scene delivery receipt has no parent")?;
                if master != delivery_root.join("master.mkv") {
                    bail!("selected scene master is outside rechecked delivery directory");
                }
                verify_native_delivery_proof(&receipt, job_ref, delivery_ref)?;
                let checked = if receipt["schema"] == "reel.scene-build-receipt.v1" {
                    // The exact native proof above binds a successful full check.
                    // Rehash all current source/output bytes and compare the plan;
                    // final conform still decodes and compares every ordered frame.
                    scene_delivery::recheck_native_receipt(&job_path, asset_root, delivery_root)?
                } else {
                    scene_delivery::check(&job_path, asset_root, delivery_root)?
                };
                let checked_master = checked
                    .outputs
                    .get("master.mkv")
                    .context("rechecked scene delivery lacks master")?;
                if checked_master.sha256 != segment.master.sha256
                    || checked_master.bytes != segment.master.bytes
                {
                    bail!("rechecked scene delivery differs from selected master");
                }
                true
            }
            (SegmentKind::Scene, None, None) => false,
            (SegmentKind::Scene, _, _) => {
                bail!("scene delivery recheck requires both job and receipt")
            }
            (SegmentKind::EpisodePresentation, None, None) => false,
            (SegmentKind::EpisodePresentation, _, _) => {
                bail!("episode presentation cannot supply a scene delivery recheck")
            }
        };
        match segment.kind {
            SegmentKind::Scene => {
                presentation_counts.push(None);
                seen_scenes.push(segment.id.clone());
                let role = segment.role.as_deref().unwrap_or("chapter-scenes");
                if role != "chapter-scenes" || observed_roles.last().is_none_or(|last| last != role)
                {
                    observed_roles.push(role.to_owned());
                }
            }
            SegmentKind::EpisodePresentation => {
                let input_frames = receipt["frames"]
                    .as_u64()
                    .context("presentation receipt lacks decoded frame count")?;
                let input_samples = receipt["samples"]
                    .as_u64()
                    .context("presentation receipt lacks decoded sample count")?;
                if input_frames == 0
                    || input_samples == 0
                    || receipt["episode_id"] != manifest.episode_id
                    || receipt["timestamps_verified"] != true
                    || !matches!(
                        receipt["technical_validation_state"].as_str(),
                        Some("decoded-source-equivalent" | "rendered-and-checked")
                    )
                {
                    bail!("presentation source receipt lacks selected technical validation");
                }
                presentation_counts.push(Some((input_frames, input_samples)));
                observed_roles.push(segment.id.clone());
                if !seen_roles.insert(segment.id.clone()) {
                    bail!("duplicate presentation role");
                }
                let invocation = episode
                    .presentation
                    .iter()
                    .find(|p| p.role == segment.id)
                    .context("unknown episode presentation role")?;
                let key = invocation
                    .asset_binding
                    .as_deref()
                    .context("presentation requires selected episode binding")?;
                let asset = presentation
                    .selected_inputs
                    .get(key)
                    .context("missing selected presentation master")?;
                if asset.sha256 != segment.master.sha256 || asset.bytes != segment.master.bytes {
                    bail!("presentation master differs from selected scoped asset");
                }
                if receipt["template_id"] != invocation.template_id {
                    bail!("presentation template mismatch");
                }
                let template_sha = presentation
                    .template_definitions
                    .get(&invocation.template_id)
                    .context("selected presentation template definition missing")?;
                if receipt["template_definition_sha256"] != *template_sha {
                    bail!("presentation receipt differs from selected template definition");
                }
                if ordered_units_v2 {
                    verify_source_text_evidence(
                        &master_template.ordered_units[segment_index],
                        segment,
                        &manifest,
                        &receipt,
                        invocation,
                        [&episode_bindings, &season],
                        input_root,
                    )?;
                }
            }
        }
        sources.push(master);
        upstream_verified.push(rechecked);
        presentation_verified.push(presentation_rechecked);
    }
    if seen_scenes != *expected_scenes {
        bail!("episode scene order or coverage mismatch");
    }
    if seen_roles
        != episode
            .presentation
            .iter()
            .map(|p| p.role.clone())
            .collect()
    {
        bail!("episode presentation coverage mismatch");
    }
    if !ordered_units_v2 {
        let expected_roles = master_template
            .ordered_roles
            .iter()
            .filter(|role| !optional_set.contains(*role) || observed_roles.contains(*role))
            .cloned()
            .collect::<Vec<_>>();
        if observed_roles != expected_roles {
            bail!("episode segment order differs from selected master template");
        }
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".episode-conform-")
        .tempdir_in(parent)?;
    let work = stage.path();
    let temp = tempfile::tempdir_in(work)?;
    let mut normalized = Vec::new();
    let mut records = Vec::new();
    let mut common: Option<MediaFacts> = None;
    let mut frame_cursor = 0u64;
    let mut sample_cursor = 0u64;
    for (index, (segment, source)) in manifest.segments.iter().zip(&sources).enumerate() {
        let facts = probe(source)?;
        if segment.kind == SegmentKind::Scene
            && facts.video_codec == "h264"
            && (segment.delivery_job.is_none() || segment.delivery_receipt.is_none())
        {
            bail!("H264 scene requires its verified lossless delivery job and receipt");
        }
        if let Some(first) = &common {
            if (facts.width, facts.height, &facts.fps) != (first.width, first.height, &first.fps) {
                bail!("episode segments differ in geometry or frame rate");
            }
        } else {
            common = Some(facts.clone());
        }
        let target_codec = if manifest
            .compact_delivery
            .as_ref()
            .and_then(|profile| profile.intermediate_video_encoding.as_deref())
            == Some("h264-lossless")
        {
            "h264"
        } else {
            "ffv1"
        };
        let needs_video_normalization = facts.video_codec != target_codec
            || (target_codec == "h264" && facts.reordered_video_frames != 0);
        let needs_normalization =
            facts.sample_rate != manifest.output_sample_rate || needs_video_normalization;
        let mut selected = if !needs_normalization {
            source.clone()
        } else {
            let path = temp.path().join(format!("normalized-{index:03}.mkv"));
            let mut encoder = command("ffmpeg");
            encoder
                .args(["-v", "error", "-nostdin", "-i"])
                .arg(source)
                .args(["-map", "0:v:0", "-map", "0:a:0"]);
            if !needs_video_normalization {
                encoder.args(["-c:v", "copy"]);
            } else if target_codec == "h264" {
                encoder.args([
                    "-c:v", "libx264", "-crf", "0", "-preset", "veryfast", "-bf", "0", "-pix_fmt",
                    "yuv444p",
                ]);
            } else {
                encoder.args(["-c:v", "ffv1", "-pix_fmt", "yuv444p"]);
            }
            if facts.sample_rate == manifest.output_sample_rate {
                encoder.args(["-c:a", "copy"]);
            } else {
                encoder
                    .args(["-af"])
                    .arg(format!(
                        "aresample={}:async=0:first_pts=0",
                        manifest.output_sample_rate
                    ))
                    .args(["-c:a", "pcm_s24le", "-ac", "2", "-ar"])
                    .arg(manifest.output_sample_rate.to_string());
            }
            let status = encoder.arg(&path).status()?;
            if !status.success() {
                bail!("episode audio normalization failed");
            }
            path
        };
        let selected_facts = probe(&selected)?;
        if selected_facts.sample_rate != manifest.output_sample_rate {
            bail!("normalization sample rate mismatch");
        }
        let (picture_hash, picture_bytes) = decoded_digest(&selected, true)?;
        let (mut audio_hash, audio_bytes) = decoded_digest(&selected, false)?;
        let frame_bytes = selected_facts.width * selected_facts.height * 3;
        if picture_bytes == 0
            || picture_bytes % frame_bytes != 0
            || audio_bytes == 0
            || audio_bytes % 6 != 0
        {
            bail!("decoded segment does not contain complete frames/stereo samples");
        }
        let frames = picture_bytes / frame_bytes;
        let mut samples = audio_bytes / 6;
        let (input_frames, input_samples) = if !needs_normalization {
            (frames, samples)
        } else {
            let (source_picture_hash, source_picture_bytes) = decoded_digest(source, true)?;
            let (source_audio_hash, source_audio_bytes) = decoded_digest(source, false)?;
            if source_picture_hash != picture_hash
                || source_picture_bytes != picture_bytes
                || source_audio_bytes % 6 != 0
                || (facts.sample_rate == manifest.output_sample_rate
                    && source_audio_hash != audio_hash)
            {
                bail!("audio normalization changed picture or input sample packing");
            }
            let input_samples = source_audio_bytes / 6;
            let expected = ((input_samples as u128 * manifest.output_sample_rate as u128)
                + (facts.sample_rate as u128 / 2))
                / facts.sample_rate as u128;
            if (samples as i128 - expected as i128).abs() > 2 {
                bail!("audio normalization inserted or removed more than two samples");
            }
            (source_picture_bytes / frame_bytes, input_samples)
        };
        if let Some((expected_frames, expected_samples)) = presentation_counts[index] {
            if input_frames != expected_frames || input_samples != expected_samples {
                bail!("presentation decoded media differs from its source receipt");
            }
        }
        let mut audio_padding_samples = 0;
        if manifest.audio_frame_conform.as_deref() == Some("pad-silence-to-picture-boundaries") {
            let (num, den) = fps_parts(&selected_facts.fps)?;
            let boundary = |frame: u64| -> Result<u64> {
                let value =
                    (u128::from(frame) * u128::from(manifest.output_sample_rate) * u128::from(den)
                        + u128::from(num) / 2)
                        / u128::from(num);
                u64::try_from(value).context("frame boundary sample overflow")
            };
            let target = boundary(
                frame_cursor
                    .checked_add(frames)
                    .context("frame clock overflow")?,
            )?
            .checked_sub(boundary(frame_cursor)?)
            .context("frame boundary order")?;
            audio_padding_samples = target
                .checked_sub(samples)
                .context("native audio exceeds selected picture boundary; trimming is forbidden")?;
            if u128::from(audio_padding_samples) * u128::from(num)
                > u128::from(manifest.output_sample_rate) * u128::from(den) + u128::from(num)
            {
                bail!("frame conform padding exceeds one frame");
            }
            if audio_padding_samples != 0 {
                let expected =
                    decoded_digest_with_padding(&selected, false, audio_padding_samples)?;
                let padded = temp.path().join(format!("padded-{index:03}.mkv"));
                let status = command("ffmpeg")
                    .args(["-v", "error", "-nostdin", "-i"])
                    .arg(&selected)
                    .args(["-map", "0:v:0", "-map", "0:a:0", "-c:v", "copy", "-af"])
                    .arg(format!(
                        "apad=pad_len={audio_padding_samples},asetpts=N/SR/TB"
                    ))
                    .args(["-c:a", "pcm_s24le"])
                    .arg(&padded)
                    .status()?;
                if !status.success() {
                    bail!("frame audio padding failed");
                }
                let actual = decoded_digest(&padded, false)?;
                if actual != expected {
                    bail!("frame conform changed native audio prefix or nonzero padded tail");
                }
                selected = padded;
                audio_hash = actual.0;
                samples = actual.1 / 6;
            }
        }
        records.push(SegmentReceipt {
            kind: if segment.kind == SegmentKind::Scene {
                "scene"
            } else {
                "episode-presentation"
            }
            .into(),
            id: segment.id.clone(),
            selected_master_sha256: segment.master.sha256.clone(),
            selected_master_bytes: segment.master.bytes,
            source_receipt_sha256: segment.source_receipt.sha256.clone(),
            upstream_delivery_verified: upstream_verified[index],
            upstream_presentation_verified: presentation_verified[index],
            input_sample_rate: facts.sample_rate,
            output_sample_rate: selected_facts.sample_rate,
            input_frames,
            input_samples,
            frames,
            samples,
            audio_padding_samples,
            start_frame: frame_cursor,
            start_sample: sample_cursor,
            decoded_picture_sha256: picture_hash,
            decoded_audio_sha256: audio_hash,
        });
        frame_cursor += frames;
        sample_cursor += samples;
        let (fps_numerator, fps_denominator) = fps_parts(&selected_facts.fps)?;
        let drift = (u128::from(frame_cursor)
            * u128::from(manifest.output_sample_rate)
            * u128::from(fps_denominator))
        .abs_diff(u128::from(sample_cursor) * u128::from(fps_numerator));
        if drift > u128::from(manifest.output_sample_rate) * u128::from(fps_denominator) {
            bail!("cumulative episode audio/picture drift exceeds one frame");
        }
        normalized.push(selected);
    }
    let list_path = temp.path().join("segments.ffconcat");
    let mut list = fs::File::create(&list_path)?;
    writeln!(list, "ffconcat version 1.0")?;
    for path in &normalized {
        let escaped = path
            .to_string_lossy()
            .replace('\\', "/")
            .replace('\'', "'\\''");
        writeln!(list, "file '{escaped}'")?;
    }
    drop(list);
    let master = work.join("master.mkv");
    let mut concat = command("ffmpeg");
    concat
        .args([
            "-v",
            "error",
            "-nostdin",
            "-reinit_filter",
            "0",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
        ])
        .arg(&list_path)
        .args(["-map", "0:v:0", "-map", "0:a:0"]);
    if manifest.audio_frame_conform.is_some() {
        // Preserve the episode-wide setpts/asetpts counters when an adopted
        // display changes SAR or color metadata. Filter reinitialization would
        // restart N at zero; the muxer then clamps the remaining video PTS to
        // the preceding frame. Ordered decoded-content checks still reject any
        // incompatible source property that changes the selected pixels/audio.
        let facts = common.as_ref().context("no common frame clock")?;
        let (num, den) = fps_parts(&facts.fps)?;
        concat
            .args(["-vf"])
            .arg(format!("setpts=N*{den}/({num}*TB)"))
            .args([
                "-af",
                "asetpts=N/SR/TB",
                "-fps_mode",
                "passthrough",
                "-c:a",
                "pcm_s24le",
            ]);
        if manifest
            .compact_delivery
            .as_ref()
            .and_then(|p| p.intermediate_video_encoding.as_deref())
            == Some("h264-lossless")
        {
            concat.args([
                "-c:v", "libx264", "-crf", "0", "-preset", "veryfast", "-bf", "0", "-pix_fmt",
                "yuv444p",
            ]);
        } else {
            concat.args(["-c:v", "ffv1", "-pix_fmt", "yuv444p"]);
        }
    } else {
        concat.args(["-c", "copy"]);
    }
    let status = concat.arg(&master).status()?;
    if !status.success() {
        bail!("episode lossless concat failed");
    }
    let first = common.context("no episode media")?;
    let video_lengths = records
        .iter()
        .map(|r| r.frames * first.width * first.height * 3)
        .collect::<Vec<_>>();
    let audio_lengths = records.iter().map(|r| r.samples * 6).collect::<Vec<_>>();
    let actual_video = episode_delivery::decode_segments(&master, true, &video_lengths)?;
    let actual_audio = episode_delivery::decode_segments(&master, false, &audio_lengths)?;
    for (index, record) in records.iter().enumerate() {
        if actual_video[index] != record.decoded_picture_sha256
            || actual_audio[index] != record.decoded_audio_sha256
        {
            bail!("episode decoded segment differs from selected source {index}");
        }
    }
    let (fps_numerator, fps_denominator) = fps_parts(&first.fps)?;
    episode_delivery::verify_timestamps(
        &master,
        manifest.output_sample_rate,
        fps_numerator,
        fps_denominator,
    )?;
    let mut boundary_findings = Vec::new();
    for index in 0..normalized.len().saturating_sub(1) {
        let left = &records[index];
        let right = &records[index + 1];
        let left_audio = episode_delivery::edge_audio(
            &normalized[index],
            left.samples,
            manifest.output_sample_rate,
            true,
        )?;
        let right_audio = episode_delivery::edge_audio(
            &normalized[index + 1],
            right.samples,
            manifest.output_sample_rate,
            false,
        )?;
        let jump = (0..2)
            .map(|channel| {
                f64::from((left_audio[left_audio.len() - 2 + channel] - right_audio[channel]).abs())
            })
            .fold(0.0, f64::max);
        if jump > 0.1 {
            boundary_findings.push(episode_delivery::BoundaryFinding {
                left_scene: left.id.clone(),
                right_scene: right.id.clone(),
                code: "audio-step".into(),
                measured: jump,
                threshold: 0.1,
                disposition: "needs-review".into(),
            });
        }
        let black = episode_delivery::black_edge(
            &normalized[index],
            left.frames - 1,
            first.width as u32,
            first.height as u32,
        )? || episode_delivery::black_edge(
            &normalized[index + 1],
            0,
            first.width as u32,
            first.height as u32,
        )?;
        if black {
            boundary_findings.push(episode_delivery::BoundaryFinding {
                left_scene: left.id.clone(),
                right_scene: right.id.clone(),
                code: "black-at-cut".into(),
                measured: 1.0,
                threshold: 0.0,
                disposition: "needs-review".into(),
            });
        }
    }
    let version = command("ffmpeg").arg("-version").output()?;
    let ffmpeg_version = String::from_utf8_lossy(&version.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .to_owned();
    let encoded_delivery = if let Some(profile) = &manifest.compact_delivery {
        if profile.crf > 51 || !(32..=512).contains(&profile.audio_bitrate_kbps) {
            bail!("invalid compact episode encoding profile");
        }
        let target = work.join("episode.mp4");
        let status = command("ffmpeg")
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(&master)
            .args(["-map", "0:v:0", "-map", "0:a:0", "-c:v", "libx264", "-crf"])
            .arg(profile.crf.to_string())
            .args(["-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a"])
            .arg(format!("{}k", profile.audio_bitrate_kbps))
            .args(["-movflags", "+faststart"])
            .arg(&target)
            .status()?;
        if !status.success() {
            bail!("compact episode encoding failed");
        }
        let decoded = command("ffmpeg")
            .args(["-v", "error", "-xerror", "-nostdin", "-i"])
            .arg(&target)
            .args(["-map", "0:v:0", "-map", "0:a:0", "-f", "null", "-"])
            .status()?;
        if !decoded.success() {
            bail!("compact episode full decode failed");
        }
        let probe = command("ffprobe")
            .args([
                "-v",
                "error",
                "-count_frames",
                "-show_streams",
                "-of",
                "json",
            ])
            .arg(&target)
            .output()?;
        if !probe.status.success() {
            bail!("compact episode probe failed");
        }
        let facts: serde_json::Value = serde_json::from_slice(&probe.stdout)?;
        let streams = facts["streams"]
            .as_array()
            .context("compact episode streams missing")?;
        let video = streams
            .iter()
            .find(|s| s["codec_type"] == "video")
            .context("compact video missing")?;
        let audio = streams
            .iter()
            .find(|s| s["codec_type"] == "audio")
            .context("compact audio missing")?;
        let (encoded_num, encoded_den) = fps_parts(
            video["r_frame_rate"]
                .as_str()
                .context("compact frame rate missing")?,
        )?;
        let (source_num, source_den) = fps_parts(&first.fps)?;
        let audio_duration: f64 = audio["duration"]
            .as_str()
            .context("compact audio duration missing")?
            .parse()?;
        let expected_audio_duration = sample_cursor as f64 / f64::from(manifest.output_sample_rate);
        // MP4 edit lists trim encoder delay; allow one AAC frame plus one source
        // video frame for container quantization, never cumulative scene padding.
        let duration_tolerance =
            1024.0 / f64::from(manifest.output_sample_rate) + source_den as f64 / source_num as f64;
        if video["codec_name"] != "h264"
            || video["width"].as_u64() != Some(first.width)
            || video["height"].as_u64() != Some(first.height)
            || video["nb_read_frames"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                != Some(frame_cursor)
            || audio["codec_name"] != "aac"
            || audio["sample_rate"]
                .as_str()
                .and_then(|s| s.parse::<u32>().ok())
                != Some(manifest.output_sample_rate)
            || audio["channels"].as_u64() != Some(2)
            || u128::from(encoded_num) * u128::from(source_den)
                != u128::from(source_num) * u128::from(encoded_den)
            || !audio_duration.is_finite()
            || (audio_duration - expected_audio_duration).abs() > duration_tolerance
        {
            bail!("compact episode media differs from selected delivery policy");
        }
        Some(EncodedDelivery {
            filename: "episode.mp4".into(),
            sha256: file_sha(&target)?,
            bytes: fs::metadata(&target)?.len(),
            video_codec: "h264".into(),
            audio_codec: "aac".into(),
            crf: profile.crf,
            audio_bitrate_kbps: profile.audio_bitrate_kbps,
            decoded_frames: frame_cursor,
            full_decode_verified: true,
            lossless_master_retained: profile.retain_lossless_master,
        })
    } else {
        None
    };
    let receipt = Receipt {
        schema: "reel.episode-conform-receipt.v1".into(),
        episode_id: manifest.episode_id,
        language: manifest.language,
        manifest_sha256: sha(&manifest_bytes),
        episode_presentation_fingerprint_sha256: presentation.fingerprint_sha256,
        ffmpeg_version,
        output_sample_rate: manifest.output_sample_rate,
        total_frames: frame_cursor,
        total_samples: sample_cursor,
        master_sha256: file_sha(&master)?,
        master_bytes: fs::metadata(&master)?.len(),
        encoded_delivery,
        decoded_master_matches_ordered_segments: true,
        timestamps_verified: true,
        boundary_findings,
        boundary_review_state: "open; inspect and disposition adjacent cuts".into(),
        external_layer_review_state: "open; source delivery layers need independent evidence"
            .into(),
        upstream_delivery_recheck_state: if manifest
            .segments
            .iter()
            .enumerate()
            .all(|(i, segment)| segment.kind != SegmentKind::Scene || upstream_verified[i])
        {
            "verified-for-all-scene-segments".into()
        } else {
            "open; some scene segments lack selected delivery job and receipt recheck".into()
        },
        upstream_presentation_recheck_state: if manifest.segments.iter().enumerate().all(
            |(i, segment)| {
                segment.kind != SegmentKind::EpisodePresentation || presentation_verified[i]
            },
        ) {
            "verified-for-all-presentation-segments".into()
        } else {
            "open; some presentation segments lack an independent upstream recheck".into()
        },
        creative_review_state: "open; no principal approval inferred".into(),
        segments: records,
        publication: "not-authorized".into(),
    };
    fs::write(
        work.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    if manifest
        .compact_delivery
        .as_ref()
        .is_some_and(|profile| !profile.retain_lossless_master)
    {
        // This file was created by this bounded conform, and all source masters
        // remain untouched. Its exact hash stays in the provenance receipt.
        fs::remove_file(&master)?;
    }
    drop(temp);
    fs::rename(work, output)?;
    Ok(receipt)
}
