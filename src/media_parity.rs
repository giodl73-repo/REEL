//! Exact native SDR content comparison through the existing input-provider port.
//! Containers and codecs may differ. This does not select or approve either file.
use crate::scene_delivery::{self, FileRef, InputFiles, MediaProgram};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{ChildStdout, Command, Stdio},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Shape {
    pub width: u32,
    pub height: u32,
    pub fps_numerator: u64,
    pub fps_denominator: u64,
    pub frames: u64,
    pub sample_rate: u32,
    pub samples: u64,
}
#[derive(Debug, Serialize)]
pub struct Digests {
    pub yuv444p_frame_sequence_sha256: String,
    pub rgb24_frame_sequence_sha256: String,
    pub pcm24_sha256: String,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub shape: Shape,
    pub reference: Digests,
    pub candidate: Digests,
    pub picture_planes_equal: bool,
    pub displayed_rgb_equal: bool,
    pub color_metadata_equal: bool,
    pub pcm_equal: bool,
    pub native_clock_checked: bool,
    pub equal: bool,
    pub approval_transferred: bool,
}
fn ratio(value: &str) -> Result<(u64, u64)> {
    let (n, d) = value.split_once('/').context("missing native rational")?;
    let pair = (n.parse()?, d.parse()?);
    if pair.0 == 0 || pair.1 == 0 {
        bail!("zero native rational");
    }
    Ok(pair)
}
fn mul(a: u128, b: u128) -> Result<u128> {
    a.checked_mul(b)
        .context("native comparison size/clock overflow")
}
fn rounded(n: u128, d: u128) -> Result<u128> {
    n.checked_add(d / 2)
        .context("native comparison clock overflow")
        .map(|n| n / d)
}
fn video_bytes(shape: &Shape) -> Result<u64> {
    if shape.width == 0
        || shape.height == 0
        || shape.frames == 0
        || shape.samples == 0
        || shape.sample_rate == 0
        || shape.fps_numerator == 0
        || shape.fps_denominator == 0
    {
        bail!("empty native comparison shape");
    }
    u64::from(shape.width)
        .checked_mul(u64::from(shape.height))
        .and_then(|v| v.checked_mul(3))
        .context("frame size overflow")
}
fn facts(path: &Path, shape: &Shape, tools: &(impl InputFiles + ?Sized)) -> Result<Value> {
    let probe = scene_delivery::probe_with_inputs(tools, path)?;
    let streams = probe["streams"]
        .as_array()
        .context("comparison streams missing")?;
    let v = streams
        .iter()
        .find(|v| v["codec_type"] == "video")
        .context("comparison video missing")?;
    let a = streams
        .iter()
        .find(|v| v["codec_type"] == "audio")
        .context("comparison audio missing")?;
    let (n, d) = ratio(
        v["r_frame_rate"]
            .as_str()
            .context("comparison rate missing")?,
    )?;
    if streams.len() != 2
        || v["width"] != shape.width
        || v["height"] != shape.height
        || mul(n.into(), shape.fps_denominator.into())?
            != mul(d.into(), shape.fps_numerator.into())?
        || a["channels"] != 2
        || a["sample_rate"].as_str() != Some(&shape.sample_rate.to_string())
        || v["start_pts"].as_i64() != Some(0)
        || a["start_pts"].as_i64() != Some(0)
        || v["sample_aspect_ratio"] != "1:1"
    {
        bail!("comparison media differs from native stream/clock shape");
    }
    if !["yuv444p", "yuv422p", "yuv420p", "rgb24", "bgr24", "gbrp"]
        .contains(&v["pix_fmt"].as_str().unwrap_or(""))
        || ["smpte2084", "arib-std-b67"].contains(&v["color_transfer"].as_str().unwrap_or(""))
    {
        bail!("comparison requires admitted 8-bit SDR picture; no HDR/depth reduction");
    }
    // Decoder PCM24 must not silently reduce a higher-depth master.
    let raw_depth = a["bits_per_raw_sample"]
        .as_str()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    let depth = u64::from(raw_depth).max(a["bits_per_sample"].as_u64().unwrap_or(0));
    let integer = match a["sample_fmt"].as_str() {
        Some("u8" | "u8p") => depth <= 8,
        Some("s16" | "s16p") => depth <= 16,
        Some("s32" | "s32p") => (1..=24).contains(&depth),
        _ => false,
    };
    if !integer {
        bail!("comparison requires integer audio at most PCM24 precision");
    }
    Ok(v.clone())
}

