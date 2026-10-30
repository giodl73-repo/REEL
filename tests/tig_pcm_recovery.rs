#![cfg(all(windows, feature = "tig-publication"))]
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tig_core::Object;

fn wave() -> Vec<u8> {
    let bytes = 48_000u32 * 6;
    let mut raw = Vec::new();
    raw.extend(b"RIFF");
    raw.extend((bytes + 36).to_le_bytes());
    raw.extend(b"WAVEfmt ");
    raw.extend(16u32.to_le_bytes());
    raw.extend(1u16.to_le_bytes());
    raw.extend(2u16.to_le_bytes());
    raw.extend(48000u32.to_le_bytes());
    raw.extend(288000u32.to_le_bytes());
    raw.extend(6u16.to_le_bytes());
    raw.extend(24u16.to_le_bytes());
    raw.extend(b"data");
    raw.extend(bytes.to_le_bytes());
    raw.resize(bytes as usize + 44, 0);
    raw
}
fn pin(path: &Path) -> Value {
    let identity = reel::media_runtime::ByteIdentity::observe_file(path).unwrap();
    json!({"path":path,"sha256":identity.sha256,"bytes":identity.bytes})
}
fn program(name: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join(format!("{name}.exe")))
        .find(|p| p.is_file())
        .unwrap()
        .canonicalize()
        .unwrap()
}
fn fixture() -> (tempfile::TempDir, Value) {
    let root = tempfile::tempdir().unwrap();
    for directory in [
        "source/objects",
        "destination/objects/sha256",
        "coordination",
        "artifact",
    ] {
        fs::create_dir_all(root.path().join(directory)).unwrap();
    }
    let original = wave();
    let target = Object::from_bytes(&original);
    let input = root.path().join("original.wav");
    fs::write(&input, &original).unwrap();
    let carrier = root.path().join("carrier.mkv");
    let status = Command::new(program("ffmpeg"))
        .args([
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=16x16:r=24:d=1",
            "-i",
        ])
        .arg(&input)
        .args(["-c:v", "ffv1", "-c:a", "pcm_s24le", "-shortest"])
        .arg(&carrier)
        .status()
        .unwrap();
    assert!(status.success());
    let object = Object::from_bytes(&fs::read(&carrier).unwrap());
    fs::rename(
        &carrier,
        root.path()
            .join("source/objects")
            .join(object.sha256.as_str()),
    )
    .unwrap();
    fs::remove_file(input).unwrap();
    let tool = PathBuf::from(env!("CARGO_BIN_EXE_reel-tig-job"));
    let profile = json!({"schema":"reel.tig-pcm-wave-recovery.experimental-1","byte_source":{"places":[{"root":root.path().join("source"),"layout":"flat"}],"required":"prevent-mutation"},"carrier":object,"target":target,"sample_rate":48000,"channels":2,"bits_per_sample":24,"max_output_bytes":target.bytes,"timeout_seconds":30,"runtime":{"host":reel::media_runtime::ByteIdentity::observe_file(&tool).unwrap(),"ffmpeg":pin(&program("ffmpeg")),"ffprobe":pin(&program("ffprobe")),"dependencies":{}},"publication":{"artifact_root":root.path().join("artifact"),"namespace":"recovery-fixture","storage":{"path_policy":"configured-local-roots","cache_roots":[root.path().join("destination")],"coordination_root":root.path().join("coordination"),"capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0,"max_object_bytes":1048576}},"destination":root.path().join("destination"),"reader_limits":{"object_bytes":1048576,"total_bytes":4194304,"requests":32,"live_readers":8,"verify_millis":30000},"protection":"prevent-mutation","request":{"operation":"restore-original","owner":"recovery-fixture","object":target,"required":"process_restart"}}});
    (root, profile)
}
fn run(root: &Path, profile: &Value, publish: bool) -> std::process::Output {
    let path = root.join("profile.json");
    fs::write(&path, serde_json::to_vec(profile).unwrap()).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_reel-tig-job"));
    command.arg("recover-pcm-wave").arg(path);
    if publish {
        command.arg("--publish");
    }
    command.output().unwrap()
}
fn destination(root: &Path, profile: &Value) -> PathBuf {
    reel_assets::object(
        &root.join("destination"),
        profile["target"]["sha256"].as_str().unwrap(),
    )
}
#[test]
fn exact_original_recovers_without_original_file_and_publishes_only_on_explicit_request() {
    let (root, profile) = fixture();
    let report = run(root.path(), &profile, false);
    assert!(
        report.status.success(),
        "{}",
        String::from_utf8_lossy(&report.stderr)
    );
    let value: Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(value["verified_target"], true);
    assert_eq!(value["published"], false);
    assert!(!destination(root.path(), &profile).exists());
    assert_eq!(
        fs::read_dir(root.path().join("coordination"))
            .unwrap()
            .count(),
        0
    );
    let report = run(root.path(), &profile, true);
    assert!(
        report.status.success(),
        "{}",
        String::from_utf8_lossy(&report.stderr)
    );
    assert_eq!(
        fs::read(destination(root.path(), &profile)).unwrap(),
        wave()
    );
    fs::remove_file(
        root.path()
            .join("source/objects")
            .join(profile["carrier"]["sha256"].as_str().unwrap()),
    )
    .unwrap();
    let report = run(root.path(), &profile, true);
    assert!(
        report.status.success(),
        "{}",
        String::from_utf8_lossy(&report.stderr)
    );
    let value: Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(value["reused_verified_object"], true);
    assert_eq!(value["decoded_this_run"], false);
    let mut independent = profile.clone();
    independent["publication"]["request"]["operation"] = json!("independent-reuse");
    let report = run(root.path(), &independent, true);
    assert!(
        report.status.success(),
        "{}",
        String::from_utf8_lossy(&report.stderr)
    );
    let value: Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(value["reused_verified_object"], true);
    assert_eq!(value["decoded_this_run"], false);
    // Existing destination bytes are verified, never trusted by pathname alone.
    fs::write(destination(root.path(), &profile), b"corrupt").unwrap();
    independent["publication"]["request"]["operation"] = json!("reject-corrupt-reuse");
    let report = run(root.path(), &independent, true);
    assert!(!report.status.success());
    assert_eq!(
        fs::read(destination(root.path(), &profile)).unwrap(),
        b"corrupt"
    );
}
#[test]
fn wrong_target_shape_budget_and_publication_target_cannot_install_bytes() {
    let (root, base) = fixture();
    for mode in [
        "hash",
        "shape",
        "budget",
        "publication",
        "timeout",
        "unknown",
    ] {
        let mut profile = base.clone();
        profile["publication"]["request"]["operation"] = json!(format!("restore-{mode}"));
        match mode {
            "hash" => {
                profile["target"]["sha256"] = json!("f".repeat(64));
                profile["publication"]["request"]["object"] = profile["target"].clone();
            }
            "shape" => profile["sample_rate"] = json!(44100),
            "budget" => profile["max_output_bytes"] = json!(1),
            "publication" => {
                profile["publication"]["request"]["object"]["sha256"] = json!("e".repeat(64))
            }
            "timeout" => profile["timeout_seconds"] = json!(0),
            _ => profile["unexpected"] = json!(true),
        }
        let report = run(root.path(), &profile, true);
        assert!(!report.status.success(), "mode {mode}");
        assert!(!destination(root.path(), &profile).exists());
    }
    assert!(!destination(root.path(), &base).exists());
}
