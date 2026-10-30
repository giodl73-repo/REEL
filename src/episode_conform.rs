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

#[cfg(feature = "tig-publication")]
type NativePresentations<'a> =
    BTreeMap<String, &'a crate::tig_presentation::jobs::VerifiedPresentation<'a>>;
#[cfg(not(feature = "tig-publication"))]
type NativePresentations<'a> = BTreeMap<String, &'a ()>;
#[cfg(feature = "tig-publication")]
type NativeScenes<'a> =
    BTreeMap<String, &'a crate::tig_scene_production::jobs::reference::VerifiedScene<'a>>;
#[cfg(not(feature = "tig-publication"))]
type NativeScenes<'a> = BTreeMap<String, &'a ()>;

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
}

fn is_false(value: &bool) -> bool {
    !value
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

#[derive(Debug, Serialize)]
pub struct GeometryVerification {
    pub input_width: u64,
    pub input_height: u64,
    pub input_sample_aspect_ratio: String,
    #[serde(skip_serializing_if = "is_false")]
    pub sample_aspect_ratio_assumed_square: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assumed_color_space: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometry_verification: Option<GeometryVerification>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_geometry: Option<OutputGeometry>,
    pub total_frames: u64,
    pub total_samples: u64,
    pub master_sha256: String,
    pub master_bytes: u64,
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

#[derive(Clone)]
pub(crate) struct MediaFacts {
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

/// Honor explicit provider selection/errors; only the legacy ambient name uses
/// an exact PATH resolver. No ambient retry after an admitted-tool failure.
pub(crate) fn command_with_inputs(
    inputs: &(impl scene_delivery::InputFiles + ?Sized),
    program: scene_delivery::MediaProgram,
) -> Result<Command> {
    let selected = inputs.media_program(program)?;
    let path = if selected == Path::new(program.ambient_name()) {
        media_tool_in_path(
            program.ambient_name(),
            &std::env::var_os("PATH").context("tool PATH is missing")?,
        )?
    } else {
        selected
    };
    #[allow(unused_mut)]
    let mut command = Command::new(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    Ok(command)
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

    #[test]
    fn explicit_tool_selection_and_refusal_never_fall_back_to_path() {
        struct Provider(Option<PathBuf>);
        impl scene_delivery::InputFiles for Provider {
            fn verified_path(&self, _: &scene_delivery::FileRef) -> Result<PathBuf> {
                bail!("no files admitted")
            }
            fn media_program(&self, _: scene_delivery::MediaProgram) -> Result<PathBuf> {
                self.0.clone().context("tool not admitted")
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let selected = temp.path().join("explicit-nonexistent-tool");
        let mut command = command_with_inputs(
            &Provider(Some(selected.clone())),
            scene_delivery::MediaProgram::Ffmpeg,
        )
        .unwrap();
        assert_eq!(command.get_program(), selected.as_os_str());
        assert!(command.output().is_err());
        assert!(
            command_with_inputs(&Provider(None), scene_delivery::MediaProgram::Ffprobe).is_err()
        );
    }
}

pub(crate) fn probe_with_inputs(
    path: &Path,
    inputs: &(impl scene_delivery::InputFiles + ?Sized),
) -> Result<MediaFacts> {
    let output = command_with_inputs(inputs, scene_delivery::MediaProgram::Ffprobe)?
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

#[cfg(test)]
fn decoded_digest(path: &Path, video: bool) -> Result<(String, u64)> {
    decoded_digest_with_padding(path, video, 0)
}

pub(crate) fn decoded_digest_with_inputs(
    path: &Path,
    video: bool,
    inputs: &(impl scene_delivery::InputFiles + ?Sized),
) -> Result<(String, u64)> {
    decoded_digest_with_options(path, video, 0, None, "yuv444p", inputs)
}

#[cfg(test)]
fn decoded_digest_with_padding(
    path: &Path,
    video: bool,
    padding_samples: u64,
) -> Result<(String, u64)> {
    decoded_digest_with_filter(path, video, padding_samples, None)
}

#[cfg(test)]
fn decoded_digest_with_filter(
    path: &Path,
    video: bool,
    padding_samples: u64,
    video_filter: Option<&str>,
) -> Result<(String, u64)> {
    decoded_digest_with_pixel_format(path, video, padding_samples, video_filter, "yuv444p")
}

fn verify_color_space_declaration(
    source: &Path,
    space: &str,
    inputs: &(impl scene_delivery::InputFiles + ?Sized),
) -> Result<()> {
    let original = decoded_digest_with_options(source, true, 0, None, "rgb24", inputs)?;
    let declared = decoded_digest_with_options(
        source,
        true,
        0,
        Some(&format!("setparams=colorspace={space}")),
        "rgb24",
        inputs,
    )?;
    if declared != original {
        bail!("unspecified color-space declaration changes decoded source colors");
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
        let error = verify_color_space_declaration(&path, "bt470bg", Path::new(".")).unwrap_err();
        assert!(error.to_string().contains("changes decoded source colors"));
        verify_color_space_declaration(&path, "bt709", Path::new(".")).unwrap();
    }
}

#[cfg(test)]
fn decoded_digest_with_pixel_format(
    path: &Path,
    video: bool,
    padding_samples: u64,
    video_filter: Option<&str>,
    pixel_format: &str,
) -> Result<(String, u64)> {
    decoded_digest_with_options(
        path,
        video,
        padding_samples,
        video_filter,
        pixel_format,
        Path::new("."),
    )
}

fn decoded_digest_with_options(
    path: &Path,
    video: bool,
    padding_samples: u64,
    video_filter: Option<&str>,
    pixel_format: &str,
    inputs: &(impl scene_delivery::InputFiles + ?Sized),
) -> Result<(String, u64)> {
    decoded_digest_command(
        command_with_inputs(inputs, scene_delivery::MediaProgram::Ffmpeg)?,
        path,
        video,
        padding_samples,
        video_filter,
        pixel_format,
    )
}

fn decoded_digest_command(
    mut cmd: Command,
    path: &Path,
    video: bool,
    padding_samples: u64,
    video_filter: Option<&str>,
    pixel_format: &str,
) -> Result<(String, u64)> {
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

fn verify_semantic_scene_controls(
    segment: &Segment,
    language: &str,
    receipt: &serde_json::Value,
    controls: &dyn scene_delivery::InputFiles,
) -> Result<()> {
    let reference = segment
        .semantic_delivery
        .as_ref()
        .context("direct scene delivery lacks semantic selection")?;
    let semantic: semantic_delivery::SemanticDelivery =
        serde_yaml::from_slice(&scene_delivery::read_control(controls, reference)?)?;
    let job = segment
        .delivery_job
        .as_ref()
        .context("semantic scene lacks delivery job")?;
    if semantic.scene_id.as_deref() != Some(segment.id.as_str())
        || semantic.language.as_deref() != Some(language)
        || semantic.scene_delivery_job.sha256 != job.sha256
        || semantic.scene_delivery_job.bytes != job.bytes
        || scene_delivery::scoped_control(reference, &semantic.scene_delivery_job)?.path
            != scene_delivery::control_name(&job.path)?
    {
        bail!("semantic control selection differs from scene/language/delivery scope");
    }
    let planned = semantic_delivery::plan_with_control_files(reference, controls)?;
    if planned.scene_delivery_job_sha256 != job.sha256
        || receipt["plan"]["job_sha256"] != job.sha256
    {
        bail!("semantic control selection differs from rendered job");
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
    input_root: &dyn scene_delivery::InputFiles,
) -> Result<()> {
    if unit.source_ids.is_empty() {
        return Ok(());
    }
    let reference = segment
        .source_text_evidence
        .as_ref()
        .context("editable display lacks source-text evidence")?;
    let evidence: SourceTextEvidence =
        serde_json::from_slice(&scene_delivery::read_control(input_root, reference)?)?;
    if let Some(adoption_reference) = &segment.adoption_manifest {
        let adoption: presentation_adopt::Manifest = serde_json::from_slice(
            &scene_delivery::read_control(input_root, adoption_reference)?,
        )?;
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
    build_impl(
        manifest_path,
        input_root,
        asset_root,
        output,
        Provided::default(),
    )
}

/// Recheck every native scene with its borrowed, protected primary inputs.
/// Metadata and fresh delivery outputs retain the existing local-root contract;
/// this does not hydrate/cache/materialize primary media or grant producer currency.
/// Coverage must be exact. Imported legacy-source rechecks need their own adapter
/// and refuse here rather than falling back to ambient filesystem inputs.
pub fn build_with_scene_inputs(
    manifest_path: &Path,
    input_root: &Path,
    asset_root: &Path,
    output: &Path,
    scene_inputs: &BTreeMap<String, &dyn scene_delivery::InputFiles>,
) -> Result<Receipt> {
    build_impl(
        manifest_path,
        input_root,
        asset_root,
        output,
        Provided {
            scene_inputs: Some(scene_inputs),
            ..Provided::default()
        },
    )
}

/// Consume every native scene's primary inputs and delivery outputs through
/// protected providers. Source/control JSON remains under input_root; presentation
/// segments retain the existing local asset contract. Native scene outputs never
/// fall back to that root. Both provider maps must cover exactly the native scenes.
pub fn build_with_scene_files(
    manifest_path: &Path,
    input_root: &Path,
    asset_root: &Path,
    output: &Path,
    scene_inputs: &BTreeMap<String, &dyn scene_delivery::InputFiles>,
    scene_outputs: &BTreeMap<String, &dyn scene_delivery::InputFiles>,
) -> Result<Receipt> {
    build_impl(
        manifest_path,
        input_root,
        asset_root,
        output,
        Provided {
            scene_inputs: Some(scene_inputs),
            scene_outputs: Some(scene_outputs),
            ..Provided::default()
        },
    )
}

/// Borrow the exact controls/source and selected output providers for one
/// adopted presentation. Keep both owners alive through the complete conform.
pub struct PresentationFiles<'a> {
    pub inputs: &'a dyn scene_delivery::InputFiles,
    pub outputs: &'a dyn scene_delivery::InputFiles,
}

/// Recheck adopted presentations directly against cache-backed source and output
/// files. Other segments retain their existing local contract. Coverage is exact
/// and a provider refusal never falls back to local files. This path deliberately
/// performs full independent presentation checks, without implicit cache writes.
pub fn build_with_presentation_files(
    manifest_path: &Path,
    input_root: &Path,
    asset_root: &Path,
    output: &Path,
    presentations: &BTreeMap<String, PresentationFiles<'_>>,
) -> Result<Receipt> {
    build_impl(
        manifest_path,
        input_root,
        asset_root,
        output,
        Provided {
            presentations: Some(presentations),
            ..Provided::default()
        },
    )
}

/// Combine exact native-scene and adopted-presentation provider coverage. The
/// episode's bounded source/control JSON keeps its explicit local-root contract.
pub fn build_with_segment_files(
    manifest_path: &Path,
    input_root: &Path,
    asset_root: &Path,
    output: &Path,
    scene_inputs: &BTreeMap<String, &dyn scene_delivery::InputFiles>,
    scene_outputs: &BTreeMap<String, &dyn scene_delivery::InputFiles>,
    presentations: &BTreeMap<String, PresentationFiles<'_>>,
) -> Result<Receipt> {
    build_impl(
        manifest_path,
        input_root,
        asset_root,
        output,
        Provided {
            scene_inputs: Some(scene_inputs),
            scene_outputs: Some(scene_outputs),
            presentations: Some(presentations),
            ..Provided::default()
        },
    )
}

/// Exact borrowed component coverage and explicit episode media programs. Keep
/// every provider owner alive through the operation. Programs do not establish
/// producer currency; the admitted job must separately bind runtime identity.
pub struct Files<'a> {
    pub scene_inputs: &'a BTreeMap<String, &'a dyn scene_delivery::InputFiles>,
    pub scene_outputs: &'a BTreeMap<String, &'a dyn scene_delivery::InputFiles>,
    pub presentations: &'a BTreeMap<String, PresentationFiles<'a>>,
    pub programs: &'a dyn scene_delivery::InputFiles,
}

pub fn build_with_files(
    manifest_path: &Path,
    input_root: &Path,
    asset_root: &Path,
    output: &Path,
    files: Files<'_>,
) -> Result<Receipt> {
    build_impl(
        manifest_path,
        input_root,
        asset_root,
        output,
        Provided {
            scene_inputs: Some(files.scene_inputs),
            scene_outputs: Some(files.scene_outputs),
            presentations: Some(files.presentations),
            programs: Some(files.programs),
            ..Provided::default()
        },
    )
}

/// Exact cache/provider-backed parent manifest and top-level episode controls.
/// Scene source/semantic controls keep their explicit input_root contract. The
/// caller retains every provider owner through the complete synchronous build.
/// Provider errors never fall back to local controls or ambient programs.
pub fn build_with_parent_controls(
    manifest: &scene_delivery::FileRef,
    input_root: &Path,
    asset_root: &Path,
    output: &Path,
    files: Files<'_>,
    controls: &dyn scene_delivery::InputFiles,
) -> Result<Receipt> {
    build_impl(
        &manifest.path,
        input_root,
        asset_root,
        output,
        Provided {
            scene_inputs: Some(files.scene_inputs),
            scene_outputs: Some(files.scene_outputs),
            presentations: Some(files.presentations),
            programs: Some(files.programs),
            controls: Some(controls),
            manifest: Some(manifest),
            ..Provided::default()
        },
    )
}

/// Exact production metadata pin for one native scene; the provider also owns
/// its original job, cue contract and semantic selection. Not media outputs.
pub struct SceneControls<'a> {
    pub files: &'a dyn scene_delivery::InputFiles,
    pub production: &'a scene_delivery::FileRef,
}
/// Full direct-native control path: parent and per-scene metadata plus explicit
/// primary/output/runtime providers. Imported/presentation routes refuse until
/// separately supported; no fallback to input_root source files is permitted.
pub fn build_with_all_control_files(
    manifest: &scene_delivery::FileRef,
    input_root: &Path,
    asset_root: &Path,
    output: &Path,
    files: Files<'_>,
    controls: &dyn scene_delivery::InputFiles,
    scenes: &BTreeMap<String, SceneControls<'_>>,
) -> Result<Receipt> {
    build_impl(
        &manifest.path,
        input_root,
        asset_root,
        output,
        Provided {
            scene_inputs: Some(files.scene_inputs),
            scene_outputs: Some(files.scene_outputs),
            presentations: Some(files.presentations),
            programs: Some(files.programs),
            controls: Some(controls),
            manifest: Some(manifest),
            scene_controls: Some(scenes),
            ..Provided::default()
        },
    )
}

/// Complete metadata coverage for direct native scenes and adopted
/// presentations. Composition source movies remain separate from primary
/// scene inputs and from retained component outputs.
pub struct ComponentControls<'a> {
    pub parent: &'a dyn scene_delivery::InputFiles,
    pub scenes: &'a BTreeMap<String, SceneControls<'a>>,
    pub presentations: &'a BTreeMap<String, &'a dyn scene_delivery::InputFiles>,
}

/// Assemble with no source checkout fallback. Every scene and presentation
/// must have exactly one metadata provider and the corresponding media
/// provider in `files`. This checks content, not producer lineage or approval.
pub fn build_with_component_controls(
    manifest: &scene_delivery::FileRef,
    output: &Path,
    files: Files<'_>,
    controls: ComponentControls<'_>,
) -> Result<Receipt> {
    build_impl(
        &manifest.path,
        Path::new(""),
        Path::new(""),
        output,
        Provided {
            scene_inputs: Some(files.scene_inputs),
            scene_outputs: Some(files.scene_outputs),
            presentations: Some(files.presentations),
            programs: Some(files.programs),
            controls: Some(controls.parent),
            manifest: Some(manifest),
            scene_controls: Some(controls.scenes),
            presentation_controls: Some(controls.presentations),
            ..Provided::default()
        },
    )
}

#[cfg(feature = "tig-publication")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_with_current_components(
    manifest: &scene_delivery::FileRef,
    output: &Path,
    files: Files<'_>,
    controls: &dyn scene_delivery::InputFiles,
    scenes: &BTreeMap<String, SceneControls<'_>>,
    native: &NativePresentations<'_>,
    native_scenes: &NativeScenes<'_>,
) -> Result<Receipt> {
    build_impl(
        &manifest.path,
        Path::new(""),
        Path::new(""),
        output,
        Provided {
            scene_inputs: Some(files.scene_inputs),
            scene_outputs: Some(files.scene_outputs),
            programs: Some(files.programs),
            controls: Some(controls),
            manifest: Some(manifest),
            scene_controls: Some(scenes),
            native_presentations: Some(native),
            native_scenes: Some(native_scenes),
            ..Provided::default()
        },
    )
}

#[cfg(feature = "tig-publication")]
fn native_presentation(
    native: Option<&NativePresentations<'_>>,
    segment: &Segment,
) -> Result<Option<(PathBuf, serde_json::Value)>> {
    let Some(proof) = native.and_then(|proofs| proofs.get(&segment.id)) else {
        return Ok(None);
    };
    let raw = proof.declaration()?;
    if segment.kind != SegmentKind::EpisodePresentation
        || segment.adoption_manifest.is_some()
        || segment.delivery_job.is_some()
        || segment.delivery_receipt.is_some()
        || segment.semantic_delivery.is_some()
        || segment.imported_source_manifest.is_some()
        || segment.imported_source_root.is_some()
        || segment.master.sha256 != proof.selected().sha256
        || segment.master.bytes != proof.selected().bytes
        || segment.source_receipt.sha256 != sha(&raw)
        || segment.source_receipt.bytes != raw.len() as u64
    {
        bail!("native presentation differs from sealed current component proof");
    }
    Ok(Some((
        proof.selected_path()?,
        serde_json::from_slice(&raw)?,
    )))
}
#[cfg(not(feature = "tig-publication"))]
fn native_presentation(
    _native: Option<&NativePresentations<'_>>,
    _segment: &Segment,
) -> Result<Option<(PathBuf, serde_json::Value)>> {
    Ok(None)
}

#[cfg(feature = "tig-publication")]
fn native_scene(
    native: Option<&NativeScenes<'_>>,
    segment: &Segment,
) -> Result<Option<(PathBuf, serde_json::Value)>> {
    let Some(proof) = native.and_then(|proofs| proofs.get(&segment.id)) else {
        return Ok(None);
    };
    let raw = proof.declaration()?;
    if segment.kind != SegmentKind::Scene
        || segment.adoption_manifest.is_some()
        || segment.delivery_job.is_some()
        || segment.delivery_receipt.is_some()
        || segment.semantic_delivery.is_some()
        || segment.imported_source_manifest.is_some()
        || segment.imported_source_root.is_some()
        || segment.source_text_evidence.is_some()
        || segment.master.sha256 != proof.selected().sha256
        || segment.master.bytes != proof.selected().bytes
        || segment.source_receipt.sha256 != sha(&raw)
        || segment.source_receipt.bytes != raw.len() as u64
    {
        bail!("native scene differs from sealed current component proof");
    }
    Ok(Some((
        proof.selected_path()?,
        serde_json::from_slice(&raw)?,
    )))
}
#[cfg(not(feature = "tig-publication"))]
fn native_scene(
    _native: Option<&NativeScenes<'_>>,
    _segment: &Segment,
) -> Result<Option<(PathBuf, serde_json::Value)>> {
    Ok(None)
}

fn read_parent_control(
    files: &dyn scene_delivery::InputFiles,
    reference: &scene_delivery::FileRef,
) -> Result<Vec<u8>> {
    const MAX: u64 = 4 * 1024 * 1024;
    if reference.bytes > MAX {
        bail!("episode parent control exceeds metadata budget");
    }
    let raw = files.control_bytes(reference)?;
    if raw.len() as u64 != reference.bytes
        || raw.len() as u64 > MAX
        || sha(&raw) != reference.sha256
    {
        bail!("episode parent control differs from its admitted pin");
    }
    Ok(raw)
}

#[derive(Default)]
struct Provided<'a> {
    scene_inputs: Option<&'a BTreeMap<String, &'a dyn scene_delivery::InputFiles>>,
    scene_outputs: Option<&'a BTreeMap<String, &'a dyn scene_delivery::InputFiles>>,
    presentations: Option<&'a BTreeMap<String, PresentationFiles<'a>>>,
    programs: Option<&'a dyn scene_delivery::InputFiles>,
    controls: Option<&'a dyn scene_delivery::InputFiles>,
    manifest: Option<&'a scene_delivery::FileRef>,
    scene_controls: Option<&'a BTreeMap<String, SceneControls<'a>>>,
    presentation_controls: Option<&'a BTreeMap<String, &'a dyn scene_delivery::InputFiles>>,
    native_presentations: Option<&'a NativePresentations<'a>>,
    native_scenes: Option<&'a NativeScenes<'a>>,
}

fn build_impl(
    manifest_path: &Path,
    input_root: &Path,
    asset_root: &Path,
    output: &Path,
    provided: Provided<'_>,
) -> Result<Receipt> {
    let Provided {
        scene_inputs,
        scene_outputs,
        presentations,
        programs,
        controls,
        manifest: pinned_manifest,
        scene_controls,
        presentation_controls,
        native_presentations,
        native_scenes,
    } = provided;
    let explicit_programs = programs.is_some();
    let local_programs = PathBuf::from(".");
    let local_controls = input_root.to_path_buf();
    let programs = programs.unwrap_or(&local_programs);
    if output.exists() {
        bail!("episode output must be a new directory");
    }
    if explicit_programs {
        command_with_inputs(programs, scene_delivery::MediaProgram::Ffmpeg)?;
        command_with_inputs(programs, scene_delivery::MediaProgram::Ffprobe)?;
    }
    let manifest_bytes = if let Some(reference) = pinned_manifest {
        read_parent_control(
            controls.context("parent manifest requires explicit controls")?,
            reference,
        )?
    } else {
        fs::read(manifest_path)?
    };
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    if let Some(native) = native_scenes {
        let scenes = manifest
            .segments
            .iter()
            .filter(|s| s.kind == SegmentKind::Scene)
            .collect::<Vec<_>>();
        let ids = scenes
            .iter()
            .map(|s| s.id.as_str())
            .collect::<BTreeSet<_>>();
        if ids.len() != scenes.len() || native.keys().any(|id| !ids.contains(id.as_str())) {
            bail!("native scene proof coverage differs from parent segments");
        }
    }
    if let Some(native) = native_presentations {
        let ids = manifest
            .segments
            .iter()
            .filter(|s| s.kind == SegmentKind::EpisodePresentation)
            .map(|s| s.id.as_str())
            .collect::<BTreeSet<_>>();
        if ids.len() != native.len() || !native.keys().all(|id| ids.contains(id.as_str())) {
            bail!("native presentation proof coverage differs from parent segments");
        }
    }
    if let Some(scenes) = scene_controls {
        let ids = manifest
            .segments
            .iter()
            .filter(|s| {
                s.kind == SegmentKind::Scene && native_scenes.is_none_or(|p| !p.contains_key(&s.id))
            })
            .map(|s| s.id.as_str())
            .collect::<BTreeSet<_>>();
        if ids.len()
            != manifest
                .segments
                .iter()
                .filter(|s| {
                    s.kind == SegmentKind::Scene
                        && native_scenes.is_none_or(|p| !p.contains_key(&s.id))
                })
                .count()
            || scenes.len() != ids.len()
            || !scenes.keys().all(|id| ids.contains(id.as_str()))
            || manifest.segments.iter().any(|s| {
                if s.kind == SegmentKind::EpisodePresentation {
                    return presentation_controls.is_none() && native_presentations.is_none();
                }
                if native_scenes.is_some_and(|p| p.contains_key(&s.id)) {
                    return false;
                }
                s.semantic_delivery.is_none()
                    || s.delivery_job.is_none()
                    || s.delivery_receipt.is_none()
                    || s.imported_source_root.is_some()
                    || s.imported_source_manifest.is_some()
                    || s.adoption_manifest.is_some()
                    || s.source_text_evidence.is_some()
            })
        {
            bail!("explicit scene controls require exact direct-native coverage");
        }
    }
    if let Some(metadata) = presentation_controls {
        let ids = manifest
            .segments
            .iter()
            .filter(|s| s.kind == SegmentKind::EpisodePresentation)
            .map(|s| s.id.as_str())
            .collect::<BTreeSet<_>>();
        if metadata.len() != ids.len()
            || !metadata.keys().all(|id| ids.contains(id.as_str()))
            || presentations.is_none_or(|files| !metadata.keys().eq(files.keys()))
        {
            bail!("presentation controls require exact adopted component coverage");
        }
    }
    if let Some(files) = presentations {
        let selected = manifest
            .segments
            .iter()
            .filter(|s| s.kind == SegmentKind::EpisodePresentation)
            .collect::<Vec<_>>();
        let ids = selected
            .iter()
            .map(|s| s.id.as_str())
            .collect::<BTreeSet<_>>();
        if ids.len() != selected.len()
            || ids.len() != files.len()
            || files.keys().any(|id| !ids.contains(id.as_str()))
            || selected.iter().any(|s| {
                s.adoption_manifest.is_none()
                    && native_presentations.is_none_or(|p| !p.contains_key(&s.id))
            })
        {
            bail!("provided presentation files require exact adopted recheck coverage");
        }
    }
    if let Some(inputs) = scene_inputs {
        let scenes = manifest
            .segments
            .iter()
            .filter(|s| {
                s.kind == SegmentKind::Scene && native_scenes.is_none_or(|p| !p.contains_key(&s.id))
            })
            .collect::<Vec<_>>();
        let ids = scenes
            .iter()
            .map(|s| s.id.as_str())
            .collect::<BTreeSet<_>>();
        if ids.len() != scenes.len()
            || ids.len() != inputs.len()
            || inputs.keys().any(|id| !ids.contains(id.as_str()))
            || scenes.iter().any(|s| {
                s.delivery_job.is_none()
                    || s.delivery_receipt.is_none()
                    || s.imported_source_manifest.is_some()
                    || s.imported_source_root.is_some()
            })
        {
            bail!("provided scene inputs require exact native recheck coverage");
        }
        if let Some(outputs) = scene_outputs
            && (!inputs.keys().eq(outputs.keys())
                || scenes.iter().any(|s| s.semantic_delivery.is_none()))
        {
            bail!("provided scene outputs require exact direct native recheck coverage");
        }
    }
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
        if let Some(files) = controls {
            read_parent_control(files, item)
        } else {
            Ok(fs::read(scene_delivery::checked_file(input_root, item)?)?)
        }
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
    let mut selected_episode = episode.clone();
    if native_presentations.is_some() {
        // Validate the layout/catalog contract, while selected byte authority
        // comes from the current Tig component records and their sealed proofs.
        for invocation in &mut selected_episode.presentation {
            invocation.asset_binding = None;
        }
    }
    let presentation =
        resolve_episode_presentation(&catalog, &selected_episode, &season, &episode_bindings)?;
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
    let mut presentation_cache_hits = 0;
    let mut presentation_cache_misses = 0;
    for (segment_index, segment) in manifest.segments.iter().enumerate() {
        if !ordered_units_v2 && segment.source_text_evidence.is_some() {
            bail!("v1 episode segment cannot carry v2 source-text evidence");
        }
        let provided_presentation = presentations
            .filter(|_| segment.kind == SegmentKind::EpisodePresentation)
            .and_then(|files| files.get(&segment.id));
        let provided_outputs = scene_outputs
            .filter(|_| segment.kind == SegmentKind::Scene)
            .and_then(|outputs| outputs.get(&segment.id))
            .copied()
            .or_else(|| provided_presentation.map(|p| p.outputs));
        let native = if segment.kind == SegmentKind::Scene {
            native_scene(native_scenes, segment)?
        } else {
            native_presentation(native_presentations, segment)?
        };
        let (master, receipt, receipt_path) = if let Some((master, receipt)) = &native {
            (master.clone(), receipt.clone(), None)
        } else {
            let (master, receipt_path) = if let Some(files) = provided_outputs {
                let mut master = segment.master.clone();
                master.path = "master.mkv".into();
                let mut receipt = segment.source_receipt.clone();
                receipt.path = "receipt.json".into();
                (
                    files.verified_path(&master)?,
                    files.verified_path(&receipt)?,
                )
            } else {
                (
                    scene_delivery::checked_file(asset_root, &segment.master)?,
                    scene_delivery::checked_file(asset_root, &segment.source_receipt)?,
                )
            };
            let receipt: serde_json::Value = serde_json::from_slice(&fs::read(&receipt_path)?)?;
            selected_receipt(segment, &manifest.language, &receipt)?;
            (master, receipt, Some(receipt_path))
        };
        if segment.imported_source_manifest.is_some()
            && receipt["episode_id"] != manifest.episode_id
        {
            bail!("imported scene source proof belongs to another episode");
        }
        let presentation_rechecked = if native.is_some() {
            segment.kind == SegmentKind::EpisodePresentation
        } else {
            match (segment.kind, &segment.adoption_manifest) {
                (SegmentKind::Scene, None) => false,
                (SegmentKind::Scene, Some(_)) => {
                    bail!("scene cannot supply a presentation adoption manifest")
                }
                (SegmentKind::EpisodePresentation, Some(adoption)) => {
                    if receipt["technical_validation_state"] != "decoded-source-equivalent" {
                        bail!("adoption manifest requires decoded-source-equivalent receipt");
                    }
                    let stage_parent = output
                        .parent()
                        .filter(|path| !path.as_os_str().is_empty())
                        .unwrap_or(Path::new("."));
                    if let Some(metadata) = presentation_controls.and_then(|p| p.get(&segment.id)) {
                        let files =
                            provided_presentation.context("presentation media provider missing")?;
                        let mut master = segment.master.clone();
                        master.path = "master.mkv".into();
                        let mut receipt = segment.source_receipt.clone();
                        receipt.path = "receipt.json".into();
                        presentation_adopt::check_with_control_files(
                            &presentation_adopt::Selection {
                                manifest: adoption,
                                master: &master,
                                receipt: &receipt,
                            },
                            *metadata,
                            files.inputs,
                            files.outputs,
                            stage_parent,
                        )?;
                        presentation_cache_misses += 1;
                    } else if let Some(files) = provided_presentation {
                        let adoption_path = scene_delivery::checked_file(input_root, adoption)?;
                        presentation_adopt::check(
                            &adoption_path,
                            files.inputs,
                            files.inputs,
                            &master,
                            receipt_path
                                .as_deref()
                                .context("legacy presentation receipt path missing")?,
                            stage_parent,
                        )?;
                        presentation_cache_misses += 1;
                    } else if manifest.reuse_verified_presentations {
                        let adoption_path = scene_delivery::checked_file(input_root, adoption)?;
                        let hit = presentation_adopt::check_cached(
                            &adoption_path,
                            input_root,
                            asset_root,
                            &master,
                            receipt_path
                                .as_deref()
                                .context("legacy presentation receipt path missing")?,
                            stage_parent,
                            &asset_root.join(".reel-verification-cache/presentation-adoption-v1"),
                        )?;
                        if hit {
                            presentation_cache_hits += 1;
                        } else {
                            presentation_cache_misses += 1;
                        }
                    } else {
                        let adoption_path = scene_delivery::checked_file(input_root, adoption)?;
                        presentation_adopt::check(
                            &adoption_path,
                            input_root,
                            asset_root,
                            &master,
                            receipt_path
                                .as_deref()
                                .context("legacy presentation receipt path missing")?,
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
            }
        };
        let rechecked = if native.is_some() && segment.kind == SegmentKind::Scene {
            true
        } else {
            match (
                segment.kind,
                &segment.delivery_job,
                &segment.delivery_receipt,
            ) {
                (SegmentKind::Scene, Some(job_ref), Some(delivery_ref)) => {
                    let provided_controls =
                        scene_controls.and_then(|scenes| scenes.get(&segment.id));
                    let (source_root, job_path) = if provided_controls.is_some() {
                        (None, None)
                    } else {
                        let source_root = scoped_import_root(
                            input_root,
                            segment.imported_source_root.as_deref(),
                        )?;
                        let job_path = scene_delivery::checked_file(&source_root, job_ref)?;
                        (Some(source_root), Some(job_path))
                    };
                    if let Some(controls) = provided_controls {
                        verify_semantic_scene_controls(
                            segment,
                            &manifest.language,
                            &receipt,
                            controls.files,
                        )?;
                    } else if segment.semantic_delivery.is_some() {
                        verify_semantic_scene_scope(
                            segment,
                            &manifest.language,
                            &receipt,
                            input_root,
                        )?;
                    } else {
                        verify_native_delivery_proof(&receipt, job_ref, delivery_ref)?;
                    }
                    let checked = if let Some(files) = provided_outputs {
                        let inputs = scene_inputs
                            .and_then(|items| items.get(&segment.id))
                            .context("missing provided scene inputs")?;
                        let mut reference = delivery_ref.clone();
                        reference.path = "receipt.json".into();
                        if let Some(controls) = provided_controls {
                            scene_delivery::check_with_all_files(
                                job_ref,
                                controls.production,
                                *inputs,
                                files,
                                &reference,
                                controls.files,
                            )?
                        } else {
                            scene_delivery::check_with_output_files(
                                job_path.as_ref().context("local job path missing")?,
                                *inputs,
                                files,
                                &reference,
                            )?
                        }
                    } else {
                        let job_path = job_path
                            .as_ref()
                            .context("scene controls require explicit output files")?;
                        let source_root = source_root
                            .as_deref()
                            .context("local source root missing")?;
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
                        if let Some(inputs) = scene_inputs {
                            let inputs = inputs
                                .get(&segment.id)
                                .context("missing provided scene inputs")?;
                            scene_delivery::check_with_inputs(job_path, *inputs, delivery_root)?
                        } else if let Some(imported) = &segment.imported_source_manifest {
                            imported_scene_proof::recheck(
                                source_root,
                                imported,
                                asset_root,
                                delivery_root,
                                &receipt,
                            )?
                        } else if receipt["schema"] == "reel.scene-build-receipt.v1" {
                            // The exact native proof above binds a successful full check.
                            // Rehash all current source/output bytes and compare the plan;
                            // final conform still decodes and compares every ordered frame.
                            scene_delivery::recheck_native_receipt(
                                job_path,
                                asset_root,
                                delivery_root,
                            )?
                        } else {
                            scene_delivery::check(job_path, asset_root, delivery_root)?
                        }
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
            }
        };
        match segment.kind {
            SegmentKind::Scene => {
                presentation_counts.push(if native.is_some() {
                    Some((
                        receipt["frames"]
                            .as_u64()
                            .context("scene reference lacks frames")?,
                        receipt["samples"]
                            .as_u64()
                            .context("scene reference lacks samples")?,
                    ))
                } else {
                    None
                });
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
                    || receipt["timestamps_verified"] != true
                    || (native.is_none()
                        && (receipt["episode_id"] != manifest.episode_id
                            || !matches!(
                                receipt["technical_validation_state"].as_str(),
                                Some("decoded-source-equivalent" | "rendered-and-checked")
                            )))
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
                if native.is_some() {
                    if !invocation
                        .content
                        .as_object()
                        .is_some_and(|content| content.is_empty())
                    {
                        bail!("native presentation requires a current invocation-content binding");
                    }
                    if ordered_units_v2
                        && !master_template.ordered_units[segment_index]
                            .source_ids
                            .is_empty()
                    {
                        bail!("native editable display requires its current source-text contract");
                    }
                } else {
                    let key = invocation
                        .asset_binding
                        .as_deref()
                        .context("presentation requires selected episode binding")?;
                    let asset = presentation
                        .selected_inputs
                        .get(key)
                        .context("missing selected presentation master")?;
                    if asset.sha256 != segment.master.sha256 || asset.bytes != segment.master.bytes
                    {
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
                            presentation_controls
                                .and_then(|p| p.get(&segment.id))
                                .copied()
                                .or(controls)
                                .unwrap_or(&local_controls),
                        )?;
                    }
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
        let facts = probe_with_inputs(source, programs)?;
        let mut output_facts = facts.clone();
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
        if segment.kind == SegmentKind::Scene
            && facts.video_codec == "h264"
            && (segment.delivery_job.is_none() || segment.delivery_receipt.is_none())
            && native_scenes.is_none_or(|proofs| !proofs.contains_key(&segment.id))
        {
            bail!("H264 scene requires its verified delivery or sealed primary-reference proof");
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
            || facts.video_codec != target_codec
            || (target_codec == "h264" && facts.reordered_video_frames != 0);
        let needs_normalization =
            facts.sample_rate != manifest.output_sample_rate || needs_video_normalization;
        let mut selected = if !needs_normalization {
            source.clone()
        } else {
            let path = temp.path().join(format!("normalized-{index:03}.mkv"));
            let mut encoder = command_with_inputs(programs, scene_delivery::MediaProgram::Ffmpeg)?;
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
        let selected_facts = probe_with_inputs(&selected, programs)?;
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
        let (picture_hash, picture_bytes) = decoded_digest_with_inputs(&selected, true, programs)?;
        let (mut audio_hash, audio_bytes) = decoded_digest_with_inputs(&selected, false, programs)?;
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
        let mut geometry_verification = None;
        let (input_frames, input_samples) = if !needs_normalization {
            (frames, samples)
        } else {
            let (source_picture_hash, source_picture_bytes) =
                decoded_digest_with_inputs(source, true, programs)?;
            if let Some(space) = &assumed_color_space {
                verify_color_space_declaration(source, space, programs)?;
            }
            let (source_audio_hash, source_audio_bytes) =
                decoded_digest_with_inputs(source, false, programs)?;
            let expected_picture = if let Some(filter) = &scale_filter {
                decoded_digest_with_options(source, true, 0, Some(filter), "yuv444p", programs)?
            } else {
                (source_picture_hash.clone(), source_picture_bytes)
            };
            let source_frame_bytes = facts.width * facts.height * 3;
            if source_picture_bytes == 0
                || source_picture_bytes % source_frame_bytes != 0
                || source_picture_bytes / source_frame_bytes != frames
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
        if manifest.output_geometry.is_some() && geometry_verification.is_none() {
            geometry_verification = Some(GeometryVerification {
                input_width: facts.width,
                input_height: facts.height,
                input_sample_aspect_ratio: facts.sample_aspect_ratio.clone(),
                sample_aspect_ratio_assumed_square: assumed_square,
                assumed_color_space: assumed_color_space.clone(),
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
        if manifest.output_geometry.is_some() {
            let (num, den) = fps_parts(&facts.fps)?;
            episode_delivery::verify_timestamps_with_inputs(
                source,
                facts.sample_rate,
                num,
                den,
                programs,
            )?;
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
                let expected = decoded_digest_with_options(
                    &selected,
                    false,
                    audio_padding_samples,
                    None,
                    "yuv444p",
                    programs,
                )?;
                let padded = temp.path().join(format!("padded-{index:03}.mkv"));
                let status = command_with_inputs(programs, scene_delivery::MediaProgram::Ffmpeg)?
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
                let actual = decoded_digest_with_inputs(&padded, false, programs)?;
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
            geometry_verification,
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
    let mut concat = command_with_inputs(programs, scene_delivery::MediaProgram::Ffmpeg)?;
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
    if manifest.output_geometry.is_some() {
        let final_facts = probe_with_inputs(&master, programs)?;
        if final_facts.sample_aspect_ratio != "1:1"
            || final_facts.color_properties != first.color_properties
        {
            bail!("conformed master changed declared sample aspect ratio or color properties");
        }
    }
    let video_lengths = records
        .iter()
        .map(|r| r.frames * first.width * first.height * 3)
        .collect::<Vec<_>>();
    let audio_lengths = records.iter().map(|r| r.samples * 6).collect::<Vec<_>>();
    let actual_video =
        episode_delivery::decode_segments_with_inputs(&master, true, &video_lengths, programs)?;
    let actual_audio =
        episode_delivery::decode_segments_with_inputs(&master, false, &audio_lengths, programs)?;
    for (index, record) in records.iter().enumerate() {
        if actual_video[index] != record.decoded_picture_sha256
            || actual_audio[index] != record.decoded_audio_sha256
        {
            bail!("episode decoded segment differs from selected source {index}");
        }
    }
    let (fps_numerator, fps_denominator) = fps_parts(&first.fps)?;
    episode_delivery::verify_timestamps_with_inputs(
        &master,
        manifest.output_sample_rate,
        fps_numerator,
        fps_denominator,
        programs,
    )?;
    let mut boundary_findings = Vec::new();
    for index in 0..normalized.len().saturating_sub(1) {
        let left = &records[index];
        let right = &records[index + 1];
        let left_audio = episode_delivery::edge_audio_with_inputs(
            &normalized[index],
            left.samples,
            manifest.output_sample_rate,
            true,
            programs,
        )?;
        let right_audio = episode_delivery::edge_audio_with_inputs(
            &normalized[index + 1],
            right.samples,
            manifest.output_sample_rate,
            false,
            programs,
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
        let black = episode_delivery::black_edge_with_inputs(
            &normalized[index],
            left.frames - 1,
            first.width as u32,
            first.height as u32,
            programs,
        )? || episode_delivery::black_edge_with_inputs(
            &normalized[index + 1],
            0,
            first.width as u32,
            first.height as u32,
            programs,
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
    let version = command_with_inputs(programs, scene_delivery::MediaProgram::Ffmpeg)?
        .arg("-version")
        .output()?;
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
        let mut compact_encoder =
            command_with_inputs(programs, scene_delivery::MediaProgram::Ffmpeg)?;
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
        let decoded = command_with_inputs(programs, scene_delivery::MediaProgram::Ffmpeg)?
            .args(["-v", "error", "-xerror", "-nostdin", "-i"])
            .arg(&target)
            .args(["-map", "0:v:0", "-map", "0:a:0", "-f", "null", "-"])
            .status()?;
        if !decoded.success() {
            bail!("compact episode full decode failed");
        }
        let probe = command_with_inputs(programs, scene_delivery::MediaProgram::Ffprobe)?
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
        episode_presentation_fingerprint_sha256: presentation.fingerprint_sha256,
        ffmpeg_version,
        output_sample_rate: manifest.output_sample_rate,
        output_geometry: manifest.output_geometry.clone(),
        total_frames: frame_cursor,
        total_samples: sample_cursor,
        master_sha256: file_sha(&master)?,
        master_bytes: fs::metadata(&master)?.len(),
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
        fs::remove_file(&master)?;
    }
    drop(temp);
    fs::rename(work, output)?;
    Ok(receipt)
}
