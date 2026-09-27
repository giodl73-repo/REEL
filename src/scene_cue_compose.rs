//! Compose ordered language-local PCM takes for one canonical scene cue.
//! Segment clocks come from verified WAV samples, never authored seconds.

use anyhow::{Context, Result, bail};
use reel_assembly::scene_authoring::{Scene, ScopedBindings};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Component, Path},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComposeManifest {
    pub schema: String,
    pub scene: String,
    pub language: String,
    pub cue_id: String,
    pub segment_bindings: String,
    pub output_wav: String,
    pub output_receipt: String,
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn checked(root: &Path, relative: &str) -> Result<std::path::PathBuf> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("cue composition paths must be project-root relative");
    }
    let full = root.join(path);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent)?;
        if !parent.canonicalize()?.starts_with(root) {
            bail!("cue composition path escapes project root");
        }
    }
    Ok(full)
}

fn read_pcm24_mono(bytes: &[u8]) -> Result<(u32, &[u8])> {
    if bytes.len() < 44 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        bail!("take is not RIFF/WAVE");
    }
    let mut cursor = 12usize;
    let mut format = None;
    let mut data = None;
    while cursor + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into()?) as usize;
        let start = cursor + 8;
        let end = start.checked_add(size).context("WAV chunk overflow")?;
        if end > bytes.len() {
            bail!("WAV chunk exceeds file");
        }
        match &bytes[cursor..cursor + 4] {
            b"fmt " => {
                if size < 16 || format.is_some() {
                    bail!("invalid WAV format chunk");
                }
                let code = u16::from_le_bytes(bytes[start..start + 2].try_into()?);
                let channels = u16::from_le_bytes(bytes[start + 2..start + 4].try_into()?);
                let rate = u32::from_le_bytes(bytes[start + 4..start + 8].try_into()?);
                let block = u16::from_le_bytes(bytes[start + 12..start + 14].try_into()?);
                let bits = u16::from_le_bytes(bytes[start + 14..start + 16].try_into()?);
                if code != 1 || channels != 1 || rate == 0 || block != 3 || bits != 24 {
                    bail!("cue takes must be mono PCM24 WAV");
                }
                format = Some(rate);
            }
            b"data" => {
                if data.is_some() || size == 0 || size % 3 != 0 {
                    bail!("invalid PCM24 sample payload");
                }
                data = Some(&bytes[start..end]);
            }
            _ => {}
        }
        cursor = end + (size & 1);
    }
    Ok((
        format.context("WAV format absent")?,
        data.context("WAV data absent")?,
    ))
}

fn wav(samples: &[u8], rate: u32) -> Result<Vec<u8>> {
    let data_size = u32::try_from(samples.len()).context("WAV data exceeds 4 GiB")?;
    let riff_size = data_size.checked_add(36).context("WAV size overflow")?;
    let byte_rate = rate.checked_mul(3).context("WAV rate overflow")?;
    let mut out = Vec::with_capacity(samples.len() + 44);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    out.extend_from_slice(samples);
    Ok(out)
}

