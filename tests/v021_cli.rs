use std::{fs, process::Command};

use tempfile::tempdir;

const SERIES: &str = "manifests/templates/episodic-series.yaml";
const PLANNING: &str = "manifests/fixtures/two-speaker-untimed/planning.yaml";
const MEASUREMENTS: &str = "manifests/fixtures/two-speaker-untimed/cue-measurements.yaml";

fn successful(binary: &str, args: &[&str]) -> String {
    let output = Command::new(binary)
        .args(args)
        .output()
        .expect("command runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn cli_exposes_all_v021_series_and_continuity_commands() {
    let binary = env!("CARGO_BIN_EXE_reel");
    assert!(
        successful(binary, &["series-validate", SERIES, "--output", "json"])
            .contains("\"continuous_coverage\": true")
    );
    assert!(
        successful(binary, &["series-plan", SERIES, "--output", "json"])
            .contains("\"id\": \"S1E01\"")
    );
    assert!(
        successful(binary, &["series-timing-audit", SERIES, "--output", "json"])
            .contains("\"within_range_episodes\": 1")
    );
    assert!(
        successful(binary, &["series-coverage", SERIES, "--output", "json"])
            .contains("\"continuous\": true")
    );
    assert!(
        !successful(binary, &["series-review-queue", SERIES, "--output", "json"])
            .contains("\"human_approvals\"")
    );
    assert!(
        successful(
            binary,
            &[
                "continuity-validate",
                "manifests/fixtures/shared-continuity/registry.yaml",
                "--output",
                "json",
            ],
        )
        .contains("\"entities\": 7")
    );

    let temp = tempdir().unwrap();
    let output_dir = temp.path().join("episode");
    let compose = Command::new(binary)
        .args(["episode-compose", SERIES, "S1E01", "--output-dir"])
        .arg(&output_dir)
        .args(["--output", "json"])
        .output()
        .expect("episode compose runs");
    assert!(
        compose.status.success(),
        "{}",
        String::from_utf8_lossy(&compose.stderr)
    );
    assert!(output_dir.join("manifest.yaml").is_file());
    assert!(output_dir.join("captions.srt").is_file());
    assert!(output_dir.join("lineage.json").is_file());
    assert!(output_dir.join("coverage.json").is_file());
    assert!(output_dir.join("duration.json").is_file());
}

#[test]
fn cli_imports_mapped_srt_into_a_new_v02_derivative() {
    let binary = env!("CARGO_BIN_EXE_reel");
    let temp = tempdir().unwrap();
    let packet = temp.path().join("packet");
    let conform = Command::new(binary)
        .args([
            "conform",
            PLANNING,
            "--cues",
            MEASUREMENTS,
            "--speaker-tempo",
            "narrator=85",
            "--output-dir",
        ])
        .arg(&packet)
        .output()
        .expect("conform runs");
    assert!(conform.status.success());
    let imported = temp.path().join("imported.yaml");
    let import = Command::new(binary)
        .arg("cue-import-srt")
        .arg(packet.join("manifest.yaml"))
        .args([
            "manifests/fixtures/cue-import/captions.es.srt",
            "--mapping",
            "manifests/fixtures/cue-import/mapping.yaml",
            "--output",
        ])
        .arg(&imported)
        .args(["--format", "json"])
        .output()
        .expect("cue import runs");
    assert!(
        import.status.success(),
        "{}",
        String::from_utf8_lossy(&import.stderr)
    );
    let exported = temp.path().join("exported.srt");
    let caption_export = Command::new(binary)
        .arg("caption-export")
        .arg(&imported)
        .arg("--output")
        .arg(&exported)
        .output()
        .expect("caption export runs");
    assert!(caption_export.status.success());
    assert_eq!(
        fs::read_to_string(exported).unwrap(),
        fs::read_to_string("manifests/fixtures/cue-import/captions.es.srt").unwrap()
    );
}
