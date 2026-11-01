//! Independent retained-master comparison using current native picture/mixer logic.
//! No old production receipt, staged movie file or cache publication required.
use crate::scene_delivery::{
    self, ExternalLayerRenderMode, FileRef, InputFiles, Job, MediaProgram, Plan,
};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
mod pipeline;

pub struct Budget {
    pub max_staged_bytes: u64,
    pub timeout_seconds: u64,
    pub max_picture_processes: u16,
}
/// Technical observation only. The owning caller retains source/runtime/byte
/// guards and separately establishes job ownership, approval and custody.
pub struct PrimaryReference {
    job: FileRef,
    master: FileRef,
    frames: u64,
    samples: u64,
    yuv: String,
    rgb: String,
    pcm: String,
    staged: u64,
    reservation: u64,
    picture_processes: usize,
}
impl PrimaryReference {
    /// Exact decoded frame and PCM-sample totals checked by this comparison.
    pub fn counts(&self) -> (u64, u64) {
        (self.frames, self.samples)
    }
    pub fn declaration(&self) -> Value {
        json!({"schema":"reel.scene-primary-reference.experimental-1","job":self.job,"master":self.master,
            "frames":self.frames,"samples":self.samples,"yuv_sha256":self.yuv,"rgb_sha256":self.rgb,"pcm_sha256":self.pcm,
            "staged_bytes":self.staged,"staging_reservation_bytes":self.reservation,"movie_files_written":0,
            "picture_pipeline_processes":self.picture_processes,
            "original_producer_assertion":false,"approval":false,"complete_environment":false,"independent_custody_verified":false,"authorizes_eviction":false})
    }
}
fn command(inputs: &dyn InputFiles, program: MediaProgram, root: &Path) -> Result<Command> {
    let mut command = Command::new(inputs.media_program(program)?);
    command.current_dir(root).args(["-v", "error"]);
    if program == MediaProgram::Ffmpeg {
        command.args([
            "-hide_banner",
            "-nostdin",
            "-threads",
            "1",
            "-filter_complex_threads",
            "1",
            "-filter_threads",
            "1",
        ]);
    }
    Ok(command)
}
fn protocol(command: &mut Command) {
    command.args(["-protocol_whitelist", "file,pipe"]);
}
fn encode(command: &mut Command, job: &Job) {
    command
        .args(scene_delivery::lossless_video_encoding(
            job.still_sequence_encoding.as_deref(),
        ))
        .args(["-threads:v", "1"])
        .args(["-pix_fmt", "yuv444p", "-f", "matroska", "pipe:1"]);
}
fn picture_pipeline(
    job: &Job,
    plan: &Plan,
    inputs: &dyn InputFiles,
    root: &Path,
) -> Result<(Vec<Command>, Vec<tempfile::NamedTempFile>)> {
    let args = scene_delivery::picture_filter_command(job, plan, inputs)?;
    let mut first = command(inputs, MediaProgram::Ffmpeg, root)?;
    for arg in args {
        if arg == "-i" {
            protocol(&mut first);
            first.args(["-threads", "1"]);
        }
        first.arg(arg);
    }
    first.args([
        "-r",
        &format!("{}/{}", plan.fps_numerator, plan.fps_denominator),
    ]);
    encode(&mut first, job);
    let mut commands = vec![first];
    let mut sequence_controls = vec![];
    for (index, layer) in job
        .external_layers
        .iter()
        .filter(|layer| {
            matches!(
                layer.render_mode,
                ExternalLayerRenderMode::AssOverlay
                    | ExternalLayerRenderMode::TimedVideoOverlay
                    | ExternalLayerRenderMode::TimedStillOverlay
                    | ExternalLayerRenderMode::TimedPngSequenceOverlay
            )
        })
        .enumerate()
    {
        let mut next = command(inputs, MediaProgram::Ffmpeg, root)?;
        protocol(&mut next);
        next.args(["-i", "pipe:0"]);
        match layer.render_mode {
            ExternalLayerRenderMode::AssOverlay => {
                let name = format!("presentation-{index:03}.ass");
                scene_delivery::copy_ass_layer(root, inputs, layer, &name)?;
                next.args([
                    "-vf",
                    &scene_delivery::ass_overlay_filter(layer.font.is_some(), &name),
                    "-an",
                    "-r",
                    &format!("{}/{}", plan.fps_numerator, plan.fps_denominator),
                ]);
            }
            ExternalLayerRenderMode::TimedVideoOverlay
            | ExternalLayerRenderMode::TimedStillOverlay
            | ExternalLayerRenderMode::TimedPngSequenceOverlay => {
                let path = if layer.render_mode == ExternalLayerRenderMode::TimedPngSequenceOverlay
                {
                    let file = scene_delivery::png_sequence::prepare(inputs, root, &layer.frames)?;
                    let path = file.path().to_owned();
                    sequence_controls.push(file);
                    path
                } else {
                    inputs.verified_path(scene_delivery::timed_overlay_source(inputs, layer)?)?
                };
                let span = plan
                    .external_layer_spans
                    .iter()
                    .find(|span| span.attachment_id == layer.attachment_id)
                    .context("reference overlay span missing")?;
                protocol(&mut next);
                next.args(scene_delivery::timed_overlay_input_options(
                    plan,
                    &layer.render_mode,
                ));
                next.args(["-threads", "1", "-i"]).arg(path).args([
                    "-filter_complex",
                    &scene_delivery::timed_overlay_filter_for_mode(plan, span, &layer.render_mode),
                    "-map",
                    "[v]",
                    "-an",
                    "-frames:v",
                    &plan.frame_count.to_string(),
                ]);
            }
            _ => unreachable!(),
        }
        encode(&mut next, job);
        commands.push(next);
    }
    if let Some(camera) = &job.post_compose_camera {
        let mut next = command(inputs, MediaProgram::Ffmpeg, root)?;
        protocol(&mut next);
        next.args([
            "-i",
            "pipe:0",
            "-vf",
            &scene_delivery::post_camera_filter(
                plan,
                camera,
                u64::from(job.width),
                u64::from(job.height),
            ),
            "-frames:v",
            &plan.frame_count.to_string(),
            "-an",
        ]);
        encode(&mut next, job);
        commands.push(next);
    }
    let mut hash = command(inputs, MediaProgram::Ffmpeg, root)?;
    protocol(&mut hash);
    hash.args(["-i", "pipe:0"]);
    hash_video(&mut hash, false);
    commands.push(hash);
    Ok((commands, sequence_controls))
}
fn hash_video(command: &mut Command, audio: bool) {
    command.args([
        "-filter_complex",
        "[0:v:0]split=2[yuv][rgb]",
        "-map",
        "[yuv]",
        "-map",
        "[rgb]",
        "-c:v",
        "rawvideo",
        "-threads:v",
        "1",
        "-pix_fmt:v:0",
        "yuv444p",
        "-pix_fmt:v:1",
        "rgb24",
        "-fps_mode:v:0",
        "passthrough",
        "-fps_mode:v:1",
        "passthrough",
    ]);
    if audio {
        command.args(["-map", "0:a:0", "-c:a", "pcm_s24le"]);
    }
    command.args(["-f", "streamhash", "-hash", "sha256", "pipe:1"]);
}
fn staging_bound(job: &Job, plan: &Plan) -> Result<u64> {
    let mut bytes = plan
        .duration_samples
        .checked_mul(56)
        .context("reference audio bound overflow")?;
    // Native mixing can copy source metadata into float/PCM WAV headers. Bound
    // that contribution by the complete input object sizes, including recoding.
    for bus in ["D", "M", "E"] {
        if let Some(audio) = job.audio.iter().find(|audio| audio.bus == bus) {
            bytes = bytes
                .checked_add(
                    audio
                        .source
                        .bytes
                        .checked_mul(16)
                        .context("audio metadata bound overflow")?,
                )
                .context("reference staging bound overflow")?;
        }
    }
    for layer in &job.external_layers {
        if layer.render_mode == ExternalLayerRenderMode::TimedPngSequenceOverlay {
            bytes = bytes
                .checked_add(scene_delivery::png_sequence::CONTROL_BYTES)
                .context("sequence control staging overflow")?;
        }
        if layer.render_mode == ExternalLayerRenderMode::AssOverlay {
            bytes = bytes
                .checked_add(layer.evidence.bytes)
                .context("ASS staging overflow")?;
            if let Some(font) = &layer.font {
                bytes = bytes
                    .checked_add(font.bytes)
                    .context("font staging overflow")?;
            }
        }
    }
    bytes
        .checked_add(1024 * 1024)
        .context("reference header bound overflow")
}
fn staged_bytes(root: &Path) -> Result<u64> {
    let mut pending = vec![root.to_owned()];
    let mut total = 0u64;
    while let Some(root) = pending.pop() {
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                total = total
                    .checked_add(entry.metadata()?.len())
                    .context("reference staging overflow")?;
            } else {
                bail!("unexpected reference staging entry");
            }
        }
    }
    Ok(total)
}
#[allow(clippy::too_many_arguments)]
/// Compare the retained stream with native picture/mixer reconstruction.
/// Caller admits the conservative scratch reservation before execution and keeps
/// all source/runtime/output guards alive. Production uses the existing bounded
/// job worker to contain the native mixer and every subprocess; this low-level
/// entry point limits its owned pipe commands and verifies its success deadline.
pub fn compare_master_to_primaries(
    job: &FileRef,
    production: &FileRef,
    controls: &dyn InputFiles,
    inputs: &dyn InputFiles,
    outputs: &dyn InputFiles,
    master: &FileRef,
    stage_parent: &Path,
    budget: &Budget,
) -> Result<PrimaryReference> {
    outputs.validate_reference_name(master)?;
    if !stage_parent.is_absolute()
        || !stage_parent.is_dir()
        || budget.max_staged_bytes == 0
        || !(1..=3600).contains(&budget.timeout_seconds)
        || !(2..=130).contains(&budget.max_picture_processes)
    {
        bail!("invalid reference admission");
    }
    let deadline = Instant::now() + Duration::from_secs(budget.timeout_seconds);
    let (native, plan) =
        scene_delivery::plan_with_control_files(job, production, inputs, controls)?;
    if plan.fps_numerator == 0
        || plan.fps_denominator == 0
        || plan.fps_numerator > i32::MAX as u64
        || plan.fps_denominator > i32::MAX as u64
    {
        bail!("reference frame rate exceeds codec bounds");
    }
    let reservation = staging_bound(&native, &plan)?;
    let picture_processes = 2
        + usize::from(native.post_compose_camera.is_some())
        + native
            .external_layers
            .iter()
            .filter(|layer| {
                matches!(
                    layer.render_mode,
                    ExternalLayerRenderMode::AssOverlay
                        | ExternalLayerRenderMode::TimedVideoOverlay
                        | ExternalLayerRenderMode::TimedStillOverlay
                        | ExternalLayerRenderMode::TimedPngSequenceOverlay
                )
            })
            .count();
    if picture_processes > usize::from(budget.max_picture_processes) {
        bail!("primary reference picture processes exceed admission");
    }
    if reservation > budget.max_staged_bytes {
        bail!("primary reference staging exceeds admission");
    }
    let selected = outputs.verified_path(master)?;
    check_clocks(&selected, &native, &plan, inputs, stage_parent, deadline)?;
    let stage = tempfile::Builder::new()
        .prefix(".scene-primary-reference-")
        .tempdir_in(stage_parent)?;
    let (picture_commands, sequence_controls) =
        picture_pipeline(&native, &plan, inputs, stage.path())?;
    let expected_picture = parse_hashes(
        &pipeline::run(picture_commands, 16384, deadline)?,
        &['v', 'v'],
    )?;
    drop(sequence_controls);
    scene_delivery::render_audio_stems(&native, &plan, inputs, stage.path())?;
    let mut audio = command(inputs, MediaProgram::Ffmpeg, stage.path())?;
    protocol(&mut audio);
    audio.arg("-i").arg(stage.path().join("mix.wav")).args([
        "-map",
        "0:a:0",
        "-c:a",
        "pcm_s24le",
        "-f",
        "streamhash",
        "-hash",
        "sha256",
        "pipe:1",
    ]);
    let expected_audio = parse_hashes(&pipeline::run(vec![audio], 16384, deadline)?, &['a'])?;
    let mut actual = command(inputs, MediaProgram::Ffmpeg, stage.path())?;
    protocol(&mut actual);
    actual.arg("-i").arg(selected);
    hash_video(&mut actual, true);
    let actual = parse_hashes(
        &pipeline::run(vec![actual], 16384, deadline)?,
        &['v', 'v', 'a'],
    )?;
    if actual[0] != expected_picture[0]
        || actual[1] != expected_picture[1]
        || actual[2] != expected_audio[0]
    {
        bail!("retained scene differs from its current primary reference");
    }
    let staged = staged_bytes(stage.path())?;
    if staged > reservation {
        bail!("reference staging exceeded conservative reservation");
    }
    Ok(PrimaryReference {
        job: job.clone(),
        master: master.clone(),
        frames: plan.frame_count,
        samples: plan.duration_samples,
        yuv: actual[0].clone(),
        rgb: actual[1].clone(),
        pcm: actual[2].clone(),
        staged,
        reservation,
        picture_processes,
    })
}

