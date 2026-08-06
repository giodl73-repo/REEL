use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::Value;
use tempfile::tempdir;

const MANIFEST: &str = "manifests/fixtures/vertical-sound-off/manifest.yaml";
const ASSETS: &str = "manifests/fixtures/vertical-sound-off";

#[test]
#[ignore = "requires external FFmpeg/ffprobe and renders four six-second variants"]
fn comparison_composes_three_caption_variants_and_rejects_changed_audio() {
    let dir = tempdir().unwrap();
    let audio = dir.path().join("fixed.wav");
    let changed_audio = dir.path().join("changed.wav");
    let chime = dir.path().join("chime.wav");
    write_sine_wav(&audio, 6.0, 0.16, 220.0);
    write_sine_wav(&changed_audio, 6.0, 0.16, 330.0);
    write_sine_wav(&chime, 0.2, 0.04, 660.0);

    let mut variants = Vec::new();
    for (index, caption) in [
        "A quiet road remains in view.",
        "The road remains quiet in view.",
        "In view, the quiet road remains.",
    ]
    .into_iter()
    .enumerate()
    {
        variants.push(render_variant(dir.path(), index + 1, caption, &audio));
    }
    let changed = render_variant(
        dir.path(),
        4,
        "In view, the quiet road remains.",
        &changed_audio,
    );

    let contract = dir.path().join("comparison.yaml");
    write_contract(&contract, &variants, &chime);
    let output = dir.path().join("caption-review.mp4");
    let composed = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("comparison-compose")
        .arg(&contract)
        .arg("--output")
        .arg(&output)
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert!(
        composed.status.success(),
        "{}",
        String::from_utf8_lossy(&composed.stderr)
    );
    let command_report: Value = serde_json::from_slice(&composed.stdout).unwrap();
    assert_eq!(command_report["schema"], "reel.comparison-compose.v0.1");
    assert_eq!(command_report["children"], 3);
    assert!(output.is_file());
    let artifact = output.with_extension("comparison.artifacts.json");
    let receipt = output.with_extension("comparison.receipt.json");
    assert!(artifact.is_file());
    assert!(receipt.is_file());
    let local: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
    assert_eq!(local["changed_dimension"], "captions");
    assert_eq!(local["children"].as_array().unwrap().len(), 3);
    assert_eq!(
        local["children"]
            .as_array()
            .unwrap()
            .iter()
            .map(|child| child["presented_label"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["A", "B", "C"])
    );
    assert_eq!(local["inclusion_order_is_approval"], false);
    let shareable = fs::read_to_string(&receipt).unwrap();
    assert!(!shareable.contains("seed-for-review"));
    assert!(!shareable.contains("variant-1"));
    assert!(!shareable.contains(&dir.path().display().to_string()));
    assert!(!shareable.contains("presented_label"));
    let receipt_check = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("comparison-receipt-check")
        .arg(&receipt)
        .arg(&output)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(receipt_check.status.success());
    let checked: Value = serde_json::from_slice(&receipt_check.stdout).unwrap();
    assert_eq!(checked["schema"], "reel.comparison-receipt-check.v0.1");
    assert_eq!(checked["children"], 3);

    let rejected_contract = dir.path().join("comparison-changed-audio.yaml");
    write_contract(
        &rejected_contract,
        &[variants[0].clone(), variants[1].clone(), changed],
        &chime,
    );
    let rejected_output = dir.path().join("rejected.mp4");
    let rejected = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("comparison-compose")
        .arg(&rejected_contract)
        .arg("--output")
        .arg(&rejected_output)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("fixed dimension voice"));
    assert!(!rejected_output.exists());
    assert!(
        !rejected_output
            .with_extension("comparison.artifacts.json")
            .exists()
    );
    assert!(
        !rejected_output
            .with_extension("comparison.receipt.json")
            .exists()
    );
}

#[derive(Clone)]
struct VariantPaths {
    id: String,
    video: PathBuf,
    artifact: PathBuf,
    receipt: PathBuf,
}

