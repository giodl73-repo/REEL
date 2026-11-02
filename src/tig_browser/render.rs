//! Native streaming renderer for an explicitly admitted existing workspace.
//! The caller/outer job owns durable source claims, capacity and publication.
use super::{
    Prepared,
    capture::{self, Frames},
};
use crate::{
    media_runtime::{BoundPrograms, BoundTool},
    scene_delivery::{InputFiles, MediaProgram},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest as _, Sha256};
use std::{
    fs,
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    path::Path,
    time::{Duration, Instant},
};
use tig_core::Object;
mod browser;
mod processes;
use processes::{Task, native_path, pause, usage};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub capture: capture::Limits,
    pub max_workspace_bytes: u64,
    pub max_output_bytes: u64,
    pub browser_memory_bytes: usize,
    pub browser_processes: u32,
    pub encoder_memory_bytes: usize,
    pub encoder_processes: u32,
}
impl Limits {
    fn browser(&self) -> reel_assets::jobs::QuietLimits {
        reel_assets::jobs::QuietLimits {
            memory_bytes: self.browser_memory_bytes,
            processes: self.browser_processes,
        }
    }
    fn encoder(&self) -> reel_assets::jobs::QuietLimits {
        reel_assets::jobs::QuietLimits {
            memory_bytes: self.encoder_memory_bytes,
            processes: self.encoder_processes,
        }
    }
    pub(crate) fn validate(&self) -> Result<()> {
        self.capture.validate()?;
        if self.max_output_bytes == 0
            || self.max_output_bytes >= self.max_workspace_bytes
            || self.max_workspace_bytes > 9_007_199_254_740_991
            || self.browser_memory_bytes < 16 * 1024 * 1024
            || self.browser_memory_bytes as u64 > 16 * 1024 * 1024 * 1024
            || self.encoder_memory_bytes < 16 * 1024 * 1024
            || self.encoder_memory_bytes as u64 > 16 * 1024 * 1024 * 1024
            || !(1..=256).contains(&self.browser_processes)
            || !(1..=256).contains(&self.encoder_processes)
        {
            bail!("invalid browser native execution bounds");
        }
        Ok(())
    }
}
/// Private completed byte observation, not a produced claim or selected output.
pub struct Rendered {
    pub output: Object,
    pub capture: capture::Captured,
    pub frames: u64,
    pub samples_per_channel: Option<u64>,
    pub pcm_sha256: Option<String>,
    pub producer: Object,
}
/// Exact declared native producer contract. Process/storage limits are separate
/// operational admission. This profile specifies FFV1/yuv444p, square pixels,
/// source CFR and, when present, padded/trimmed PCM24/48kHz/stereo WAV audio.
/// No delivery codec or original retained-output correspondence is asserted.
pub fn producer(
    programs: &BoundPrograms,
    browser: &BoundTool,
    clean_picture: bool,
) -> Result<Object> {
    Ok(Object::from_bytes(&producer_document(
        programs,
        browser,
        clean_picture,
    )?))
}
pub fn producer_document(
    programs: &BoundPrograms,
    browser: &BoundTool,
    clean_picture: bool,
) -> Result<Vec<u8>> {
    Ok(tig_core::identity::canonical_json(&json!({
        "schema":"reel.tig-browser-native-producer.experimental-1",
        "media_runtime":programs.identity(),"browser":browser.identity(),"clean_picture":clean_picture,
        "profile":"ffv1-yuv444p-square-cfr;wav-to-padded-trimmed-pcm24-48000-stereo/v1",
        "renderer":Object::from_bytes(include_bytes!("render.rs")),
        "browser_adapter":Object::from_bytes(include_bytes!("render/browser.rs")),
        "process_adapter":Object::from_bytes(include_bytes!("render/processes.rs")),
        "capture_adapter":Object::from_bytes(include_bytes!("capture.rs")),
        "capture_transport":Object::from_bytes(include_bytes!("capture/protocol.rs"))
    }))?)
}

