//! Prove strict-check exit codes, output publication and standalone exports at the CLI boundary.
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_reel-assets"))
        .current_dir(root)
        .arg("--root")
        .arg(root)
        .args(args)
        .env_remove("CAMINOS_TRANSFER_CACHE")
        .output()
        .unwrap()
}
#[test]
fn inspection_reports_gaps_but_strict_check_fails_and_outputs_never_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir(root.join(".local")).unwrap();
    fs::write(root.join(".local/storage-rust.json"),json!({"path_policy":"configured-local-roots","cache_roots":[root.join("active")],"archive_roots":[root.join("offline")]}).to_string()).unwrap();
    fs::write(root.join("catalog.json"), r#"{"videos":[]}"#).unwrap();
    fs::write(
        root.join("unknown.json"),
        r#"{"schema":"unsupported.example.v1"}"#,
    )
    .unwrap();
    fs::write(root.join("profile.json"),json!({"schema":"reel.asset-inspection-project.v1","project_id":"test","roots":["unknown.json"]}).to_string()).unwrap();
    let result = run(&root, &["reference-inspect", "profile.json", "report.json"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let before = fs::read(root.join("report.json")).unwrap();
    assert!(
        !run(&root, &["reference-inspect", "profile.json", "report.json"])
            .status
            .success()
    );
    assert_eq!(before, fs::read(root.join("report.json")).unwrap());
    assert!(
        !run(&root, &["reference-check", "profile.json", "strict.json"])
            .status
            .success()
    );
    assert!(root.join("strict.json").is_file());
    assert!(
        run(
            &root,
            &["reference-query", "report.json", "repo:unknown.json"]
        )
        .status
        .success()
    );
    assert!(
        run(&root, &["reference-review", "report.json", "review.html"])
            .status
            .success()
    );
    let result = run(&root, &["schema-catalogue", "schemas.json"]);
    assert!(result.status.success());
    let catalogue: Value =
        serde_json::from_slice(&fs::read(root.join("schemas.json")).unwrap()).unwrap();
    assert_eq!(catalogue["contracts"].as_array().unwrap().len(), 11);
    assert!(!root.join("active").exists());
    assert!(!root.join("offline").exists());
    // Catalogue export is a pure code operation, independent of private storage controls.
    fs::remove_file(root.join(".local/storage-rust.json")).unwrap();
    assert!(
        run(&root, &["schema-catalogue", "schemas-without-cache.json"])
            .status
            .success()
    );
}
