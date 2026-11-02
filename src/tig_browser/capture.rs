//! Bounded CDP capture over exact guarded resources and a synchronous consumer.
//! No process launch, output publication, filesystem staging or runtime admission.
use super::Prepared;
use crate::{
    browser_composition::BrowserCompositionManifest, production, scene_delivery::InputFiles,
};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    time::{Duration, Instant},
};

mod protocol;
pub use protocol::{Transport, WebSocketTransport};

/// The caller must hold source/program protection and enclosing job containment
/// for this whole operation, including the consumer's final drain/check.
pub trait Resources {
    fn check(&mut self) -> Result<()>;
    /// Exact declared URL -> verified bytes and MIME. Unknown URLs must fail.
    fn get(&mut self, url: &str, max_bytes: u64) -> Result<(Vec<u8>, &'static str)>;
}
/// Synchronous backpressure: capture never queues the next frame until return.
/// Ownership, bounded writes, cancellation and child reaping belong to the
/// admitted consumer/outer worker; this interface is not process containment.
pub trait Frames {
    fn write(&mut self, index: u64, png: &[u8]) -> Result<()>;
    fn finish(&mut self) -> Result<()>;
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub max_requests: u64,
    pub max_resource_bytes: u64,
    pub max_served_bytes: u64,
    pub max_message_bytes: usize,
    pub max_messages: u64,
    pub max_png_bytes: usize,
    pub max_frames: u64,
    pub max_canvas_pixels: u64,
    pub max_elapsed_ms: u64,
    pub max_io_wait_ms: u64,
}
impl Limits {
    fn validate(&self) -> Result<()> {
        if !(1..=1_000_000).contains(&self.max_requests)
            || !(1..=64 * 1024 * 1024).contains(&self.max_resource_bytes)
            || self.max_served_bytes == 0
            || !(4096..=128 * 1024 * 1024).contains(&self.max_message_bytes)
            || !(1..=10_000_000).contains(&self.max_messages)
            || !(32..=64 * 1024 * 1024).contains(&self.max_png_bytes)
            || !(1..=1_000_000).contains(&self.max_frames)
            || !(1..=67_108_864).contains(&self.max_canvas_pixels)
            || !(1..=86_400_000).contains(&self.max_elapsed_ms)
            || !(1..=60_000).contains(&self.max_io_wait_ms)
        {
            bail!("invalid browser capture limits");
        }
        Ok(())
    }
}

/// Private summary only. It is not an output receipt or evidence of a running
/// browser, resource containment, original production, approval or byte custody.
pub struct Captured {
    pub frames: u64,
    pub frame_sequence_sha256: String,
    pub requests: u64,
    pub served_bytes: u64,
}

pub fn resource_url(path: &str) -> Result<String> {
    tig_core::LogicalPath::parse(path)?;
    let mut url = String::from("https://reel.invalid/v1/");
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            url.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(url, "%{byte:02X}")?;
        }
    }
    Ok(url)
}

fn mime(path: &str) -> Result<&'static str> {
    Ok(
        match path
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "svg" => "image/svg+xml",
            "html" | "htm" => "text/html; charset=utf-8",
            "css" => "text/css; charset=utf-8",
            "js" => "text/javascript; charset=utf-8",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            "ttf" => "font/ttf",
            "otf" => "font/otf",
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            "wav" => "audio/wav",
            _ => bail!("unsupported browser resource type"),
        },
    )
}

