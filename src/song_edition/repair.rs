//! Deterministic, same-clock repair materialization from explicit raw candidates.
//! Source interpretation and provider/acoustic/listening authority stay separate.
use super::{lyrics, packet, source, wave};
use anyhow::{Context, Result, bail};
use reel_music::hash::sha256_bytes;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};

const META: u64 = 65536;
const AUDIO: u64 = 512 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    schema: String,
    source_version: String,
    original_package: source::Pin,
    original_request: source::Pin,
    original_generation: source::Pin,
    canonical_text: source::Pin,
    recording: source::Pin,
    sample_rate: u32,
    channels: u16,
    source_frames: u64,
    patches: Vec<Patch>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Patch {
    start_frame: u64,
    end_frame: u64,
    fade_frames: u64,
    source_lines: [usize; 2],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Candidates {
    schema: String,
    recipe: source::Pin,
    patches: Vec<source::Input>,
}
fn read_metadata(path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(META + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > META {
        bail!("repair metadata exceeds bound");
    }
    Ok(bytes)
}
fn pin(bytes: &[u8]) -> source::Pin {
    source::Pin {
        sha256: sha256_bytes(bytes),
        bytes: bytes.len() as u64,
    }
}
struct Loaded {
    original: source::VerifiedOriginal,
    recipe: Recipe,
    recipe_pin: source::Pin,
    audio: wave::Wave,
    geometry: Vec<Value>,
}
fn load(admission: &Path, recipe_path: &Path) -> Result<Loaded> {
    // Re-inspect original evidence; a serialized context alone is never trusted.
    let original = source::inspect(admission)?;
    let raw = read_metadata(recipe_path)?;
    let recipe: Recipe = serde_json::from_value(packet::parse(&raw)?)?;
    let context = original.context();
    if recipe.schema != "reel.original-song-repair.v1"
        || context["source_version"] != recipe.source_version
        || context["sample_rate"] != recipe.sample_rate
        || context["channels"] != recipe.channels
        || context["source_frames"] != recipe.source_frames
    {
        bail!("repair source version or native clock differs");
    }
    for (key, expected) in [
        ("original_package", &recipe.original_package),
        ("original_request", &recipe.original_request),
        ("original_generation", &recipe.original_generation),
        ("canonical_text", &recipe.canonical_text),
        ("recording", &recipe.recording),
    ] {
        if serde_json::from_value::<source::Pin>(context[key].clone())? != *expected {
            bail!("repair original source pin differs");
        }
    }
    if !(1..=3).contains(&recipe.patches.len()) {
        bail!("repair needs one to three masks");
    }
    let lines = lyrics::lines(
        context["canonical_lyrics"]
            .as_str()
            .context("canonical text missing")?,
    );
    let mut previous_end = 0;
    let mut geometry = Vec::new();
    for patch in &recipe.patches {
        let [first, last] = patch.source_lines;
        if patch.start_frame < previous_end
            || patch.start_frame >= patch.end_frame
            || patch.end_frame > recipe.source_frames
            || patch.end_frame - patch.start_frame > u64::from(recipe.sample_rate) * 30
            || patch.fade_frames > u64::from(recipe.sample_rate)
            || patch.fade_frames > (patch.end_frame - patch.start_frame - 1) / 2
            || first == 0
            || first > last
            || last > lines.len()
        {
            bail!("repair mask, fades or canonical line range differs");
        }
        let context_start = patch
            .start_frame
            .saturating_sub(u64::from(recipe.sample_rate) * 5);
        let context_end =
            (patch.end_frame + u64::from(recipe.sample_rate) * 5).min(recipe.source_frames);
        geometry.push(
            json!({"start_frame":patch.start_frame,"end_frame":patch.end_frame,
            "fade_frames":patch.fade_frames,"context_start_frame":context_start,
            "context_end_frame":context_end,"source_lines":patch.source_lines,
            "supplied_lyrics":lines[first-1..last].join("\n")}),
        );
        previous_end = patch.end_frame;
    }
    let audio = wave::decode(original.recording_bytes())?;
    if audio.float_data.is_none() {
        bail!("masked repair requires original float32 WAVE; integer conversion is not admitted");
    }
    Ok(Loaded {
        original,
        recipe,
        recipe_pin: pin(&raw),
        audio,
        geometry,
    })
}
fn summary(loaded: &Loaded) -> Value {
    json!({"schema":"reel.original-song-repair-plan.v1",
        "recipe":loaded.recipe_pin,"sample_rate":loaded.audio.rate,"channels":loaded.audio.channels,
        "source_frames":loaded.recipe.source_frames,"patch_count":loaded.recipe.patches.len(),
        "maximum_output_wave_bytes":loaded.recipe.recording.bytes,
        "provider_executed":false,"provider_execution_verified":false,
        "performed_words_verified":false,"human_approval":false,"selected":false,"shareable":false})
}
pub fn plan(admission: &Path, recipe: &Path) -> Result<Value> {
    Ok(summary(&load(admission, recipe)?))
}
fn build(loaded: &Loaded, candidates_path: &Path) -> Result<(Vec<u8>, Value)> {
    let candidates_raw = read_metadata(candidates_path)?;
    let candidates: Candidates = serde_json::from_value(packet::parse(&candidates_raw)?)?;
    if candidates.schema != "reel.original-song-repair-candidates.v1"
        || candidates.recipe != loaded.recipe_pin
        || candidates.patches.len() != loaded.recipe.patches.len()
    {
        bail!("repair candidate roster or recipe pin differs");
    }
    if candidates
        .patches
        .iter()
        .map(|p| p.object.bytes)
        .try_fold(0u64, |sum, n| sum.checked_add(n))
        .is_none_or(|total| total > AUDIO)
    {
        bail!("repair candidate total exceeds audio bound");
    }
    let mut held = Vec::new();
    let mut result = loaded.audio.samples.clone();
    let channels = usize::from(loaded.audio.channels);
    let mut candidate_pins = Vec::new();
    for ((patch, geometry), input) in loaded
        .recipe
        .patches
        .iter()
        .zip(&loaded.geometry)
        .zip(candidates.patches)
    {
        let candidate = source::held(input, AUDIO)?;
        let audio = wave::decode(&candidate.bytes)?;
        let context_start = geometry["context_start_frame"].as_u64().unwrap();
        let context_end = geometry["context_end_frame"].as_u64().unwrap();
        if audio.rate != loaded.audio.rate
            || audio.channels != loaded.audio.channels
            || audio.float_data.is_none()
            || audio.samples.len() as u64 != (context_end - context_start) * channels as u64
            || audio.samples.iter().any(|s| s.abs() > 1.001)
        {
            bail!("repair candidate native clock, extent or signal differs");
        }
        for frame in patch.start_frame..patch.end_frame {
            let left = frame - patch.start_frame;
            let right = patch.end_frame - 1 - frame;
            let edge = left.min(right);
            let gain = if edge < patch.fade_frames {
                if patch.fade_frames == 1 {
                    0.0
                } else {
                    edge as f32 / (patch.fade_frames - 1) as f32
                }
            } else {
                1.0
            };
            for channel in 0..channels {
                let at = frame as usize * channels + channel;
                let raw_at = (frame - context_start) as usize * channels + channel;
                result[at] = if gain == 0.0 {
                    loaded.audio.samples[at]
                } else if gain == 1.0 {
                    audio.samples[raw_at]
                } else {
                    loaded.audio.samples[at] * (1.0 - gain) + audio.samples[raw_at] * gain
                };
            }
        }
        candidate_pins.push(candidate.pin.clone());
        held.push(candidate);
    }
    // Independent traversal checks every protected interleaved sample bit.
    let mut protected = 0u64;
    for (index, (before, after)) in loaded.audio.samples.iter().zip(&result).enumerate() {
        let frame = (index / channels) as u64;
        if !loaded
            .recipe
            .patches
            .iter()
            .any(|p| p.start_frame <= frame && frame < p.end_frame)
        {
            if before.to_bits() != after.to_bits() {
                bail!("protected repair sample changed");
            }
            protected += 1;
        }
    }
    // Preserve the original container/format/metadata, touching only PCM bytes.
    let mut bytes = loaded.original.recording_bytes().to_vec();
    let data = loaded
        .audio
        .float_data
        .clone()
        .context("original float data missing")?;
    for (chunk, sample) in bytes[data].chunks_exact_mut(4).zip(&result) {
        chunk.copy_from_slice(&sample.to_le_bytes());
    }
    let reopened = wave::decode(&bytes)?;
    if reopened
        .samples
        .iter()
        .map(|s| s.to_bits())
        .ne(result.iter().map(|s| s.to_bits()))
    {
        bail!("repair WAVE round trip changed samples");
    }
    let receipt = json!({"schema":"reel.original-song-repair-receipt.v1",
        "source_context":pin(&serde_json::to_vec(loaded.original.context())?),
        "source_recording":loaded.recipe.recording,"recipe":loaded.recipe_pin,
        "candidate_roster":pin(&candidates_raw),"raw_candidates":candidate_pins,
        "recording":pin(&bytes),"sample_rate":loaded.audio.rate,"channels":loaded.audio.channels,
        "source_frames":loaded.recipe.source_frames,"patches":loaded.geometry,
        "protected_interleaved_samples":protected,"outside_mask_sample_bits_exact":true,
        "native_clock_unchanged":true,"lossless_write_read_exact":true,
        "original_container_and_format_preserved":true,
        "provider_execution_verified":false,"performed_words_verified":false,
        "acoustic_continuity":"unreviewed","beat_alignment":"unreviewed",
        "human_listening":"pending","mapping_authority_verified":false,
        "human_approval":false,"selected":false,"shareable":false});
    Ok((bytes, receipt))
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
pub fn materialize(
    admission: &Path,
    recipe: &Path,
    candidates: &Path,
    output: &Path,
) -> Result<Value> {
    if !output.is_absolute() || output.exists() {
        bail!("repair output must be new and absolute");
    }
    let loaded = load(admission, recipe)?;
    let (bytes, receipt) = build(&loaded, candidates)?;
    fs::create_dir(output)?;
    write_new(&output.join("recording.wav"), &bytes)?;
    write_new(
        &output.join("repair-receipt.json"),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    check(admission, recipe, candidates, output)
}
pub fn check(admission: &Path, recipe: &Path, candidates: &Path, output: &Path) -> Result<Value> {
    let loaded = load(admission, recipe)?;
    let (expected, receipt) = build(&loaded, candidates)?;
    let recording = source::held(
        source::Input {
            path: output.join("recording.wav"),
            object: pin(&expected),
        },
        AUDIO,
    )?;
    let receipt_raw = serde_json::to_vec_pretty(&receipt)?;
    let checked_receipt = source::held(
        source::Input {
            path: output.join("repair-receipt.json"),
            object: pin(&receipt_raw),
        },
        4 * 1024 * 1024,
    )?;
    let mut report = summary(&loaded);
    report["schema"] = json!("reel.original-song-repair-check.v1");
    report["outside_mask_sample_bits_exact"] = json!(true);
    report["recording"] = serde_json::to_value(recording.pin)?;
    report["receipt"] = serde_json::to_value(checked_receipt.pin)?;
    Ok(report)
}

#[cfg(test)]
mod tests;
