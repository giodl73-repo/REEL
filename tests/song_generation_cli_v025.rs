use std::process::Command;

#[test]
fn song_cli_validates_plans_and_rechecks_packet() {
    let reel = env!("CARGO_BIN_EXE_reel");
    let manifest = "manifests/fixtures/song-generation/song.yaml";
    let validated = Command::new(reel)
        .args(["song-validate", manifest, "--output", "json"])
        .output()
        .expect("validation command runs");
    assert!(validated.status.success());
    assert!(String::from_utf8_lossy(&validated.stdout).contains("\"verified\": true"));

    let temp = tempfile::tempdir().expect("tempdir");
    let packet = temp.path().join("packet");
    let planned = Command::new(reel)
        .arg("song-engine-plan")
        .arg(manifest)
        .arg("--output-dir")
        .arg(&packet)
        .args(["--output", "json"])
        .output()
        .expect("plan command runs");
    assert!(
        planned.status.success(),
        "{}",
        String::from_utf8_lossy(&planned.stderr)
    );

    let checked = Command::new(reel)
        .arg("song-engine-plan-check")
        .arg(&packet)
        .arg(manifest)
        .args(["--output", "json"])
        .output()
        .expect("check command runs");
    assert!(checked.status.success());
}
