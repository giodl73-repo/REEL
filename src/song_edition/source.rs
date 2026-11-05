//! Explicit, caller-audited interpretation of retained original song evidence.
//! Never rewrites an original request or executes a provider.
use super::{packet, wave};
use anyhow::{Context, Result, bail};
use reel_music::hash::sha256_bytes;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const META: u64 = 4 * 1024 * 1024;
const AUDIO: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    path: PathBuf,
    object: Pin,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Admission {
    schema: String,
    source_version: String,
    original_package: Input,
    original_request: Input,
    original_generation: Input,
    canonical_text: Input,
    recording: Input,
    interpretation: Interpretation,
}
/// These pointers are an explicit producer policy, not inferred schema semantics.
/// The exact policy is retained with the result and needs its own audit.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Interpretation {
    package_schema: String,
    package_version_pointer: String,
    package_request_pointer: String,
    package_generation_pointer: String,
    package_recording_pointer: String,
    request_schema: String,
    generation_schema: String,
    canonical_lyrics_pointer: String,
    render_lyrics_pointer: String,
    private_only_pointer: String,
    external_upload_pointer: String,
    identifiable_imitation_pointer: String,
    public_release_pointer: String,
    generation_request_sha256_pointer: String,
    generation_canonical_sha256_pointer: String,
    generation_render_sha256_pointer: String,
    generation_outputs_pointer: String,
    generation_private_flag_pointer: String,
    generation_external_upload_pointer: String,
    #[serde(default)]
    allow_single_terminal_lf_difference: bool,
}
struct Held {
    _file: File,
    bytes: Vec<u8>,
    pin: Pin,
}
fn valid_pin(pin: &Pin, max: u64) -> bool {
    pin.bytes > 0
        && pin.bytes <= max
        && pin.sha256.len() == 64
        && pin
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn held(input: Input, max: u64) -> Result<Held> {
    if !input.path.is_absolute() || !valid_pin(&input.object, max) {
        bail!("original input needs an absolute named file and bounded exact pin");
    }
    let metadata = fs::symlink_metadata(&input.path)?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = metadata.file_type().is_symlink();
    if !metadata.is_file() || reparse {
        bail!("original input must be a regular non-reparse file");
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let mut file = options.open(&input.path)?;
    if file.metadata()?.len() != input.object.bytes {
        bail!("original input byte count differs");
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(input.object.bytes + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 != input.object.bytes || sha256_bytes(&bytes) != input.object.sha256 {
        bail!("original input hash differs");
    }
    Ok(Held {
        _file: file,
        bytes,
        pin: input.object,
    })
}
fn at<'a>(value: &'a Value, pointer: &str) -> Result<&'a Value> {
    if !pointer.starts_with('/') || pointer.len() > 256 || pointer.chars().any(char::is_control) {
        bail!("interpretation needs a bounded explicit JSON pointer");
    }
    value
        .pointer(pointer)
        .context("interpreted original field missing")
}
fn flag(value: &Value, pointer: &str, expected: bool) -> Result<()> {
    if at(value, pointer)?.as_bool() != Some(expected) {
        bail!("original private restriction differs");
    }
    Ok(())
}
fn text<'a>(value: &'a Value, pointer: &str) -> Result<&'a str> {
    at(value, pointer)?
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 131072)
        .context("original lyric text is missing or exceeds its bound")
}
fn matching_text(a: &str, b: &str, allow_lf: bool) -> bool {
    a == b || allow_lf && (a.strip_suffix('\n') == Some(b) || b.strip_suffix('\n') == Some(a))
}
fn object_pin(value: &Value) -> Result<Pin> {
    let p = Pin {
        sha256: value["sha256"]
            .as_str()
            .context("generation output hash missing")?
            .into(),
        bytes: value["bytes"]
            .as_u64()
            .context("generation output byte count missing")?,
    };
    if !valid_pin(&p, AUDIO) {
        bail!("generation output pin differs from bounded contract");
    }
    Ok(p)
}

