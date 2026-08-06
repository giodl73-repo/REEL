use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::Value;
use tempfile::tempdir;

const MANIFEST: &str = "manifests/fixtures/vertical-sound-off/manifest.yaml";
const ASSETS: &str = "manifests/fixtures/vertical-sound-off";

#[test]
#[ignore = "requires external FFmpeg/ffprobe and renders two sanitized variants"]
fn long_slates_fit_and_private_layout_packet_detects_tampering() {
    let dir = tempdir().unwrap();
    let audio = dir.path().join("fixed.wav");
    write_sine_wav(&audio, 6.0);
    let variants = [
        render_variant(dir.path(), 1, "The quiet road stays in view.", &audio),
        render_variant(dir.path(), 2, "For review, the road stays in view.", &audio),
    ];
    let contract = dir.path().join("long-copy.yaml");
    write_contract(&contract, &variants, false);
    let video = dir.path().join("long-copy.mp4");
    let composed = reel(&[
        "comparison-compose",
        path(&contract),
        "--output",
        path(&video),
        "--format",
        "json",
    ]);
    assert_success(&composed);

    let artifact = video.with_extension("comparison.artifacts.json");
    let receipt = video.with_extension("comparison.receipt.json");
    let local: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
    let layouts = local["slate_layouts"].as_array().unwrap();
    assert_eq!(layouts.len(), 3);
    assert!(
        layouts
            .iter()
            .all(|layout| layout["inside_safe_area"] == true)
    );
    assert!(layouts.iter().all(|layout| {
        layout["lines"].as_array().unwrap().len()
            <= layout["maximum_lines"].as_u64().unwrap() as usize
    }));
    assert!(layouts[0]["lines"].as_array().unwrap().len() > 2);
    assert!(
        local["maximum_slate_occupied_screen_percent"]
            .as_f64()
            .unwrap()
            > 0.0
    );

    let shareable = fs::read_to_string(&receipt).unwrap();
    for private_value in [
        "An intentionally expansive opening title",
        "compact speaker badge",
        "presented_label",
        "slate_layout",
        &dir.path().display().to_string(),
    ] {
        assert!(!shareable.contains(private_value));
    }

    let packet_dir = dir.path().join("layout-packet");
    let packet = reel(&[
        "comparison-layout",
        path(&artifact),
        "--output-dir",
        path(&packet_dir),
        "--output",
        "json",
    ]);
    assert_success(&packet);
    let packet_json: Value = serde_json::from_slice(&packet.stdout).unwrap();
    assert_eq!(packet_json["schema"], "reel.comparison-layout.v0.1");
    assert_eq!(packet_json["images"].as_array().unwrap().len(), 3);
    assert!(packet_dir.join("opening.png").is_file());
    assert!(packet_dir.join("variant-01.png").is_file());
    assert!(packet_dir.join("variant-02.png").is_file());
    assert_layout_passes(&packet_dir, 3);

    let opening = packet_dir.join("opening.png");
    let opening_bytes = fs::read(&opening).unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&opening)
        .unwrap()
        .write_all(b"tamper")
        .unwrap();
    assert_layout_fails(&packet_dir);
    fs::write(&opening, &opening_bytes).unwrap();
    assert_layout_passes(&packet_dir, 3);

    let video_bytes = fs::read(&video).unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&video)
        .unwrap()
        .write_all(b"tamper")
        .unwrap();
    assert_layout_fails(&packet_dir);
    fs::write(&video, &video_bytes).unwrap();
    assert_layout_passes(&packet_dir, 3);

    let artifact_bytes = fs::read(&artifact).unwrap();
    let artifact_text = String::from_utf8(artifact_bytes.clone()).unwrap();
    fs::write(
        &artifact,
        artifact_text.replacen("\"verified\": true", "\"verified\": false", 1),
    )
    .unwrap();
    assert_layout_fails(&packet_dir);
    fs::write(&artifact, artifact_bytes).unwrap();
    assert_layout_passes(&packet_dir, 3);

    let infeasible = dir.path().join("infeasible.yaml");
    write_contract(&infeasible, &variants, true);
    let rejected_video = dir.path().join("rejected.mp4");
    let rejected = reel(&[
        "comparison-compose",
        path(&infeasible),
        "--output",
        path(&rejected_video),
    ]);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("cannot fit"));
    assert!(!rejected_video.exists());
    assert!(
        !rejected_video
            .with_extension("comparison.artifacts.json")
            .exists()
    );
    assert!(
        !rejected_video
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
            "1\n00:00:00,000 --> 00:00:03,000\n{caption}\n\n2\n00:00:03,000 --> 00:00:06,000\nThe final frame remains neutral.\n"
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
        .args(["--width", "1280", "--height", "720"])
        .arg("--output")
        .arg(&video)
        .output()
        .unwrap();
    assert_success(&rendered);
    let artifact = video.with_extension("artifacts.json");
    let receipt = root.join(format!("variant-{index}.receipt.json"));
    let retained = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("animatic-receipt")
        .arg(&artifact)
        .arg("--output")
        .arg(&receipt)
        .output()
        .unwrap();
    assert_success(&retained);
    VariantPaths {
        id: format!("variant-{index}"),
        video,
        artifact,
        receipt,
    }
}

