//! Checked original-text character alignment; no recording, selection or network.
use crate::scene_authoring::{
    ProviderAlignedWord, ProviderAlignmentOutput, ProviderWordImport, WordTimingEvidence,
    import_aligned_words,
};
use anyhow::{Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const PROVIDER: &str = "elevenlabs-tts-character-alignment";
const MAX_BYTES: usize = 1024 * 1024;
const MAX_CHARACTERS: usize = 32_768;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Alignment {
    characters: Vec<String>,
    character_start_times_seconds: Vec<f64>,
    character_end_times_seconds: Vec<f64>,
}

/// `json` is the separately retained original `alignment` object from the timed
/// TTS response, not the audio envelope or `normalized_alignment`. Its exact hash
/// and length must match `spec.alignment`. Callers separately verify the take.
///
/// `segments` explicitly supplies language-local word boundaries, interleaved
/// with whitespace segments. Concatenation must reproduce the source bytes.
/// This supports authored segmentation for languages without spaces and never
/// normalizes punctuation, accents, Unicode or project wording.
pub fn import_elevenlabs_characters(
    json: &[u8],
    spec: &ProviderWordImport,
    segments: &[String],
) -> Result<WordTimingEvidence> {
    ensure!(json.len() <= MAX_BYTES, "character alignment byte bound");
    let hash: String = Sha256::digest(json)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    ensure!(
        spec.alignment.bytes == json.len() as u64 && spec.alignment.sha256 == hash,
        "character alignment bytes differ"
    );
    ensure!(
        spec.sample_rate > 0 && (1..=(1u64 << 53)).contains(&spec.cue_end_sample),
        "character alignment sample clock refused"
    );
    ensure!(
        !segments.is_empty()
            && segments.len() <= MAX_CHARACTERS
            && segments.iter().all(|s| !s.is_empty())
            && segments
                .iter()
                .flat_map(|s| s.bytes())
                .eq(spec.spoken_text.bytes()),
        "character alignment word segmentation differs"
    );
    let alignment: Alignment = serde_json::from_slice(json)?;
    let count = alignment.characters.len();
    ensure!(
        (1..=MAX_CHARACTERS).contains(&count)
            && alignment.character_start_times_seconds.len() == count
            && alignment.character_end_times_seconds.len() == count
            && alignment.characters.iter().all(|s| s.chars().count() == 1)
            && alignment.characters.concat() == spec.spoken_text,
        "character alignment does not reproduce original source"
    );
    let mut prior = (0.0, 0.0);
    for (&start, &end) in alignment
        .character_start_times_seconds
        .iter()
        .zip(&alignment.character_end_times_seconds)
    {
        ensure!(
            start.is_finite()
                && end.is_finite()
                && start >= prior.0
                && end >= prior.1
                && end >= start
                && end * f64::from(spec.sample_rate) <= spec.cue_end_sample as f64,
            "character alignment clock outside selected take or backward"
        );
        prior = (start, end);
    }
    let mut offset = 0;
    let mut words = Vec::new();
    for segment in segments {
        let end = offset + segment.chars().count();
        ensure!(
            segment.chars().all(char::is_whitespace) || segment.chars().all(|c| !c.is_whitespace()),
            "character alignment segment mixes words and whitespace"
        );
        words.push(ProviderAlignedWord {
            text: segment.clone(),
            start: alignment.character_start_times_seconds[offset],
            end: alignment.character_end_times_seconds[end - 1],
        });
        offset = end;
    }
    import_aligned_words(ProviderAlignmentOutput { words }, spec, PROVIDER)
}