fn render_variant(root: &Path, index: usize, caption: &str, audio: &Path) -> VariantPaths {
    let captions = root.join(format!("captions-{index}.srt"));
    fs::write(
        &captions,
        format!(
            "1\n00:00:00,000 --> 00:00:02,750\n{caption}\n\n2\n00:00:02,750 --> 00:00:06,000\nThe ending holds without a claim.\n"
        ),
    )
    .unwrap();
    let video = root.join(format!("variant-{index}.mp4"));
    let rendered = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["animatic-render", MANIFEST, "--asset-root", ASSETS])
        .arg("--audio")
        .arg(audio)
        .arg("--captions")
        .arg(&captions)
        .args(["--width", "720", "--height", "1280"])
        .arg("--output")
        .arg(&video)
        .output()
        .unwrap();
    assert!(
        rendered.status.success(),
        "{}",
        String::from_utf8_lossy(&rendered.stderr)
    );
    let artifact = video.with_extension("artifacts.json");
    let receipt = root.join(format!("variant-{index}.receipt.json"));
    let retained = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("animatic-receipt")
        .arg(&artifact)
        .arg("--output")
        .arg(&receipt)
        .output()
        .unwrap();
    assert!(
        retained.status.success(),
        "{}",
        String::from_utf8_lossy(&retained.stderr)
    );
    VariantPaths {
        id: format!("variant-{index}"),
        video,
        artifact,
        receipt,
    }
}

fn write_contract(path: &Path, variants: &[VariantPaths], chime: &Path) {
    let mut yaml = format!(
        "schema: reel.comparison.v0.1\nid: caption-policy-review\nopening:\n  title: Controlled caption review\n  instructions: Compare caption presentation only. Inclusion and order are not approval.\n  duration_ms: 1000\nlabel_mode: blinded\nblind_seed: seed-for-review\nchanged_dimension: captions\nfixed_dimensions: [motion, voice, mix, visual-treatment, duration, stream-facts]\nvariant_slate_duration_ms: 1000\nprotected_silence_ms: 200\nchime: '{}'\nreplay: false\nvariants:\n",
        yaml_path(chime)
    );
    for variant in variants {
        yaml.push_str(&format!(
            "  - id: {}\n    video: '{}'\n    receipt: '{}'\n    artifact: '{}'\n",
            variant.id,
            yaml_path(&variant.video),
            yaml_path(&variant.receipt),
            yaml_path(&variant.artifact)
        ));
    }
    fs::write(path, yaml).unwrap();
}

fn yaml_path(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "/")
        .replace('\'', "''")
}

fn write_sine_wav(path: &Path, seconds: f64, amplitude: f64, frequency: f64) {
    let sample_rate = 48_000_u32;
    let samples = (f64::from(sample_rate) * seconds).round() as u32;
    let data_bytes = samples * 2;
    let mut file = fs::File::create(path).unwrap();
    file.write_all(b"RIFF").unwrap();
    file.write_all(&(36 + data_bytes).to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt ").unwrap();
    file.write_all(&16_u32.to_le_bytes()).unwrap();
    file.write_all(&1_u16.to_le_bytes()).unwrap();
    file.write_all(&1_u16.to_le_bytes()).unwrap();
    file.write_all(&sample_rate.to_le_bytes()).unwrap();
    file.write_all(&(sample_rate * 2).to_le_bytes()).unwrap();
    file.write_all(&2_u16.to_le_bytes()).unwrap();
    file.write_all(&16_u16.to_le_bytes()).unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&data_bytes.to_le_bytes()).unwrap();
    for index in 0..samples {
        let phase =
            2.0 * std::f64::consts::PI * frequency * f64::from(index) / f64::from(sample_rate);
        let sample = (phase.sin() * amplitude * f64::from(i16::MAX)).round() as i16;
        file.write_all(&sample.to_le_bytes()).unwrap();
    }
}
