use std::{fs, process::Command};

#[test]
fn one_command_build_refuses_unrendered_poem_template() {
    let root = tempfile::tempdir().unwrap();
    let from = format!(
        "{}/tests/fixtures/scene-authoring",
        env!("CARGO_MANIFEST_DIR")
    );
    for name in [
        "catalog",
        "episode",
        "scene",
        "policy",
        "season-bindings",
        "episode-bindings",
        "scene-bindings",
    ] {
        fs::copy(
            format!("{from}/{name}.json"),
            root.path().join(format!("{name}.json")),
        )
        .unwrap();
    }
    fs::write(root.path().join("semantic-delivery.json"), "{}").unwrap();
    let manifest = serde_json::json!({
        "schema":"reel.scene-build.v1", "scene_id":"scene-poem", "language":"es",
        "catalog":"catalog.json", "episode":"episode.json", "scene":"scene.json",
        "policy":"policy.json", "season_bindings":"season-bindings.json",
        "episode_bindings":"episode-bindings.json", "scene_bindings":"scene-bindings.json",
        "semantic_delivery":"semantic-delivery.json"
    });
    fs::write(
        root.path().join("build.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let output_dir = root.path().join("output");
    let result = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("build")
        .arg(root.path())
        .arg("build.json")
        .arg("--asset-root")
        .arg(root.path())
        .arg("--output-dir")
        .arg(&output_dir)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("template presentation requires"));
    assert!(!output_dir.exists());
}