fn parse_hashes(raw: &[u8], types: &[char]) -> Result<Vec<String>> {
    let raw = std::str::from_utf8(raw)?;
    let lines = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let current = lines.first() == Some(&"#format: stream checksums");
    if current {
        if !lines.contains(&"#version: 2") || !lines.contains(&"#hash: SHA256") {
            bail!("unsupported scene checksum headers");
        }
        for (index, kind) in types.iter().enumerate() {
            let media = format!(
                "#media_type {index}: {}",
                if *kind == 'v' { "video" } else { "audio" }
            );
            let codec = format!(
                "#codec_id {index}: {}",
                if *kind == 'v' {
                    "rawvideo"
                } else {
                    "pcm_s24le"
                }
            );
            if !lines.contains(&media.as_str()) || !lines.contains(&codec.as_str()) {
                bail!("scene checksum stream headers differ");
            }
        }
    }
    let mut hashes = BTreeMap::new();
    for line in lines {
        if current && line.starts_with('#') {
            continue;
        }
        let parts = line.split(',').map(str::trim).collect::<Vec<_>>();
        if parts.len() != 3 {
            bail!("malformed scene stream checksum");
        }
        let index = parts[0].parse::<usize>()?;
        let kind = types
            .get(index)
            .context("unexpected scene checksum stream")?;
        let digest = if current {
            if parts[1].parse::<u64>()? == 0 {
                bail!("empty scene checksum stream");
            }
            parts[2]
        } else {
            if parts[1] != kind.to_string() {
                bail!("scene checksum type differs");
            }
            let (algorithm, digest) = parts[2]
                .split_once('=')
                .context("checksum algorithm missing")?;
            if !algorithm.eq_ignore_ascii_case("sha256") {
                bail!("unsupported scene checksum algorithm");
            }
            digest
        };
        if digest.len() != 64
            || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            || hashes.insert(index, digest.to_ascii_lowercase()).is_some()
        {
            bail!("invalid or duplicate scene checksum");
        }
    }
    if hashes.len() != types.len() {
        bail!("incomplete scene checksum set");
    }
    Ok(hashes.into_values().collect())
}