fn with_framehash<T>(
    program: &Path,
    path: &Path,
    options: &[&str],
    inspect: impl FnOnce(&mut BufReader<ChildStdout>) -> Result<T>,
) -> Result<T> {
    let mut child = Command::new(program)
        .args([
            "-v",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "file,pipe",
            "-i",
        ])
        .arg(path)
        .args(options)
        .args(["-f", "framehash", "-hash", "sha256", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut reader = BufReader::new(child.stdout.take().context("framehash pipe missing")?);
    let result = inspect(&mut reader);
    drop(reader);
    if result.is_err() {
        let _ = child.kill();
    }
    let success = child.wait()?.success();
    let value = result?;
    if !success {
        bail!("comparison media decode failed");
    }
    Ok(value)
}
fn hash_line(reader: &mut impl BufRead) -> Result<Option<String>> {
    let mut line = Vec::new();
    if reader.by_ref().take(1025).read_until(b'\n', &mut line)? == 0 {
        return Ok(None);
    }
    if line.len() > 1024 {
        bail!("framehash record exceeds bound");
    }
    Ok(Some(std::str::from_utf8(&line)?.trim().into()))
}
/// Framehash emits bounded metadata, not raw frames. The source demuxer clock
/// remains in force; passthrough does not repair or interpolate timestamps.
fn picture_digest(program: &Path, path: &Path, shape: &Shape, format: &str) -> Result<String> {
    with_framehash(
        program,
        path,
        &[
            "-map",
            "0:v:0",
            "-fps_mode",
            "passthrough",
            "-enc_time_base:v",
            "demux",
            "-pix_fmt",
            format,
        ],
        |reader| {
            let mut time_base = None;
            let mut index = 0u64;
            let frame_bytes = video_bytes(shape)?;
            let mut hash = Sha256::new();
            while let Some(line) = hash_line(reader)? {
                if let Some(value) = line.strip_prefix("#tb 0: ") {
                    if time_base.replace(ratio(value)?).is_some() {
                        bail!("duplicate framehash clock");
                    }
                    continue;
                }
                if line.starts_with('#') || line.is_empty() {
                    continue;
                }
                let fields = line.split(',').map(str::trim).collect::<Vec<_>>();
                if fields.len() != 6 || fields[0] != "0" || index >= shape.frames {
                    bail!("unexpected framehash record/count");
                }
                let (tn, td) = time_base.context("framehash native clock missing")?;
                let pts: u128 = fields[2].parse()?;
                let expected = rounded(
                    mul(mul(index.into(), shape.fps_denominator.into())?, td.into())?,
                    mul(shape.fps_numerator.into(), tn.into())?,
                )?;
                let duration_n = mul(shape.fps_denominator.into(), td.into())?;
                let duration_d = mul(shape.fps_numerator.into(), tn.into())?;
                let duration_floor = duration_n / duration_d;
                let duration_ceil = duration_floor + u128::from(duration_n % duration_d != 0);
                let duration: u128 = fields[3].parse()?;
                if pts != expected
                    || fields[1].parse::<u128>()? != pts
                    || duration_floor == 0
                    || duration < duration_floor
                    || duration > duration_ceil
                    || fields[4].parse::<u64>()? != frame_bytes
                    || fields[5].len() != 64
                    || !fields[5]
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    bail!(
                        "decoded frame differs from native clock/size/hash contract at frame {index}: PTS {pts}, expected {expected}; duration {duration}, quantized interval {duration_floor}..{duration_ceil}"
                    );
                }
                hash.update(fields[5].as_bytes());
                hash.update(b"\n");
                index += 1;
            }
            if index != shape.frames {
                bail!("decoded frame count differs from native shape");
            }
            Ok(hash
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect())
        },
    )
}
fn audio_clock(program: &Path, path: &Path, shape: &Shape) -> Result<()> {
    with_framehash(
        program,
        path,
        &["-map", "0:a:0", "-c:a", "pcm_s24le"],
        |reader| {
            let mut time_base = None;
            let mut samples = 0u64;
            while let Some(line) = hash_line(reader)? {
                if let Some(value) = line.strip_prefix("#tb 0: ") {
                    if time_base.replace(ratio(value)?).is_some() {
                        bail!("duplicate audio clock");
                    }
                    continue;
                }
                if line.starts_with('#') || line.is_empty() {
                    continue;
                }
                let fields = line.split(',').map(str::trim).collect::<Vec<_>>();
                if fields.len() != 6 || fields[0] != "0" {
                    bail!("unexpected audio framehash record");
                }
                let (n, d) = time_base.context("audio clock missing")?;
                let duration = fields[3].parse::<u64>()?;
                if mul(n.into(), shape.sample_rate.into())? != u128::from(d)
                    || duration == 0
                    || fields[1].parse::<u64>()? != samples
                    || fields[2].parse::<u64>()? != samples
                    || fields[4].parse::<u64>()?
                        != duration
                            .checked_mul(6)
                            .context("audio packet size overflow")?
                {
                    bail!("decoded audio differs from contiguous native sample clock");
                }
                samples = samples
                    .checked_add(duration)
                    .context("audio clock overflow")?;
                if samples > shape.samples {
                    bail!("extra native audio samples");
                }
            }
            if samples != shape.samples {
                bail!("native audio sample count differs");
            }
            Ok(())
        },
    )
}
fn digest(program: &Path, path: &Path, shape: &Shape) -> Result<Digests> {
    audio_clock(program, path, shape)?;
    let audio_bytes = shape
        .samples
        .checked_mul(6)
        .context("PCM comparison size overflow")?;
    Ok(Digests {
        yuv444p_frame_sequence_sha256: picture_digest(program, path, shape, "yuv444p")?,
        rgb24_frame_sequence_sha256: picture_digest(program, path, shape, "rgb24")?,
        pcm24_sha256: crate::episode_delivery::decode_segments_with_program(
            program,
            path,
            false,
            &[audio_bytes],
        )?
        .remove(0),
    })
}
/// Both providers stay borrowed through all probes and synchronous decoders.
/// A provider must enforce exact byte pins and its declared protection lifetime.
/// Local Path providers preserve existing local checks; they add no custody or
/// mutation protection. No source/output role becomes a production dependency.
/// The candidate provider selects validation tools for both files; its explicit
/// refusal never falls back to PATH. This is separate from producer admission.
pub fn compare_with_inputs(
    reference: &FileRef,
    reference_inputs: &(impl InputFiles + ?Sized),
    candidate: &FileRef,
    candidate_inputs: &(impl InputFiles + ?Sized),
    shape: &Shape,
) -> Result<Report> {
    video_bytes(shape)?;
    let program = candidate_inputs.media_program(MediaProgram::Ffmpeg)?;
    let reference_path = reference_inputs.verified_path(reference)?;
    let candidate_path = candidate_inputs.verified_path(candidate)?;
    // Both stream shapes are checked before expensive decoding.
    let reference_facts = facts(&reference_path, shape, candidate_inputs)?;
    let candidate_facts = facts(&candidate_path, shape, candidate_inputs)?;
    let color_metadata_equal = [
        "color_range",
        "color_space",
        "color_transfer",
        "color_primaries",
        "chroma_location",
    ]
    .iter()
    .all(|key| reference_facts[key] == candidate_facts[key]);
    let reference = digest(&program, &reference_path, shape)?;
    let candidate = digest(&program, &candidate_path, shape)?;
    let picture_planes_equal =
        reference.yuv444p_frame_sequence_sha256 == candidate.yuv444p_frame_sequence_sha256;
    let displayed_rgb_equal = color_metadata_equal
        && reference.rgb24_frame_sequence_sha256 == candidate.rgb24_frame_sequence_sha256;
    let pcm_equal = reference.pcm24_sha256 == candidate.pcm24_sha256;
    Ok(Report {
        schema: "reel.native-media-parity.v1",
        shape: shape.clone(),
        reference,
        candidate,
        picture_planes_equal,
        displayed_rgb_equal,
        color_metadata_equal,
        pcm_equal,
        native_clock_checked: true,
        equal: picture_planes_equal && displayed_rgb_equal && pcm_equal,
        approval_transferred: false,
    })
}
