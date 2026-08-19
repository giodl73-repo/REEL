use std::{fs, process::Command};

use tempfile::tempdir;

const ROOT: &str = "manifests/fixtures/sprite-library";

fn reel() -> Command {
    Command::new(env!("CARGO_BIN_EXE_reel"))
}

fn resolve_plan(directory: &std::path::Path) -> std::path::PathBuf {
    let plan = directory.join("plan.json");
    let output = reel()
        .args([
            "sprite-cast-resolve",
            &format!("{ROOT}/library.yaml"),
            &format!("{ROOT}/profile.yaml"),
            &format!("{ROOT}/cast.yaml"),
            "--output-path",
        ])
        .arg(&plan)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    plan
}

#[test]
fn materializes_and_reuses_content_addressed_pngs() {
    let directory = tempdir().unwrap();
    let plan = resolve_plan(directory.path());
    let cache = directory.path().join("cache");
    let receipt = directory.path().join("receipt.json");
    let run = |receipt_path: Option<&std::path::Path>| {
        let mut command = reel();
        command
            .arg("sprite-cache-materialize")
            .arg(format!("{ROOT}/catalog.yaml"))
            .arg(&plan)
            .arg(&cache)
            .args(["--width", "64", "--height", "64", "--output", "json"]);
        if let Some(path) = receipt_path {
            command.arg("--receipt-path").arg(path);
        }
        command.output().unwrap()
    };
    let first = run(Some(&receipt));
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_report: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first_report["written_outputs"], 3);
    assert_eq!(first_report["reused_outputs"], 0);
    let second = run(None);
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_report: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(second_report["written_outputs"], 0);
    assert_eq!(second_report["reused_outputs"], 3);
    assert!(
        !fs::read_to_string(receipt)
            .unwrap()
            .contains(directory.path().to_str().unwrap())
    );
}

#[test]
fn creates_a_path_free_contact_sheet_report() {
    let directory = tempdir().unwrap();
    let plan = resolve_plan(directory.path());
    let cache = directory.path().join("cache");
    let receipt = directory.path().join("receipt.json");
    let materialize = reel()
        .arg("sprite-cache-materialize")
        .arg(format!("{ROOT}/catalog.yaml"))
        .arg(&plan)
        .arg(&cache)
        .args(["--width", "64", "--height", "64", "--receipt-path"])
        .arg(&receipt)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(materialize.status.success());
    let sheet = directory.path().join("sheet.png");
    let output = reel()
        .arg("sprite-cache-contact-sheet")
        .arg(&receipt)
        .arg(&cache)
        .arg(&sheet)
        .args(["--columns", "2", "--tile-size", "64", "--output", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["width"], 128);
    assert_eq!(report["height"], 128);
    assert_eq!(report["cells"].as_array().unwrap().len(), 3);
    assert!(sheet.exists());
}

#[test]
fn rejects_catalog_hash_tampering_before_writing_cache() {
    let directory = tempdir().unwrap();
    let plan = resolve_plan(directory.path());
    let catalog = directory.path().join("catalog.yaml");
    fs::copy(
        format!("{ROOT}/two-color.ppm"),
        directory.path().join("two-color.ppm"),
    )
    .unwrap();
    fs::write(
        &catalog,
        fs::read_to_string(format!("{ROOT}/catalog.yaml"))
            .unwrap()
            .replace("04d51491", "14d51491"),
    )
    .unwrap();
    let output = reel()
        .arg("sprite-cache-materialize")
        .arg(catalog)
        .arg(plan)
        .arg(directory.path().join("cache"))
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("source hash does not match"));
}

#[test]
fn changing_one_recipe_invalidates_only_its_consumers() {
    let directory = tempdir().unwrap();
    let plan = resolve_plan(directory.path());
    let cache = directory.path().join("cache");
    let first = reel()
        .arg("sprite-cache-materialize")
        .arg(format!("{ROOT}/catalog.yaml"))
        .arg(&plan)
        .arg(&cache)
        .args(["--width", "64", "--height", "64", "--output", "json"])
        .output()
        .unwrap();
    assert!(first.status.success());

    fs::copy(
        format!("{ROOT}/two-color.ppm"),
        directory.path().join("two-color.ppm"),
    )
    .unwrap();
    fs::copy(
        format!("{ROOT}/two-color-reversed.ppm"),
        directory.path().join("two-color-reversed.ppm"),
    )
    .unwrap();
    let original = r#"  fixture/identity/keeper/v1:
    kind: asset
    path: two-color.ppm
    sha256: 04d51491a8fda866404157f3c5e591e3f8d7150ef98240ffb9205590428b5477
    mirror_behavior: preserve"#;
    let replacement = r#"  fixture/identity/keeper/v1:
    kind: asset
    path: two-color-reversed.ppm
    sha256: 9f7f24e597977ca2351861b331fc5a48579cc7d8e900399c5d47a29d75c1d77e
    mirror_behavior: preserve"#;
    let catalog = directory.path().join("catalog.yaml");
    let source = fs::read_to_string(format!("{ROOT}/catalog.yaml"))
        .unwrap()
        .replace(original, replacement);
    assert!(source.contains("two-color-reversed.ppm"));
    fs::write(&catalog, source).unwrap();
    let second = reel()
        .arg("sprite-cache-materialize")
        .arg(&catalog)
        .arg(&plan)
        .arg(&cache)
        .args(["--width", "64", "--height", "64", "--output", "json"])
        .output()
        .unwrap();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(report["reused_outputs"], 2);
    assert_eq!(report["written_outputs"], 1);
    let keeper = report["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["character"] == "keeper")
        .unwrap();
    assert_eq!(keeper["reused"], false);
}