/// Borrow the already-held native input provider. No directory view, primary
/// copies, working-file fallback or new cache is created. Each request rechecks
/// the exact length/hash as well as provider authority before and after reading.
pub struct Guarded<'a> {
    files: &'a dyn InputFiles,
    inputs: BTreeMap<String, (&'a crate::scene_delivery::FileRef, &'static str)>,
    check: Box<dyn FnMut() -> Result<()> + 'a>,
}
impl<'a> Guarded<'a> {
    pub fn new(
        plan: &'a Prepared,
        files: &'a dyn InputFiles,
        check: impl FnMut() -> Result<()> + 'a,
    ) -> Result<Self> {
        let inputs = plan
            .inputs
            .iter()
            .map(|(path, file)| Ok((resource_url(path)?, (file, mime(path)?))))
            .collect::<Result<_>>()?;
        Ok(Self {
            files,
            inputs,
            check: Box::new(check),
        })
    }
}
impl Resources for Guarded<'_> {
    fn check(&mut self) -> Result<()> {
        (self.check)()
    }
    fn get(&mut self, url: &str, max_bytes: u64) -> Result<(Vec<u8>, &'static str)> {
        self.check()?;
        let (file, mime) = self
            .inputs
            .get(url)
            .context("browser URL is not declared")?;
        if file.bytes > max_bytes.min(64 * 1024 * 1024) {
            bail!("browser resource exceeds byte budget");
        }
        let path = self.files.verified_path(file)?;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(file.bytes + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 != file.bytes || production::sha256_bytes(&bytes) != file.sha256 {
            bail!("browser resource differs from admitted bytes");
        }
        let mime = *mime;
        self.check()?;
        Ok((bytes, mime))
    }
}

struct Session<'a> {
    transport: &'a mut dyn Transport,
    resources: &'a mut dyn Resources,
    limits: &'a Limits,
    deadline: Instant,
    next: u64,
    messages: u64,
    requests: u64,
    served: u64,
    source_url: String,
    source_served: bool,
}
impl Session<'_> {
    fn check(&mut self) -> Result<Duration> {
        self.resources.check()?;
        Ok(self
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|v| !v.is_zero())
            .context("browser capture deadline elapsed")?
            .min(Duration::from_millis(self.limits.max_io_wait_ms)))
    }
    fn send(&mut self, method: &str, params: Value) -> Result<u64> {
        let remaining = self.check()?;
        let id = self.next;
        self.next = self
            .next
            .checked_add(1)
            .context("browser command overflow")?;
        let raw = serde_json::to_string(&json!({"id":id,"method":method,"params":params}))?;
        if raw.len() > self.limits.max_message_bytes {
            bail!("browser command exceeds byte budget");
        }
        self.transport.send(&raw, remaining)?;
        self.check()?;
        Ok(id)
    }
    fn fetch(&mut self, params: &Value) -> Result<u64> {
        self.requests += 1;
        if self.requests > self.limits.max_requests {
            bail!("browser request budget exceeded");
        }
        let id = params["requestId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
            .context("invalid browser request identity")?;
        let request = &params["request"];
        if request["method"] != "GET"
            || request.get("postData").is_some()
            || request.get("postDataEntries").is_some()
            || request["hasPostData"] == true
            || params.get("responseStatusCode").is_some()
            || params.get("redirectedRequestId").is_some()
            || request["headers"]
                .as_object()
                .is_none_or(|h| h.keys().any(|k| k.eq_ignore_ascii_case("range")))
        {
            bail!("browser request is outside exact resource contract");
        }
        let url = request["url"]
            .as_str()
            .context("browser request URL missing")?;
        let remaining = self
            .limits
            .max_served_bytes
            .checked_sub(self.served)
            .context("browser resource budget exceeded")?;
        // Reserve header/command overhead before reading or base64 allocation,
        // rather than detecting an oversized response after building it.
        let wire_bytes = ((self.limits.max_message_bytes - 2048) / 4 * 3) as u64;
        let admitted = remaining
            .min(self.limits.max_resource_bytes)
            .min(wire_bytes);
        let (bytes, mime) = self.resources.get(url, admitted)?;
        if bytes.len() as u64 > admitted
            || mime.len() > 128
            || !mime.is_ascii()
            || mime.chars().any(char::is_control)
        {
            bail!("browser provider exceeded resource bound");
        }
        self.served += bytes.len() as u64;
        self.source_served |= url == self.source_url;
        self.send("Fetch.fulfillRequest", json!({"requestId":id,"responseCode":200,
            "responseHeaders":[
                {"name":"Content-Type","value":mime},
                {"name":"Content-Length","value":bytes.len().to_string()},
                {"name":"Cache-Control","value":"no-store"},
                {"name":"X-Content-Type-Options","value":"nosniff"},
                {"name":"Content-Security-Policy","value":"default-src 'none'; img-src 'self' data:; font-src 'self'; style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'; connect-src 'none'; worker-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'"}
            ],"body":STANDARD.encode(bytes)}))
    }
    /// Service asynchronous Fetch events even while waiting for another CDP
    /// response. Every fulfill acknowledgement is checked before returning.
    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let wanted = self.send(method, params)?;
        let mut pending = BTreeSet::new();
        let mut result = None;
        while result.is_none() || !pending.is_empty() {
            let remaining = self.check()?;
            self.messages += 1;
            if self.messages > self.limits.max_messages {
                bail!("browser message budget exceeded");
            }
            let raw = self
                .transport
                .receive(self.limits.max_message_bytes, remaining)?;
            if raw.len() > self.limits.max_message_bytes {
                bail!("browser message exceeds byte budget");
            }
            self.check()?;
            let value = tig_core::identity::parse_json(raw.as_bytes())?;
            if let Some(id) = value.get("id") {
                let id = id.as_u64().context("invalid browser response identity")?;
                if value.get("error").is_some_and(|e| !e.is_null()) {
                    bail!("browser command refused");
                }
                if id == wanted && result.is_none() {
                    result = Some(
                        value
                            .get("result")
                            .context("browser response result missing")?
                            .clone(),
                    );
                } else if !pending.remove(&id) {
                    bail!("unexpected browser response identity");
                }
            } else {
                match value["method"]
                    .as_str()
                    .context("browser event method missing")?
                {
                    "Runtime.bindingCalled"
                        if value["params"]["name"] == "__reelResourceRefusal" =>
                    {
                        bail!("browser document violated resource policy");
                    }
                    "Fetch.requestPaused" => {
                        pending.insert(self.fetch(&value["params"])?);
                    }
                    "Fetch.authRequired"
                    | "Inspector.detached"
                    | "Inspector.targetCrashed"
                    | "Target.detachedFromTarget" => {
                        bail!("browser execution detached or requires authentication")
                    }
                    _ => (),
                }
            }
        }
        result.context("browser command did not complete")
    }
}

