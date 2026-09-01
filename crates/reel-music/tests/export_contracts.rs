use std::{fs, path::PathBuf};

use tempfile::tempdir;

fn model_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("manifests/fixtures/music-model-corrected/model.yaml")
}

#[test]
fn writes_and_revalidates_a_model_bound_export_plan() {
    let temporary = tempdir().unwrap();
    let plan = temporary.path().join("plan.json");
    let report = reel_music::export::write(&model_fixture(), &plan).unwrap();
    assert!(report.verified);
    assert!(!report.shareable);
    assert_eq!(report.artifacts, 3);
    assert_eq!(report.lyric_layers, 0);
    assert!(reel_music::export::validate(&plan, &model_fixture()).is_ok());
    assert!(reel_music::export::write(&model_fixture(), &plan).is_err());
}

#[test]
fn rejects_a_hand_edited_or_stale_export_plan() {
    let temporary = tempdir().unwrap();
    let plan = temporary.path().join("plan.json");
    reel_music::export::write(&model_fixture(), &plan).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&plan).unwrap()).unwrap();
    value["duration_ticks"] = serde_json::json!(3840);
    fs::write(&plan, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    assert!(reel_music::export::validate(&plan, &model_fixture()).is_err());
}
