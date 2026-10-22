//! Hash-bound, language-local lossless episode conform from selected scene and
//! presentation masters. Source selection belongs to the authoring bindings.

use crate::{
    episode_delivery, imported_scene_proof, presentation_adopt, scene_delivery, semantic_delivery,
};
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
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

mod checkpoint;

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
    fn imported_roots_reject_outside_symlink_targets() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        assert_eq!(scoped_import_root(root.path(), None).unwrap(), root.path());
        let inside = root.path().join("inside");
        fs::create_dir(&inside).unwrap();
        assert_eq!(
            scoped_import_root(root.path(), Some(Path::new("inside"))).unwrap(),
            fs::canonicalize(&inside).unwrap()
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
            assert!(scoped_import_root(root.path(), Some(Path::new("escape"))).is_err());
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // Junction creation requires no Developer Mode or symlink privilege.
            let status = Command::new("cmd")
                .creation_flags(0x08000000)
                .args(["/c", "mklink", "/J"])
                .arg(root.path().join("escape"))
                .arg(outside.path())
                .output()
                .unwrap();
            assert!(
                status.status.success(),
                "{}",
                String::from_utf8_lossy(&status.stderr)
            );
            assert!(scoped_import_root(root.path(), Some(Path::new("escape"))).is_err());
        }
    }

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
    /// Explicit delivery geometry. Without this, sources must have identical sizes.
    #[serde(default)]
    pub output_geometry: Option<OutputGeometry>,
    /// Explicit repeat-only delivery clock; source frames are never discarded.
    #[serde(default)]
    pub output_frame_rate: Option<OutputFrameRate>,
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
    /// Reuse exact successful adopted-presentation verification, after current
    /// dependencies and tool binaries have been rehashed. Defaults to full checks.
    #[serde(default)]
    pub reuse_verified_presentations: bool,
    /// Optional read-only assembled candidate. Every source and every ordered
    /// decoded frame/sample is still checked before it can be published.
    #[serde(default)]
    pub preassembled_master: Option<scene_delivery::FileRef>,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutputGeometry {
    pub width: u32,
    pub height: u32,
    /// Only aspect-preserving Lanczos scaling is supported; no crop or stretch.
    pub policy: String,
    /// Explicit author declaration for legacy streams with no usable SAR tag.
    #[serde(default, skip_serializing_if = "is_false")]
    pub assume_square_for_unspecified_sar: bool,
    /// Resolve an unspecified matrix only when independent decoded RGB agrees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assume_color_space_for_unspecified: Option<String>,
    /// Resolve an unspecified range only when original decoded RGB is unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assume_color_range_for_unspecified: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutputFrameRate {
    pub numerator: u32,
    pub denominator: u32,
    pub policy: String,
}

impl OutputFrameRate {
    fn clock(&self) -> Result<(u64, u64)> {
        let (mut a, mut b) = (u64::from(self.numerator), u64::from(self.denominator));
        if a == 0
            || b == 0
            || a > 1_000_000
            || b > 1_000_000
            || a > 120 * b
            || self.policy != "repeat-only"
        {
            bail!("invalid repeat-only output frame rate");
        }
        let (n, d) = (a, b);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        Ok((n / a, d / a))
    }

    fn filter(&self, source: &str) -> Result<Option<String>> {
        let (n, d) = self.clock()?;
        let (sn, sd) = fps_parts(source)?;
        if u128::from(n) * u128::from(sd) < u128::from(sn) * u128::from(d) {
            bail!("repeat-only output frame rate would discard source frames");
        }
        Ok(
            (u128::from(n) * u128::from(sd) != u128::from(sn) * u128::from(d))
                .then(|| format!("fps=fps={n}/{d}:start_time=0:round=up:eof_action=pass")),
        )
    }

    fn frames(&self, input_frames: u64, source: &str) -> Result<u64> {
        let (n, d) = self.clock()?;
        let (sn, sd) = fps_parts(source)?;
        let numerator = u128::from(input_frames) * u128::from(n) * u128::from(sd);
        let denominator = u128::from(d) * u128::from(sn);
        u64::try_from(numerator.div_ceil(denominator)).context("repeat-only frame count overflow")
    }
}

#[derive(Debug, Serialize)]
pub struct FrameRateVerification {
    pub input_fps: String,
    pub output_fps: String,
    pub input_frames: u64,
    /// Excludes separately recorded silent tail holds.
    pub output_frames: u64,
    pub repeated_frames: u64,
    pub transformed_source_matches_selected: bool,
}

#[cfg(test)]
mod repeat_only_tests {
    use super::*;

    #[test]
    fn clock_is_explicit_reduced_bounded_and_cannot_drop_frames() {
        let rate = OutputFrameRate {
            numerator: 60,
            denominator: 2,
            policy: "repeat-only".into(),
        };
        assert_eq!(rate.clock().unwrap(), (30, 1));
        assert!(rate.filter("30/1").unwrap().is_none());
        assert!(
            rate.filter("25/1")
                .unwrap()
                .unwrap()
                .contains("round=up:eof_action=pass")
        );
        assert_eq!(rate.frames(11, "25/1").unwrap(), 14);
        assert_eq!(rate.frames(3015, "30/1").unwrap(), 3015);
        assert!(rate.filter("50/1").is_err());
        for (numerator, denominator, policy) in [
            (0, 1, "repeat-only"),
            (30, 0, "repeat-only"),
            (121, 1, "repeat-only"),
            (30, 1, "nearest"),
        ] {
            assert!(
                OutputFrameRate {
                    numerator,
                    denominator,
                    policy: policy.into()
                }
                .clock()
                .is_err()
            );
        }
    }

    #[test]
    fn moving_source_retains_every_frame_and_audio_through_fractional_end() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.mkv");
        let selected = temp.path().join("selected.mkv");
        let status = command("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=32x32:r=25:d=0.44",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=437:sample_rate=48000:duration=0.44",
                "-ac",
                "2",
                "-shortest",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuv444p",
                "-c:a",
                "pcm_s24le",
            ])
            .arg(&source)
            .status()
            .unwrap();
        assert!(status.success());
        let rate = OutputFrameRate {
            numerator: 30,
            denominator: 1,
            policy: "repeat-only".into(),
        };
        let status = command("ffmpeg")
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(&source)
            .args(["-map", "0:v:0", "-map", "0:a:0", "-vf"])
            .arg(rate.filter("25/1").unwrap().unwrap())
            .args(["-c:v", "ffv1", "-pix_fmt", "yuv444p", "-c:a", "copy"])
            .arg(&selected)
            .status()
            .unwrap();
        assert!(status.success());
        let raw = |path: &Path| {
            let output = command("ffmpeg")
                .args(["-v", "error", "-nostdin", "-i"])
                .arg(path)
                .args(["-map", "0:v:0", "-pix_fmt", "rgb24", "-f", "rawvideo", "-"])
                .output()
                .unwrap();
            assert!(output.status.success());
            output.stdout
        };
        let before = raw(&source);
        let after = raw(&selected);
        let bytes_per_frame = 32 * 32 * 3;
        assert_eq!(before.len() / bytes_per_frame, 11);
        assert_eq!(after.len() / bytes_per_frame, 14);
        let mut original = before.chunks_exact(bytes_per_frame).peekable();
        for frame in after.chunks_exact(bytes_per_frame) {
            if original.peek().is_some_and(|next| *next == frame) {
                original.next();
            }
        }
        assert!(
            original.next().is_none(),
            "an original moving frame was lost or reordered"
        );
        assert_eq!(probe(&selected).unwrap().fps, "30/1");
        assert_eq!(
            decoded_digest(&source, false).unwrap(),
            decoded_digest(&selected, false).unwrap()
        );
    }
}

