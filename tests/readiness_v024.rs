use std::fs;

use tempfile::tempdir;

fn manifest(shots: &str) -> String {
    format!(
        r#"
manifest_version: reel.manifest.v0.2
profile: animatic
timing_status: conformed
work: readiness-fixture
title: Readiness fixture
scenes:
  - {{ id: scene-01, duration_seconds: 2.0 }}
shots:
{shots}
"#
    )
}

fn validate(shots: &str) -> reel::production::ProductionValidationReport {
    let dir = tempdir().unwrap();
    let path = dir.path().join("manifest.yaml");
    fs::write(&path, manifest(shots)).unwrap();
    reel::production::validate(&reel::production::load(path).unwrap()).unwrap()
}

#[test]
fn separates_timing_from_explicit_asset_readiness() {
    let report = validate(
        r#"  - { id: planned, scene_id: scene-01, start_seconds: 0.0, duration_seconds: 1.0, visual_asset_status: planned-unrendered }
  - { id: candidate, scene_id: scene-01, start_seconds: 1.0, duration_seconds: 1.0, visual_asset: candidate.png, visual_asset_status: candidate-unreviewed }
"#,
    );

    assert!(report.timing_ready);
    assert!(!report.generation_ready);
    assert!(!report.asset_ready);
    assert!(!report.preview_ready);
    assert!(!report.delivery_ready);
    assert_eq!(report.asset_status_counts["planned-unrendered"], 1);
    assert_eq!(report.asset_status_counts["candidate"], 1);
    assert!(
        report
            .gated_commands
            .contains(&"animatic-render".to_string())
    );
    assert_eq!(report.semantic_blockers.len(), 2);
}

#[test]
fn preserves_legacy_asset_backed_manifests_as_ready() {
    let report = validate(
        r#"  - { id: first, scene_id: scene-01, start_seconds: 0.0, duration_seconds: 1.0, visual_asset: first.png }
  - { id: second, scene_id: scene-01, start_seconds: 1.0, duration_seconds: 1.0, visual_asset: second.png }
"#,
    );

    assert!(report.timing_ready);
    assert!(report.generation_ready);
    assert!(report.asset_ready);
    assert!(report.preview_ready);
    assert!(report.delivery_ready);
    assert_eq!(report.asset_status_counts["selected"], 2);
    assert!(report.semantic_blockers.is_empty());
}

#[test]
fn requires_an_explicit_prompt_render_contract() {
    let report = validate(
        r#"  - { id: first, scene_id: scene-01, start_seconds: 0.0, duration_seconds: 1.0, visual_prompt: generated first, render_from_prompt: true }
  - { id: second, scene_id: scene-01, start_seconds: 1.0, duration_seconds: 1.0, visual_prompt: generated second, render_from_prompt: true }
"#,
    );

    assert!(report.generation_ready);
    assert!(!report.asset_ready);
    assert!(!report.preview_ready);
    assert!(!report.delivery_ready);
    assert_eq!(report.asset_status_counts["prompt-renderable"], 2);
    assert!(
        report
            .semantic_blockers
            .iter()
            .any(|blocker| blocker.contains("require prompt rendering"))
    );
}

#[test]
fn rejects_selected_media_without_a_renderable_source() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("manifest.yaml");
    fs::write(
        &path,
        manifest(
            r#"  - { id: first, scene_id: scene-01, start_seconds: 0.0, duration_seconds: 1.0, visual_asset_status: selected }
  - { id: second, scene_id: scene-01, start_seconds: 1.0, duration_seconds: 1.0, visual_asset: second.png, visual_asset_status: selected }
"#,
        ),
    )
    .unwrap();

    let error = reel::production::validate(&reel::production::load(path).unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("declares selected media without visual_asset"));
}
