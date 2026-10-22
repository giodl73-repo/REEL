//! The comparison must read existing state without selecting, deprecating, or copying payloads.
use reel_assets::versions::{State, Version};
use std::{collections::BTreeSet, fs, process::Command};

#[test]
fn comparison_is_read_only_and_rejects_report_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("registry")).unwrap();
    fs::write(root.join("catalog.json"), br#"{"videos":[]}"#).unwrap();
    let old = "a".repeat(64);
    let new = "b".repeat(64);
    let mut state = State {
        coverage_complete: true,
        ..State::default()
    };
    for (v, h) in [("v1", &old), ("v2", &new)] {
        state
            .register(Version {
                logical_id: if v == "v1" { "legacy" } else { "asset" }.into(),
                version_id: v.into(),
                manifest_sha256: h.clone(),
                payloads: BTreeSet::from([h.clone()]),
                dependencies: vec![],
                supersedes: None,
                deprecated: false,
            })
            .unwrap();
    }
    state.select("asset", "v2").unwrap();
    let original = serde_json::to_vec(&state).unwrap();
    fs::write(root.join("registry/versions.json"), &original).unwrap();
    fs::write(
        root.join("registry/objects.tsv"),
        format!("{old}\t20\n{new}\t30\n"),
    )
    .unwrap();
    let report = root.join("report.json");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_reel-assets"))
            .arg("--root")
            .arg(root)
            .args(["version-diff", "legacy", "v1", "asset", "v2"])
            .arg(&report)
            .output()
            .unwrap()
    };
    let out = run();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(
        value["eviction_candidate_sha256s"],
        serde_json::json!([old])
    );
    assert_eq!(value["candidate_logical_bytes"], 20);
    assert_eq!(
        fs::read(root.join("registry/versions.json")).unwrap(),
        original
    );
    assert!(!root.join(".local").exists());
    assert!(!run().status.success());
}