fn scoped_import_root(input_root: &Path, scope: Option<&Path>) -> Result<PathBuf> {
    let Some(scope) = scope else {
        return Ok(input_root.to_path_buf());
    };
    if scope.as_os_str().is_empty()
        || scope.is_absolute()
        || scope
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        bail!("imported source root must be a nonempty relative directory without traversal");
    }
    let root = fs::canonicalize(input_root)?;
    let selected = fs::canonicalize(input_root.join(scope))?;
    if !selected.is_dir() || !selected.starts_with(&root) {
        bail!("imported source root escapes its authoring directory");
    }
    Ok(selected)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeometryVerification {
    pub input_width: u64,
    pub input_height: u64,
    pub input_sample_aspect_ratio: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub sample_aspect_ratio_assumed_square: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assumed_color_space: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assumed_color_range: Option<String>,
    pub color_properties: BTreeMap<String, String>,
    pub output_width: u64,
    pub output_height: u64,
    pub scaling_applied: bool,
    pub decoded_source_picture_sha256: String,
    pub transformed_source_matches_selected: bool,
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
    /// Prior complete scene check, permitting master-only output staging.
    /// Restricted to the semantic-scoped direct native delivery route.
    #[serde(default)]
    pub master_proof: Option<scene_delivery::FileRef>,
    /// Explicit original checker for a previously issued master proof. Without
    /// this FileRef, the proof must identify the current executable as before.
    #[serde(default)]
    pub master_proof_validator: Option<scene_delivery::FileRef>,
    /// Selected semantic scene scope for a direct scene-delivery receipt.
    /// This route always performs the full upstream delivery check.
    #[serde(default)]
    pub semantic_delivery: Option<scene_delivery::FileRef>,
    /// Exact source-adoption manifest for an inherited presentation master.
    #[serde(default)]
    pub adoption_manifest: Option<scene_delivery::FileRef>,
    /// V2 source-text selection evidence for an editable episode display.
    #[serde(default)]
    pub source_text_evidence: Option<scene_delivery::FileRef>,
    /// Exact original-graph scene verifier inputs for an imported native scene.
    /// This cannot substitute for a scene-authoring build receipt implicitly.
    #[serde(default)]
    pub imported_source_manifest: Option<scene_delivery::FileRef>,
    /// Relative authoring directory of an independently rebuildable imported
    /// scene package. Its job and import refs are local to this directory.
    /// Hash-bound source evidence remains unchanged when packages are hydrated.
    #[serde(default)]
    pub imported_source_root: Option<PathBuf>,
    /// Explicit silent hold after the complete source: repeat its final picture
    /// and append zero audio, without trimming any selected source content.
    #[serde(default)]
    pub tail_hold_frames: u64,
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    #[serde(skip_serializing_if = "u64_is_zero")]
    pub tail_hold_frames: u64,
    #[serde(skip_serializing_if = "u64_is_zero")]
    pub tail_hold_samples: u64,
    pub start_frame: u64,
    pub start_sample: u64,
    pub decoded_picture_sha256: String,
    pub decoded_audio_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometry_verification: Option<GeometryVerification>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_rate_verification: Option<FrameRateVerification>,
}

fn u64_is_zero(value: &u64) -> bool {
    *value == 0
}

fn hold_samples(
    frames: u64,
    start_frame: u64,
    sample_rate: u32,
    fps_num: u64,
    fps_den: u64,
) -> Result<u64> {
    if fps_num == 0
        || fps_den == 0
        || sample_rate == 0
        || u128::from(frames) * u128::from(fps_den) > u128::from(fps_num) * 60
    {
        bail!("silent picture hold requires a valid clock and at most sixty seconds");
    }
    let boundary = |frame: u64| -> Result<u64> {
        let value = u128::from(frame)
            .checked_mul(u128::from(sample_rate))
            .and_then(|v| v.checked_mul(u128::from(fps_den)))
            .and_then(|v| v.checked_add(u128::from(fps_num) / 2))
            .context("hold sample clock overflow")?
            / u128::from(fps_num);
        u64::try_from(value).context("hold sample clock overflow")
    };
    boundary(
        start_frame
            .checked_add(frames)
            .context("hold frame clock overflow")?,
    )?
    .checked_sub(boundary(start_frame)?)
    .context("hold sample clock order")
}

/// Verify the entire prefix and the added tail independently of the encoder.
type MediaDigest = (String, u64);

fn append_silent_picture_hold(
    source: &Path,
    output: &Path,
    frames: u64,
    samples: u64,
    video_codec: &str,
) -> Result<(MediaDigest, MediaDigest)> {
    if frames == 0 || samples == 0 || !matches!(video_codec, "h264" | "ffv1") {
        bail!("invalid silent picture hold");
    }
    let filter = format!("tpad=stop_mode=clone:stop={frames}");
    let expected_picture = decoded_digest_with_filter(source, true, 0, Some(&filter))?;
    let expected_audio = decoded_digest_with_padding(source, false, samples)?;
    let mut encoder = command("ffmpeg");
    encoder
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(source)
        .args(["-map", "0:v:0", "-map", "0:a:0", "-vf", &filter, "-af"])
        .arg(format!("apad=pad_len={samples},asetpts=N/SR/TB"));
    if video_codec == "h264" {
        encoder.args([
            "-c:v", "libx264", "-crf", "0", "-preset", "veryfast", "-bf", "0",
        ]);
    } else {
        encoder.args(["-c:v", "ffv1"]);
    }
    if !encoder
        .args([
            "-pix_fmt",
            "yuv444p",
            "-fps_mode",
            "passthrough",
            "-c:a",
            "pcm_s24le",
        ])
        .arg(output)
        .status()?
        .success()
    {
        bail!("silent picture hold encoding failed");
    }
    let picture = decoded_digest(output, true)?;
    let audio = decoded_digest(output, false)?;
    if picture != expected_picture || audio != expected_audio {
        bail!("silent picture hold changed source prefix, last frame or zero-audio tail");
    }
    Ok((picture, audio))
}

#[cfg(test)]
mod silent_hold_tests {
    use super::*;

    #[test]
    fn hold_clock_is_bounded_checked_and_uses_global_fractional_frame_boundaries() {
        assert_eq!(hold_samples(48, 24, 48000, 24, 1).unwrap(), 96000);
        assert_eq!(hold_samples(60, 17, 44100, 30000, 1001).unwrap(), 88288);
        assert!(hold_samples(1441, 0, 48000, 24, 1).is_err());
        assert!(hold_samples(48, u64::MAX, 48000, 24, 1).is_err());
        assert!(hold_samples(1, u64::MAX - 1, u32::MAX, u64::MAX, u64::MAX).is_err());
        assert!(hold_samples(1, 0, 48000, 0, 1).is_err());
    }