fn listener() -> Result<TcpListener> {
    let l = TcpListener::bind("127.0.0.1:0")?;
    l.set_nonblocking(true)?;
    Ok(l)
}
fn accept(listener: &TcpListener, task: &mut Task, deadline: Instant) -> Result<TcpStream> {
    loop {
        match listener.accept() {
            Ok((stream, peer)) => {
                if !peer.ip().is_loopback() {
                    bail!("encoder stream is not local");
                }
                stream.set_nonblocking(true)?;
                return Ok(stream);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                task.check_running()?;
                pause(deadline)?;
            }
            Err(e) => return Err(e.into()),
        }
    }
}
struct Encoder {
    task: Task,
    stream: TcpStream,
    deadline: Instant,
    next: u64,
    expected: u64,
    io_wait: Duration,
}
impl Frames for Encoder {
    fn write(&mut self, index: u64, mut png: &[u8]) -> Result<()> {
        if index != self.next || index >= self.expected {
            bail!("encoder frame sequence differs");
        }
        let deadline = (Instant::now() + self.io_wait).min(self.deadline);
        while !png.is_empty() {
            self.task.check_running()?;
            if Instant::now() >= deadline {
                bail!("encoder frame write deadline elapsed");
            }
            match self.stream.write(png) {
                Ok(0) => bail!("encoder stream ended"),
                Ok(n) => png = &png[n..],
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => pause(deadline)?,
                Err(e) => return Err(e.into()),
            }
        }
        self.next += 1;
        Ok(())
    }
    fn finish(&mut self) -> Result<()> {
        if self.next != self.expected {
            bail!("encoder did not receive every frame");
        }
        self.stream.shutdown(Shutdown::Write)?;
        self.task.wait(self.deadline)
    }
}