fn check_clocks(
    path: &Path,
    job: &Job,
    plan: &Plan,
    inputs: &dyn InputFiles,
    root: &Path,
    deadline: Instant,
) -> Result<()> {
    let mut probe = command(inputs, MediaProgram::Ffprobe, root)?;
    protocol(&mut probe);
    probe.args(["-show_streams", "-of", "json"]).arg(path);
    let metadata: Value =
        serde_json::from_slice(&pipeline::run(vec![probe], 1024 * 1024, deadline)?)?;
    let streams = metadata["streams"]
        .as_array()
        .context("reference master streams missing")?;
    if streams.len() != 2 {
        bail!("reference master requires one picture and one audio stream");
    }
    let video = streams
        .iter()
        .find(|stream| stream["codec_type"] == "video")
        .context("master picture missing")?;
    let audio = streams
        .iter()
        .find(|stream| stream["codec_type"] == "audio")
        .context("master audio missing")?;
    let fps = rational(
        video["r_frame_rate"]
            .as_str()
            .context("master fps missing")?,
    )?;
    if video["width"].as_u64() != Some(u64::from(job.width))
        || video["height"].as_u64() != Some(u64::from(job.height))
        || video["sample_aspect_ratio"] != "1:1"
        || fps.0 as u128 * plan.fps_denominator as u128
            != fps.1 as u128 * plan.fps_numerator as u128
        || audio["sample_rate"].as_str() != Some(plan.sample_rate.to_string().as_str())
        || audio["channels"].as_u64() != Some(2)
    {
        bail!("reference master geometry or audio format differs");
    }
    for (kind, stream, expected) in [
        ("v:0", video, plan.frame_count),
        ("a:0", audio, plan.duration_samples),
    ] {
        let base = rational(
            stream["time_base"]
                .as_str()
                .context("master timebase missing")?,
        )?;
        let mut probe = command(inputs, MediaProgram::Ffprobe, root)?;
        protocol(&mut probe);
        probe
            .args([
                "-select_streams",
                kind,
                "-show_frames",
                "-show_entries",
                "frame=best_effort_timestamp,nb_samples",
                "-of",
                "json",
            ])
            .arg(path);
        let maximum = expected
            .saturating_mul(256)
            .saturating_add(1024 * 1024)
            .min(64 * 1024 * 1024) as usize;
        let decoded: Value =
            serde_json::from_slice(&pipeline::run(vec![probe], maximum, deadline)?)?;
        let mut count = 0u64;
        let mut previous = None;
        for frame in decoded["frames"]
            .as_array()
            .context("decoded master frames missing")?
        {
            let tick = frame["best_effort_timestamp"]
                .as_i64()
                .filter(|tick| *tick >= 0)
                .context("decoded master timestamp missing")? as u64;
            if (count == 0 && tick != 0) || previous.is_some_and(|previous| tick < previous) {
                bail!("master clock is not contiguous from zero");
            }
            let (rate_num, rate_den, amount) = if kind == "v:0" {
                (plan.fps_numerator, plan.fps_denominator, 1)
            } else {
                (
                    u64::from(plan.sample_rate),
                    1,
                    frame["nb_samples"]
                        .as_u64()
                        .filter(|amount| *amount > 0)
                        .context("decoded audio sample count missing")?,
                )
            };
            let actual = u128::from(tick) * u128::from(base.0) * u128::from(rate_num);
            let wanted = u128::from(count) * u128::from(rate_den) * u128::from(base.1);
            if actual.abs_diff(wanted) > u128::from(base.0) * u128::from(rate_num) {
                bail!("master timestamp differs from native frame/sample clock");
            }
            count = count
                .checked_add(amount)
                .filter(|count| *count <= expected)
                .context("master frame/sample count exceeds native plan")?;
            previous = Some(tick);
        }
        if count != expected {
            bail!("master frame/sample count differs from native plan");
        }
    }
    Ok(())
}
fn rational(raw: &str) -> Result<(u64, u64)> {
    let (num, den) = raw
        .split_once('/')
        .context("invalid native media rational")?;
    let num = num.parse::<u64>()?;
    let den = den.parse::<u64>()?;
    if num == 0 || den == 0 || num > i32::MAX as u64 || den > i32::MAX as u64 {
        bail!("native media rational exceeds codec bounds");
    }
    Ok((num, den))
}

#[cfg(test)]
#[path = "scene_primary_reference/tests.rs"]
mod tests;