pub fn compose(root: &Path, cache_root: &Path, manifest: &ComposeManifest) -> Result<Value> {
    if manifest.schema != "reel.scene-cue-compose.v1" {
        bail!("unsupported cue composition schema");
    }
    let root = root.canonicalize()?;
    let cache_root = cache_root.canonicalize()?;
    let scene_path = checked(&root, &manifest.scene)?;
    let bindings_path = checked(&root, &manifest.segment_bindings)?;
    let output_path = checked(&root, &manifest.output_wav)?;
    let receipt_path = checked(&root, &manifest.output_receipt)?;
    if output_path.exists() || receipt_path.exists() || output_path == receipt_path {
        bail!("cue composition outputs must be new and distinct");
    }
    let scene_bytes = fs::read(&scene_path)?;
    let binding_bytes = fs::read(&bindings_path)?;
    let scene: Scene = serde_json::from_slice(&scene_bytes)?;
    let bindings: ScopedBindings = serde_json::from_slice(&binding_bytes)?;
    let cue = scene
        .languages
        .get(&manifest.language)
        .context("scene language absent")?
        .cues
        .iter()
        .find(|cue| cue.cue_id == manifest.cue_id)
        .context("scene cue absent")?;
    if cue.take_segments.len() < 2 {
        bail!("ordered cue composition requires at least two segments");
    }
    let mut payload = Vec::new();
    let mut rate = None;
    let mut inputs = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    for segment in &cue.take_segments {
        if segment.segment_id.trim().is_empty()
            || segment.speaker_id.trim().is_empty()
            || segment.sample_count == 0
            || !ids.insert(&segment.segment_id)
        {
            bail!("invalid or repeated ordered cue segment");
        }
        let source = bindings
            .assets
            .get(&segment.asset_binding)
            .with_context(|| format!("missing segment binding {}", segment.asset_binding))?;
        if source.sha256.len() != 64
            || !source.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || source.cache_uri != format!("cache://sha256/{}", source.sha256)
            || source.bytes == 0
        {
            bail!("invalid segment cache reference");
        }
        let path = cache_root
            .join("objects")
            .join("sha256")
            .join(&source.sha256[..2])
            .join(&source.sha256);
        let bytes = fs::read(&path).with_context(|| path.display().to_string())?;
        if bytes.len() as u64 != source.bytes || digest(&bytes) != source.sha256 {
            bail!("segment cache bytes differ from scoped binding");
        }
        let (segment_rate, samples) = read_pcm24_mono(&bytes)?;
        if rate.is_some_and(|previous| previous != segment_rate) {
            bail!("ordered cue segments have different sample rates");
        }
        rate = Some(segment_rate);
        if samples.len() as u64 / 3 != segment.sample_count {
            bail!("ordered cue segment sample count differs from scene");
        }
        let start_sample = payload.len() as u64 / 3;
        payload.extend_from_slice(samples);
        inputs.push(json!({
            "segment_id":segment.segment_id,
            "speaker_id":segment.speaker_id,
            "asset_binding":segment.asset_binding,
            "logical_id":source.logical_id,
            "sha256":source.sha256,
            "bytes":source.bytes,
            "start_sample":start_sample,
            "end_sample":payload.len() as u64 / 3
        }));
    }
    let output = wav(&payload, rate.context("cue has no sample rate")?)?;
    let output_sha = digest(&output);
    let receipt = json!({
        "schema":"reel.scene-cue-compose-receipt.v1",
        "scene_id":scene.scene_id,
        "language":manifest.language,
        "cue_id":manifest.cue_id,
        "scene_sha256":digest(&scene_bytes),
        "segment_bindings_sha256":digest(&binding_bytes),
        "sample_rate":rate,
        "sample_count":payload.len() as u64 / 3,
        "segments":inputs,
        "output_sha256":output_sha,
        "output_bytes":output.len(),
        "output_cache_uri":format!("cache://sha256/{output_sha}"),
        "state":"deterministic technical composition; segment review and permissions remain external"
    });
    fs::write(&output_path, &output)?;
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::{ComposeManifest, compose, digest, read_pcm24_mono, wav};
    use serde_json::json;
    use std::fs;

    #[test]
    fn pcm24_round_trip_preserves_exact_samples() {
        let samples = [1, 2, 3, 4, 5, 6];
        let bytes = wav(&samples, 24_000).unwrap();
        let (rate, actual) = read_pcm24_mono(&bytes).unwrap();
        assert_eq!(rate, 24_000);
        assert_eq!(actual, samples);
    }

    #[test]
    fn truncated_or_wrong_format_is_rejected() {
        assert!(read_pcm24_mono(b"not a wav").is_err());
        let mut bytes = wav(&[1, 2, 3], 24_000).unwrap();
        bytes[22] = 2;
        assert!(read_pcm24_mono(&bytes).is_err());
    }

    #[test]
    fn ordered_cue_composition_preserves_speakers_and_rejects_tampered_cache() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let cache = root.join("cache");
        fs::create_dir(&cache).unwrap();
        let inputs = [
            ("narrator", [1u8, 2, 3], "voice.narrator"),
            ("character", [4u8, 5, 6], "voice.character"),
        ];
        let mut assets = serde_json::Map::new();
        for (id, samples, binding) in inputs {
            let bytes = wav(&samples, 24_000).unwrap();
            let sha = digest(&bytes);
            let path = cache.join("objects").join("sha256").join(&sha[..2]);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join(&sha), &bytes).unwrap();
            assets.insert(
                binding.into(),
                json!({"logical_id":id,"sha256":sha,"bytes":bytes.len(),"cache_uri":format!("cache://sha256/{sha}"),"selection_state":"candidate-unselected"}),
            );
        }
        let scene = json!({
            "schema":"reel.scene-authoring.v2","scene_id":"scene","episode_id":"episode",
            "authoring_state":"source-frozen","source_authority_id":"source",
            "languages":{"es":{"cues":[{
                "cue_id":"cue","source_id":"source","exact_text_sha256":"a".repeat(64),
                "narration_slot_id":"slot","take_segments":[
                    {"segment_id":"S01","speaker_id":"narrator","asset_binding":"voice.narrator","sample_count":1},
                    {"segment_id":"S02","speaker_id":"character","asset_binding":"voice.character","sample_count":1}
                ]
            }]}}
        });
        fs::write(root.join("scene.json"), serde_json::to_vec(&scene).unwrap()).unwrap();
        fs::write(
            root.join("bindings.json"),
            serde_json::to_vec(&json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"scene","assets":assets})).unwrap(),
        )
        .unwrap();
        let manifest = ComposeManifest {
            schema: "reel.scene-cue-compose.v1".into(),
            scene: "scene.json".into(),
            language: "es".into(),
            cue_id: "cue".into(),
            segment_bindings: "bindings.json".into(),
            output_wav: "out/cue.wav".into(),
            output_receipt: "out/receipt.json".into(),
        };
        let receipt = compose(root, &cache, &manifest).unwrap();
        let result = fs::read(root.join("out/cue.wav")).unwrap();
        assert_eq!(read_pcm24_mono(&result).unwrap().1, &[1, 2, 3, 4, 5, 6]);
        assert_eq!(receipt["segments"][0]["speaker_id"], "narrator");
        assert_eq!(receipt["segments"][1]["speaker_id"], "character");
        assert_eq!(receipt["segments"][1]["start_sample"], 1);
        assert_eq!(receipt["output_sha256"], digest(&result));
        fs::remove_file(root.join("out/cue.wav")).unwrap();
        fs::remove_file(root.join("out/receipt.json")).unwrap();
        let sha = assets["voice.character"]["sha256"].as_str().unwrap();
        fs::write(
            cache
                .join("objects")
                .join("sha256")
                .join(&sha[..2])
                .join(sha),
            b"tampered",
        )
        .unwrap();
        assert!(compose(root, &cache, &manifest).is_err());
    }
}
