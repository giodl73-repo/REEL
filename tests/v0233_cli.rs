use std::{fs, path::Path, process::Command};

use sha2::{Digest, Sha256};
use tempfile::tempdir;

const SPRITE_ROOT: &str = "manifests/fixtures/sprite-library";
const CHOREOGRAPHY: &str = "manifests/fixtures/choreography/simple-handoff.yaml";
const BASE_ASSETS: &str = "manifests/fixtures/choreography/assets.yaml";
const CHOREOGRAPHY_SHA: &str = "fd46892b48b865e7c1fb1ad270e21bb971ee8a81e033ca4b4bfa1388ca612c9a";

fn reel() -> Command {
    Command::new(env!("CARGO_BIN_EXE_reel"))
}

fn sha(path: impl AsRef<Path>) -> String {
    Sha256::digest(fs::read(path).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn prepare(directory: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let plan = directory.join("plan.json");
    let resolve = reel()
        .args([
            "sprite-cast-resolve",
            &format!("{SPRITE_ROOT}/library.yaml"),
            &format!("{SPRITE_ROOT}/profile.yaml"),
            &format!("{SPRITE_ROOT}/cast.yaml"),
            "--output-path",
        ])
        .arg(&plan)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(resolve.status.success());
    let cache = directory.join("cache");
    let receipt = directory.join("receipt.json");
    let materialize = reel()
        .arg("sprite-cache-materialize")
        .arg(format!("{SPRITE_ROOT}/catalog.yaml"))
        .arg(&plan)
        .arg(&cache)
        .args(["--width", "64", "--height", "64", "--receipt-path"])
        .arg(&receipt)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(materialize.status.success());
    (cache, receipt)
}

fn staging_base(directory: &Path) -> std::path::PathBuf {
    let fixture = Path::new("manifests/fixtures/choreography/assets");
    let mut source = fs::read_to_string(BASE_ASSETS).unwrap();
    for name in [
        "background.ppm",
        "blue.ppm",
        "red.ppm",
        "yellow.ppm",
        "green.ppm",
        "gray.ppm",
        "token.ppm",
    ] {
        let absolute = fixture.join(name).canonicalize().unwrap();
        source = source.replace(name, &absolute.to_string_lossy().replace('\\', "/"));
    }
    let output = directory.join("base-assets.yaml");
    fs::write(&output, source).unwrap();
    output
}

fn binding(receipt_sha: &str, base_sha: &str, preserve: &str) -> String {
    format!(
        r#"schema: reel.sprite-choreography-binding.v0.1
choreography_sha256: {CHOREOGRAPHY_SHA}
base_assets_sha256: {base_sha}
materialization_receipt_sha256: {receipt_sha}
performers:
  initiator:
    character: initiator
    default_request: carry
  receiver:
    character: keeper
    default_request: set
    poses: {{ celebrate: set }}
  observer:
    character: initiator
    default_request: carry
    poses: {{ recoil: release }}
preserve_unmapped_performers: {preserve}
"#,
        base_sha = base_sha
    )
}

#[test]
fn stages_cache_assets_and_compiles_choreography() {
    let directory = tempdir().unwrap();
    let (cache, receipt) = prepare(directory.path());
    let base_assets = staging_base(directory.path());
    let binding_path = directory.path().join("binding.yaml");
    fs::write(
        &binding_path,
        binding(&sha(&receipt), &sha(&base_assets), "[]"),
    )
    .unwrap();
    let staged = directory.path().join("staged-assets.yaml");
    let stage = reel()
        .arg("sprite-choreography-stage")
        .arg(&binding_path)
        .arg(&receipt)
        .arg(&base_assets)
        .arg(&cache)
        .arg(&staged)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(
        stage.status.success(),
        "{}",
        String::from_utf8_lossy(&stage.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&stage.stdout).unwrap();
    assert_eq!(report["cache_bound_performers"], 3);
    assert_eq!(report["cache_bindings"].as_array().unwrap().len(), 5);
    assert!(!String::from_utf8_lossy(&stage.stdout).contains(directory.path().to_str().unwrap()));

    let manifest = directory.path().join("manifest.yaml");
    let compile = reel()
        .arg("choreography-sprite-manifest")
        .arg(CHOREOGRAPHY)
        .arg(&staged)
        .arg("--output-path")
        .arg(&manifest)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    assert!(manifest.exists());
}

#[test]
fn rejects_stale_receipt_binding() {
    let directory = tempdir().unwrap();
    let (cache, receipt) = prepare(directory.path());
    let base_assets = staging_base(directory.path());
    let binding_path = directory.path().join("binding.yaml");
    fs::write(
        &binding_path,
        binding(&"0".repeat(64), &sha(&base_assets), "[]"),
    )
    .unwrap();
    let output = reel()
        .arg("sprite-choreography-stage")
        .arg(binding_path)
        .arg(receipt)
        .arg(&base_assets)
        .arg(cache)
        .arg(directory.path().join("staged.yaml"))
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("receipt hash does not match"));
}

#[test]
fn rejects_implicit_unmapped_performer() {
    let directory = tempdir().unwrap();
    let (cache, receipt) = prepare(directory.path());
    let base_assets = staging_base(directory.path());
    let binding_path = directory.path().join("binding.yaml");
    let source = binding(&sha(&receipt), &sha(&base_assets), "[]").replace(
        "  observer:\n    character: initiator\n    default_request: carry\n    poses: { recoil: release }\n",
        "",
    );
    fs::write(&binding_path, source).unwrap();
    let output = reel()
        .arg("sprite-choreography-stage")
        .arg(binding_path)
        .arg(receipt)
        .arg(&base_assets)
        .arg(cache)
        .arg(directory.path().join("staged.yaml"))
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("explicitly cache-bind or preserve every performer")
    );
}
