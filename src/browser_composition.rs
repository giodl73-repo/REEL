use std::{
    collections::BTreeSet,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tempfile::{Builder, TempDir};
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

use crate::{adapters::ffmpeg::FfmpegAdapter, production};

pub const MANIFEST_SCHEMA: &str = "reel.browser-composition.v0.1";
pub const ARTIFACT_SCHEMA: &str = "reel.browser-composition-artifacts.v0.1";
mod clock;
pub(crate) use clock::source_clock_event;
pub(crate) mod pcm_window;
pub use pcm_window::PcmWindow;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserCompositionManifest {
    pub schema: String,
    pub source: HashedAsset,
    #[serde(default)]
    pub assets: Vec<HashedAsset>,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frame_count: u64,
    /// Regions allowed to change; pixels outside them must remain byte-stable.
    #[serde(default)]
    pub dynamic_regions: Vec<PixelRegion>,
    #[serde(default)]
    pub audio: Option<HashedAsset>,
    /// Explicit measured clock, also present in assets with its exact hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_clock: Option<HashedAsset>,
    /// Exact 48kHz stereo PCM excerpt with integer linear fades. Optional;
    /// ordinary WAV padding/trimming remains unchanged when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_window: Option<PcmWindow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lossless_master: Option<LosslessMaster>,
}

/// Opt-in FFV1/YUV444/PCM24 master. Anchors are source sample positions:
/// pickup, three ordered entrances, and phrase endpoint.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LosslessMaster {
    pub source_clock: HashedAsset,
    pub anchor_ids: [String; 5],
    pub sample_rate: u32,
    pub source_samples: u64,
    pub start_sample: u64,
    pub end_sample: u64,
    pub anchors: [u64; 5],
    pub selected_pcm_sha256: String,
    pub envelope_pcm_sha256: String,
    #[serde(default)]
    pub fade_in_samples: u64,
    #[serde(default)]
    pub fade_out_samples: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MasterVerification {
    pub encoding: String,
    pub selected_pcm_sha256: String,
    pub envelope_pcm_sha256: String,
    pub samples: u64,
    pub decoded_yuv444_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PixelRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HashedAsset {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug)]
pub struct RenderOptions {
    pub manifest: PathBuf,
    pub asset_root: PathBuf,
    pub browser: PathBuf,
    pub output: PathBuf,
    pub clean_picture: bool,
    /// Keep the exact captured PNG for a single-frame composition.
    pub poster: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Poster {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FrameLineage {
    pub index: u64,
    pub sha256: String,
    pub sequence_sha256: String,
    pub stable_background_sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InputLineage {
    pub role: String,
    pub relative_path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BrowserCompositionReport {
    pub schema: String,
    pub manifest_sha256: String,
    pub browser_version: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frame_count: u64,
    pub duration_ms: u64,
    pub clean_picture: bool,
    pub persistent_dom: bool,
    pub network_policy: String,
    pub inputs: Vec<InputLineage>,
    pub frames: Vec<FrameLineage>,
    pub output_sha256: String,
    pub output_bytes: u64,
    pub output_duration_ms: u64,
    pub audio_streams: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub master_verification: Option<MasterVerification>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<Poster>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BrowserCompositionCheck {
    pub schema: String,
    pub passed: bool,
    pub verified_inputs: usize,
    pub verified_frames: usize,
    pub output_sha256: String,
    pub duration_ms: u64,
}

struct BrowserGuard(Child);
impl Drop for BrowserGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn relative_path(value: &str) -> Result<PathBuf> {
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("browser composition paths must stay relative to the declared asset root");
    }
    Ok(path.to_path_buf())
}

fn validate_hash(hash: &str) -> Result<()> {
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("browser composition asset has an invalid SHA-256");
    }
    Ok(())
}

fn load_manifest(path: &Path) -> Result<BrowserCompositionManifest> {
    let manifest: BrowserCompositionManifest = serde_yaml::from_slice(&fs::read(path)?)?;
    validate_manifest(manifest)
}

pub(crate) fn validate_manifest(
    manifest: BrowserCompositionManifest,
) -> Result<BrowserCompositionManifest> {
    if manifest.schema != MANIFEST_SCHEMA {
        bail!("unsupported browser composition schema");
    }
    if manifest.width == 0
        || manifest.height == 0
        || manifest.width % 2 != 0
        || manifest.height % 2 != 0
        || manifest.fps == 0
        || manifest.fps > 60
        || manifest.frame_count == 0
    {
        bail!("browser composition dimensions, FPS, or frame count are invalid");
    }
    for region in &manifest.dynamic_regions {
        if region.width == 0
            || region.height == 0
            || region.x.saturating_add(region.width) > manifest.width
            || region.y.saturating_add(region.height) > manifest.height
        {
            bail!("browser composition dynamic region is outside the delivery canvas");
        }
    }
    if let Some(m) = &manifest.lossless_master {
        let samples = m
            .end_sample
            .checked_sub(m.start_sample)
            .context("invalid master sample window")?;
        if manifest.audio.is_none()
            || ![44100, 48000].contains(&m.sample_rate)
            || m.end_sample > m.source_samples
            || samples == 0
            || u128::from(samples) * u128::from(manifest.fps)
                != u128::from(manifest.frame_count) * u128::from(m.sample_rate)
            || m.anchors[0] != m.start_sample
            || m.anchors[4] != m.end_sample
            || m.anchors.windows(2).any(|w| w[0] >= w[1])
            || m.fade_in_samples
                .checked_add(m.fade_out_samples)
                .is_none_or(|n| n > samples)
        {
            bail!("invalid lossless master source clock, anchors, or envelope");
        }
        let anchors = anchor_frames(m, manifest.fps)?;
        if anchors.windows(2).any(|w| w[0] >= w[1]) {
            bail!("collapsed master frame anchors");
        }
        if !manifest
            .assets
            .iter()
            .any(|a| a.path == m.source_clock.path && a.sha256 == m.source_clock.sha256)
        {
            bail!("source clock must be a declared hashed asset");
        }
        validate_hash(&m.selected_pcm_sha256)?;
        validate_hash(&m.envelope_pcm_sha256)?;
    }
    let mut paths = BTreeSet::new();
    for asset in std::iter::once(&manifest.source)
        .chain(&manifest.assets)
        .chain(manifest.audio.iter())
    {
        relative_path(&asset.path)?;
        validate_hash(&asset.sha256)?;
        if !paths.insert(&asset.path) {
            bail!("browser composition contains a duplicate asset path");
        }
    }
    if let Some(clock) = &manifest.source_clock
        && (manifest.audio.is_none()
            || !manifest
                .assets
                .iter()
                .any(|a| a.path == clock.path && a.sha256 == clock.sha256))
    {
        bail!("browser source clock must name a declared asset and audio");
    }
    if let Some(window) = &manifest.audio_window {
        window.validate(manifest.frame_count, manifest.fps)?;
        if manifest.audio.is_none() || manifest.source_clock.is_none() {
            bail!("exact PCM window needs original audio and an explicit source clock");
        }
    }
    Ok(manifest)
}

fn stage_asset(root: &Path, stage: &Path, asset: &HashedAsset, role: &str) -> Result<InputLineage> {
    let relative = relative_path(&asset.path)?;
    let source = root
        .join(&relative)
        .canonicalize()
        .with_context(|| format!("missing browser composition {role} {}", asset.path))?;
    if !source.starts_with(root) {
        bail!("browser composition asset escapes the declared root");
    }
    let actual = production::sha256_path(&source)?;
    if !actual.eq_ignore_ascii_case(&asset.sha256) {
        bail!(
            "browser composition {role} hash mismatch for {}",
            asset.path
        );
    }
    if matches!(
        source.extension().and_then(|value| value.to_str()),
        Some("html" | "htm" | "svg" | "css" | "js")
    ) {
        let text = fs::read_to_string(&source)?;
        let lower = text
            .to_ascii_lowercase()
            .replace("http://www.w3.org/2000/svg", "")
            .replace("http://www.w3.org/1999/xlink", "");
        if ["http://", "https://", "file://", "<base", "//cdn."]
            .iter()
            .any(|needle| lower.contains(needle))
        {
            bail!("browser composition source contains a forbidden external or absolute reference");
        }
    }
    let destination = stage.join(&relative);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&source, &destination)?;
    Ok(InputLineage {
        role: role.to_string(),
        relative_path: asset.path.clone(),
        sha256: actual,
    })
}

fn free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn http_json(port: u16, path: &str) -> Result<Value> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )?;
    let mut reader = BufReader::new(stream);
    let mut content_length = None;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = Some(value.trim().parse::<usize>()?);
        }
    }
    let length = content_length
        .ok_or_else(|| anyhow!("browser debugging response has no content length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}

fn discover_page(port: u16) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let failure = match http_json(port, "/json") {
            Ok(Value::Array(targets)) => {
                if let Some(url) = targets.iter().find_map(|target| {
                    (target["type"] == "page" && target["url"] == "about:blank")
                        .then(|| target["webSocketDebuggerUrl"].as_str().map(str::to_string))
                        .flatten()
                }) {
                    return Ok(url);
                }
                "debug endpoint exposed no about:blank page".to_string()
            }
            Ok(_) => "debug endpoint returned a non-array target list".to_string(),
            Err(error) => error.to_string(),
        };
        if Instant::now() >= deadline {
            bail!("timed out waiting for Chromium debugging endpoint: {failure}");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn cdp(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value> {
    socket.send(Message::Text(
        json!({"id": id, "method": method, "params": params})
            .to_string()
            .into(),
    ))?;
    loop {
        let value: Value = serde_json::from_str(socket.read()?.to_text()?)?;
        if value["id"].as_u64() == Some(id) {
            if !value["error"].is_null() {
                bail!("Chromium {method} failed: {}", value["error"]);
            }
            return Ok(value["result"].clone());
        }
    }
}

fn file_url(path: &Path) -> String {
    let display = path.display().to_string();
    let display = display.strip_prefix(r"\\?\").unwrap_or(&display);
    format!("file:///{}", display.replace('\\', "/").replace(' ', "%20"))
}

pub(crate) fn frame_clock_with_source(fps: u32, source_clock: Option<&Value>) -> String {
    let source = source_clock
        .map(|value| format!(",sourceClock:{value}"))
        .unwrap_or_default();
    format!(
        r#"(()=>{{let f=0;const fps={fps};Object.defineProperty(window,'__REEL_FRAME__',{{get:()=>f}});window.__reelSetFrame=(n)=>{{f=n;document.documentElement.dataset.reelFrame=String(n);document.documentElement.dataset.reelTime=(n/fps).toFixed(9);window.dispatchEvent(new CustomEvent('reelframe',{{detail:{{frame:n,time:n/fps,fps{source}}}}}));}};Date.now=()=>Math.round(f*1000/fps);window.requestAnimationFrame=(cb)=>{{cb(f*1000/fps);return f+1;}};window.cancelAnimationFrame=()=>{{}};}})()"#
    )
}

pub(crate) fn stable_background_hash(
    png: &[u8],
    regions: &[PixelRegion],
) -> Result<Option<String>> {
    if regions.is_empty() {
        return Ok(None);
    }
    let mut image = image::load_from_memory_with_format(png, image::ImageFormat::Png)?.to_rgba8();
    Ok(Some(stable_background_pixels(&mut image, regions)))
}

pub(crate) fn stable_background_pixels(
    image: &mut image::RgbaImage,
    regions: &[PixelRegion],
) -> String {
    for region in regions {
        for y in region.y..region.y + region.height {
            for x in region.x..region.x + region.width {
                image.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
            }
        }
    }
    production::sha256_bytes(image.as_raw())
}

fn fraction(value: &str) -> Result<f64> {
    let (numerator, denominator) = value
        .split_once('/')
        .ok_or_else(|| anyhow!("invalid frame-rate fraction"))?;
    let denominator = denominator.parse::<f64>()?;
    if denominator == 0.0 {
        bail!("invalid zero frame-rate denominator");
    }
    Ok(numerator.parse::<f64>()? / denominator)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundSourceClock {
    schema: String,
    source_sha256: String,
    sample_rate: u32,
    total_samples: u64,
    evidence: crate::scene_delivery::FileRef,
    anchors: Vec<BoundAnchor>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundAnchor {
    id: String,
    sample: u64,
}
fn verify_source_clock(root: &Path, manifest: &BrowserCompositionManifest) -> Result<()> {
    let Some(m) = &manifest.lossless_master else {
        return Ok(());
    };
    let path = root
        .join(relative_path(&m.source_clock.path)?)
        .canonicalize()?;
    if !path.starts_with(root)
        || fs::metadata(&path)?.len() > 4 * 1024 * 1024
        || production::sha256_path(&path)? != m.source_clock.sha256
    {
        bail!("source clock changed or escapes root");
    }
    let c: BoundSourceClock = serde_json::from_slice(&fs::read(path)?)?;
    if c.schema != "reel.presentation-source-clock.v1"
        || c.source_sha256
            != manifest
                .audio
                .as_ref()
                .context("clock audio missing")?
                .sha256
        || c.sample_rate != m.sample_rate
        || c.total_samples != m.source_samples
    {
        bail!("source clock differs from selected original audio");
    }
    let mut anchors = std::collections::BTreeMap::new();
    for a in c.anchors {
        if a.id.trim().is_empty()
            || a.sample > c.total_samples
            || anchors.insert(a.id, a.sample).is_some()
        {
            bail!("invalid source clock anchor");
        }
    }
    let mut used = BTreeSet::new();
    for (id, sample) in m.anchor_ids.iter().zip(m.anchors) {
        if !used.insert(id) || anchors.get(id) != Some(&sample) {
            bail!("selected source clock anchor differs");
        }
    }
    let evidence_path = c
        .evidence
        .path
        .to_str()
        .context("clock evidence path must be UTF8")?;
    if !manifest
        .assets
        .iter()
        .any(|a| a.path == evidence_path && a.sha256 == c.evidence.sha256)
    {
        bail!("clock evidence must be a declared hashed asset");
    }
    crate::scene_delivery::checked_file(root, &c.evidence)?;
    Ok(())
}
fn anchor_frames(m: &LosslessMaster, fps: u32) -> Result<Vec<u64>> {
    m.anchors
        .iter()
        .map(|s| {
            u64::try_from(
                (u128::from(
                    s.checked_sub(m.start_sample)
                        .context("anchor before window")?,
                ) * u128::from(fps)
                    + u128::from(m.sample_rate) / 2)
                    / u128::from(m.sample_rate),
            )
            .context("anchor frame overflow")
        })
        .collect()
}
fn decode_hash(path: &Path, format: &str, fps: Option<u32>) -> Result<String> {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error"]);
    if let Some(fps) = fps {
        cmd.args(["-framerate", &fps.to_string()]);
    }
    cmd.arg("-i").arg(path);
    if format == "s24le" {
        cmd.args(["-map", "0:a:0", "-c:a", "pcm_s24le", "-f", "s24le"]);
    } else {
        cmd.args(["-map", "0:v:0", "-pix_fmt", format, "-f", "rawvideo"]);
    }
    let mut child = cmd.arg("-").stdout(Stdio::piped()).spawn()?;
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    let mut input = child.stdout.take().context("decode output missing")?;
    let mut buffer = [0; 65536];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    if !child.wait()?.success() {
        bail!("master identity decode failed");
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>())
}
fn full_probe(path: &Path) -> Result<Value> {
    Ok(serde_json::from_str(&FfmpegAdapter.run_ffprobe(
        &[
            "-v".into(),
            "error".into(),
            "-show_streams".into(),
            "-of".into(),
            "json".into(),
        ],
        &[FfmpegAdapter.path_argument(path)?],
    )?)?)
}
fn prepare_master_pcm(destination: &Path, source: &Path, m: &LosslessMaster) -> Result<PathBuf> {
    let probe = full_probe(source)?;
    let streams = probe["streams"]
        .as_array()
        .context("source audio streams missing")?;
    if streams.len() != 1
        || streams[0]["codec_type"] != "audio"
        || streams[0]["sample_rate"]
            .as_str()
            .and_then(|v| v.parse::<u32>().ok())
            != Some(m.sample_rate)
        || streams[0]["channels"] != 2
        || !matches!(
            streams[0]["codec_name"].as_str(),
            Some("pcm_s16le" | "pcm_s24le" | "pcm_s32le" | "pcm_f32le" | "pcm_f64le")
        )
    {
        bail!("master requires original stereo PCM at declared rate");
    }
    let raw = tempfile::NamedTempFile::new()?;
    if !Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-i"])
        .arg(source)
        .args(["-map", "0:a:0", "-c:a", "pcm_s24le", "-f", "s24le"])
        .arg(raw.path())
        .status()?
        .success()
    {
        bail!("master PCM normalization failed");
    }
    if fs::metadata(raw.path())?.len()
        != m.source_samples
            .checked_mul(6)
            .context("source PCM overflow")?
    {
        bail!("source PCM sample count changed");
    }
    use std::io::{Seek, SeekFrom};
    let mut file = fs::File::open(raw.path())?;
    file.seek(SeekFrom::Start(
        m.start_sample
            .checked_mul(6)
            .context("PCM offset overflow")?,
    ))?;
    let count = m.end_sample - m.start_sample;
    let mut writer = std::io::BufWriter::new(fs::File::create(destination)?);
    use sha2::{Digest, Sha256};
    let mut selected = Sha256::new();
    let mut envelope = Sha256::new();
    for i in 0..count {
        let mut sample = [0u8; 6];
        file.read_exact(&mut sample)?;
        selected.update(sample);
        let gain = if i < m.fade_in_samples {
            Some((i, m.fade_in_samples))
        } else if i >= count - m.fade_out_samples {
            Some((count - 1 - i, m.fade_out_samples))
        } else {
            None
        };
        if let Some((n, d)) = gain {
            for c in [0, 3] {
                let v = i32::from_le_bytes([0, sample[c], sample[c + 1], sample[c + 2]]) >> 8;
                let v = (i128::from(v) * i128::from(n) / i128::from(d)) as i32;
                sample[c..c + 3].copy_from_slice(&v.to_le_bytes()[..3]);
            }
        }
        envelope.update(sample);
        writer.write_all(&sample)?;
    }
    if selected
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        != m.selected_pcm_sha256
        || envelope
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
            != m.envelope_pcm_sha256
    {
        bail!("selected/enveloped PCM identity differs");
    }
    writer.flush()?;
    Ok(destination.into())
}

fn verify_video_probe(probe: &Value, manifest: &BrowserCompositionManifest) -> Result<usize> {
    let streams = probe["streams"]
        .as_array()
        .ok_or_else(|| anyhow!("invalid ffprobe streams"))?;
    let video = streams
        .iter()
        .find(|stream| stream["codec_type"] == "video")
        .ok_or_else(|| anyhow!("browser composition has no video stream"))?;
    if manifest.lossless_master.is_some()
        && (video["codec_name"] != "ffv1"
            || video["pix_fmt"] != "yuv444p"
            || streams
                .iter()
                .find(|s| s["codec_type"] == "audio")
                .is_none_or(|a| {
                    a["codec_name"] != "pcm_s24le"
                        || a["channels"] != 2
                        || a["sample_rate"]
                            .as_str()
                            .and_then(|v| v.parse::<u32>().ok())
                            != manifest.lossless_master.as_ref().map(|m| m.sample_rate)
                }))
    {
        bail!("invalid lossless master codecs");
    }
    if video["width"].as_u64() != Some(u64::from(manifest.width))
        || video["height"].as_u64() != Some(u64::from(manifest.height))
        || fraction(video["avg_frame_rate"].as_str().unwrap_or("0/1"))? != f64::from(manifest.fps)
        || (manifest.lossless_master.is_none()
            && video["nb_frames"]
                .as_str()
                .and_then(|value| value.parse::<u64>().ok())
                != Some(manifest.frame_count))
    {
        bail!("browser composition output has dropped, duplicated, or non-CFR video frames");
    }
    Ok(streams
        .iter()
        .filter(|stream| stream["codec_type"] == "audio")
        .count())
}

fn validate_poster(options: &RenderOptions, frame_count: u64) -> Result<()> {
    let Some(poster) = &options.poster else {
        return Ok(());
    };
    if frame_count != 1 {
        bail!("a lossless poster requires exactly one composition frame");
    }
    if poster.exists()
        || poster
            .extension()
            .is_none_or(|ext| !ext.eq_ignore_ascii_case("png"))
    {
        bail!("poster must be a new PNG file");
    }
    let parent = |path: &Path| {
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_path_buf()
    };
    let output_parent = parent(&options.output);
    let poster_parent = parent(poster);
    fs::create_dir_all(&output_parent)?;
    fs::create_dir_all(&poster_parent)?;
    if output_parent.canonicalize()? != poster_parent.canonicalize()? {
        bail!("poster must be beside the composition video and receipt");
    }
    Ok(())
}

fn publish_poster(source: &Path, destination: &Path) -> Result<Poster> {
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = Builder::new()
        .prefix(".reel-poster-")
        .suffix(".png")
        .tempfile_in(parent)?;
    std::io::copy(&mut fs::File::open(source)?, &mut temp)?;
    temp.persist_noclobber(destination)?;
    Ok(Poster {
        path: destination
            .file_name()
            .and_then(|n| n.to_str())
            .context("non-UTF8 poster name")?
            .into(),
        sha256: production::sha256_path(destination)?,
        bytes: fs::metadata(destination)?.len(),
    })
}

fn verify_poster(poster: &Poster, frame: &FrameLineage, root: &Path) -> Result<()> {
    let relative = relative_path(&poster.path)?;
    if relative.components().count() != 1
        || relative
            .extension()
            .is_none_or(|ext| !ext.eq_ignore_ascii_case("png"))
    {
        bail!("poster must be a sibling file");
    }
    let root = if root.as_os_str().is_empty() {
        Path::new(".")
    } else {
        root
    }
    .canonicalize()?;
    let path = root.join(relative).canonicalize()?;
    if !path.starts_with(&root) {
        bail!("poster escapes its receipt directory");
    }
    if poster.sha256 != frame.sha256
        || poster.bytes != fs::metadata(&path)?.len()
        || production::sha256_path(path)? != poster.sha256
    {
        bail!("poster differs from the exact captured composition frame");
    }
    Ok(())
}

pub fn render(options: &RenderOptions) -> Result<BrowserCompositionReport> {
    if options.output.exists() {
        bail!("browser composition output must be a new file");
    }
    let manifest_path = options.manifest.canonicalize()?;
    let manifest = load_manifest(&manifest_path)?;
    if manifest.lossless_master.is_some() && manifest.source_clock.is_some() {
        bail!("choose one explicit browser source-clock declaration");
    }
    if manifest.audio_window.is_some() {
        bail!("exact PCM windows require the bounded native browser renderer");
    }
    validate_poster(options, manifest.frame_count)?;
    let root = options.asset_root.canonicalize()?;
    let stage = TempDir::new()?;
    let mut inputs = vec![InputLineage {
        role: "manifest".into(),
        relative_path: "manifest".into(),
        sha256: production::sha256_path(&manifest_path)?,
    }];
    inputs.push(stage_asset(
        &root,
        stage.path(),
        &manifest.source,
        "source",
    )?);
    for asset in &manifest.assets {
        inputs.push(stage_asset(&root, stage.path(), asset, "asset")?);
    }
    let audio = manifest
        .audio
        .as_ref()
        .map(|asset| {
            let lineage = stage_asset(&root, stage.path(), asset, "master-audio")?;
            Ok::<_, anyhow::Error>((stage.path().join(relative_path(&asset.path)?), lineage))
        })
        .transpose()?;
    if let Some((_, lineage)) = &audio {
        inputs.push(lineage.clone());
    }

    verify_source_clock(&root, &manifest)?;
    let browser = options.browser.canonicalize()?;
    let port = free_port()?;
    let profile = TempDir::new()?;
    let child = Command::new(&browser)
        .args([
            "--headless=new",
            "--disable-gpu",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-component-update",
            "--disable-extensions",
            "--disable-sync",
            "--metrics-recording-only",
            "--mute-audio",
            "--hide-scrollbars",
            "--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE localhost",
            &format!("--remote-debugging-port={port}"),
            &format!("--user-data-dir={}", profile.path().display()),
            "about:blank",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let _guard = BrowserGuard(child);
    let ws_url = discover_page(port)?;
    let (mut socket, _) = tungstenite::connect(&ws_url)?;
    if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    }
    let mut id = 1_u64;
    let version = cdp(&mut socket, id, "Browser.getVersion", json!({}))?;
    id += 1;
    let browser_version = version["product"]
        .as_str()
        .ok_or_else(|| anyhow!("browser did not report a product version"))?
        .to_string();
    cdp(&mut socket, id, "Page.enable", json!({}))?;
    id += 1;
    cdp(&mut socket, id, "Runtime.enable", json!({}))?;
    id += 1;
    cdp(&mut socket, id, "Network.enable", json!({}))?;
    id += 1;
    cdp(
        &mut socket,
        id,
        "Network.setBlockedURLs",
        json!({"urls": ["http://*", "https://*", "ws://*", "wss://*"]}),
    )?;
    id += 1;
    let source_clock = if let Some(master) = &manifest.lossless_master {
        Some(
            json!({"sampleRate":master.sample_rate,"startSample":master.start_sample,"endSample":master.end_sample,"anchors":master.anchors,"anchorFrames":anchor_frames(master,manifest.fps)?}),
        )
    } else {
        source_clock_event(&manifest, |asset, limit| {
            let mut raw = Vec::new();
            fs::File::open(stage.path().join(relative_path(&asset.path)?))?
                .take(limit + 1)
                .read_to_end(&mut raw)?;
            if raw.len() as u64 > limit {
                bail!("browser clock metadata exceeds bound");
            }
            Ok(raw)
        })?
    };
    let clock = frame_clock_with_source(manifest.fps, source_clock.as_ref());
    cdp(
        &mut socket,
        id,
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source": clock}),
    )?;
    id += 1;
    cdp(
        &mut socket,
        id,
        "Emulation.setDeviceMetricsOverride",
        json!({"width":manifest.width,"height":manifest.height,"deviceScaleFactor":1,"mobile":false}),
    )?;
    id += 1;
    let source = stage
        .path()
        .join(relative_path(&manifest.source.path)?)
        .canonicalize()?;
    let expected_url = file_url(&source);
    cdp(
        &mut socket,
        id,
        "Page.navigate",
        json!({"url":expected_url}),
    )?;
    id += 1;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let state = cdp(
            &mut socket,
            id,
            "Runtime.evaluate",
            json!({"expression":"JSON.stringify({url:location.href,state:document.readyState})","returnByValue":true}),
        )?;
        id += 1;
        if let Some(value) = state["result"]["value"].as_str() {
            let loaded: Value = serde_json::from_str(value)?;
            if loaded["url"] == expected_url && loaded["state"] == "complete" {
                break;
            }
        }
        if Instant::now() >= deadline {
            bail!("browser composition did not finish loading");
        }
        thread::sleep(Duration::from_millis(20));
    }
    if manifest.lossless_master.is_some() {
        let fonts = cdp(
            &mut socket,
            id,
            "Runtime.evaluate",
            json!({"expression":"document.fonts.ready.then(()=>Array.from(document.fonts).every(f=>f.status==='loaded'))","returnByValue":true,"awaitPromise":true}),
        )?;
        id += 1;
        if fonts["result"]["value"] != true {
            bail!("master fonts did not load");
        }
    }
    if options.clean_picture {
        cdp(
            &mut socket,
            id,
            "Runtime.evaluate",
            json!({"expression":"document.documentElement.dataset.reelCleanPicture='true';const s=document.createElement('style');s.textContent='[data-reel-disclosure],.reel-disclosure{display:none!important}';document.head.appendChild(s)"}),
        )?;
        id += 1;
    }
    let frames_dir = stage.path().join("captured-frames");
    fs::create_dir(&frames_dir)?;
    let mut frames = Vec::with_capacity(manifest.frame_count as usize);
    for frame in 0..manifest.frame_count {
        let expression = format!(
            "window.__reelSetFrame({frame});JSON.stringify({{url:location.href,frame:window.__REEL_FRAME__}})"
        );
        let state = cdp(
            &mut socket,
            id,
            "Runtime.evaluate",
            json!({"expression":expression,"returnByValue":true,"awaitPromise":true}),
        )?;
        id += 1;
        let state: Value = serde_json::from_str(state["result"]["value"].as_str().unwrap_or("{}"))?;
        if state["url"] != expected_url || state["frame"].as_u64() != Some(frame) {
            bail!(
                "composition navigated away or did not accept frame {frame}: expected {expected_url}, observed {}",
                state
            );
        }
        let shot = cdp(
            &mut socket,
            id,
            "Page.captureScreenshot",
            json!({"format":"png","fromSurface":true,"captureBeyondViewport":false}),
        )?;
        id += 1;
        let bytes = base64::engine::general_purpose::STANDARD.decode(
            shot["data"]
                .as_str()
                .ok_or_else(|| anyhow!("Chromium returned no screenshot"))?,
        )?;
        let path = frames_dir.join(format!("frame-{frame:08}.png"));
        fs::write(&path, &bytes)?;
        let sha256 = production::sha256_path(&path)?;
        let sequence_sha256 = production::sha256_bytes(format!("{frame}:{sha256}").as_bytes());
        let stable_background_sha256 = stable_background_hash(&bytes, &manifest.dynamic_regions)?;
        frames.push(FrameLineage {
            index: frame,
            sha256,
            sequence_sha256,
            stable_background_sha256,
        });
    }
    if !manifest.dynamic_regions.is_empty()
        && frames
            .iter()
            .map(|frame| frame.stable_background_sha256.as_deref())
            .collect::<BTreeSet<_>>()
            .len()
            != 1
    {
        bail!("pixels outside declared dynamic regions changed between browser frames");
    }
    drop(socket);
    let master_pcm = manifest
        .lossless_master
        .as_ref()
        .map(|m| {
            prepare_master_pcm(
                &stage.path().join("master.pcm"),
                &audio.as_ref().context("master audio missing")?.0,
                m,
            )
        })
        .transpose()?;
    let duration_seconds = manifest.frame_count as f64 / f64::from(manifest.fps);
    let output_parent = options.output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_parent)?;
    let temp = Builder::new()
        .prefix(".reel-browser-")
        .suffix(if manifest.lossless_master.is_some() {
            ".mkv"
        } else {
            ".mp4"
        })
        .tempfile_in(output_parent)?
        .into_temp_path();
    let adapter = FfmpegAdapter;
    let mut args = vec!["-y", "-hide_banner", "-loglevel", "warning", "-framerate"]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    args.push(manifest.fps.to_string());
    args.extend([
        "-i".into(),
        adapter.path_argument(&frames_dir.join("frame-%08d.png"))?,
    ]);
    if let Some((audio_path, _)) = &audio {
        if let Some(m) = &manifest.lossless_master {
            args.extend([
                "-f".into(),
                "s24le".into(),
                "-ar".into(),
                m.sample_rate.to_string(),
                "-ac".into(),
                "2".into(),
            ]);
        }
        args.extend([
            "-i".into(),
            adapter.path_argument(master_pcm.as_ref().unwrap_or(audio_path))?,
        ]);
    }
    if audio.is_some() && manifest.lossless_master.is_none() {
        args.extend([
            "-filter_complex".into(),
            format!("[1:a:0]apad,atrim=duration={duration_seconds:.9}[a]"),
            "-map".into(),
            "0:v:0".into(),
            "-map".into(),
            "[a]".into(),
        ]);
    }
    args.extend([
        "-frames:v".into(),
        manifest.frame_count.to_string(),
        "-r".into(),
        manifest.fps.to_string(),
    ]);
    if manifest.lossless_master.is_some() {
        args.extend(
            [
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-c:v",
                "ffv1",
                "-level",
                "3",
                "-pix_fmt",
                "yuv444p",
                "-c:a",
                "pcm_s24le",
            ]
            .map(str::to_string),
        );
    } else {
        args.extend(
            [
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-crf",
                "18",
                "-movflags",
                "+faststart",
            ]
            .map(str::to_string),
        );
        if audio.is_some() {
            args.extend(["-c:a", "aac", "-b:a", "192k"].map(str::to_string));
        }
    }
    args.push(adapter.path_argument(&temp)?);
    adapter.run_ffmpeg(&args, &[])?;
    let output_duration_ms =
        (adapter.ffprobe_duration(&temp)?.parse::<f64>()? * 1000.0).round() as u64;
    let duration_ms = (duration_seconds * 1000.0).round() as u64;
    if output_duration_ms != duration_ms {
        bail!("browser composition output duration is not exact");
    }
    let probe: Value = if manifest.lossless_master.is_some() {
        full_probe(&temp)?
    } else {
        serde_json::from_str(&adapter.ffprobe_json(&temp)?)?
    };
    let audio_streams = verify_video_probe(&probe, &manifest)?;
    if audio_streams != usize::from(audio.is_some()) {
        bail!("browser composition audio stream binding failed");
    }
    let master_verification = if let Some(m) = &manifest.lossless_master {
        let expected = decode_hash(
            &frames_dir.join("frame-%08d.png"),
            "yuv444p",
            Some(manifest.fps),
        )?;
        let actual = decode_hash(&temp, "yuv444p", None)?;
        if expected != actual || decode_hash(&temp, "s24le", None)? != m.envelope_pcm_sha256 {
            bail!("lossless master decoded pixel/PCM identity failed");
        }
        Some(MasterVerification {
            encoding: "ffv1-yuv444p-pcm-s24le.v1".into(),
            selected_pcm_sha256: m.selected_pcm_sha256.clone(),
            envelope_pcm_sha256: m.envelope_pcm_sha256.clone(),
            samples: m.end_sample - m.start_sample,
            decoded_yuv444_sha256: actual,
        })
    } else {
        None
    };
    temp.persist(&options.output)?;
    let poster = options
        .poster
        .as_ref()
        .map(|path| publish_poster(&frames_dir.join("frame-00000000.png"), path))
        .transpose()?;
    if let Some(poster) = &poster {
        verify_poster(
            poster,
            &frames[0],
            options
                .output
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?;
    }
    let report = BrowserCompositionReport {
        schema: ARTIFACT_SCHEMA.into(),
        manifest_sha256: production::sha256_path(&manifest_path)?,
        browser_version,
        width: manifest.width,
        height: manifest.height,
        fps: manifest.fps,
        frame_count: manifest.frame_count,
        duration_ms,
        clean_picture: options.clean_picture,
        persistent_dom: true,
        network_policy: "local-staged-assets-only; external HTTP(S)/WebSocket blocked".into(),
        inputs,
        frames,
        output_sha256: production::sha256_path(&options.output)?,
        output_bytes: fs::metadata(&options.output)?.len(),
        output_duration_ms,
        audio_streams,
        master_verification,
        poster,
    };
    fs::write(
        options.output.with_extension("browser-artifacts.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

pub fn check(
    report_path: &Path,
    manifest_path: &Path,
    asset_root: &Path,
    video: &Path,
) -> Result<BrowserCompositionCheck> {
    let report: BrowserCompositionReport = serde_json::from_slice(&fs::read(report_path)?)?;
    let manifest = load_manifest(manifest_path)?;
    if report.schema != ARTIFACT_SCHEMA
        || report.manifest_sha256 != production::sha256_path(manifest_path)?
        || report.frame_count != manifest.frame_count
        || report.fps != manifest.fps
        || report.frames.len() as u64 != manifest.frame_count
        || !report.persistent_dom
    {
        bail!("browser composition artifact contract is inconsistent");
    }
    for (expected, frame) in report.frames.iter().enumerate() {
        if frame.index != expected as u64
            || frame.sequence_sha256
                != production::sha256_bytes(format!("{}:{}", frame.index, frame.sha256).as_bytes())
        {
            bail!("browser composition frame sequence is dropped, duplicated, or reordered");
        }
    }
    if let Some(poster) = &report.poster {
        if report.frame_count != 1 {
            bail!("poster requires a single-frame composition receipt");
        }
        verify_poster(
            poster,
            &report.frames[0],
            report_path.parent().unwrap_or(Path::new(".")),
        )?;
    }
    if !manifest.dynamic_regions.is_empty()
        && report
            .frames
            .iter()
            .map(|frame| frame.stable_background_sha256.as_deref())
            .collect::<BTreeSet<_>>()
            .len()
            != 1
    {
        bail!("browser composition stable background evidence is inconsistent");
    }
    let root = asset_root.canonicalize()?;
    for asset in std::iter::once(&manifest.source)
        .chain(&manifest.assets)
        .chain(manifest.audio.iter())
    {
        let path = root.join(relative_path(&asset.path)?).canonicalize()?;
        if !path.starts_with(&root)
            || !production::sha256_path(path)?.eq_ignore_ascii_case(&asset.sha256)
        {
            bail!("browser composition input changed after render");
        }
    }
    verify_source_clock(&root, &manifest)?;
    let output_hash = production::sha256_path(video)?;
    if output_hash != report.output_sha256 {
        bail!("browser composition video hash mismatch");
    }
    let duration_ms =
        (FfmpegAdapter.ffprobe_duration(video)?.parse::<f64>()? * 1000.0).round() as u64;
    if duration_ms != report.duration_ms || duration_ms != report.output_duration_ms {
        bail!("browser composition duration is not exact");
    }
    let probe: Value = if manifest.lossless_master.is_some() {
        full_probe(video)?
    } else {
        serde_json::from_str(&FfmpegAdapter.ffprobe_json(video)?)?
    };
    let audio_streams = verify_video_probe(&probe, &manifest)?;
    match (&manifest.lossless_master, &report.master_verification) {
        (None, None) => {}
        (Some(m), Some(e)) => {
            let scratch = TempDir::new()?;
            let source = root.join(relative_path(
                &manifest
                    .audio
                    .as_ref()
                    .context("master audio missing")?
                    .path,
            )?);
            prepare_master_pcm(&scratch.path().join("selected.pcm"), &source, m)?;
            if e.encoding != "ffv1-yuv444p-pcm-s24le.v1"
                || e.selected_pcm_sha256 != m.selected_pcm_sha256
                || e.envelope_pcm_sha256 != m.envelope_pcm_sha256
                || e.samples != m.end_sample - m.start_sample
                || decode_hash(video, "s24le", None)? != e.envelope_pcm_sha256
                || decode_hash(video, "yuv444p", None)? != e.decoded_yuv444_sha256
            {
                bail!("master verification changed");
            }
        }
        _ => bail!("master verification mode mismatch"),
    }
    if audio_streams != report.audio_streams {
        bail!("browser composition audio stream count changed");
    }
    Ok(BrowserCompositionCheck {
        schema: "reel.browser-composition-check.v0.1".into(),
        passed: true,
        verified_inputs: report.inputs.len(),
        verified_frames: report.frames.len(),
        output_sha256: output_hash,
        duration_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poster_preserves_capture_bytes_and_rejects_tampering_and_overwrite() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("capture.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([18, 86, 52, 255]))
            .save(&source)
            .unwrap();
        let destination = dir.path().join("poster.png");
        let poster = publish_poster(&source, &destination).unwrap();
        let frame = FrameLineage {
            index: 0,
            sha256: production::sha256_path(&source).unwrap(),
            sequence_sha256: String::new(),
            stable_background_sha256: None,
        };
        verify_poster(&poster, &frame, dir.path()).unwrap();
        assert_eq!(fs::read(&source).unwrap(), fs::read(&destination).unwrap());
        assert!(publish_poster(&source, &destination).is_err());
        assert_eq!(fs::read(&source).unwrap(), fs::read(&destination).unwrap());
        fs::write(&destination, b"tampered").unwrap();
        assert!(verify_poster(&poster, &frame, dir.path()).is_err());
        let escaped = Poster {
            path: "../capture.png".into(),
            ..poster
        };
        assert!(verify_poster(&escaped, &frame, dir.path()).is_err());
    }

    #[test]
    fn poster_preflight_rejects_animation_existing_files_and_other_directories() {
        let dir = TempDir::new().unwrap();
        let mut options = RenderOptions {
            manifest: "unused".into(),
            asset_root: dir.path().into(),
            browser: "unused".into(),
            output: dir.path().join("composition.mp4"),
            clean_picture: true,
            poster: Some(dir.path().join("poster.png")),
        };
        assert!(validate_poster(&options, 2).is_err());
        validate_poster(&options, 1).unwrap();
        fs::write(options.poster.as_ref().unwrap(), b"keep existing bytes").unwrap();
        assert!(validate_poster(&options, 1).is_err());
        options.poster = Some(dir.path().join("other/poster.png"));
        assert!(validate_poster(&options, 1).is_err());
        assert_eq!(
            fs::read(dir.path().join("poster.png")).unwrap(),
            b"keep existing bytes"
        );
    }

    #[test]
    fn poster_check_accepts_a_receipt_filename_in_the_current_directory() {
        let file = Builder::new()
            .prefix(".reel-poster-test-")
            .suffix(".png")
            .tempfile_in(".")
            .unwrap();
        image::RgbaImage::from_pixel(8, 8, image::Rgba([18, 86, 52, 255]))
            .save(file.path())
            .unwrap();
        let sha256 = production::sha256_path(file.path()).unwrap();
        let poster = Poster {
            path: file.path().file_name().unwrap().to_str().unwrap().into(),
            sha256: sha256.clone(),
            bytes: fs::metadata(file.path()).unwrap().len(),
        };
        let frame = FrameLineage {
            index: 0,
            sha256,
            sequence_sha256: String::new(),
            stable_background_sha256: None,
        };
        verify_poster(&poster, &frame, Path::new("")).unwrap();
    }

    #[test]
    #[ignore = "requires local Chromium-compatible browser and FFmpeg"]
    fn single_frame_browser_poster_is_the_exact_capture_and_is_rechecked() {
        let edge = PathBuf::from(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe");
        assert!(
            edge.is_file(),
            "explicit browser integration test needs Edge"
        );
        let dir = TempDir::new().unwrap();
        let html = dir.path().join("poster.html");
        fs::write(
            &html,
            "<!doctype html><style>html,body{margin:0;background:#125634}</style>",
        )
        .unwrap();
        let manifest = BrowserCompositionManifest {
            schema: MANIFEST_SCHEMA.into(),
            source: HashedAsset {
                path: "poster.html".into(),
                sha256: production::sha256_path(&html).unwrap(),
            },
            assets: vec![],
            width: 160,
            height: 90,
            fps: 25,
            frame_count: 1,
            dynamic_regions: vec![],
            audio: None,
            source_clock: None,
            audio_window: None,
            lossless_master: None,
        };
        let manifest_path = dir.path().join("composition.json");
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let video = dir.path().join("composition.mp4");
        let png = dir.path().join("composition.png");
        let report = render(&RenderOptions {
            manifest: manifest_path.clone(),
            asset_root: dir.path().into(),
            browser: edge,
            output: video.clone(),
            clean_picture: true,
            poster: Some(png.clone()),
        })
        .unwrap();
        let poster = report.poster.unwrap();
        assert_eq!(poster.sha256, report.frames[0].sha256);
        assert_eq!(
            image::open(&png).unwrap().to_rgba8().get_pixel(5, 5).0,
            [18, 86, 52, 255]
        );
        let receipt = video.with_extension("browser-artifacts.json");
        assert!(
            check(&receipt, &manifest_path, dir.path(), &video)
                .unwrap()
                .passed
        );
        fs::write(&png, b"not the captured frame").unwrap();
        assert!(check(&receipt, &manifest_path, dir.path(), &video).is_err());
    }
    #[test]
    #[ignore = "requires local Chromium-compatible browser and FFmpeg"]
    fn ffv1_master_keeps_three_overlapping_cels_and_exact_pcm_window() {
        let dir = TempDir::new().unwrap();
        let html = dir.path().join("title.html");
        fs::copy(r"C:\Windows\Fonts\arial.ttf", dir.path().join("title.ttf")).unwrap();
        fs::write(&html,r##"<!doctype html><style>@font-face{font-family:Title;src:url(title.ttf)}html,body{margin:0;background:#faf7ed;font-family:Title}svg{position:absolute}</style><svg width="160" height="90"><rect id="a" x="10" y="10" width="60" height="60" fill="red"/><rect id="b" x="40" y="20" width="60" height="60" fill="green"/><rect id="c" x="70" y="30" width="60" height="60" fill="blue"/><text id="t" x="20" y="45">Editable title</text></svg><script>addEventListener('reelframe',e=>{const k=e.detail.sourceClock.anchorFrames;a.style.opacity=e.detail.frame>=k[1]?1:0;b.style.opacity=e.detail.frame>=k[2]?1:0;c.style.opacity=e.detail.frame>=k[3]?1:0;b.setAttribute('x',40+e.detail.frame);c.setAttribute('y',30-e.detail.frame);t.style.opacity=e.detail.frame<e.detail.sourceClock.anchorFrames[1]?0:1})</script>"##).unwrap();
        let wav = dir.path().join("theme.wav");
        assert!(
            Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=48000:duration=0.3",
                    "-ac",
                    "2",
                    "-c:a",
                    "pcm_s24le"
                ])
                .arg(&wav)
                .status()
                .unwrap()
                .success()
        );
        let raw = dir.path().join("theme.pcm");
        assert!(
            Command::new("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(&wav)
                .args(["-f", "s24le"])
                .arg(&raw)
                .status()
                .unwrap()
                .success()
        );
        let bytes = fs::read(&raw).unwrap();
        let selected = &bytes[1600 * 6..11200 * 6];
        let hash = production::sha256_bytes(selected);
        fs::write(
            dir.path().join("clock-evidence.json"),
            br#"{"fixture":"producer sample selections; no human approval"}"#,
        )
        .unwrap();
        let evidence = fs::read(dir.path().join("clock-evidence.json")).unwrap();
        fs::write(dir.path().join("clock.json"),serde_json::to_vec(&json!({"schema":"reel.presentation-source-clock.v1","source_sha256":production::sha256_path(&wav).unwrap(),"sample_rate":48000,"total_samples":14400,"evidence":{"path":"clock-evidence.json","sha256":production::sha256_bytes(&evidence),"bytes":evidence.len()},"anchors":[{"id":"pickup","sample":1600},{"id":"h1","sample":3200},{"id":"h2","sample":4800},{"id":"h3","sample":8000},{"id":"endpoint","sample":11200}]})).unwrap()).unwrap();
        let clock_asset = HashedAsset {
            path: "clock.json".into(),
            sha256: production::sha256_path(dir.path().join("clock.json")).unwrap(),
        };
        let manifest = BrowserCompositionManifest {
            schema: MANIFEST_SCHEMA.into(),
            source: HashedAsset {
                path: "title.html".into(),
                sha256: production::sha256_path(&html).unwrap(),
            },
            assets: vec![
                HashedAsset {
                    path: "title.ttf".into(),
                    sha256: production::sha256_path(dir.path().join("title.ttf")).unwrap(),
                },
                clock_asset.clone(),
                HashedAsset {
                    path: "clock-evidence.json".into(),
                    sha256: production::sha256_bytes(&evidence),
                },
            ],
            width: 160,
            height: 90,
            fps: 30,
            frame_count: 6,
            dynamic_regions: vec![],
            audio: Some(HashedAsset {
                path: "theme.wav".into(),
                sha256: production::sha256_path(&wav).unwrap(),
            }),
            source_clock: None,
            audio_window: None,
            lossless_master: Some(LosslessMaster {
                source_clock: clock_asset,
                anchor_ids: ["pickup", "h1", "h2", "h3", "endpoint"].map(str::to_owned),
                sample_rate: 48000,
                source_samples: 14400,
                start_sample: 1600,
                end_sample: 11200,
                anchors: [1600, 3200, 4800, 8000, 11200],
                selected_pcm_sha256: hash.clone(),
                envelope_pcm_sha256: hash,
                fade_in_samples: 0,
                fade_out_samples: 0,
            }),
        };
        let m = dir.path().join("manifest.json");
        fs::write(&m, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let output = dir.path().join("title.mkv");
        let report = render(&RenderOptions {
            manifest: m.clone(),
            asset_root: dir.path().into(),
            browser: PathBuf::from(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
            output: output.clone(),
            clean_picture: true,
            poster: None,
        })
        .unwrap();
        assert_eq!(report.master_verification.as_ref().unwrap().samples, 9600);
        assert_ne!(report.frames[0].sha256, report.frames[1].sha256);
        assert!(
            check(
                &output.with_extension("browser-artifacts.json"),
                &m,
                dir.path(),
                &output
            )
            .unwrap()
            .passed
        );
        let mut bad = manifest.clone();
        bad.lossless_master.as_mut().unwrap().anchors[2] = 3200;
        fs::write(&m, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(load_manifest(&m).is_err());
        bad = manifest.clone();
        bad.lossless_master.as_mut().unwrap().end_sample -= 1;
        fs::write(&m, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(load_manifest(&m).is_err());
        bad = manifest;
        bad.lossless_master.as_mut().unwrap().envelope_pcm_sha256 = "0".repeat(64);
        assert!(
            prepare_master_pcm(
                &dir.path().join("bad.pcm"),
                &wav,
                bad.lossless_master.as_ref().unwrap()
            )
            .is_err()
        );
    }

    #[test]
    fn linear_master_envelope_has_exact_pcm_samples_and_rejects_wrong_length() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("constant.pcm");
        let mut bytes = vec![];
        for _ in 0..4 {
            for _ in 0..2 {
                bytes.extend_from_slice(&1000i32.to_le_bytes()[..3]);
            }
        }
        fs::write(&input, &bytes).unwrap();
        let wav = dir.path().join("constant.wav");
        assert!(
            Command::new("ffmpeg")
                .args([
                    "-v", "error", "-f", "s24le", "-ar", "48000", "-ac", "2", "-i"
                ])
                .arg(&input)
                .args(["-c:a", "pcm_s24le"])
                .arg(&wav)
                .status()
                .unwrap()
                .success()
        );
        let mut expected = vec![];
        for value in [0i32, 500, 500, 0] {
            for _ in 0..2 {
                expected.extend_from_slice(&value.to_le_bytes()[..3]);
            }
        }
        let mut m = LosslessMaster {
            source_clock: HashedAsset {
                path: "unused".into(),
                sha256: "0".repeat(64),
            },
            anchor_ids: ["pickup", "h1", "h2", "h3", "endpoint"].map(str::to_owned),
            sample_rate: 48000,
            source_samples: 4,
            start_sample: 0,
            end_sample: 4,
            anchors: [0, 1, 2, 3, 4],
            selected_pcm_sha256: production::sha256_bytes(&bytes),
            envelope_pcm_sha256: production::sha256_bytes(&expected),
            fade_in_samples: 2,
            fade_out_samples: 2,
        };
        let output = dir.path().join("envelope.pcm");
        prepare_master_pcm(&output, &wav, &m).unwrap();
        assert_eq!(fs::read(output).unwrap(), expected);
        m.source_samples = 5;
        assert!(prepare_master_pcm(&dir.path().join("wrong.pcm"), &wav, &m).is_err());
    }

    #[test]
    fn source_sample_anchors_use_nearest_rational_frames() {
        let m = LosslessMaster {
            source_clock: HashedAsset {
                path: "clock.json".into(),
                sha256: "0".repeat(64),
            },
            anchor_ids: ["pickup", "h1", "h2", "h3", "endpoint"].map(str::to_owned),
            sample_rate: 48000,
            source_samples: 9600,
            start_sample: 0,
            end_sample: 9600,
            anchors: [0, 800, 2400, 5600, 9600],
            selected_pcm_sha256: "0".repeat(64),
            envelope_pcm_sha256: "0".repeat(64),
            fade_in_samples: 0,
            fade_out_samples: 0,
        };
        assert_eq!(anchor_frames(&m, 30).unwrap(), vec![0, 1, 2, 4, 6]);
    }

    #[test]
    fn rejects_parent_paths_and_external_references() {
        assert!(relative_path("../outside.svg").is_err());
        assert!(relative_path("safe/score.svg").is_ok());
    }

    #[test]
    fn dynamic_region_mask_detects_only_background_instability() {
        let mut first = image::RgbaImage::from_pixel(8, 8, image::Rgba([20, 30, 40, 255]));
        let mut second = first.clone();
        first.put_pixel(2, 2, image::Rgba([255, 190, 0, 255]));
        second.put_pixel(2, 2, image::Rgba([0, 0, 0, 255]));
        let region = PixelRegion {
            x: 2,
            y: 2,
            width: 1,
            height: 1,
        };
        let encode = |image: image::RgbaImage| {
            let mut bytes = Vec::new();
            image::DynamicImage::ImageRgba8(image)
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    image::ImageFormat::Png,
                )
                .unwrap();
            bytes
        };
        assert_eq!(
            stable_background_hash(&encode(first), std::slice::from_ref(&region)).unwrap(),
            stable_background_hash(&encode(second.clone()), std::slice::from_ref(&region)).unwrap()
        );
        second.put_pixel(7, 7, image::Rgba([99, 99, 99, 255]));
        let unchanged = image::RgbaImage::from_pixel(8, 8, image::Rgba([20, 30, 40, 255]));
        assert_ne!(
            stable_background_hash(&encode(unchanged), std::slice::from_ref(&region)).unwrap(),
            stable_background_hash(&encode(second), &[region]).unwrap()
        );
    }

    #[test]
    #[ignore = "requires local Chromium-compatible browser and FFmpeg"]
    fn svg_score_highlight_changes_without_background_instability() {
        let edge = PathBuf::from(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe");
        if !edge.is_file() {
            return;
        }
        let dir = TempDir::new().unwrap();
        let html = dir.path().join("score.html");
        fs::write(
            &html,
            r##"<!doctype html><html><head><style>html,body{margin:0;background:#faf7ed}.note{fill:#222}.active{fill:#d4a017}</style></head><body><svg xmlns="http://www.w3.org/2000/svg" width="160" height="90"><path d="M0 20H160M0 30H160M0 40H160M0 50H160M0 60H160" stroke="#444"/><ellipse id="a" class="note" cx="60" cy="40" rx="8" ry="6"/><ellipse id="b" class="note" cx="100" cy="50" rx="8" ry="6"/></svg><script>addEventListener('reelframe',e=>{a.classList.toggle('active',e.detail.frame===0);b.classList.toggle('active',e.detail.frame!==0)})</script></body></html>"##,
        )
        .unwrap();
        let manifest = BrowserCompositionManifest {
            schema: MANIFEST_SCHEMA.into(),
            source: HashedAsset {
                path: "score.html".into(),
                sha256: production::sha256_path(&html).unwrap(),
            },
            assets: vec![],
            width: 160,
            height: 90,
            fps: 10,
            frame_count: 2,
            dynamic_regions: vec![PixelRegion {
                x: 50,
                y: 30,
                width: 60,
                height: 30,
            }],
            audio: None,
            source_clock: None,
            audio_window: None,
            lossless_master: None,
        };
        let manifest_path = dir.path().join("composition.yaml");
        fs::write(&manifest_path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
        let output = dir.path().join("score.mp4");
        let report = render(&RenderOptions {
            manifest: manifest_path.clone(),
            asset_root: dir.path().into(),
            browser: edge,
            output: output.clone(),
            clean_picture: true,
            poster: None,
        })
        .unwrap();
        assert_ne!(report.frames[0].sha256, report.frames[1].sha256);
        assert_eq!(
            report.frames[0].stable_background_sha256,
            report.frames[1].stable_background_sha256
        );
        assert_eq!(report.duration_ms, 200);
        assert!(
            check(
                &output.with_extension("browser-artifacts.json"),
                &manifest_path,
                dir.path(),
                &output
            )
            .unwrap()
            .passed
        );
    }
}
