//! Portable movie production intent. Binding grants no execution or custody.
use crate::{
    Artifact, ErrorKind, Packet, Result, ToolCapability, decode, fail,
    publication::{InputReader, portable_source},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub timeout_seconds: u64,
    pub memory_bytes: u64,
    pub processes: u32,
    pub workspace_bytes: u64,
    pub output_bytes: u64,
    pub frames: u64,
    pub samples: u64,
    pub raw_rgb_bytes: u64,
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        if !(1..=3600).contains(&self.timeout_seconds)
            || !(64 * 1024 * 1024..=16 * 1024 * 1024 * 1024).contains(&self.memory_bytes)
            || !(2..=32).contains(&self.processes)
            || self.output_bytes == 0
            || self.output_bytes > 128 * 1024 * 1024
            || self.workspace_bytes < self.output_bytes
            || self.workspace_bytes > 512 * 1024 * 1024
            || self.frames == 0
            || self.frames > 1_000_000
            || self.samples == 0
            || self.samples > 384_000 * 3600
            || self.raw_rgb_bytes == 0
            || self.raw_rgb_bytes > 16 * 1024 * 1024 * 1024
        {
            return fail(ErrorKind::Budget, "film-process-limits");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub job_path: String,
    pub production_path: String,
    /// Artifact name -> portable path under the owned packet directory.
    pub output_paths: BTreeMap<String, String>,
    pub limits: Limits,
    /// V2 only. A declared streaming envelope, not native execution authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streaming: Option<Streaming>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Streaming {
    pub schema: String,
    pub input_bytes: u64,
    pub input_object_bytes: u64,
    pub verify_millis: u64,
}
impl Streaming {
    pub fn validate(&self) -> Result<()> {
        if self.schema != "reel.workflow-film-streaming.v1" {
            return fail(ErrorKind::Invalid, "film-streaming-version");
        }
        if self.input_object_bytes == 0
            || self.input_object_bytes > self.input_bytes
            || self.input_bytes > 64 * 1024 * 1024 * 1024
            || !(1..=300_000).contains(&self.verify_millis)
        {
            return fail(ErrorKind::Budget, "film-streaming-input-bounds");
        }
        Ok(())
    }
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        match (self.schema.as_str(), &self.streaming) {
            ("reel.workflow-film-render.v1", None) => self.limits.validate(),
            ("reel.workflow-film-render.v2", Some(streaming)) => {
                streaming.validate()?;
                let l = &self.limits;
                if !(1..=18_000).contains(&l.timeout_seconds)
                    || !(64 * 1024 * 1024..=16 * 1024 * 1024 * 1024).contains(&l.memory_bytes)
                    || !(2..=32).contains(&l.processes)
                    || l.output_bytes == 0
                    || l.output_bytes > 16 * 1024 * 1024 * 1024
                    || l.workspace_bytes < l.output_bytes
                    || l.workspace_bytes > 64 * 1024 * 1024 * 1024
                    || l.frames == 0
                    || l.frames > 1_000_000
                    || l.samples == 0
                    || l.samples > 384_000 * 18_000u64
                    || l.raw_rgb_bytes == 0
                    || l.raw_rgb_bytes > 1024 * 1024 * 1024 * 1024
                {
                    return fail(ErrorKind::Budget, "film-streaming-process-bounds");
                }
                Ok(())
            }
            _ => fail(ErrorKind::Invalid, "film-render-version-envelope"),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Bound {
    request: Request,
    aliases: BTreeMap<String, String>,
    job: Artifact,
    production: Artifact,
    task_key: String,
}
impl Bound {
    pub fn task_key(&self) -> &str {
        &self.task_key
    }
    /// Current native producers retain their qualified v1 execution contract.
    /// V2 is authored intent until every native budget/recovery boundary is ported.
    pub fn require_native_v1(&self) -> Result<()> {
        if self.request.schema != "reel.workflow-film-render.v1" {
            return fail(
                ErrorKind::Unsupported,
                "film-streaming-native-not-qualified",
            );
        }
        Ok(())
    }
    pub fn request(&self) -> &Request {
        &self.request
    }
    pub fn aliases(&self) -> &BTreeMap<String, String> {
        &self.aliases
    }
    pub fn job(&self) -> &Artifact {
        &self.job
    }
    pub fn production(&self) -> &Artifact {
        &self.production
    }
}
fn output_path(path: &str) -> bool {
    portable_source(path)
        && path.split('/').count() <= 16
        && path.split('/').all(|part| {
            let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            !part.ends_with('.')
                && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !(stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        })
}
fn checked(reader: &mut impl InputReader, artifact: &Artifact) -> Result<Vec<u8>> {
    if artifact.bytes > crate::MAX_CONTROL_BYTES as u64 {
        return fail(ErrorKind::Budget, "film-render-control-bytes");
    }
    let raw = reader.read(artifact, crate::MAX_CONTROL_BYTES as u64)?;
    if raw.len() as u64 != artifact.bytes || crate::digest(&raw) != artifact.sha256 {
        return fail(ErrorKind::Integrity, "film-render-control-pin");
    }
    Ok(raw)
}
pub fn bind(packet: &Packet, reader: &mut impl InputReader) -> Result<Bound> {
    crate::check_packet(packet)?;
    let tool = packet.task.tool.as_ref().ok_or_else(|| crate::Error {
        kind: ErrorKind::Missing,
        code: "film-render-tool".into(),
    })?;
    if tool.capability != ToolCapability::FilmRender || tool.part.is_some() {
        return fail(ErrorKind::Unsupported, "film-render-capability");
    }
    if tool
        .auxiliary_executables
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != BTreeSet::from(["ffmpeg", "ffprobe"])
    {
        return fail(ErrorKind::Conflict, "film-render-media-programs");
    }
    let request: Request = decode(&checked(reader, &tool.request)?)?;
    request.validate()?;
    if let Some(streaming) = &request.streaming {
        let bytes = packet.task.inputs.iter().try_fold(0u64, |n, a| {
            if a.bytes > streaming.input_object_bytes {
                return fail(ErrorKind::Budget, "film-streaming-input-object");
            }
            n.checked_add(a.bytes).ok_or_else(|| crate::Error {
                kind: ErrorKind::Budget,
                code: "film-streaming-input-overflow".into(),
            })
        })?;
        if bytes > streaming.input_bytes {
            return fail(ErrorKind::Budget, "film-streaming-input-total");
        }
        let outputs = packet
            .task
            .outputs
            .values()
            .try_fold(0u64, |n, c| n.checked_add(c.max_bytes))
            .ok_or_else(|| crate::Error {
                kind: ErrorKind::Budget,
                code: "film-streaming-output-overflow".into(),
            })?;
        if outputs > request.limits.output_bytes {
            return fail(ErrorKind::Budget, "film-streaming-output-ceilings");
        }
    }
    if !portable_source(&request.job_path)
        || !portable_source(&request.production_path)
        || request.job_path == request.production_path
        || request.job_path == "request.json"
        || request.production_path == "request.json"
        || request.output_paths.keys().ne(packet.task.outputs.keys())
        || request.output_paths.len() + 1 > 128
        || packet.task.outputs.values().any(|c| !c.required)
    {
        return fail(ErrorKind::Invalid, "film-render-request");
    }
    for (name, path) in [
        ("execution-request", "render-request.json"),
        ("execution-result", "render-result.json"),
        ("delivery-receipt", "delivery/receipt.json"),
    ] {
        if request.output_paths.get(name).map(String::as_str) != Some(path)
            || packet
                .task
                .outputs
                .get(name)
                .is_none_or(|c| c.media_type != "application/json")
        {
            return fail(ErrorKind::Conflict, "film-render-evidence-outputs");
        }
    }
    let mut folded = BTreeSet::new();
    for (name, path) in &request.output_paths {
        if !output_path(path)
            || !folded.insert(path.to_ascii_lowercase())
            || !(path == "render-request.json"
                || path == "render-result.json"
                || path.starts_with("delivery/"))
        {
            return fail(ErrorKind::Conflict, "film-render-output-path");
        }
        if media_type(path)? != packet.task.outputs[name].media_type {
            return fail(ErrorKind::Conflict, "film-render-output-media-type");
        }
    }
    for path in &folded {
        for (at, _) in path.match_indices('/') {
            if folded.contains(&path[..at]) {
                return fail(ErrorKind::Conflict, "film-render-output-directory");
            }
        }
    }
    let mut aliases = tool.source_bindings.clone();
    if aliases
        .insert("request.json".into(), tool.request.logical_id.clone())
        .is_some()
    {
        return fail(ErrorKind::Conflict, "film-render-request-alias");
    }
    if aliases.values().collect::<BTreeSet<_>>()
        != packet.task.inputs.iter().map(|a| &a.logical_id).collect()
    {
        return fail(ErrorKind::Conflict, "film-render-input-closure");
    }
    if aliases.get(&request.job_path) != Some(&tool.publication.logical_id) {
        return fail(ErrorKind::Conflict, "film-render-job-binding");
    }
    let production = aliases
        .get(&request.production_path)
        .and_then(|id| packet.task.inputs.iter().find(|a| &a.logical_id == id))
        .filter(|a| a.media_type == "application/json")
        .ok_or_else(|| crate::Error {
            kind: ErrorKind::Missing,
            code: "film-render-production-binding".into(),
        })?;
    // Metadata must verify now. Renderer-specific semantic/primary validation
    // remains inside the bounded native renderer and independent media checker.
    checked(reader, &tool.publication)?;
    checked(reader, production)?;
    Ok(Bound {
        request,
        aliases,
        job: tool.publication.clone(),
        production: production.clone(),
        task_key: packet.task_key.clone(),
    })
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub task_key: String,
    pub publication: Artifact,
    pub request: Artifact,
    pub auxiliary_executables: BTreeMap<String, Artifact>,
    pub output_paths: BTreeMap<String, String>,
    pub outputs: crate::ArtifactSet,
    pub human_approval: bool,
    pub retained_owner_acknowledged: bool,
}
/// Hash/correspondence check only; the native renderer and independent checker
/// establish actual media validity. A supplied receipt does not prove execution.
pub fn capture_outputs(
    packet: &Packet,
    bound: &Bound,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<Receipt> {
    capture_header(packet, bound, files.keys())?;
    files.values().try_fold(0u64, |n, raw| {
        n.checked_add(raw.len() as u64)
            .filter(|n| *n <= bound.request.limits.output_bytes)
            .ok_or_else(|| crate::Error {
                kind: ErrorKind::Budget,
                code: "film-render-produced-bytes".into(),
            })
    })?;
    let pins = files
        .iter()
        .map(|(path, raw)| {
            (
                path.clone(),
                OutputContent {
                    sha256: crate::digest(raw),
                    bytes: raw.len() as u64,
                },
            )
        })
        .collect();
    capture_output_pins(packet, bound, &pins)
}

/// Caller-observed content identity, not a file guard or execution attestation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputContent {
    pub sha256: String,
    pub bytes: u64,
}

fn capture_header<'a>(
    packet: &Packet,
    bound: &Bound,
    paths: impl Iterator<Item = &'a String>,
) -> Result<()> {
    crate::check_packet(packet)?;
    if bound.task_key != packet.task_key {
        return fail(ErrorKind::Conflict, "film-render-bound-task");
    }
    if paths.collect::<BTreeSet<_>>() != bound.request.output_paths.values().collect() {
        return fail(ErrorKind::Conflict, "film-render-produced-file-set");
    }
    Ok(())
}

/// Build the identical receipt from verified streamed-file identities. Callers
/// must verify and hold the actual files separately; pins alone confer no trust.
pub fn capture_output_pins(
    packet: &Packet,
    bound: &Bound,
    files: &BTreeMap<String, OutputContent>,
) -> Result<Receipt> {
    capture_header(packet, bound, files.keys())?;
    let tool = packet.task.tool.as_ref().ok_or_else(|| crate::Error {
        kind: ErrorKind::Missing,
        code: "film-render-tool".into(),
    })?;
    let mut outputs = crate::ArtifactSet::new();
    let mut total = 0u64;
    for (name, path) in &bound.request.output_paths {
        let pin = &files[path];
        total = total
            .checked_add(pin.bytes)
            .filter(|n| *n <= bound.request.limits.output_bytes)
            .ok_or_else(|| crate::Error {
                kind: ErrorKind::Budget,
                code: "film-render-produced-bytes".into(),
            })?;
        let media_type = media_type(path)?;
        outputs.insert(
            name.clone(),
            Artifact {
                logical_id: format!("workflow/film/{}/{name}", packet.task_key),
                sha256: pin.sha256.clone(),
                bytes: pin.bytes,
                media_type: media_type.into(),
            },
        );
    }
    reel_publication::check_outputs(&packet.task.outputs, &outputs)?;
    Ok(Receipt {
        schema: "reel.workflow-film-worker-receipt.v1".into(),
        task_key: packet.task_key.clone(),
        publication: tool.publication.clone(),
        request: tool.request.clone(),
        auxiliary_executables: tool.auxiliary_executables.clone(),
        output_paths: bound.request.output_paths.clone(),
        outputs,
        human_approval: false,
        retained_owner_acknowledged: false,
    })
}
pub fn media_type(path: &str) -> Result<&'static str> {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "json" => Ok("application/json"),
        "mkv" => Ok("video/x-matroska"),
        "mp4" => Ok("video/mp4"),
        "wav" => Ok("audio/wav"),
        "png" => Ok("image/png"),
        "svg" => Ok("image/svg+xml"),
        "ttf" => Ok("font/ttf"),
        "otf" => Ok("font/otf"),
        "ass" => Ok("text/x-ssa"),
        "srt" => Ok("application/x-subrip"),
        "txt" => Ok("text/plain"),
        _ => fail(ErrorKind::Unsupported, "film-render-output-media-type"),
    }
}