fn write_contract(pathname: &Path, variants: &[VariantPaths], infeasible: bool) {
    let opening_title = if infeasible {
        "x".repeat(500)
    } else {
        "An intentionally expansive opening title for a controlled caption-presentation comparison that previously crossed both horizontal edges".to_string()
    };
    let label_one = if infeasible {
        "y".repeat(500)
    } else {
        "Presentation with a compact speaker badge on the first audible entrance and no repeated identity marker".to_string()
    };
    let label_two = if infeasible {
        "z".repeat(500)
    } else {
        "Presentation with a persistent speaker badge while all spoken words and timing remain unchanged".to_string()
    };
    let mut yaml = format!(
        "schema: reel.comparison.v0.1\nid: long-copy-review\nopening:\n  title: '{opening_title}'\n  instructions: Compare only the declared caption treatment. Keep voice, timing, motion, mix, and visual treatment fixed. Inclusion order is not preference, consent, or approval.\n  duration_ms: 2000\nlabel_mode: descriptive\nchanged_dimension: captions\nfixed_dimensions: [motion, voice, mix, visual-treatment, duration, stream-facts]\nvariant_slate_duration_ms: 2000\nprotected_silence_ms: 200\nreplay: false\nvariants:\n"
    );
    for (variant, label) in variants.iter().zip([label_one, label_two]) {
        yaml.push_str(&format!(
            "  - id: {}\n    label: '{}'\n    video: '{}'\n    receipt: '{}'\n    artifact: '{}'\n",
            variant.id,
            label,
            yaml_path(&variant.video),
            yaml_path(&variant.receipt),
            yaml_path(&variant.artifact)
        ));
    }
    fs::write(pathname, yaml).unwrap();
}

fn assert_layout_passes(packet_dir: &Path, images: u64) {
    let output = reel(&[
        "comparison-layout-check",
        path(packet_dir),
        "--output",
        "json",
    ]);
    assert_success(&output);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "reel.comparison-layout-check.v0.1");
    assert_eq!(report["images"], images);
    assert_eq!(report["passed"], true);
}

fn assert_layout_fails(packet_dir: &Path) {
    let output = reel(&["comparison-layout-check", path(packet_dir)]);
    assert!(!output.status.success());
}

fn reel(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(args)
        .output()
        .unwrap()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn path(pathname: &Path) -> &str {
    pathname.to_str().unwrap()
}

fn yaml_path(pathname: &Path) -> String {
    pathname
        .display()
        .to_string()
        .replace('\\', "/")
        .replace('\'', "''")
}

fn write_sine_wav(pathname: &Path, seconds: f64) {
    let sample_rate = 48_000_u32;
    let samples = (f64::from(sample_rate) * seconds).round() as u32;
    let data_bytes = samples * 2;
    let mut file = fs::File::create(pathname).unwrap();
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
        let phase = 2.0 * std::f64::consts::PI * 220.0 * f64::from(index) / f64::from(sample_rate);
        let sample = (phase.sin() * 0.14 * f64::from(i16::MAX)).round() as i16;
        file.write_all(&sample.to_le_bytes()).unwrap();
    }
}