fn arguments(
    plan: &Prepared,
    endpoint: &str,
    output: &Path,
    audio: Option<&Path>,
    max_output_bytes: u64,
) -> Result<Vec<String>> {
    let manifest = &plan.manifest;
    let mut args = [
        "-y",
        "-v",
        "error",
        "-nostdin",
        "-threads",
        "1",
        "-filter_threads",
        "1",
        "-filter_complex_threads",
        "1",
        "-protocol_whitelist",
        "file,tcp",
        "-f",
        "image2pipe",
        "-c:v",
        "png",
        "-framerate",
    ]
    .map(str::to_owned)
    .to_vec();
    args.extend([manifest.fps.to_string(), "-i".into(), endpoint.into()]);
    if let Some(audio) = audio {
        let samples = sample_count(plan)?;
        args.extend(["-f".into(),"wav".into(),"-i".into(),native_path(audio)?,"-filter_complex".into(),
            format!("[1:a:0]aresample=48000,aformat=channel_layouts=stereo,apad,atrim=end_sample={samples},asetpts=PTS-STARTPTS[a]"),
            "-map".into(),"0:v:0".into(),"-map".into(),"[a]".into(),"-c:a".into(),"pcm_s24le".into(),
            "-ar".into(),"48000".into(),"-ac".into(),"2".into()]);
    } else {
        args.push("-an".into());
    }
    args.extend([
        "-vf".into(),
        "format=yuv444p,setsar=1".into(),
        "-frames:v".into(),
        manifest.frame_count.to_string(),
        "-r".into(),
        manifest.fps.to_string(),
        "-c:v".into(),
        "ffv1".into(),
        "-level".into(),
        "3".into(),
        "-pix_fmt".into(),
        "yuv444p".into(),
        "-threads:v".into(),
        "1".into(),
        "-f".into(),
        "matroska".into(),
        "-fs".into(),
        max_output_bytes.to_string(),
        native_path(output)?,
    ]);
    Ok(args)
}
fn sample_count(plan: &Prepared) -> Result<u64> {
    let value = plan
        .manifest
        .frame_count
        .checked_mul(48000)
        .context("browser audio clock overflow")?;
    if value % u64::from(plan.manifest.fps) != 0 {
        bail!("browser clock cannot map to integral 48kHz samples");
    }
    Ok(value / u64::from(plan.manifest.fps))
}
#[allow(clippy::too_many_arguments)]
fn probe(
    programs: &BoundPrograms,
    inputs: &dyn InputFiles,
    path: &Path,
    workspace: &Path,
    limits: &Limits,
    deadline: Instant,
    plan: &Prepared,
) -> Result<()> {
    let metadata = tempfile::Builder::new()
        .prefix("browser-probe-")
        .suffix(".json")
        .tempfile_in(workspace)?;
    let args=vec!["-v".into(),"error".into(),"-count_frames".into(),"-show_entries".into(),
        "stream=codec_type,codec_name,width,height,pix_fmt,avg_frame_rate,nb_read_frames,sample_aspect_ratio,sample_rate,channels".into(),
        "-of".into(),"json".into(),"-o".into(),native_path(metadata.path())?,native_path(path)?];
    let mut task = Task::start(
        &programs
            .with_inputs(inputs)
            .media_program(MediaProgram::Ffprobe)?,
        args,
        workspace,
        limits.max_workspace_bytes,
        deadline
            .checked_duration_since(Instant::now())
            .context("browser probe deadline elapsed")?,
        limits.encoder(),
    )?;
    task.wait(deadline)?;
    if metadata.as_file().metadata()?.len() > 65536 {
        bail!("browser probe exceeds metadata budget");
    }
    let report = tig_core::identity::parse_json(&fs::read(metadata.path())?)?;
    let streams = report["streams"]
        .as_array()
        .context("browser probe streams missing")?;
    let video = streams
        .iter()
        .filter(|v| v["codec_type"] == "video")
        .collect::<Vec<_>>();
    let audio = streams
        .iter()
        .filter(|v| v["codec_type"] == "audio")
        .collect::<Vec<_>>();
    let m = &plan.manifest;
    if video.len() != 1
        || audio.len() != usize::from(m.audio.is_some())
        || streams.len() != video.len() + audio.len()
    {
        bail!("browser output stream set differs");
    }
    let video = video[0];
    if video["codec_name"] != "ffv1"
        || video["pix_fmt"] != "yuv444p"
        || video["sample_aspect_ratio"] != "1:1"
        || video["width"].as_u64() != Some(u64::from(m.width))
        || video["height"].as_u64() != Some(u64::from(m.height))
        || video["avg_frame_rate"] != format!("{}/1", m.fps)
        || video["nb_read_frames"]
            .as_str()
            .and_then(|v| v.parse::<u64>().ok())
            != Some(m.frame_count)
    {
        bail!("browser native output format or frame clock differs");
    }
    if let Some(audio) = audio.first()
        && (audio["codec_name"] != "pcm_s24le"
            || audio["sample_rate"] != "48000"
            || audio["channels"] != 2)
    {
        bail!("browser native audio format differs");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn pcm(
    programs: &BoundPrograms,
    inputs: &dyn InputFiles,
    path: &Path,
    workspace: &Path,
    limits: &Limits,
    deadline: Instant,
    samples: u64,
) -> Result<String> {
    let l = listener()?;
    let endpoint = format!("tcp://127.0.0.1:{}", l.local_addr()?.port());
    let args = vec![
        "-v".into(),
        "error".into(),
        "-nostdin".into(),
        "-threads".into(),
        "1".into(),
        "-i".into(),
        native_path(path)?,
        "-map".into(),
        "0:a:0".into(),
        "-vn".into(),
        "-c:a".into(),
        "pcm_s24le".into(),
        "-f".into(),
        "s24le".into(),
        endpoint,
    ];
    let mut task = Task::start(
        &programs
            .with_inputs(inputs)
            .media_program(MediaProgram::Ffmpeg)?,
        args,
        workspace,
        limits.max_workspace_bytes,
        deadline
            .checked_duration_since(Instant::now())
            .context("browser audio check deadline elapsed")?,
        limits.encoder(),
    )?;
    let mut stream = accept(&l, &mut task, deadline)?;
    let expected = samples
        .checked_mul(6)
        .context("browser PCM count overflow")?;
    let mut total = 0u64;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                total = total
                    .checked_add(n as u64)
                    .context("browser PCM byte overflow")?;
                if total > expected {
                    bail!("browser output has excess PCM samples");
                }
                hash.update(&buffer[..n]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => pause(deadline)?,
            Err(e) => return Err(e.into()),
        }
        if Instant::now() >= deadline {
            bail!("browser PCM check deadline elapsed");
        }
    }
    task.wait(deadline)?;
    if total != expected {
        bail!("browser output PCM sample count differs");
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Low-level trusted worker operation. Caller must first reserve peak capacity,
/// enroll/fence the complete source/job and keep the supplied input/program
/// guards alive. Workspace usage is observed, not a filesystem hard quota.
/// No cache setup, hydration, durable claims, output adoption or publication.
#[allow(clippy::too_many_arguments)]
pub fn native(
    plan: &Prepared,
    inputs: &dyn InputFiles,
    programs: &BoundPrograms,
    browser: &BoundTool,
    workspace: &Path,
    output: &Path,
    limits: &Limits,
    clean_picture: bool,
    mut authority: impl FnMut() -> Result<()>,
) -> Result<Rendered> {
    limits.validate()?;
    authority()?;
    crate::browser_composition::validate_manifest(plan.manifest.clone())?;
    if serde_json::to_vec(&plan.manifest)? != plan.manifest_bytes
        || !workspace.is_absolute()
        || !workspace.is_dir()
        || output.parent() != Some(workspace)
        || output.symlink_metadata().is_ok()
        || output.extension().is_none_or(|v| v != "mkv")
    {
        bail!("invalid browser native workspace or output contract");
    }
    if plan.manifest.frame_count > limits.capture.max_frames
        || u64::from(plan.manifest.width) * u64::from(plan.manifest.height)
            > limits.capture.max_canvas_pixels
    {
        bail!("browser geometry exceeds execution admission");
    }
    for (path, file) in &plan.inputs {
        if file.bytes > limits.capture.max_resource_bytes
            && plan.manifest.audio.as_ref().is_none_or(|v| &v.path != path)
        {
            bail!("browser resource exceeds execution admission");
        }
        inputs.verified_path(file)?;
    }
    let audio = plan
        .manifest
        .audio
        .as_ref()
        .map(|v| {
            inputs.verified_path(
                plan.inputs
                    .get(&v.path)
                    .context("browser audio primary missing")?,
            )
        })
        .transpose()?;
    let samples = audio.as_ref().map(|_| sample_count(plan)).transpose()?;
    let producer = producer(programs, browser, clean_picture)?;
    drop(capture::Guarded::new(plan, inputs, || Ok(()))?);
    usage(workspace, limits.max_workspace_bytes)?;
    let deadline = Instant::now() + Duration::from_millis(limits.capture.max_elapsed_ms);
    let candidate = tempfile::Builder::new()
        .prefix("browser-native-")
        .suffix(".mkv")
        .tempfile_in(workspace)?
        .into_temp_path();
    let mut runtime = browser::Browser::start(
        browser,
        workspace,
        limits.max_workspace_bytes,
        Duration::from_millis(limits.capture.max_elapsed_ms),
        limits.browser(),
        limits.capture.max_message_bytes,
        Duration::from_millis(limits.capture.max_io_wait_ms),
    )?;
    authority()?;
    let l = listener()?;
    let endpoint = format!("tcp://127.0.0.1:{}", l.local_addr()?.port());
    let mut task = Task::start(
        &programs
            .with_inputs(inputs)
            .media_program(MediaProgram::Ffmpeg)?,
        arguments(
            plan,
            &endpoint,
            &candidate,
            audio.as_deref(),
            limits.max_output_bytes,
        )?,
        workspace,
        limits.max_workspace_bytes,
        deadline
            .checked_duration_since(Instant::now())
            .context("browser encode deadline elapsed")?,
        limits.encoder(),
    )?;
    let stream = accept(&l, &mut task, deadline)?;
    let mut encoder = Encoder {
        task,
        stream,
        deadline,
        next: 0,
        expected: plan.manifest.frame_count,
        io_wait: Duration::from_millis(limits.capture.max_io_wait_ms),
    };
    let mut resources = capture::Guarded::new(plan, inputs, || {
        authority()?;
        usage(workspace, limits.max_workspace_bytes)?;
        Ok(())
    })?;
    let captured = capture::capture(
        &mut runtime.transport,
        &mut resources,
        &plan.manifest,
        clean_picture,
        &limits.capture,
        &mut encoder,
    )
    .context("stream browser frames into native encoder")?;
    drop(resources);
    drop(encoder);
    runtime.close(deadline).context("close owned browser")?;
    authority()?;
    if fs::metadata(&candidate)?.len() > limits.max_output_bytes {
        bail!("browser native output exceeds declared budget");
    }
    probe(
        programs, inputs, &candidate, workspace, limits, deadline, plan,
    )?;
    let pcm_sha256 = samples
        .map(|n| pcm(programs, inputs, &candidate, workspace, limits, deadline, n))
        .transpose()?;
    authority()?;
    usage(workspace, limits.max_workspace_bytes)?;
    let output_pin = Object {
        sha256: tig_core::Digest::parse(&crate::production::sha256_path(&candidate)?)?,
        bytes: fs::metadata(&candidate)?.len(),
    };
    candidate
        .persist_noclobber(output)
        .map_err(|_| anyhow::anyhow!("browser output publication path is occupied"))?;
    Ok(Rendered {
        output: output_pin,
        capture: captured,
        frames: plan.manifest.frame_count,
        samples_per_channel: samples,
        pcm_sha256,
        producer,
    })
}

#[cfg(test)]
mod tests;
