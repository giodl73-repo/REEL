//! Select an existing presentation master through scoped bindings, then make
//! a decoded-content-equivalent lossless segment for episode conform.

use crate::{episode_conform, episode_delivery, scene_delivery};
use anyhow::{Context, Result, bail};
use reel_assembly::scene_authoring::{ScopedBindings, TemplateCatalog};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::Stdio,
};

pub const SCHEMA: &str = "reel.presentation-adopt.v1";

/// A frame-exact excerpt of a hash-bound source. Audio uses the same rational
/// frame clock; callers cannot independently shift the spoken performance.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceRange {
    pub start_frame: u64,
    pub frame_count: u64,
}

/// Explicit repair of an inherited audio timestamp fault. Decoded samples are
/// kept in order; no resampling, padding, shifting or performance edits occur.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AudioClockRepair {
    pub policy: String,
    pub evidence_pointer: String,
}

/// Select samples by the source picture's decoded PTS, rather than assuming
/// that both streams begin at sample/frame zero. No caller-authored offset.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AudioRangeOrigin {
    pub policy: String,
    pub evidence_pointer: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourcePts {
    pub ticks: i64,
    pub time_base_numerator: u64,
    pub time_base_denominator: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AudioSampleWindow {
    pub selected_video_pts: SourcePts,
    pub first_audio_pts: SourcePts,
    pub selected_audio_anchor_pts: SourcePts,
    pub audio_anchor_sample: u64,
    pub start_sample: u64,
    pub end_sample: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub role: String,
    pub language: String,
    pub season_id: String,
    pub episode_id: String,
    pub template_id: String,
    pub catalog: scene_delivery::FileRef,
    pub template_definition: scene_delivery::FileRef,
    #[serde(default)]
    pub source_template: Option<scene_delivery::FileRef>,
    pub season_bindings: scene_delivery::FileRef,
    pub episode_bindings: scene_delivery::FileRef,
    pub source_binding: String,
    pub source: scene_delivery::FileRef,
    #[serde(default)]
    pub selection_evidence: Option<scene_delivery::FileRef>,
    #[serde(default)]
    pub evidence_hash_pointer: Option<String>,
    #[serde(default)]
    pub source_range: Option<SourceRange>,
    #[serde(default)]
    pub evidence_range_pointer: Option<String>,
    #[serde(default)]
    pub audio_clock_repair: Option<AudioClockRepair>,
    #[serde(default)]
    pub audio_range_origin: Option<AudioRangeOrigin>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Template {
    schema: String,
    template_id: String,
    kind: String,
    width: u64,
    height: u64,
    fps_numerator: u64,
    fps_denominator: u64,
    sample_rate: u32,
    duration_frames: u64,
    #[serde(default)]
    source_template_sha256: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub role: String,
    pub language: String,
    pub season_id: String,
    pub episode_id: String,
    pub template_id: String,
    pub template_definition_sha256: String,
    pub source_template_sha256: Option<String>,
    pub source_binding: String,
    pub source_logical_id: String,
    pub source_cache_uri: String,
    pub source_sha256: String,
    pub source_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_range: Option<SourceRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_range_pointer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_clock_repair: Option<AudioClockRepair>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_range_origin: Option<AudioRangeOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_sample_window: Option<AudioSampleWindow>,
    pub selection_evidence_sha256: Option<String>,
    pub evidence_hash_pointer: Option<String>,
    pub master_sha256: String,
    pub master_bytes: u64,
    pub decoded_picture_sha256: String,
    pub decoded_audio_sha256: String,
    pub frames: u64,
    pub samples: u64,
    pub timestamps_verified: bool,
    pub source_content_matches_lossless_master: bool,
    pub technical_validation_state: String,
    pub creative_review_state: String,
    pub publication: String,
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn range_clocks(range: &SourceRange, definition: &Template) -> Result<(u64, u64, u64)> {
    if range.frame_count == 0 || range.frame_count != definition.duration_frames {
        bail!("source range must cover exactly the selected template duration");
    }
    let end_frame = range
        .start_frame
        .checked_add(range.frame_count)
        .context("source frame range overflow")?;
    let sample = |frame: u64| -> Result<u64> {
        let n = u128::from(frame)
            .checked_mul(u128::from(definition.sample_rate))
            .and_then(|v| v.checked_mul(u128::from(definition.fps_denominator)))
            .context("source sample range overflow")?
            .checked_div(u128::from(definition.fps_numerator))
            .context("invalid source frame clock")?;
        u64::try_from(n).context("source sample range overflow")
    };
    Ok((end_frame, sample(range.start_frame)?, sample(end_frame)?))
}

fn pts_difference(a: &SourcePts, b: &SourcePts) -> Result<(i128, i128)> {
    if a.time_base_numerator == 0
        || b.time_base_numerator == 0
        || a.time_base_denominator == 0
        || b.time_base_denominator == 0
    {
        bail!("invalid decoded source time base");
    }
    let product = |ticks: i64, n: u64, d: u64| {
        i128::from(ticks)
            .checked_mul(i128::from(n))
            .and_then(|v| v.checked_mul(i128::from(d)))
            .context("decoded source PTS overflow")
    };
    let numerator = product(a.ticks, a.time_base_numerator, b.time_base_denominator)?
        .checked_sub(product(
            b.ticks,
            b.time_base_numerator,
            a.time_base_denominator,
        )?)
        .context("decoded source PTS overflow")?;
    let denominator = i128::from(a.time_base_denominator)
        .checked_mul(i128::from(b.time_base_denominator))
        .context("decoded source PTS overflow")?;
    Ok((numerator, denominator))
}

#[cfg(test)]
fn sample_window(
    video: SourcePts,
    audio: SourcePts,
    range: &SourceRange,
    definition: &Template,
) -> Result<AudioSampleWindow> {
    anchored_sample_window(video, audio.clone(), audio, 0, range, definition)
}

fn anchored_sample_window(
    video: SourcePts,
    first_audio: SourcePts,
    audio: SourcePts,
    anchor_sample: u64,
    range: &SourceRange,
    definition: &Template,
) -> Result<AudioSampleWindow> {
    range_clocks(range, definition)?;
    let (n, d) = pts_difference(&video, &audio)?;
    let rate = i128::from(definition.sample_rate);
    let start = n
        .checked_mul(rate)
        .context("decoded sample window overflow")?
        .div_euclid(d);
    let fps = i128::from(definition.fps_numerator);
    let duration = i128::from(range.frame_count)
        .checked_mul(i128::from(definition.fps_denominator))
        .and_then(|v| v.checked_mul(d))
        .context("decoded sample window overflow")?;
    let end_n = n
        .checked_mul(fps)
        .and_then(|v| v.checked_add(duration))
        .and_then(|v| v.checked_mul(rate))
        .context("decoded sample window overflow")?;
    let end_d = d
        .checked_mul(fps)
        .context("decoded sample window overflow")?;
    let end = end_n.div_euclid(end_d);
    let start_sample = u64::try_from(start)
        .context("decoded sample window starts before source audio")?
        .checked_add(anchor_sample)
        .context("decoded sample window overflow")?;
    let end_sample = u64::try_from(end)
        .context("decoded sample window overflow")?
        .checked_add(anchor_sample)
        .context("decoded sample window overflow")?;
    if end_sample <= start_sample {
        bail!("empty decoded sample window");
    }
    Ok(AudioSampleWindow {
        selected_video_pts: video,
        first_audio_pts: first_audio,
        selected_audio_anchor_pts: audio,
        audio_anchor_sample: anchor_sample,
        start_sample,
        end_sample,
    })
}

fn nominal_end_seconds(range: &SourceRange, definition: &Template) -> Result<u128> {
    let (end, _, _) = range_clocks(range, definition)?;
    Ok(u128::from(end)
        .checked_mul(u128::from(definition.fps_denominator))
        .context("source read boundary overflow")?
        .div_ceil(u128::from(definition.fps_numerator)))
}

fn timestamp_tolerance(pts: &SourcePts, denominator: i128, scale: u64) -> Result<u128> {
    let tolerance = (denominator / i128::from(pts.time_base_denominator))
        .checked_mul(i128::from(pts.time_base_numerator))
        .and_then(|v| v.checked_mul(i128::from(scale)))
        .context("source clock tolerance overflow")?;
    Ok(u128::try_from(tolerance)?)
}

fn probe_sample_window(
    source: &Path,
    range: &SourceRange,
    definition: &Template,
) -> Result<AudioSampleWindow> {
    let (end, _, _) = range_clocks(range, definition)?;
    // Probe the complete metadata timeline: a large pre-range reset can move
    // the needed decoded ordinal past a PTS-bounded read. The media decode
    // itself remains bounded after the exact local window is resolved.
    let output = episode_conform::command("ffprobe")
        .args(["-v", "error"])
        .args(["-show_frames", "-show_entries", "frame=media_type,stream_index,best_effort_timestamp,nb_samples:stream=index,codec_type,time_base", "-of", "json"])
        .arg(source).output()?;
    if !output.status.success() {
        bail!("decoded source clock probe failed");
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let streams = value["streams"]
        .as_array()
        .context("missing decoded source streams")?;
    let pts = |kind: &str, ticks: i64| -> Result<SourcePts> {
        let stream = streams
            .iter()
            .find(|s| s["codec_type"] == kind)
            .context("missing source stream")?;
        let time_base = stream["time_base"]
            .as_str()
            .context("missing source time base")?;
        let (n, d) = time_base
            .split_once('/')
            .context("invalid source time base")?;
        Ok(SourcePts {
            ticks,
            time_base_numerator: n.parse()?,
            time_base_denominator: d.parse()?,
        })
    };
    let frames = value["frames"]
        .as_array()
        .context("missing decoded source frames")?;
    let stream_index = |kind: &str| -> Result<u64> {
        streams
            .iter()
            .find(|s| s["codec_type"] == kind)
            .context("missing source stream")?["index"]
            .as_u64()
            .context("missing source stream index")
    };
    let video_index = stream_index("video")?;
    let audio_index = stream_index("audio")?;
    let videos = frames
        .iter()
        .filter(|f| f["stream_index"].as_u64() == Some(video_index))
        .collect::<Vec<_>>();
    let audios = frames
        .iter()
        .filter(|f| f["stream_index"].as_u64() == Some(audio_index))
        .collect::<Vec<_>>();
    let tick = |f: &serde_json::Value| {
        f["best_effort_timestamp"]
            .as_i64()
            .context("missing decoded source PTS")
    };
    let start = usize::try_from(range.start_frame).context("source frame index overflow")?;
    let end = usize::try_from(end).context("source frame index overflow")?;
    let selected = videos
        .get(start..end)
        .context("decoded source range exceeds available video")?;
    let video = pts("video", tick(selected[0])?)?;
    let first_video = pts("video", tick(videos[0])?)?;
    // Reject pre-range gaps too: a missing source frame must not silently
    // change frame-index semantics or invalidate the bounded source read.
    for (i, f) in videos[..end].iter().enumerate() {
        let current = pts("video", tick(f)?)?;
        let (n, d) = pts_difference(&current, &first_video)?;
        let observed = n
            .checked_mul(i128::from(definition.fps_numerator))
            .context("source clock overflow")?;
        let expected = i128::try_from(i)?
            .checked_mul(i128::from(definition.fps_denominator))
            .and_then(|v| v.checked_mul(d))
            .context("source clock overflow")?;
        if observed.abs_diff(expected)
            > timestamp_tolerance(&first_video, d, definition.fps_numerator)?
        {
            bail!("decoded source picture timestamp gap");
        }
    }
    let audio = pts(
        "audio",
        tick(audios.first().context("missing decoded source audio")?)?,
    )?;
    // Map the selected picture clock into decoded sample ordinals locally.
    // Earlier packet clock resets must not become invented sample offsets.
    let mut samples = 0u64;
    let mut anchor = None;
    for (index, f) in audios.iter().enumerate() {
        let current = pts("audio", tick(f)?)?;
        let count = f["nb_samples"]
            .as_u64()
            .filter(|v| *v > 0)
            .context("missing decoded audio sample count")?;
        let (n, d) = pts_difference(&video, &current)?;
        let offset = n
            .checked_mul(i128::from(definition.sample_rate))
            .context("source clock overflow")?;
        let span = i128::from(count)
            .checked_mul(d)
            .context("source clock overflow")?;
        if offset >= 0 && offset < span {
            if anchor.is_some() {
                bail!("ambiguous decoded source audio timestamp overlap");
            }
            anchor = Some((index, current, samples));
        }
        samples = samples
            .checked_add(count)
            .context("decoded source sample overflow")?;
    }
    let (index, anchor_pts, anchor_sample) =
        anchor.context("decoded source audio timestamp gap at selected picture")?;
    let window = anchored_sample_window(
        video,
        audio,
        anchor_pts.clone(),
        anchor_sample,
        range,
        definition,
    )?;
    samples = anchor_sample;
    for f in &audios[index..] {
        if samples >= window.end_sample {
            break;
        }
        let current = pts("audio", tick(f)?)?;
        let (n, d) = pts_difference(&current, &anchor_pts)?;
        let observed = n
            .checked_mul(i128::from(definition.sample_rate))
            .context("source clock overflow")?;
        let expected = i128::from(samples - anchor_sample)
            .checked_mul(d)
            .context("source clock overflow")?;
        if observed.abs_diff(expected)
            > timestamp_tolerance(&anchor_pts, d, u64::from(definition.sample_rate))?
        {
            bail!("decoded source audio timestamp gap");
        }
        let count = f["nb_samples"]
            .as_u64()
            .filter(|v| *v > 0)
            .context("missing decoded audio sample count")?;
        samples = samples
            .checked_add(count)
            .context("decoded source sample overflow")?;
    }
    if samples < window.end_sample {
        bail!("decoded source range exceeds available audio");
    }
    Ok(window)
}

fn resolved_range_filter(
    range: &SourceRange,
    definition: &Template,
    picture: bool,
    window: Option<&AudioSampleWindow>,
) -> Result<String> {
    if !picture {
        if let Some(w) = window {
            return Ok(format!(
                "atrim=start_sample={}:end_sample={},asetpts=PTS-STARTPTS",
                w.start_sample, w.end_sample
            ));
        }
    }
    range_filter(range, definition, picture)
}

fn range_filter(range: &SourceRange, definition: &Template, picture: bool) -> Result<String> {
    let (end_frame, start_sample, end_sample) = range_clocks(range, definition)?;
    Ok(if picture {
        format!(
            "trim=start_frame={}:end_frame={end_frame},setpts=PTS-STARTPTS",
            range.start_frame
        )
    } else {
        format!("atrim=start_sample={start_sample}:end_sample={end_sample},asetpts=PTS-STARTPTS")
    })
}

fn bound_source_read(
    command: &mut std::process::Command,
    range: Option<&SourceRange>,
    definition: &Template,
    window: Option<&AudioSampleWindow>,
) -> Result<()> {
    if let Some(range) = range {
        // Decode from the beginning for exact frame/sample indexing, but do
        // not scan the rest of a feature film after the selected excerpt.
        let mut seconds = nominal_end_seconds(range, definition)?;
        if let Some(w) = window {
            let p = &w.selected_video_pts;
            let n = i128::from(p.ticks)
                .checked_mul(i128::from(p.time_base_numerator))
                .and_then(|v| v.checked_mul(i128::from(definition.fps_numerator)))
                .and_then(|v| {
                    i128::from(range.frame_count)
                        .checked_mul(i128::from(definition.fps_denominator))
                        .and_then(|dt| dt.checked_mul(i128::from(p.time_base_denominator)))
                        .and_then(|dt| v.checked_add(dt))
                })
                .context("source read boundary overflow")?;
            let d = i128::from(p.time_base_denominator)
                .checked_mul(i128::from(definition.fps_numerator))
                .context("source read boundary overflow")?;
            let end = n
                .max(0)
                .checked_add(d - 1)
                .context("source read boundary overflow")?
                / d;
            seconds = seconds.max(u128::try_from(end)?);
            seconds =
                seconds.max(u128::from(w.end_sample).div_ceil(u128::from(definition.sample_rate)));
        }
        let seconds = seconds
            .checked_add(1)
            .context("source read boundary overflow")?;
        command.arg("-t").arg(seconds.to_string());
    }
    Ok(())
}

fn source_digest(
    path: &Path,
    picture: bool,
    range: Option<&SourceRange>,
    definition: &Template,
    window: Option<&AudioSampleWindow>,
) -> Result<(String, u64)> {
    let Some(range) = range else {
        return episode_conform::decoded_digest(path, picture);
    };
    let mut cmd = episode_conform::command("ffmpeg");
    cmd.args(["-v", "error", "-nostdin"]);
    bound_source_read(&mut cmd, Some(range), definition, window)?;
    cmd.arg("-i").arg(path);
    if picture {
        cmd.args(["-map", "0:v:0", "-vf"])
            .arg(resolved_range_filter(range, definition, true, window)?);
        cmd.args([
            "-fps_mode",
            "passthrough",
            "-pix_fmt",
            "yuv444p",
            "-f",
            "rawvideo",
        ]);
    } else {
        cmd.args(["-map", "0:a:0", "-af"])
            .arg(resolved_range_filter(range, definition, false, window)?);
        cmd.args(["-c:a", "pcm_s24le", "-f", "s24le"]);
    }
    let errors = tempfile::tempfile()?;
    let mut child = cmd.arg("-").stdout(Stdio::piped()).stderr(errors).spawn()?;
    let mut pipe = child.stdout.take().context("source range decoder pipe")?;
    let mut digest = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let n = pipe.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
        count = count
            .checked_add(n as u64)
            .context("source range byte count overflow")?;
    }
    if !child.wait()?.success() {
        bail!("source range decode failed");
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

fn source_facts(path: &Path) -> Result<(u64, u64, String, u32)> {
    let output = episode_conform::command("ffprobe")
        .args(["-v", "error", "-show_streams", "-of", "json"])
        .arg(path)
        .output()?;
    if !output.status.success() {
        bail!("FFprobe rejected selected presentation source");
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let streams = value["streams"]
        .as_array()
        .context("source lacks streams")?;
    let video = streams
        .iter()
        .filter(|stream| stream["codec_type"] == "video")
        .collect::<Vec<_>>();
    let audio = streams
        .iter()
        .filter(|stream| stream["codec_type"] == "audio")
        .collect::<Vec<_>>();
    if video.len() != 1 || audio.len() != 1 || audio[0]["channels"] != 2 {
        bail!("presentation source needs one picture and stereo audio stream");
    }
    let v = video[0];
    let a = audio[0];
    Ok((
        v["width"].as_u64().context("source width")?,
        v["height"].as_u64().context("source height")?,
        v["r_frame_rate"]
            .as_str()
            .context("source frame rate")?
            .into(),
        a["sample_rate"]
            .as_str()
            .context("source sample rate")?
            .parse()?,
    ))
}

fn is_native_lossless(path: &Path) -> Result<bool> {
    let output = episode_conform::command("ffprobe")
        .args(["-v", "error", "-show_streams", "-of", "json"])
        .arg(path)
        .output()?;
    if !output.status.success() {
        bail!("FFprobe rejected presentation codecs");
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let streams = value["streams"]
        .as_array()
        .context("missing source streams")?;
    Ok(streams.len() == 2
        && streams.iter().any(|s| {
            s["codec_type"] == "video" && s["codec_name"] == "ffv1" && s["pix_fmt"] == "yuv444p"
        })
        && streams.iter().any(|s| {
            s["codec_type"] == "audio" && s["codec_name"] == "pcm_s24le" && s["channels"] == 2
        }))
}

pub fn build(
    manifest_path: &Path,
    input_root: &Path,
    asset_root: &Path,
    output: &Path,
) -> Result<Receipt> {
    if output.exists() {
        bail!("presentation output must be a new directory");
    }
    let manifest: Manifest = serde_json::from_slice(&fs::read(manifest_path)?)?;
    if manifest.schema != SCHEMA
        || manifest.role.is_empty()
        || !matches!(manifest.language.as_str(), "es" | "en")
        || manifest.season_id.is_empty()
        || manifest.episode_id.is_empty()
        || manifest.template_id.is_empty()
        || manifest.source_binding.is_empty()
    {
        bail!("invalid presentation adoption manifest");
    }
    let read = |item: &scene_delivery::FileRef| -> Result<Vec<u8>> {
        Ok(fs::read(scene_delivery::checked_file(input_root, item)?)?)
    };
    let catalog: TemplateCatalog = serde_json::from_slice(&read(&manifest.catalog)?)?;
    let definition_bytes = read(&manifest.template_definition)?;
    let definition: Template = serde_json::from_slice(&definition_bytes)?;
    match (
        &definition.source_template_sha256,
        &manifest.source_template,
    ) {
        (Some(expected), Some(source)) if *expected == source.sha256 => {
            read(source)?;
        }
        (None, None) => {}
        _ => bail!("presentation source template binding is incomplete or stale"),
    }
    let selected_template = catalog
        .templates
        .iter()
        .find(|template| template.template_id == manifest.template_id)
        .context("presentation template is not selected")?;
    let poem_role_matches = manifest.role == "internal-poem" && definition.kind == "opening-poem";
    if catalog.schema != "reel.scene-template-catalog.v1"
        || definition.schema != "reel.selected-presentation-master-template.v1"
        || definition.template_id != manifest.template_id
        || !(definition.kind == manifest.role || poem_role_matches)
        || !(selected_template.kind == manifest.role
            || (manifest.role == "internal-poem" && selected_template.kind == "opening-poem"))
        || selected_template.definition_sha256 != sha(&definition_bytes)
        || definition.width == 0
        || definition.height == 0
        || definition.fps_numerator == 0
        || definition.fps_denominator == 0
        || definition.sample_rate == 0
        || definition.duration_frames == 0
    {
        bail!("selected presentation template definition mismatch");
    }
    let season: ScopedBindings = serde_json::from_slice(&read(&manifest.season_bindings)?)?;
    let episode: ScopedBindings = serde_json::from_slice(&read(&manifest.episode_bindings)?)?;
    if season.schema != "reel.scene-asset-bindings.v1"
        || episode.schema != "reel.scene-asset-bindings.v1"
        || season.scope_id != manifest.season_id
        || episode.scope_id != manifest.episode_id
    {
        bail!("invalid scoped source bindings");
    }
    let found = [&season, &episode]
        .into_iter()
        .filter_map(|scope| scope.assets.get(&manifest.source_binding))
        .collect::<Vec<_>>();
    if found.len() != 1 {
        bail!("source binding must resolve in exactly one scope");
    }
    let selected = found[0];
    if !matches!(
        selected.selection_state.as_str(),
        "selected-private-production" | "principal-approved" | "release-cleared"
    ) || selected.sha256 != manifest.source.sha256
        || selected.bytes != manifest.source.bytes
        || selected.cache_uri != format!("cache://sha256/{}", selected.sha256)
    {
        bail!("presentation source differs from selected cache binding");
    }
    let selection_evidence_sha256 = match (
        &manifest.selection_evidence,
        &manifest.evidence_hash_pointer,
    ) {
        (Some(evidence), Some(pointer)) => {
            let bytes = read(evidence)?;
            let value: serde_json::Value = serde_json::from_slice(&bytes)?;
            let selected_hash = value
                .pointer(pointer)
                .and_then(serde_json::Value::as_str)
                .context("selection evidence pointer must name a hash or cache URI")?;
            let selected_hash = selected_hash
                .strip_prefix("cache://sha256/")
                .unwrap_or(selected_hash);
            if selected_hash != selected.sha256 {
                bail!("selection evidence differs from selected source binding");
            }
            match (&manifest.source_range, &manifest.evidence_range_pointer) {
                (Some(range), Some(pointer)) => {
                    let evidenced: SourceRange = serde_json::from_value(
                        value
                            .pointer(pointer)
                            .context("selection evidence lacks source range")?
                            .clone(),
                    )?;
                    if evidenced != *range {
                        bail!("source range differs from selected evidence");
                    }
                    range_clocks(range, &definition)?;
                }
                (None, None) => {}
                _ => bail!("source range and its evidence pointer must be supplied together"),
            }
            if let Some(repair) = &manifest.audio_clock_repair {
                if manifest.source_range.is_none()
                    || repair.policy != "decoded-sample-count"
                    || value
                        .pointer(&repair.evidence_pointer)
                        .and_then(|v| v.as_str())
                        != Some(repair.policy.as_str())
                {
                    bail!(
                        "audio clock repair requires a source range and exact selected policy evidence"
                    );
                }
            }
            if let Some(origin) = &manifest.audio_range_origin {
                if manifest.source_range.is_none()
                    || origin.policy != "selected-decoded-video-pts"
                    || value
                        .pointer(&origin.evidence_pointer)
                        .and_then(|v| v.as_str())
                        != Some(origin.policy.as_str())
                {
                    bail!(
                        "audio range origin requires a source range and exact selected policy evidence"
                    );
                }
            }
            Some(evidence.sha256.clone())
        }
        (None, None)
            if manifest.source_range.is_none()
                && manifest.evidence_range_pointer.is_none()
                && manifest.audio_clock_repair.is_none()
                && manifest.audio_range_origin.is_none() =>
        {
            None
        }
        _ => bail!("selection evidence and hash pointer must be supplied together"),
    };
    let source = scene_delivery::checked_file(asset_root, &manifest.source)?;
    let (width, height, fps, sample_rate) = source_facts(&source)?;
    let expected_fps = format!(
        "{}/{}",
        definition.fps_numerator, definition.fps_denominator
    );
    if (width, height, fps.as_str(), sample_rate)
        != (
            definition.width,
            definition.height,
            expected_fps.as_str(),
            definition.sample_rate,
        )
    {
        bail!("presentation source differs from selected template media policy");
    }
    let audio_sample_window = if manifest.audio_range_origin.is_some() {
        Some(probe_sample_window(
            &source,
            manifest
                .source_range
                .as_ref()
                .context("audio range origin lacks source range")?,
            &definition,
        )?)
    } else {
        None
    };
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".presentation-adopt-")
        .tempdir_in(parent)?;
    let master = stage.path().join("master.mkv");
    if manifest.source_range.is_none() && is_native_lossless(&source)? {
        fs::copy(&source, &master)?;
    } else {
        let mut command = episode_conform::command("ffmpeg");
        command.args(["-v", "error", "-nostdin"]);
        bound_source_read(
            &mut command,
            manifest.source_range.as_ref(),
            &definition,
            audio_sample_window.as_ref(),
        )?;
        command.arg("-i").arg(&source);
        if let Some(range) = &manifest.source_range {
            command.arg("-vf").arg(resolved_range_filter(
                range,
                &definition,
                true,
                audio_sample_window.as_ref(),
            )?);
            command
                .arg("-af")
                .arg(if manifest.audio_clock_repair.is_some() {
                    let (_, mut start, mut end) = range_clocks(range, &definition)?;
                    if let Some(w) = &audio_sample_window {
                        start = w.start_sample;
                        end = w.end_sample;
                    }
                    format!("atrim=start_sample={start}:end_sample={end},asetpts=N/SR/TB")
                } else {
                    resolved_range_filter(range, &definition, false, audio_sample_window.as_ref())?
                });
        }
        let status = command
            .args([
                "-map",
                "0:v:0",
                "-map",
                "0:a:0",
                "-fps_mode",
                "passthrough",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuv444p",
                "-c:a",
                "pcm_s24le",
                "-ac",
                "2",
                "-ar",
            ])
            .arg(definition.sample_rate.to_string())
            .arg(&master)
            .stdout(Stdio::null())
            .status()?;
        if !status.success() {
            bail!("presentation lossless conversion failed");
        }
    }
    let facts = episode_conform::probe(&master)?;
    if (
        facts.width,
        facts.height,
        facts.fps.as_str(),
        facts.sample_rate,
    ) != (
        definition.width,
        definition.height,
        expected_fps.as_str(),
        definition.sample_rate,
    ) {
        bail!("converted presentation media policy mismatch");
    }
    let (source_picture_sha, source_picture_bytes) = source_digest(
        &source,
        true,
        manifest.source_range.as_ref(),
        &definition,
        audio_sample_window.as_ref(),
    )?;
    let (source_audio_sha, source_audio_bytes) = source_digest(
        &source,
        false,
        manifest.source_range.as_ref(),
        &definition,
        audio_sample_window.as_ref(),
    )?;
    let (picture_sha, picture_bytes) = episode_conform::decoded_digest(&master, true)?;
    let (audio_sha, audio_bytes) = episode_conform::decoded_digest(&master, false)?;
    let frame_bytes = definition.width * definition.height * 3;
    if picture_sha != source_picture_sha
        || picture_bytes != source_picture_bytes
        || audio_sha != source_audio_sha
        || audio_bytes != source_audio_bytes
        || picture_bytes % frame_bytes != 0
        || audio_bytes % 6 != 0
    {
        bail!("lossless presentation changed decoded source picture or audio");
    }
    let frames = picture_bytes / frame_bytes;
    let samples = audio_bytes / 6;
    if let Some(range) = &manifest.source_range {
        let (_, mut start, mut end) = range_clocks(range, &definition)?;
        if let Some(w) = &audio_sample_window {
            start = w.start_sample;
            end = w.end_sample;
        }
        if frames != range.frame_count || samples != end - start {
            bail!("source range extends beyond available picture or audio");
        }
    }
    let expected_samples = (u128::from(definition.duration_frames)
        * u128::from(definition.sample_rate)
        * u128::from(definition.fps_denominator))
        / u128::from(definition.fps_numerator);
    if frames != definition.duration_frames
        || u128::from(samples).abs_diff(expected_samples)
            > u128::from(definition.sample_rate) * u128::from(definition.fps_denominator)
                / u128::from(definition.fps_numerator)
    {
        bail!("presentation duration differs from selected template");
    }
    episode_delivery::verify_timestamps(
        &master,
        definition.sample_rate,
        definition.fps_numerator,
        definition.fps_denominator,
    )?;
    let receipt = Receipt {
        schema: "reel.presentation-master-receipt.v1".into(),
        role: manifest.role,
        language: manifest.language,
        season_id: manifest.season_id,
        episode_id: manifest.episode_id,
        template_id: manifest.template_id,
        template_definition_sha256: sha(&definition_bytes),
        source_template_sha256: definition.source_template_sha256,
        source_binding: manifest.source_binding,
        source_logical_id: selected.logical_id.clone(),
        source_cache_uri: selected.cache_uri.clone(),
        source_sha256: selected.sha256.clone(),
        source_bytes: selected.bytes,
        source_range: manifest.source_range,
        evidence_range_pointer: manifest.evidence_range_pointer,
        audio_clock_repair: manifest.audio_clock_repair,
        audio_range_origin: manifest.audio_range_origin,
        audio_sample_window,
        selection_evidence_sha256,
        evidence_hash_pointer: manifest.evidence_hash_pointer,
        master_sha256: episode_conform::file_sha(&master)?,
        master_bytes: fs::metadata(&master)?.len(),
        decoded_picture_sha256: picture_sha,
        decoded_audio_sha256: audio_sha,
        frames,
        samples,
        timestamps_verified: true,
        source_content_matches_lossless_master: true,
        technical_validation_state: "decoded-source-equivalent".into(),
        creative_review_state: "open; existing source selection does not grant creative approval"
            .into(),
        publication: "not-authorized".into(),
    };
    fs::write(
        stage.path().join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    fs::rename(stage.path(), output)?;
    Ok(receipt)
}

/// Independently rerun the selected adoption and compare decoded content with
/// an existing master. This intentionally redoes the full source conversion;
/// callers can cache the checked result at a higher build-graph layer.
pub fn check(
    manifest_path: &Path,
    input_root: &Path,
    asset_root: &Path,
    master_path: &Path,
    receipt_path: &Path,
    stage_parent: &Path,
) -> Result<()> {
    let selected: Receipt = serde_json::from_slice(&fs::read(receipt_path)?)?;
    if selected.schema != "reel.presentation-master-receipt.v1"
        || selected.technical_validation_state != "decoded-source-equivalent"
        || !selected.timestamps_verified
        || !selected.source_content_matches_lossless_master
        || selected.master_sha256 != episode_conform::file_sha(master_path)?
        || selected.master_bytes != fs::metadata(master_path)?.len()
    {
        bail!("selected presentation adoption receipt or master is invalid");
    }
    fs::create_dir_all(stage_parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".presentation-recheck-")
        .tempdir_in(stage_parent)?;
    let rebuilt = build(
        manifest_path,
        input_root,
        asset_root,
        &stage.path().join("rebuilt"),
    )?;
    let mut selected_value = serde_json::to_value(&selected)?;
    let mut rebuilt_value = serde_json::to_value(&rebuilt)?;
    for value in [&mut selected_value, &mut rebuilt_value] {
        value
            .as_object_mut()
            .context("adoption receipt is not an object")?
            .remove("master_sha256");
        value
            .as_object_mut()
            .context("adoption receipt is not an object")?
            .remove("master_bytes");
    }
    if selected_value != rebuilt_value {
        bail!("selected presentation adoption differs from current source selection");
    }
    let manifest: Manifest = serde_json::from_slice(&fs::read(manifest_path)?)?;
    let definition: Template = serde_json::from_slice(&fs::read(scene_delivery::checked_file(
        input_root,
        &manifest.template_definition,
    )?)?)?;
    let facts = episode_conform::probe(master_path)?;
    if (
        facts.width,
        facts.height,
        facts.fps.as_str(),
        facts.sample_rate,
    ) != (
        definition.width,
        definition.height,
        format!(
            "{}/{}",
            definition.fps_numerator, definition.fps_denominator
        )
        .as_str(),
        definition.sample_rate,
    ) {
        bail!("selected presentation master differs from adopted media policy");
    }
    episode_delivery::verify_timestamps(
        master_path,
        definition.sample_rate,
        definition.fps_numerator,
        definition.fps_denominator,
    )?;
    let (picture_sha, picture_bytes) = episode_conform::decoded_digest(master_path, true)?;
    let (audio_sha, audio_bytes) = episode_conform::decoded_digest(master_path, false)?;
    if picture_sha != rebuilt.decoded_picture_sha256
        || audio_sha != rebuilt.decoded_audio_sha256
        || picture_bytes == 0
        || audio_bytes == 0
    {
        bail!("selected presentation master differs from rechecked source content");
    }
    Ok(())
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct VerifiedFile {
    role: String,
    sha256: String,
    bytes: u64,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct VerificationKey {
    schema: String,
    files: Vec<VerifiedFile>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct VerificationRecord {
    schema: String,
    key: VerificationKey,
    full_check_passed: bool,
}

fn verified_file(role: &str, path: &Path) -> Result<VerifiedFile> {
    Ok(VerifiedFile {
        role: role.into(),
        sha256: episode_conform::file_sha(path)?,
        bytes: fs::metadata(path)?.len(),
    })
}

fn verification_key(
    manifest_path: &Path,
    input_root: &Path,
    asset_root: &Path,
    master_path: &Path,
    receipt_path: &Path,
) -> Result<VerificationKey> {
    let manifest: Manifest = serde_json::from_slice(&fs::read(manifest_path)?)?;
    let mut files = vec![
        verified_file("adoption-manifest", manifest_path)?,
        verified_file("selected-master", master_path)?,
        verified_file("selected-receipt", receipt_path)?,
    ];
    for (role, item) in [
        ("catalog", Some(&manifest.catalog)),
        ("template-definition", Some(&manifest.template_definition)),
        ("source-template", manifest.source_template.as_ref()),
        ("season-bindings", Some(&manifest.season_bindings)),
        ("episode-bindings", Some(&manifest.episode_bindings)),
        ("selection-evidence", manifest.selection_evidence.as_ref()),
    ] {
        if let Some(item) = item {
            scene_delivery::checked_file(input_root, item)?;
            files.push(VerifiedFile {
                role: role.into(),
                sha256: item.sha256.clone(),
                bytes: item.bytes,
            });
        }
    }
    scene_delivery::checked_file(asset_root, &manifest.source)?;
    files.push(VerifiedFile {
        role: "selected-source".into(),
        sha256: manifest.source.sha256.clone(),
        bytes: manifest.source.bytes,
    });
    files.extend([
        verified_file("validator", &std::env::current_exe()?)?,
        verified_file("ffmpeg", &episode_conform::media_tool_path("ffmpeg")?)?,
        verified_file("ffprobe", &episode_conform::media_tool_path("ffprobe")?)?,
    ]);
    Ok(VerificationKey {
        schema: "reel.presentation-adoption-verification-key.v1".into(),
        files,
    })
}

fn cache_hit(path: &Path, key: &VerificationKey) -> bool {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<VerificationRecord>(&bytes).ok())
        .is_some_and(|record| {
            record.schema == "reel.presentation-adoption-verification.v1"
                && record.full_check_passed
                && record.key == *key
        })
}

/// Reuse a successful full adoption check only after rehashing every current
/// dependency, selected output/receipt, validator, and both media tools.
/// Returns true for a verified cache hit. The cache is local build evidence,
/// never creative approval; callers must still verify the new episode output.
pub fn check_cached(
    manifest_path: &Path,
    input_root: &Path,
    asset_root: &Path,
    master_path: &Path,
    receipt_path: &Path,
    stage_parent: &Path,
    cache_root: &Path,
) -> Result<bool> {
    let key = verification_key(
        manifest_path,
        input_root,
        asset_root,
        master_path,
        receipt_path,
    )?;
    let digest = sha(&serde_json::to_vec(&key)?);
    let cache_path = cache_root.join(format!("{digest}.json"));
    if cache_hit(&cache_path, &key) {
        return Ok(true);
    }
    check(
        manifest_path,
        input_root,
        asset_root,
        master_path,
        receipt_path,
        stage_parent,
    )?;
    if verification_key(
        manifest_path,
        input_root,
        asset_root,
        master_path,
        receipt_path,
    )? != key
    {
        bail!("adoption dependencies changed during verification");
    }
    fs::create_dir_all(cache_root)?;
    let mut staged = tempfile::NamedTempFile::new_in(cache_root)?;
    staged.write_all(&serde_json::to_vec_pretty(&VerificationRecord {
        schema: "reel.presentation-adoption-verification.v1".into(),
        key,
        full_check_passed: true,
    })?)?;
    staged.as_file().sync_all()?;
    staged.persist(&cache_path)?;
    Ok(false)
}

#[cfg(test)]
mod verification_cache_tests {
    use super::*;

    #[test]
    fn decoded_pts_window_preserves_phase_and_fractional_sample_carry() {
        let mut definition: Template = serde_json::from_value(serde_json::json!({
            "schema":"reel.selected-presentation-master-template.v1", "template_id":"test",
            "kind":"narrative-scene", "width":64, "height":64,
            "fps_numerator":24, "fps_denominator":1, "sample_rate":48000, "duration_frames":1961
        }))
        .unwrap();
        let video = SourcePts {
            ticks: 3483 * 512 + 262,
            time_base_numerator: 1,
            time_base_denominator: 12288,
        };
        let audio = SourcePts {
            ticks: 0,
            time_base_numerator: 1,
            time_base_denominator: 48000,
        };
        let range = SourceRange {
            start_frame: 3483,
            frame_count: 1961,
        };
        let w = sample_window(video.clone(), audio.clone(), &range, &definition).unwrap();
        assert_eq!((w.start_sample, w.end_sample), (6_967_023, 10_889_023));
        assert_ne!(w.start_sample, range_clocks(&range, &definition).unwrap().1);
        let anchor = SourcePts {
            ticks: 6_966_272,
            ..audio.clone()
        };
        let local = anchored_sample_window(
            video.clone(),
            audio.clone(),
            anchor,
            6_967_296,
            &range,
            &definition,
        )
        .unwrap();
        assert_eq!(
            (local.start_sample, local.end_sample),
            (6_968_047, 10_890_047)
        );
        definition.fps_numerator = 3;
        definition.sample_rate = 10;
        definition.duration_frames = 1;
        let range = SourceRange {
            start_frame: 0,
            frame_count: 1,
        };
        let fractional = SourcePts {
            ticks: 2,
            time_base_numerator: 1,
            time_base_denominator: 30,
        };
        let w = sample_window(fractional, audio.clone(), &range, &definition).unwrap();
        assert_eq!((w.start_sample, w.end_sample), (0, 4)); // floor each term separately would lose one sample.
        for bad in [
            SourcePts {
                ticks: -1,
                ..video.clone()
            },
            SourcePts {
                time_base_denominator: 0,
                ..video.clone()
            },
            SourcePts {
                ticks: i64::MAX,
                time_base_numerator: u64::MAX,
                time_base_denominator: u64::MAX,
            },
        ] {
            assert!(sample_window(bad, audio.clone(), &range, &definition).is_err());
        }
    }

    #[test]
    fn excerpt_clocks_reject_empty_overflow_and_wrong_duration() {
        let definition: Template = serde_json::from_value(serde_json::json!({
            "schema":"reel.selected-presentation-master-template.v1", "template_id":"test",
            "kind":"opening-poem", "width":64, "height":64,
            "fps_numerator":24, "fps_denominator":1, "sample_rate":44100, "duration_frames":1
        }))
        .unwrap();
        assert_eq!(
            range_clocks(
                &SourceRange {
                    start_frame: 1,
                    frame_count: 1
                },
                &definition
            )
            .unwrap(),
            (2, 1837, 3675)
        );
        for range in [
            SourceRange {
                start_frame: 0,
                frame_count: 0,
            },
            SourceRange {
                start_frame: 0,
                frame_count: 2,
            },
            SourceRange {
                start_frame: u64::MAX,
                frame_count: 1,
            },
        ] {
            assert!(range_clocks(&range, &definition).is_err());
        }
    }

    #[test]
    fn only_complete_exact_success_records_can_be_reused() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cache.json");
        let key = VerificationKey {
            schema: "reel.presentation-adoption-verification-key.v1".into(),
            files: [
                "validator",
                "ffmpeg",
                "ffprobe",
                "adoption-manifest",
                "catalog",
                "template-definition",
                "source-template",
                "season-bindings",
                "episode-bindings",
                "selection-evidence",
                "selected-source",
                "selected-master",
                "selected-receipt",
            ]
            .map(|role| VerifiedFile {
                role: role.into(),
                sha256: "a".repeat(64),
                bytes: 1,
            })
            .into(),
        };
        assert!(!cache_hit(&path, &key));
        let mut record = serde_json::to_value(VerificationRecord {
            schema: "reel.presentation-adoption-verification.v1".into(),
            key: serde_json::from_value(serde_json::to_value(&key).unwrap()).unwrap(),
            full_check_passed: true,
        })
        .unwrap();
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(cache_hit(&path, &key));
        for index in 0..key.files.len() {
            record["key"]["files"][index]["sha256"] = "b".repeat(64).into();
            fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
            assert!(!cache_hit(&path, &key));
            record["key"]["files"][index]["sha256"] = "a".repeat(64).into();
            record["key"]["files"][index]["bytes"] = 2.into();
            fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
            assert!(!cache_hit(&path, &key));
            record["key"]["files"][index]["bytes"] = 1.into();
        }
        record["schema"] = "stale-version".into();
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(!cache_hit(&path, &key));
        record["schema"] = "reel.presentation-adoption-verification.v1".into();
        record["full_check_passed"] = false.into();
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(!cache_hit(&path, &key));
        fs::write(&path, b"partial{").unwrap();
        assert!(!cache_hit(&path, &key));
    }
}