/// Capture the existing persistent-DOM/reelframe clock semantics without a frame
/// spool. Caller supplies an already admitted browser session and bounded encoder.
/// Errors grant no completed result; outer job containment must reap owned work.
pub fn capture(
    transport: &mut dyn Transport,
    resources: &mut dyn Resources,
    manifest: &BrowserCompositionManifest,
    clean_picture: bool,
    limits: &Limits,
    consumer: &mut dyn Frames,
) -> Result<Captured> {
    limits.validate()?;
    crate::browser_composition::validate_manifest(manifest.clone())?;
    if manifest.frame_count > limits.max_frames
        || u64::from(manifest.width) * u64::from(manifest.height) > limits.max_canvas_pixels
    {
        bail!("browser geometry exceeds capture admission");
    }
    let url = resource_url(&manifest.source.path)?;
    let mut s = Session {
        transport,
        resources,
        limits,
        deadline: Instant::now() + Duration::from_millis(limits.max_elapsed_ms),
        next: 1,
        messages: 0,
        requests: 0,
        served: 0,
        source_url: url.clone(),
        source_served: false,
    };
    s.call("Page.enable", json!({}))?;
    s.call("Runtime.enable", json!({}))?;
    s.call(
        "Runtime.addBinding",
        json!({"name":"__reelResourceRefusal"}),
    )?;
    s.call("Page.addScriptToEvaluateOnNewDocument", json!({"source":"document.addEventListener('securitypolicyviolation',()=>window.__reelResourceRefusal('csp'))"}))?;
    s.call(
        "Fetch.enable",
        json!({"patterns":[{"urlPattern":"*","requestStage":"Request"}],"handleAuthRequests":true}),
    )?;
    s.call("Network.enable", json!({}))?;
    s.call("Network.setCacheDisabled", json!({"cacheDisabled":true}))?;
    s.call("Network.setBypassServiceWorker", json!({"bypass":true}))?;
    s.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source":crate::browser_composition::frame_clock(manifest.fps)}),
    )?;
    s.call("Emulation.setDeviceMetricsOverride", json!({"width":manifest.width,"height":manifest.height,"deviceScaleFactor":1,"mobile":false}))?;
    let nav = s.call("Page.navigate", json!({"url":url}))?;
    if nav.get("errorText").is_some() {
        bail!("browser navigation failed");
    }
    loop {
        let state = evaluation(s.call("Runtime.evaluate", json!({"expression":"JSON.stringify({url:location.href,state:document.readyState})","returnByValue":true}))?)?;
        if state["url"] == url && state["state"] == "complete" {
            break;
        }
        s.check()?;
    }
    if !s.source_served {
        bail!("browser source was not served from the admitted resource view");
    }
    if clean_picture {
        let response = s.call("Runtime.evaluate", json!({"expression":"document.documentElement.dataset.reelCleanPicture='true';const ns=document.documentElement.namespaceURI;const s=ns==='http://www.w3.org/2000/svg'?document.createElementNS(ns,'style'):document.createElement('style');s.textContent='[data-reel-disclosure],.reel-disclosure{display:none!important}';(document.head||document.documentElement).appendChild(s)"}))?;
        if response.get("exceptionDetails").is_some() {
            bail!("browser clean-picture script failed");
        }
    }
    let mut sequence = Sha256::new();
    let mut background = None;
    for frame in 0..manifest.frame_count {
        let expression = format!(
            "window.__reelSetFrame({frame});JSON.stringify({{url:location.href,frame:window.__REEL_FRAME__}})"
        );
        let state = evaluation(s.call(
            "Runtime.evaluate",
            json!({"expression":expression,"returnByValue":true,"awaitPromise":true}),
        )?)?;
        if state["url"] != url || state["frame"].as_u64() != Some(frame) {
            bail!("browser navigation or frame clock differs");
        }
        let shot = s.call(
            "Page.captureScreenshot",
            json!({"format":"png","fromSurface":true,"captureBeyondViewport":false}),
        )?;
        let encoded = shot["data"]
            .as_str()
            .context("browser screenshot missing")?;
        if encoded.len() > limits.max_png_bytes.div_ceil(3) * 4 {
            bail!("browser screenshot exceeds byte budget");
        }
        let png = STANDARD.decode(encoded)?;
        if png.len() > limits.max_png_bytes
            || png.len() < 33
            || &png[..8] != b"\x89PNG\r\n\x1a\n"
            || png[8..12] != [0, 0, 0, 13]
            || &png[12..16] != b"IHDR"
            || u32::from_be_bytes(png[16..20].try_into()?) != manifest.width
            || u32::from_be_bytes(png[20..24].try_into()?) != manifest.height
        {
            bail!("browser screenshot geometry or bytes differ");
        }
        let mut decoder =
            image::ImageReader::with_format(std::io::Cursor::new(&png), image::ImageFormat::Png);
        let mut decode_limits = image::Limits::default();
        decode_limits.max_image_width = Some(manifest.width);
        decode_limits.max_image_height = Some(manifest.height);
        decode_limits.max_alloc = Some(limits.max_canvas_pixels * 8 + 1024 * 1024);
        decoder.limits(decode_limits);
        let mut pixels = decoder.decode()?.into_rgba8();
        if pixels.width() != manifest.width || pixels.height() != manifest.height {
            bail!("decoded browser frame geometry differs");
        }
        let actual = (!manifest.dynamic_regions.is_empty()).then(|| {
            crate::browser_composition::stable_background_pixels(
                &mut pixels,
                &manifest.dynamic_regions,
            )
        });
        drop(pixels);
        if frame == 0 {
            background = actual;
        } else if actual != background {
            bail!("browser background changed outside dynamic regions");
        }
        s.check()?;
        consumer.write(frame, &png)?;
        s.check()?;
        sequence.update(frame.to_be_bytes());
        sequence.update(production::sha256_bytes(&png).as_bytes());
    }
    consumer.finish()?;
    s.check()?;
    Ok(Captured {
        frames: manifest.frame_count,
        frame_sequence_sha256: sequence
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        requests: s.requests,
        served_bytes: s.served,
    })
}
fn evaluation(response: Value) -> Result<Value> {
    if response.get("exceptionDetails").is_some() {
        bail!("browser frame script failed");
    }
    Ok(tig_core::identity::parse_json(
        response["result"]["value"]
            .as_str()
            .context("browser evaluation missing")?
            .as_bytes(),
    )?)
}

#[cfg(test)]
mod tests;