    #[test]
    fn lossless_hold_keeps_the_moving_picture_and_tone_prefix_then_adds_only_last_picture_and_zero_audio()
     {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.mkv");
        assert!(
            command("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=s=32x32:r=24:d=1",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=937:sample_rate=48000:duration=1",
                    "-c:v",
                    "ffv1",
                    "-pix_fmt",
                    "yuv444p",
                    "-c:a",
                    "pcm_s24le",
                    "-ac",
                    "2"
                ])
                .arg(&source)
                .status()
                .unwrap()
                .success()
        );
        let original_picture = decoded_digest(&source, true).unwrap();
        let original_audio = decoded_digest(&source, false).unwrap();
        for codec in ["ffv1", "h264"] {
            let output = temp.path().join(format!("held-{codec}.mkv"));
            let (picture, audio) =
                append_silent_picture_hold(&source, &output, 48, 96000, codec).unwrap();
            assert_eq!(picture.1, 72 * 32 * 32 * 3);
            assert_eq!(audio.1, 144000 * 6);
            assert_eq!(
                decoded_digest_with_filter(&output, true, 0, Some("trim=end_frame=24")).unwrap(),
                original_picture
            );
            let pcm = command("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(&output)
                .args(["-map", "0:a:0", "-c:a", "pcm_s24le", "-f", "s24le", "-"])
                .output()
                .unwrap();
            assert!(pcm.status.success());
            let prefix = Sha256::digest(&pcm.stdout[..48000 * 6])
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(prefix, original_audio.0);
            assert!(pcm.stdout[48000 * 6..].iter().all(|v| *v == 0));
            let source_bytes = fs::read(&source).unwrap();
            assert!(!source_bytes.is_empty());
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Receipt {
    pub schema: String,
    pub episode_id: String,
    pub language: String,
    pub manifest_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preparation_sha256: Option<String>,
    pub episode_presentation_fingerprint_sha256: String,
    pub ffmpeg_version: String,
    pub output_sample_rate: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_geometry: Option<OutputGeometry>,
    pub total_frames: u64,
    pub total_samples: u64,
    pub master_sha256: String,
    pub master_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preassembled_master: Option<scene_delivery::FileRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoded_delivery: Option<EncodedDelivery>,
    pub decoded_master_matches_ordered_segments: bool,
    pub timestamps_verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation_verification_cache: Option<PresentationVerificationCache>,
    pub boundary_findings: Vec<episode_delivery::BoundaryFinding>,
    pub boundary_review_state: String,
    pub external_layer_review_state: String,
    pub upstream_delivery_recheck_state: String,
    pub upstream_presentation_recheck_state: String,
    pub creative_review_state: String,
    pub segments: Vec<SegmentReceipt>,
    pub publication: String,
}

#[derive(Debug, Serialize)]
pub struct PresentationVerificationCache {
    pub reused_verified_checks: usize,
    pub fresh_full_checks: usize,
    pub policy: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MediaFacts {
    pub requires_lossless_normalization: bool,
    pub width: u64,
    pub height: u64,
    pub sample_aspect_ratio: String,
    pub color_properties: BTreeMap<String, String>,
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

fn media_tool_in_path(name: &str, search: &std::ffi::OsStr) -> Result<PathBuf> {
    for directory in std::env::split_paths(search) {
        let candidate = directory.join(if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.into()
        });
        if candidate.is_file() {
            return fs::canonicalize(candidate).context("cannot resolve media tool");
        }
    }
    bail!("cannot identify {name} executable for verification reuse")
}

pub(crate) fn media_tool_path(name: &str) -> Result<PathBuf> {
    media_tool_in_path(
        name,
        &std::env::var_os("PATH").context("tool PATH is missing")?,
    )
}

fn command_with_search(name: &str, search: Option<&std::ffi::OsStr>) -> Command {
    // Resolve to the same absolute PATH binary used by verification keys.
    // Windows' implicit executable-directory search can otherwise run a
    // different tool from the one whose bytes were validated for cache reuse.
    let program = search
        .and_then(|search| media_tool_in_path(name, search).ok())
        .unwrap_or_else(|| PathBuf::from(name));
    #[allow(unused_mut)] // Windows adds CREATE_NO_WINDOW to this command.
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW for desktop callers.
    }
    cmd
}

pub(crate) fn command(name: &str) -> Command {
    command_with_search(name, std::env::var_os("PATH").as_deref())
}

#[cfg(test)]
mod media_tool_resolution_tests {
    use super::*;

    #[test]
    fn cache_identity_and_invocation_select_the_same_absolute_tool() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let filename = if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        fs::write(first.join(filename), b"first tool").unwrap();
        fs::write(second.join(filename), b"different same-name tool").unwrap();
        let search = std::env::join_paths([&second, &first]).unwrap();
        let selected = media_tool_in_path("ffmpeg", &search).unwrap();
        assert_eq!(selected, fs::canonicalize(second.join(filename)).unwrap());
        assert_eq!(
            command_with_search("ffmpeg", Some(&search)).get_program(),
            selected.as_os_str()
        );
    }
}

pub(crate) fn probe(path: &Path) -> Result<MediaFacts> {
    probe_with_source_formats(path, false)
}

/// Only for a complete selected source whose identity is independently verified.
pub(crate) fn probe_selected_source(path: &Path) -> Result<MediaFacts> {
    probe_with_source_formats(path, true)
}

fn probe_with_source_formats(path: &Path, allow_encoded_source: bool) -> Result<MediaFacts> {
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
    let native_lossless_format = matches!(v["codec_name"].as_str(), Some("ffv1" | "h264"))
        && v["pix_fmt"] == "yuv444p"
        && a["codec_name"] == "pcm_s24le"
        && a["channels"] == 2;
    let supported_selected_format = allow_encoded_source
        && matches!(v["codec_name"].as_str(), Some("ffv1" | "h264"))
        && matches!(
            v["pix_fmt"].as_str(),
            Some("yuv420p" | "yuv422p" | "yuv444p")
        )
        && matches!(
            a["codec_name"].as_str(),
            Some("aac" | "pcm_s16le" | "pcm_s24le")
        )
        && a["channels"] == 2;
    if !native_lossless_format && !supported_selected_format {
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
        requires_lossless_normalization: !native_lossless_format,
        reordered_video_frames: v["has_b_frames"].as_u64().unwrap_or(0),
        video_codec: v["codec_name"]
            .as_str()
            .context("video codec missing")?
            .to_owned(),
        width,
        height,
        sample_aspect_ratio: v["sample_aspect_ratio"]
            .as_str()
            .unwrap_or("unknown")
            .to_owned(),
        color_properties: [
            "color_range",
            "color_space",
            "color_transfer",
            "color_primaries",
        ]
        .into_iter()
        .map(|key| {
            (
                key.to_owned(),
                v[key].as_str().unwrap_or("unknown").to_owned(),
            )
        })
        .collect(),
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
    decoded_digest_with_filter(path, video, padding_samples, None)
}

fn decoded_digest_with_filter(
    path: &Path,
    video: bool,
    padding_samples: u64,
    video_filter: Option<&str>,
) -> Result<(String, u64)> {
    decoded_digest_with_pixel_format(path, video, padding_samples, video_filter, "yuv444p")
}

fn verify_color_space_declaration(source: &Path, space: &str) -> Result<()> {
    let original = decoded_digest_with_pixel_format(source, true, 0, None, "rgb24")?;
    let declared = decoded_digest_with_pixel_format(
        source,
        true,
        0,
        Some(&format!("setparams=colorspace={space}")),
        "rgb24",
    )?;
    if declared != original {
        bail!("unspecified color-space declaration changes decoded source colors");
    }
    Ok(())
}

fn verify_color_range_declaration(source: &Path, space: Option<&str>) -> Result<()> {
    let original = decoded_digest_with_pixel_format(source, true, 0, None, "rgb24")?;
    let filter = match space {
        Some(space) => format!("setparams=range=limited:colorspace={space}"),
        None => "setparams=range=limited".into(),
    };
    let declared = decoded_digest_with_pixel_format(source, true, 0, Some(&filter), "rgb24")?;
    if declared != original {
        bail!("unspecified display range declaration changes decoded source colors");
    }
    Ok(())
}

#[cfg(test)]
mod color_declaration_tests {
    use super::*;

    #[test]
    fn declaration_rejects_equal_yuv_planes_with_different_displayed_rgb() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("known709.mkv");
        assert!(
            command("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=s=32x32:r=24:d=1",
                    "-colorspace",
                    "bt709",
                    "-c:v",
                    "ffv1",
                    "-pix_fmt",
                    "yuv444p"
                ])
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        let yuv = decoded_digest(&path, true).unwrap();
        let retagged_yuv =
            decoded_digest_with_filter(&path, true, 0, Some("setparams=colorspace=bt470bg"))
                .unwrap();
        assert_eq!(yuv, retagged_yuv);
        let error = verify_color_space_declaration(&path, "bt470bg").unwrap_err();
        assert!(error.to_string().contains("changes decoded source colors"));
        verify_color_space_declaration(&path, "bt709").unwrap();
    }
}

fn decoded_digest_with_pixel_format(
    path: &Path,
    video: bool,
    padding_samples: u64,
    video_filter: Option<&str>,
    pixel_format: &str,
) -> Result<(String, u64)> {
    if video && padding_samples == 0 {
        let result = crate::decoded_video_hash::hash(path, pixel_format, video_filter, None)?;
        return Ok((result.values[0].clone(), result.bytes));
    }
    let mut cmd = command("ffmpeg");
    cmd.args(["-v", "error", "-nostdin", "-i"]).arg(path);
    if video {
        if let Some(filter) = video_filter {
            cmd.args(["-vf", filter]);
        }
        cmd.args([
            "-map",
            "0:v:0",
            "-fps_mode",
            "passthrough",
            "-pix_fmt",
            pixel_format,
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
    if segment.master_proof_validator.is_some() && segment.master_proof.is_none() {
        bail!("a pinned master-proof validator requires a master proof");
    }
    if receipt["schema"] == "reel.scene-delivery-receipt.v0.1" {
        if segment.kind != SegmentKind::Scene
            || segment.semantic_delivery.is_none()
            || segment.delivery_job.is_none()
            || segment.role.is_some()
            || segment.imported_source_manifest.is_some()
            || segment.imported_source_root.is_some()
        {
            bail!("direct scene delivery requires a semantic scene scope and native recheck");
        }
        let delivery = segment
            .delivery_receipt
            .as_ref()
            .context("direct scene delivery requires its selected receipt")?;
        if delivery.sha256 != segment.source_receipt.sha256
            || delivery.bytes != segment.source_receipt.bytes
            || receipt["outputs"]["master.mkv"]["sha256"] != segment.master.sha256
            || receipt["outputs"]["master.mkv"]["bytes"] != segment.master.bytes
        {
            bail!("direct scene delivery does not bind selected receipt/master");
        }
        return Ok(());
    }
    if segment.master_proof.is_some() {
        bail!("scene master proof requires a semantic-scoped direct delivery receipt");
    }
    if segment.semantic_delivery.is_some() {
        bail!("semantic delivery scope requires a direct scene-delivery receipt");
    }
    if segment.imported_source_root.is_some()
        && (segment.kind != SegmentKind::Scene || segment.imported_source_manifest.is_none())
    {
        bail!("scoped imported source root requires an imported scene manifest");
    }
    let expected = match segment.kind {
        SegmentKind::Scene if segment.imported_source_manifest.is_some() => {
            imported_scene_proof::RECEIPT_SCHEMA
        }
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
        if let Some(imported) = &segment.imported_source_manifest {
            if receipt["imported_manifest_sha256"] != imported.sha256
                || segment.delivery_job.is_none()
                || segment.delivery_receipt.is_none()
            {
                bail!(
                    "imported source proof requires its exact manifest and native delivery recheck"
                );
            }
        }
        if receipt["presentation_role"].as_str() != segment.role.as_deref() {
            bail!("scene presentation role differs from source receipt");
        }
    } else if segment.role.is_some() || segment.imported_source_manifest.is_some() {
        bail!("episode presentation role is the segment ID");
    }
    Ok(())
}

fn verify_semantic_scene_scope(
    segment: &Segment,
    language: &str,
    receipt: &serde_json::Value,
    input_root: &Path,
) -> Result<()> {
    let reference = segment
        .semantic_delivery
        .as_ref()
        .context("direct scene delivery lacks semantic selection")?;
    let path = scene_delivery::checked_file(input_root, reference)?;
    let semantic: semantic_delivery::SemanticDelivery = serde_yaml::from_slice(&fs::read(&path)?)?;
    let job = segment
        .delivery_job
        .as_ref()
        .context("semantic scene lacks delivery job")?;
    if semantic.scene_id.as_deref() != Some(segment.id.as_str())
        || semantic.language.as_deref() != Some(language)
        || semantic.scene_delivery_job.sha256 != job.sha256
        || semantic.scene_delivery_job.bytes != job.bytes
    {
        bail!("semantic selection differs from scene/language/delivery job");
    }
    let semantic_job = scene_delivery::checked_file(
        path.parent().context("semantic manifest lacks parent")?,
        &semantic.scene_delivery_job,
    )?;
    if semantic_job.canonicalize()?
        != scene_delivery::checked_file(input_root, job)?.canonicalize()?
    {
        bail!("semantic selection differs from selected delivery job path");
    }
    let planned = semantic_delivery::plan(&path)?;
    if planned.scene_delivery_job_sha256 != job.sha256
        || receipt["plan"]["job_sha256"] != job.sha256
    {
        bail!("semantic selection differs from rendered job");
    }
    Ok(())
}

#[cfg(test)]
mod semantic_receipt_tests {
    use super::*;
    use serde_json::json;

    fn segment() -> Segment {
        serde_json::from_value(json!({
            "kind":"scene","id":"scene-a",
            "master":{"path":"out/master.mkv","sha256":"a".repeat(64),"bytes":7},
            "source_receipt":{"path":"out/receipt.json","sha256":"b".repeat(64),"bytes":8},
            "delivery_receipt":{"path":"out/receipt.json","sha256":"b".repeat(64),"bytes":8},
            "delivery_job":{"path":"job.json","sha256":"c".repeat(64),"bytes":9},
            "semantic_delivery":{"path":"semantic.json","sha256":"d".repeat(64),"bytes":10}
        }))
        .unwrap()
    }

    #[test]
    fn direct_receipt_requires_semantic_scope_native_recheck_and_exact_master() {
        let receipt = json!({"schema":"reel.scene-delivery-receipt.v0.1",
            "outputs":{"master.mkv":{"sha256":"a".repeat(64),"bytes":7}}});
        selected_receipt(&segment(), "es", &receipt).unwrap();
        for variant in 0..10 {
            let mut s = segment();
            match variant {
                0 => s.semantic_delivery = None,
                1 => s.delivery_job = None,
                2 => s.delivery_receipt = None,
                3 => s.master.sha256 = "e".repeat(64),
                4 => s.master.bytes += 1,
                5 => s.source_receipt.bytes += 1,
                6 => s.source_receipt.sha256 = "e".repeat(64),
                7 => s.role = Some("opening-poem".into()),
                8 => s.imported_source_manifest = s.delivery_job.clone(),
                _ => s.imported_source_root = Some("other-source".into()),
            }
            assert!(
                selected_receipt(&s, "es", &receipt).is_err(),
                "variant {variant}"
            );
        }
        let mut s = segment();
        s.kind = SegmentKind::EpisodePresentation;
        assert!(selected_receipt(&s, "es", &receipt).is_err());
    }

    #[test]
    fn semantic_scope_cannot_upgrade_an_authoring_receipt() {
        let receipt = json!({"schema":"reel.scene-build-receipt.v1",
            "scene_id":"scene-a","language":"es","master_sha256":"a".repeat(64),"master_bytes":7});
        assert!(selected_receipt(&segment(), "es", &receipt).is_err());
        let mut s = segment();
        s.semantic_delivery = None;
        selected_receipt(&s, "es", &receipt).unwrap();
        assert!(selected_receipt(&s, "en", &receipt).is_err());
    }

    #[test]
    fn semantic_scope_checks_identity_job_path_and_current_graph() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let h = "a".repeat(64);
        let job = json!({"schema":"reel.scene-delivery.v0.1","id":"scene-a",
            "contract":{"path":"contract.json","sha256":h,"bytes":1},
            "production_manifest_sha256":h,"width":64,"height":64,
            "max_composition_samples":480000,
            "pictures":[{"attachment_id":"picture","source":{"path":"cel.png","sha256":h,"bytes":1},"kind":"still","attention":"beat"}],
            "audio":[{"attachment_id":"narration","source":{"path":"voice.wav","sha256":h,"bytes":1},"bus":"D","cue_id":"cue"}],
            "buses":{"D":{"state":"intentional-silence","reason":"fixture"},
                     "M":{"state":"intentional-silence","reason":"fixture"},
                     "E":{"state":"intentional-silence","reason":"fixture"}}});
        let job_bytes = serde_json::to_vec(&job).unwrap();
        fs::write(root.join("job.json"), &job_bytes).unwrap();
        fs::write(root.join("other-job.json"), &job_bytes).unwrap();
        let job_ref = json!({"path":"job.json","sha256":sha(&job_bytes),"bytes":job_bytes.len()});
        let lock = json!({"logical_id":"lock","sha256":h});
        let mut semantic = json!({"schema":"reel.semantic-delivery.v1","id":"selected-scene",
            "target":"scene-a","scene_id":"scene-a","language":"es",
            "pointer":{"schema":"reel.selected-pointer.v1","logical_id":"current","selected_lock":lock},
            "graph":{"schema":"reel.semantic-assembly.v1","lock":lock,
                "slots":[
                    {"slot_id":"picture","beat_id":"beat","lane":"picture","disposition":"selected","selected_revision_id":"r1",
                     "revisions":[{"revision_id":"r1","asset":{"logical_id":"cel","cache_uri":format!("cache://sha256/{h}"),"sha256":h}}]},
                    {"slot_id":"narration","beat_id":"beat","lane":"narration","disposition":"selected","selected_revision_id":"r1",
                     "revisions":[{"revision_id":"r1","asset":{"logical_id":"voice","cache_uri":format!("cache://sha256/{h}"),"sha256":h}}]}],
                "events":[{"event_id":"event.es","scene_id":"scene-a","language":"es",
                    "narration":{"logical_id":"voice","sha256":h},"picture":{"logical_id":"cel","sha256":h},
                    "phrase_start_seconds":0.0,"phrase_end_seconds":1.0}],
                "nodes":[{"id":"scene-a","inputs":[],"slots":["picture","narration"],"events":["event.es"]}],"presentation_targets":[]},
            "scene_delivery_job":job_ref,"event_bindings":[{"event_id":"event.es","narration_attachment_id":"narration","picture_attachment_id":"picture"}]});
        let mut s = segment();
        s.delivery_job = Some(serde_json::from_value(job_ref).unwrap());
        let receipt = json!({"plan":{"job_sha256":sha(&job_bytes)}});
        let mut check = |value: &serde_json::Value, language: &str| {
            let bytes = serde_json::to_vec(value).unwrap();
            fs::write(root.join("semantic.json"), &bytes).unwrap();
            s.semantic_delivery = Some(scene_delivery::FileRef {
                path: "semantic.json".into(),
                sha256: sha(&bytes),
                bytes: bytes.len() as u64,
            });
            verify_semantic_scene_scope(&s, language, &receipt, root)
        };
        check(&semantic, "es").unwrap();
        // A selected episode graph can supply shared media while only this
        // scene's language-local events are rendered.
        semantic["target"] = json!("episode");
        let mut other_event = semantic["graph"]["events"][0].clone();
        other_event["event_id"] = json!("other.es");
        other_event["scene_id"] = json!("other-scene");
        semantic["graph"]["events"]
            .as_array_mut()
            .unwrap()
            .push(other_event);
        semantic["graph"]["nodes"].as_array_mut().unwrap().extend([
            json!({"id":"other-scene","inputs":[],"slots":["picture","narration"],"events":["other.es"]}),
            json!({"id":"episode","inputs":["scene-a","other-scene"],"slots":[],"events":[]}),
        ]);
        check(&semantic, "es").unwrap();
        assert!(check(&semantic, "en").is_err());
        semantic["scene_id"] = json!("other-scene");
        assert!(check(&semantic, "es").is_err());
        semantic["scene_id"] = json!("scene-a");
        semantic["scene_delivery_job"]["path"] = json!("other-job.json");
        assert!(check(&semantic, "es").is_err());
        semantic["scene_delivery_job"]["path"] = json!("job.json");
        semantic["graph"]["lock"]["sha256"] = json!("b".repeat(64));
        assert!(check(&semantic, "es").is_err());
    }
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
    build_internal(manifest_path, input_root, asset_root, output, None)?
        .context("missing final conform receipt")
}

pub fn prepare(
    manifest_path: &Path,
    input_root: &Path,
    asset_root: &Path,
    output: &Path,
) -> Result<()> {
    if input_root.canonicalize()? != asset_root.canonicalize()? {
        bail!("native preparation requires one closed input/asset workspace");
    }
    let scope = checkpoint::snapshot_job_scope(asset_root)?;
    build_internal(manifest_path, input_root, asset_root, output, Some(scope))?;
    Ok(())
}

pub fn finish(prepared: &Path, asset_root: &Path, output: &Path) -> Result<Receipt> {
    checkpoint::finish(prepared, asset_root, output)
}

fn build_internal(
    manifest_path: &Path,
    input_root: &Path,
    asset_root: &Path,
    output: &Path,
    preparation_scope: Option<Vec<scene_delivery::FileRef>>,
) -> Result<Option<Receipt>> {
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
    if let Some(rate) = &manifest.output_frame_rate {
        rate.clock()?;
        if manifest.audio_frame_conform.as_deref() != Some("pad-silence-to-picture-boundaries") {
            bail!("repeat-only frame conversion requires non-trimming audio frame conform");
        }
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
    let mut retained_sources = Vec::new();
    let mut presentation_counts = Vec::new();
    let mut presentation_cache_hits = 0;
    let mut presentation_cache_misses = 0;
    for (segment_index, segment) in manifest.segments.iter().enumerate() {
        if !ordered_units_v2 && segment.source_text_evidence.is_some() {
            bail!("v1 episode segment cannot carry v2 source-text evidence");
        }
        let master = scene_delivery::checked_file(asset_root, &segment.master)?;
        let receipt_path = scene_delivery::checked_file(asset_root, &segment.source_receipt)?;
        let receipt: serde_json::Value = serde_json::from_slice(&fs::read(&receipt_path)?)?;
        selected_receipt(segment, &manifest.language, &receipt)?;
        if segment.imported_source_manifest.is_some()
            && receipt["episode_id"] != manifest.episode_id
        {
            bail!("imported scene source proof belongs to another episode");
        }
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
                if manifest.reuse_verified_presentations {
                    let hit = presentation_adopt::check_cached(
                        &adoption_path,
                        input_root,
                        asset_root,
                        &master,
                        &receipt_path,
                        stage_parent,
                        &asset_root.join(".reel-verification-cache/presentation-adoption-v1"),
                    )?;
                    if hit {
                        presentation_cache_hits += 1;
                    } else {
                        presentation_cache_misses += 1;
                    }
                } else {
                    presentation_adopt::check(
                        &adoption_path,
                        input_root,
                        asset_root,
                        &master,
                        &receipt_path,
                        stage_parent,
                    )?;
                }
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
                let source_root =
                    scoped_import_root(input_root, segment.imported_source_root.as_deref())?;
                let job_path = scene_delivery::checked_file(&source_root, job_ref)?;
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
                if segment.semantic_delivery.is_some() {
                    verify_semantic_scene_scope(segment, &manifest.language, &receipt, input_root)?;
                } else {
                    verify_native_delivery_proof(&receipt, job_ref, delivery_ref)?;
                }
                let checked = if let Some(proof) = &segment.master_proof {
                    let proof_path = scene_delivery::checked_file(input_root, proof)?;
                    if let Some(validator) = &segment.master_proof_validator {
                        let validator = scene_delivery::checked_file(input_root, validator)?;
                        scene_delivery::recheck_master_proof_with_validator(
                            &job_path,
                            asset_root,
                            delivery_root,
                            &proof_path,
                            &validator,
                        )?
                    } else {
                        scene_delivery::recheck_master_proof(
                            &job_path,
                            asset_root,
                            delivery_root,
                            &proof_path,
                        )?
                    }
                } else if let Some(imported) = &segment.imported_source_manifest {
                    imported_scene_proof::recheck(
                        &source_root,
                        imported,
                        asset_root,
                        delivery_root,
                        &receipt,
                    )?
                } else if receipt["schema"] == "reel.scene-build-receipt.v1" {
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
        retained_sources.push(presentation_rechecked && receipt["output_mode"] == "retain-source");
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
        let facts = if retained_sources[index] {
            probe_selected_source(source)?
        } else {
            probe(source)?
        };
        let mut output_facts = facts.clone();
        let assumed_color_range = if let Some(range) = manifest
            .output_geometry
            .as_ref()
            .and_then(|geometry| geometry.assume_color_range_for_unspecified.as_ref())
        {
            if range != "tv" {
                bail!("unsupported unspecified color-range declaration");
            }
            (facts.color_properties["color_range"] == "unknown").then(|| range.clone())
        } else {
            None
        };
        if let Some(range) = &assumed_color_range {
            output_facts
                .color_properties
                .insert("color_range".into(), range.clone());
        }
        let frame_filter = if let Some(rate) = &manifest.output_frame_rate {
            let filter = rate.filter(&facts.fps)?;
            let (n, d) = rate.clock()?;
            output_facts.fps = format!("{n}/{d}");
            filter
        } else {
            None
        };
        let assumed_color_space = if let Some(space) = manifest
            .output_geometry
            .as_ref()
            .and_then(|geometry| geometry.assume_color_space_for_unspecified.as_ref())
        {
            if space != "bt470bg" {
                bail!("unsupported unspecified color-space declaration");
            }
            (facts.color_properties["color_space"] == "unknown").then(|| space.clone())
        } else {
            None
        };
        if let Some(space) = &assumed_color_space {
            output_facts
                .color_properties
                .insert("color_space".into(), space.clone());
        }
        let assumed_square = manifest.output_geometry.as_ref().is_some_and(|geometry| {
            geometry.assume_square_for_unspecified_sar
                && matches!(
                    facts.sample_aspect_ratio.as_str(),
                    "unknown" | "N/A" | "0:1"
                )
        });
        let mut scale_filter = if let Some(geometry) = &manifest.output_geometry {
            if facts.sample_aspect_ratio != "1:1" && !assumed_square {
                bail!("explicit output geometry requires square source pixels");
            }
            if geometry.width == 0
                || geometry.height == 0
                || geometry.width > 16384
                || geometry.height > 16384
                || geometry.policy != "preserve-aspect-lanczos"
            {
                bail!("invalid explicit output geometry or scaling policy");
            }
            if u128::from(facts.width) * u128::from(geometry.height)
                != u128::from(facts.height) * u128::from(geometry.width)
            {
                bail!("output geometry would change source aspect ratio");
            }
            output_facts.width = u64::from(geometry.width);
            output_facts.height = u64::from(geometry.height);
            let resized = (facts.width, facts.height) != (output_facts.width, output_facts.height);
            match (resized, assumed_square) {
                (true, true) => Some(format!(
                    "scale={}:{}:flags=lanczos,setsar=1",
                    geometry.width, geometry.height
                )),
                (true, false) => Some(format!(
                    "scale={}:{}:flags=lanczos",
                    geometry.width, geometry.height
                )),
                (false, true) => Some("setsar=1".to_owned()),
                (false, false) => None,
            }
        } else {
            None
        };
        if let Some(space) = &assumed_color_space {
            let declaration = format!("setparams=colorspace={space}");
            scale_filter = Some(match scale_filter {
                Some(filter) => format!("{filter},{declaration}"),
                None => declaration,
            });
        }
        if let Some(filter) = frame_filter {
            scale_filter = Some(match scale_filter {
                Some(existing) => format!("{existing},{filter}"),
                None => filter,
            });
        }
        if assumed_color_range.is_some() {
            scale_filter = Some(match scale_filter {
                Some(filter) => format!("{filter},setparams=range=limited"),
                None => "setparams=range=limited".into(),
            });
        }
        if segment.kind == SegmentKind::Scene
            && facts.video_codec == "h264"
            && (segment.delivery_job.is_none() || segment.delivery_receipt.is_none())
        {
            bail!("H264 scene requires its verified lossless delivery job and receipt");
        }
        if let Some(first) = &common {
            if manifest.output_geometry.is_some()
                && output_facts.color_properties != first.color_properties
            {
                bail!("explicit output geometry requires consistent source color properties");
            }
            if (output_facts.width, output_facts.height, &output_facts.fps)
                != (first.width, first.height, &first.fps)
            {
                bail!("episode segments differ in geometry or frame rate");
            }
        } else {
            common = Some(output_facts.clone());
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
        let needs_video_normalization = scale_filter.is_some()
            || facts.requires_lossless_normalization
            || facts.video_codec != target_codec
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
            if let Some(filter) = &scale_filter {
                encoder.args(["-vf", filter]);
            }
            if manifest.output_geometry.is_some() {
                for (key, flag) in [
                    ("color_range", "-color_range"),
                    ("color_space", "-colorspace"),
                    ("color_transfer", "-color_trc"),
                    ("color_primaries", "-color_primaries"),
                ] {
                    let value = &output_facts.color_properties[key];
                    if value != "unknown" {
                        encoder.args([flag, value]);
                    }
                }
            }
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
        if manifest.output_geometry.is_some()
            && (selected_facts.sample_aspect_ratio != "1:1"
                || selected_facts.color_properties != output_facts.color_properties)
        {
            bail!("geometry normalization changed sample aspect ratio or color properties");
        }
        if (
            selected_facts.width,
            selected_facts.height,
            &selected_facts.fps,
        ) != (output_facts.width, output_facts.height, &output_facts.fps)
        {
            bail!("normalization geometry or frame rate mismatch");
        }
        if selected_facts.sample_rate != manifest.output_sample_rate {
            bail!("normalization sample rate mismatch");
        }
        let (mut picture_hash, picture_bytes) = decoded_digest(&selected, true)?;
        let (mut audio_hash, audio_bytes) = decoded_digest(&selected, false)?;
        let frame_bytes = selected_facts.width * selected_facts.height * 3;
        if picture_bytes == 0
            || picture_bytes % frame_bytes != 0
            || audio_bytes == 0
            || audio_bytes % 6 != 0
        {
            bail!("decoded segment does not contain complete frames/stereo samples");
        }
        let mut frames = picture_bytes / frame_bytes;
        let mut samples = audio_bytes / 6;
        let mut geometry_verification = None;
        let (input_frames, input_samples) = if !needs_normalization {
            (frames, samples)
        } else {
            let (source_picture_hash, source_picture_bytes) = decoded_digest(source, true)?;
            if assumed_color_range.is_some() {
                verify_color_range_declaration(source, assumed_color_space.as_deref())?;
            } else if let Some(space) = &assumed_color_space {
                verify_color_space_declaration(source, space)?;
            }
            let (source_audio_hash, source_audio_bytes) = decoded_digest(source, false)?;
            let expected_picture = if let Some(filter) = &scale_filter {
                decoded_digest_with_filter(source, true, 0, Some(filter))?
            } else {
                (source_picture_hash.clone(), source_picture_bytes)
            };
            let source_frame_bytes = facts.width * facts.height * 3;
            if source_picture_bytes == 0
                || source_picture_bytes % source_frame_bytes != 0
                || manifest.output_frame_rate.as_ref().map_or_else(
                    || Ok(source_picture_bytes / source_frame_bytes),
                    |rate| rate.frames(source_picture_bytes / source_frame_bytes, &facts.fps),
                )? != frames
                || expected_picture != (picture_hash.clone(), picture_bytes)
                || source_audio_bytes % 6 != 0
                || (facts.sample_rate == manifest.output_sample_rate
                    && source_audio_hash != audio_hash)
            {
                bail!("normalization changed declared picture transform or input sample packing");
            }
            let input_samples = source_audio_bytes / 6;
            let expected = ((input_samples as u128 * manifest.output_sample_rate as u128)
                + (facts.sample_rate as u128 / 2))
                / facts.sample_rate as u128;
            if (samples as i128 - expected as i128).abs() > 2 {
                bail!("audio normalization inserted or removed more than two samples");
            }
            if manifest.output_geometry.is_some() {
                geometry_verification = Some(GeometryVerification {
                    input_width: facts.width,
                    input_height: facts.height,
                    input_sample_aspect_ratio: facts.sample_aspect_ratio.clone(),
                    sample_aspect_ratio_assumed_square: assumed_square,
                    assumed_color_space: assumed_color_space.clone(),
                    assumed_color_range: assumed_color_range.clone(),
                    color_properties: facts.color_properties.clone(),
                    output_width: selected_facts.width,
                    output_height: selected_facts.height,
                    scaling_applied: (facts.width, facts.height)
                        != (selected_facts.width, selected_facts.height),
                    decoded_source_picture_sha256: source_picture_hash,
                    transformed_source_matches_selected: true,
                });
            }
            (source_picture_bytes / source_frame_bytes, input_samples)
        };
        let frame_rate_verification = if manifest.output_frame_rate.is_some() {
            Some(FrameRateVerification {
                input_fps: facts.fps.clone(),
                output_fps: selected_facts.fps.clone(),
                input_frames,
                output_frames: frames,
                repeated_frames: frames
                    .checked_sub(input_frames)
                    .context("repeat-only normalization discarded source frames")?,
                transformed_source_matches_selected: true,
            })
        } else {
            None
        };
        if manifest.output_geometry.is_some() && geometry_verification.is_none() {
            geometry_verification = Some(GeometryVerification {
                input_width: facts.width,
                input_height: facts.height,
                input_sample_aspect_ratio: facts.sample_aspect_ratio.clone(),
                sample_aspect_ratio_assumed_square: assumed_square,
                assumed_color_space: assumed_color_space.clone(),
                assumed_color_range: assumed_color_range.clone(),
                color_properties: facts.color_properties.clone(),
                output_width: selected_facts.width,
                output_height: selected_facts.height,
                scaling_applied: false,
                decoded_source_picture_sha256: picture_hash.clone(),
                transformed_source_matches_selected: true,
            });
        }
        if let Some((expected_frames, expected_samples)) = presentation_counts[index] {
            if input_frames != expected_frames || input_samples != expected_samples {
                bail!("presentation decoded media differs from its source receipt");
            }
        }
        if manifest.output_geometry.is_some() || manifest.output_frame_rate.is_some() {
            let (num, den) = fps_parts(&facts.fps)?;
            episode_delivery::verify_timestamps(source, facts.sample_rate, num, den)?;
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
                if needs_normalization {
                    // This exact file was created above in our private temp
                    // directory. The checked padded replacement is now the
                    // only consumed copy; original selected sources remain.
                    fs::remove_file(&selected)?;
                }
                selected = padded;
                audio_hash = actual.0;
                samples = actual.1 / 6;
            }
        }
        let mut tail_hold_samples = 0;
        if segment.tail_hold_frames != 0 {
            let (num, den) = fps_parts(&selected_facts.fps)?;
            tail_hold_samples = hold_samples(
                segment.tail_hold_frames,
                frame_cursor
                    .checked_add(frames)
                    .context("hold frame clock overflow")?,
                manifest.output_sample_rate,
                num,
                den,
            )?;
            let held = temp.path().join(format!("held-{index:03}.mkv"));
            let (picture, audio) = append_silent_picture_hold(
                &selected,
                &held,
                segment.tail_hold_frames,
                tail_hold_samples,
                target_codec,
            )?;
            frames = frames
                .checked_add(segment.tail_hold_frames)
                .context("hold frame overflow")?;
            samples = samples
                .checked_add(tail_hold_samples)
                .context("hold sample overflow")?;
            if picture.1
                != frames
                    .checked_mul(frame_bytes)
                    .context("hold picture overflow")?
                || audio.1 != samples.checked_mul(6).context("hold audio overflow")?
            {
                bail!("silent picture hold has wrong frame or sample count");
            }
            selected = held;
            picture_hash = picture.0;
            audio_hash = audio.0;
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
            tail_hold_frames: segment.tail_hold_frames,
            tail_hold_samples,
            start_frame: frame_cursor,
            start_sample: sample_cursor,
            decoded_picture_sha256: picture_hash,
            decoded_audio_sha256: audio_hash,
            geometry_verification,
            frame_rate_verification,
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
    let master = match &manifest.preassembled_master {
        Some(reference) => scene_delivery::checked_file(asset_root, reference)?,
        None => work.join("master.mkv"),
    };
    if manifest.preassembled_master.is_none() {
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
    }
    let first = common.context("no episode media")?;
    if manifest.preassembled_master.is_some() {
        let candidate = probe(&master)?;
        if (
            candidate.width,
            candidate.height,
            &candidate.fps,
            candidate.sample_rate,
        ) != (
            first.width,
            first.height,
            &first.fps,
            manifest.output_sample_rate,
        ) {
            bail!("preassembled master geometry, frame rate or sample rate differs from episode");
        }
    }
    if manifest.output_geometry.is_some() {
        let final_facts = probe(&master)?;
        if final_facts.sample_aspect_ratio != "1:1"
            || final_facts.color_properties != first.color_properties
        {
            bail!("conformed master changed declared sample aspect ratio or color properties");
        }
    }
    let state = ConformState {
        manifest_bytes,
        first,
        records,
        normalized,
        master,
        frame_cursor,
        sample_cursor,
        presentation_fingerprint_sha256: presentation.fingerprint_sha256,
        presentation_cache_hits,
        presentation_cache_misses,
        preparation_sha256: None,
    };
    if preparation_scope.is_some() {
        let snapshot = preparation_scope.context("preparation scope missing")?;
        let checkpoint = checkpoint::capture(&state, work, asset_root, snapshot)?;
        fs::write(
            work.join("prepared.json"),
            serde_json::to_vec_pretty(&checkpoint)?,
        )?;
        let _ = temp.keep();
        let work = stage.keep();
        fs::rename(work, output)?;
        return Ok(None);
    }
    let external = manifest.preassembled_master.is_some();
    let receipt = finalize_conform(state, work, external)?;
    drop(temp);
    fs::rename(work, output)?;
    Ok(Some(receipt))
}

pub(crate) struct ConformState {
    pub preparation_sha256: Option<String>,
    pub manifest_bytes: Vec<u8>,
    pub first: MediaFacts,
    pub records: Vec<SegmentReceipt>,
    pub normalized: Vec<PathBuf>,
    pub master: PathBuf,
    pub frame_cursor: u64,
    pub sample_cursor: u64,
    pub presentation_fingerprint_sha256: String,
    pub presentation_cache_hits: usize,
    pub presentation_cache_misses: usize,
}

pub(crate) fn finalize_conform(
    state: ConformState,
    work: &Path,
    master_external: bool,
) -> Result<Receipt> {
    let ConformState {
        preparation_sha256,
        manifest_bytes,
        first,
        records,
        normalized,
        master,
        frame_cursor,
        sample_cursor,
        presentation_fingerprint_sha256,
        presentation_cache_hits,
        presentation_cache_misses,
    } = state;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    let upstream_verified = records
        .iter()
        .map(|r| r.upstream_delivery_verified)
        .collect::<Vec<_>>();
    let presentation_verified = records
        .iter()
        .map(|r| r.upstream_presentation_verified)
        .collect::<Vec<_>>();
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
        let mut compact_encoder = command("ffmpeg");
        compact_encoder
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(&master)
            .args(["-map", "0:v:0", "-map", "0:a:0", "-c:v", "libx264", "-crf"])
            .arg(profile.crf.to_string())
            .args(["-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a"])
            .arg(format!("{}k", profile.audio_bitrate_kbps))
            .args(["-movflags", "+faststart"]);
        if manifest.output_geometry.is_some() {
            for (key, flag) in [
                ("color_range", "-color_range"),
                ("color_space", "-colorspace"),
                ("color_transfer", "-color_trc"),
                ("color_primaries", "-color_primaries"),
            ] {
                let value = &first.color_properties[key];
                if value != "unknown" {
                    compact_encoder.args([flag, value]);
                }
            }
            // libx264 may omit its default limited-range VUI flag. MP4 does not
            // retain Matroska's range tag, so pin known range in the bitstream.
            match first.color_properties["color_range"].as_str() {
                "tv" => {
                    compact_encoder.args(["-bsf:v", "h264_metadata=video_full_range_flag=0"]);
                }
                "pc" => {
                    compact_encoder.args(["-bsf:v", "h264_metadata=video_full_range_flag=1"]);
                }
                _ => {}
            }
        }
        let status = compact_encoder.arg(&target).status()?;
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
        if manifest.output_geometry.is_some()
            && (video["sample_aspect_ratio"] != "1:1"
                || first
                    .color_properties
                    .iter()
                    .any(|(key, expected)| video[key].as_str().unwrap_or("unknown") != expected))
        {
            bail!(
                "compact delivery changed declared sample aspect ratio or color properties: expected {:?}, actual {:?}",
                first.color_properties,
                video
            );
        }
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
        preparation_sha256,
        episode_presentation_fingerprint_sha256: presentation_fingerprint_sha256,
        ffmpeg_version,
        output_sample_rate: manifest.output_sample_rate,
        output_geometry: manifest.output_geometry.clone(),
        total_frames: frame_cursor,
        total_samples: sample_cursor,
        master_sha256: file_sha(&master)?,
        master_bytes: fs::metadata(&master)?.len(),
        preassembled_master: manifest.preassembled_master.clone(),
        encoded_delivery,
        decoded_master_matches_ordered_segments: true,
        timestamps_verified: true,
        presentation_verification_cache: manifest.reuse_verified_presentations.then(|| {
            PresentationVerificationCache {
                reused_verified_checks: presentation_cache_hits,
                fresh_full_checks: presentation_cache_misses,
                policy: "Rehash all dependencies and validator/FFmpeg/FFprobe binaries; always decode and verify the new ordered episode".into(),
            }
        }),
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
        if !master_external {
            fs::remove_file(&master)?;
        }
    } else if master_external {
        // A supplied candidate is immutable input, including when a retained
        // lossless copy is requested. Never rename or delete its original.
        fs::copy(&master, work.join("master.mkv"))?;
    }
    Ok(receipt)
}
