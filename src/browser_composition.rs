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

fn stable_background_hash(png: &[u8], regions: &[PixelRegion]) -> Result<Option<String>> {
    if regions.is_empty() {
        return Ok(None);
    }
    let mut image = image::load_from_memory_with_format(png, image::ImageFormat::Png)?.to_rgba8();
    for region in regions {
        for y in region.y..region.y + region.height {
            for x in region.x..region.x + region.width {
                image.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
            }
        }
    }
    Ok(Some(production::sha256_bytes(image.as_raw())))
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

fn verify_video_probe(probe: &Value, manifest: &BrowserCompositionManifest) -> Result<usize> {
    let streams = probe["streams"]
        .as_array()
        .ok_or_else(|| anyhow!("invalid ffprobe streams"))?;
    let video = streams
        .iter()
        .find(|stream| stream["codec_type"] == "video")
        .ok_or_else(|| anyhow!("browser composition has no video stream"))?;
    if video["width"].as_u64() != Some(u64::from(manifest.width))
        || video["height"].as_u64() != Some(u64::from(manifest.height))
        || fraction(video["avg_frame_rate"].as_str().unwrap_or("0/1"))? != f64::from(manifest.fps)
        || video["nb_frames"]
            .as_str()
            .and_then(|value| value.parse::<u64>().ok())
            != Some(manifest.frame_count)
    {
        bail!("browser composition output has dropped, duplicated, or non-CFR video frames");
    }
    Ok(streams
        .iter()
        .filter(|stream| stream["codec_type"] == "audio")
        .count())
}

pub fn render(options: &RenderOptions) -> Result<BrowserCompositionReport> {
    let manifest_path = options.manifest.canonicalize()?;
    let manifest = load_manifest(&manifest_path)?;
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
    let clock = format!(
        r#"(()=>{{let f=0;const fps={};Object.defineProperty(window,'__REEL_FRAME__',{{get:()=>f}});window.__reelSetFrame=(n)=>{{f=n;document.documentElement.dataset.reelFrame=String(n);document.documentElement.dataset.reelTime=(n/fps).toFixed(9);window.dispatchEvent(new CustomEvent('reelframe',{{detail:{{frame:n,time:n/fps,fps}}}}));}};Date.now=()=>Math.round(f*1000/fps);window.requestAnimationFrame=(cb)=>{{cb(f*1000/fps);return f+1;}};window.cancelAnimationFrame=()=>{{}};}})()"#,
        manifest.fps
    );
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
    let duration_seconds = manifest.frame_count as f64 / f64::from(manifest.fps);
    let output_parent = options.output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_parent)?;
    let temp = Builder::new()
        .prefix(".reel-browser-")
        .suffix(".mp4")
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
        args.extend(["-i".into(), adapter.path_argument(audio_path)?]);
    }
    if audio.is_some() {
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
        "-c:v".into(),
        "libx264".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-crf".into(),
        "18".into(),
        "-movflags".into(),
        "+faststart".into(),
    ]);
    if audio.is_some() {
        args.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "192k".into()]);
    }
    args.push(adapter.path_argument(&temp)?);
    adapter.run_ffmpeg(&args, &[])?;
    let output_duration_ms =
        (adapter.ffprobe_duration(&temp)?.parse::<f64>()? * 1000.0).round() as u64;
    let duration_ms = (duration_seconds * 1000.0).round() as u64;
    if output_duration_ms != duration_ms {
        bail!("browser composition output duration is not exact");
    }
    let probe: Value = serde_json::from_str(&adapter.ffprobe_json(&temp)?)?;
    let audio_streams = verify_video_probe(&probe, &manifest)?;
    if audio_streams != usize::from(audio.is_some()) {
        bail!("browser composition audio stream binding failed");
    }
    temp.persist(&options.output)?;
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
    let output_hash = production::sha256_path(video)?;
    if output_hash != report.output_sha256 {
        bail!("browser composition video hash mismatch");
    }
    let duration_ms =
        (FfmpegAdapter.ffprobe_duration(video)?.parse::<f64>()? * 1000.0).round() as u64;
    if duration_ms != report.duration_ms || duration_ms != report.output_duration_ms {
        bail!("browser composition duration is not exact");
    }
    let probe: Value = serde_json::from_str(&FfmpegAdapter.ffprobe_json(video)?)?;
    let audio_streams = verify_video_probe(&probe, &manifest)?;
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