/// Opaque inspection result. Keeping it alive retains input read guards.
/// It proves correspondence under an admitted interpretation, not its authority.
pub struct VerifiedOriginal {
    context: Value,
    _inputs: Vec<Held>,
}
impl VerifiedOriginal {
    pub fn summary(&self) -> Value {
        json!({"schema":"reel.song-source-inspection-summary.v1",
            "request_receipt_link_checked":true,"recording_receipt_link_checked":true,
            "original_package_version_checked":true,
            "source_text_correspondence_checked":true,"mapping_authority_verified":false,
            "schema_interpretation_inferred":false,"original_request_rewritten":false,
            "provider_executed":false,"human_approval":false,"shareable":false,
            "sample_rate":self.context["sample_rate"],"channels":self.context["channels"],
            "source_frames":self.context["source_frames"]})
    }
    pub fn write(&self, output: &Path) -> Result<()> {
        if !output.is_absolute() || output.exists() {
            bail!("source context output must be new and absolute");
        }
        let parent = output.parent().context("source context parent missing")?;
        if !parent.is_dir() {
            bail!("source context parent must already exist");
        }
        let raw = serde_json::to_vec_pretty(&self.context)?;
        if raw.len() > META as usize {
            bail!("source context exceeds metadata bound");
        }
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(&raw)?;
        temp.as_file().sync_all()?;
        temp.persist_noclobber(output)
            .map_err(|_| anyhow::anyhow!("source context cannot overwrite output"))?;
        Ok(())
    }
}
pub fn inspect(path: &Path) -> Result<VerifiedOriginal> {
    let file = File::open(path)?;
    let mut raw = Vec::new();
    file.take(65537).read_to_end(&mut raw)?;
    if raw.len() > 65536 {
        bail!("source interpretation admission exceeds bound");
    }
    let a: Admission = serde_json::from_value(packet::parse(&raw)?)?;
    if a.schema != "reel.song-source-interpretation.v1" {
        bail!("unsupported original source interpretation");
    }
    let package = held(a.original_package, META)?;
    let request = held(a.original_request, META)?;
    let generation = held(a.original_generation, META)?;
    let canonical = held(a.canonical_text, 131072)?;
    let recording = held(a.recording, AUDIO)?;
    let p = packet::parse(&package.bytes)?;
    let r = packet::parse(&request.bytes)?;
    let g = packet::parse(&generation.bytes)?;
    let i = &a.interpretation;
    for schema in [&i.package_schema, &i.request_schema, &i.generation_schema] {
        if schema.is_empty() || schema.len() > 256 || schema.chars().any(char::is_control) {
            bail!("original schema identity missing");
        }
    }
    if p["schema"].as_str() != Some(&i.package_schema)
        || r["schema"].as_str() != Some(&i.request_schema)
        || g["schema"].as_str() != Some(&i.generation_schema)
    {
        bail!("original schema differs from admitted interpretation");
    }
    if a.source_version.trim().is_empty()
        || a.source_version.len() > 256
        || a.source_version.chars().any(char::is_control)
        || at(&p, &i.package_version_pointer)?.as_str() != Some(&a.source_version)
        || object_pin(at(&p, &i.package_request_pointer)?)? != request.pin
        || object_pin(at(&p, &i.package_generation_pointer)?)? != generation.pin
        || object_pin(at(&p, &i.package_recording_pointer)?)? != recording.pin
    {
        bail!("original package version or source bindings differ");
    }
    let pointers = [
        &i.private_only_pointer,
        &i.external_upload_pointer,
        &i.identifiable_imitation_pointer,
        &i.public_release_pointer,
    ];
    if pointers.into_iter().collect::<BTreeSet<_>>().len() != 4 {
        bail!("private restriction pointers must be distinct");
    }
    flag(&r, &i.private_only_pointer, true)?;
    flag(&r, &i.external_upload_pointer, false)?;
    flag(&r, &i.identifiable_imitation_pointer, false)?;
    flag(&r, &i.public_release_pointer, false)?;
    flag(&g, &i.generation_private_flag_pointer, true)?;
    flag(&g, &i.generation_external_upload_pointer, false)?;
    let source = std::str::from_utf8(&canonical.bytes)?;
    let requested = text(&r, &i.canonical_lyrics_pointer)?;
    if !matching_text(source, requested, i.allow_single_terminal_lf_difference)
        || source.split_whitespace().count() > 4000
    {
        bail!("canonical source differs from original request");
    }
    let rendered = text(&r, &i.render_lyrics_pointer)?;
    if at(&g, &i.generation_request_sha256_pointer)?.as_str() != Some(&request.pin.sha256)
        || at(&g, &i.generation_canonical_sha256_pointer)?.as_str()
            != Some(&sha256_bytes(requested.as_bytes()))
        || at(&g, &i.generation_render_sha256_pointer)?.as_str()
            != Some(&sha256_bytes(rendered.as_bytes()))
    {
        bail!("original generation request or lyric text link differs");
    }
    let outputs = at(&g, &i.generation_outputs_pointer)?
        .as_array()
        .context("generation output roster missing")?;
    if outputs.is_empty() || outputs.len() > 128 {
        bail!("generation output roster exceeds bound");
    }
    let pins = outputs.iter().map(object_pin).collect::<Result<Vec<_>>>()?;
    if pins.iter().filter(|p| **p == recording.pin).count() != 1
        || pins
            .iter()
            .any(|p| p.sha256 == recording.pin.sha256 && p.bytes != recording.pin.bytes)
    {
        bail!("recording is not an unambiguous original generation output");
    }
    let audio = wave::decode(&recording.bytes)?;
    let context = json!({"schema":"reel.song-repair-source-context.v1",
        "interpretation":i,"interpretation_admission_sha256":sha256_bytes(&raw),
        "source_version":a.source_version,"original_package":package.pin,
        "original_request":request.pin,"original_generation":generation.pin,
        "canonical_text":canonical.pin,"recording":recording.pin,
        "canonical_lyrics":source,"original_render_lyrics":rendered,
        "sample_rate":audio.rate,"channels":audio.channels,
        "source_frames":audio.samples.len() / usize::from(audio.channels),
        "private_scope":{"private_only":true,"external_upload":false,"identifiable_imitation":false,"public_release":false},
        "mapping_authority_verified":false,"original_request_rewritten":false,
        "performed_words_verified":false,"voice_consent_granted":false,
        "provider_executed":false,"human_approval":false,"shareable":false});
    Ok(VerifiedOriginal {
        context,
        _inputs: vec![package, request, generation, canonical, recording],
    })
}

#[cfg(test)]
mod tests;
