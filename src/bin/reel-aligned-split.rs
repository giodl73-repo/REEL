//! Split one continuous PCM16 mono performance at provider-aligned line breaks.
//! The manifest owns cue IDs and exact input hashes; no editor seconds are used.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileRef {
    path: PathBuf,
    sha256: String,
    bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    language: String,
    sample_rate: u32,
    source: FileRef,
    alignment: FileRef,
    text: String,
    cue_ids: Vec<String>,
    #[serde(default)]
    gain_db_by_cue: Vec<f64>,
}

#[derive(Deserialize)]
struct Alignment {
    characters: Vec<Character>,
}
#[derive(Deserialize)]
struct Character {
    text: String,
    end: f64,
}

#[derive(Serialize)]
struct Output {
    cue_id: String,
    path: String,
    sha256: String,
    bytes: u64,
    start_sample: usize,
    end_sample: usize,
    gain_db: f64,
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn verified(root: &Path, reference: &FileRef) -> Result<Vec<u8>> {
    let path = root.join(&reference.path);
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    if bytes.len() as u64 != reference.bytes || hash(&bytes) != reference.sha256 {
        bail!("input hash/bytes mismatch: {}", path.display());
    }
    Ok(bytes)
}

fn wav(samples: &[i16], rate: u32) -> Result<Vec<u8>> {
    let data_size = u32::try_from(samples.len().checked_mul(2).context("WAV size overflow")?)?;
    let mut out = Vec::with_capacity(44 + data_size as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_size).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    Ok(out)
}

fn run(manifest_path: &Path, root: &Path, output_dir: &Path) -> Result<()> {
    if output_dir.exists() {
        bail!("output directory already exists");
    }
    let manifest: Manifest = serde_json::from_slice(&fs::read(manifest_path)?)?;
    if manifest.schema != "reel.aligned-line-split.v1"
        || !(8_000..=192_000).contains(&manifest.sample_rate)
        || !matches!(manifest.language.as_str(), "es" | "en")
        || manifest.cue_ids.is_empty()
        || manifest.cue_ids.iter().any(|id| id.contains(['/', '\\']))
        || manifest.cue_ids.iter().collect::<HashSet<_>>().len() != manifest.cue_ids.len()
        || (!manifest.gain_db_by_cue.is_empty()
            && manifest.gain_db_by_cue.len() != manifest.cue_ids.len())
    {
        bail!("invalid aligned line split manifest");
    }
    verified(root, &manifest.source)?;
    let alignment: Alignment = serde_json::from_slice(&verified(root, &manifest.alignment)?)?;
    let aligned_text = alignment
        .characters
        .iter()
        .map(|c| c.text.as_str())
        .collect::<String>();
    if aligned_text != manifest.text {
        bail!("alignment does not match exact source text");
    }
    let newline_ends = alignment
        .characters
        .iter()
        .filter(|c| c.text == "\n")
        .map(|c| c.end)
        .collect::<Vec<_>>();
    if newline_ends.len() + 1 != manifest.cue_ids.len() {
        bail!("line count differs from cue IDs");
    }
    let source_path = root.join(&manifest.source.path);
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&source_path)
        .args(["-ac", "1", "-ar"])
        .arg(manifest.sample_rate.to_string())
        .args(["-f", "s16le", "-"])
        .output()?;
    if !decoded.status.success() || decoded.stdout.len() % 2 != 0 {
        bail!("PCM16 decode failed");
    }
    let samples = decoded
        .stdout
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect::<Vec<_>>();
    let mut boundaries = vec![0usize];
    for seconds in newline_ends {
        if !seconds.is_finite() || seconds < 0.0 {
            bail!("invalid provider line boundary");
        }
        let sample = (seconds * f64::from(manifest.sample_rate)).round() as usize;
        if sample <= *boundaries.last().unwrap() || sample >= samples.len() {
            bail!("nonmonotonic provider line boundary");
        }
        boundaries.push(sample);
    }
    boundaries.push(samples.len());
    fs::create_dir_all(output_dir)?;
    let mut outputs = Vec::new();
    for (index, cue_id) in manifest.cue_ids.iter().enumerate() {
        let gain_db = manifest.gain_db_by_cue.get(index).copied().unwrap_or(0.0);
        if !gain_db.is_finite() || gain_db.abs() > 18.0 {
            bail!("invalid cue gain");
        }
        let factor = 10f64.powf(gain_db / 20.0);
        let mut pcm = Vec::with_capacity(boundaries[index + 1] - boundaries[index]);
        for sample in &samples[boundaries[index]..boundaries[index + 1]] {
            let scaled = (f64::from(*sample) * factor).round();
            if !(-32768.0..=32767.0).contains(&scaled) {
                bail!("cue gain would clip {cue_id}");
            }
            pcm.push(scaled as i16);
        }
        let bytes = wav(&pcm, manifest.sample_rate)?;
        let name = format!("{cue_id}.wav");
        fs::write(output_dir.join(&name), &bytes)?;
        outputs.push(Output {
            cue_id: cue_id.clone(),
            path: name,
            sha256: hash(&bytes),
            bytes: bytes.len() as u64,
            start_sample: boundaries[index],
            end_sample: boundaries[index + 1],
            gain_db,
        });
    }
    fs::write(
        output_dir.join("receipt.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema":"reel.aligned-line-split-receipt.v1", "language":manifest.language,
            "source_sha256":manifest.source.sha256, "alignment_sha256":manifest.alignment.sha256,
            "sample_rate":manifest.sample_rate, "source_samples":samples.len(), "outputs":outputs
        }))?,
    )?;
    println!("{} contiguous cue slices", outputs.len());
    Ok(())
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let result =
        if let [_, command, manifest, root_flag, root, output_flag, output] = args.as_slice() {
            if command == "build" && root_flag == "--root" && output_flag == "--output-dir" {
                run(Path::new(manifest), Path::new(root), Path::new(output))
            } else {
                bail_usage()
            }
        } else {
            bail_usage()
        };
    if let Err(error) = result {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

fn bail_usage() -> Result<()> {
    bail!(
        "usage: reel-aligned-split build <manifest.json> --root <input-root> --output-dir <new-dir>"
    )
}
