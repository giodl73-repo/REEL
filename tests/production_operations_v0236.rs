use std::{fs, path::Path, process::Command};

use image::{Rgba, RgbaImage};
use serde_json::{Value, json};
use tempfile::tempdir;

fn hash(path: impl AsRef<Path>) -> String {
    reel::production::sha256_path(path).unwrap()
}

fn write_json(path: impl AsRef<Path>, value: Value) {
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&value).unwrap()),
    )
    .unwrap();
}

fn evidence(byte: char) -> String {
    byte.to_string().repeat(64)
}

fn write_picture(path: impl AsRef<Path>, width: u32, height: u32) {
    RgbaImage::from_pixel(width, height, Rgba([10, 20, 30, 255]))
        .save(path)
        .unwrap();
}

fn write_picture_manifest(path: impl AsRef<Path>, shots: usize) {
    let mut shot_yaml = String::new();
    for index in 0..shots {
        shot_yaml.push_str(&format!(
            "  - {{ id: shot-{number:02}, scene_id: scene-01, start_seconds: {index}.0, duration_seconds: 1.0, visual_prompt: fixture {number}, render_from_prompt: true }}\n",
            number = index + 1
        ));
    }
    fs::write(
        path,
        format!(
            r#"
manifest_version: reel.manifest.v0.2
profile: animatic
timing_status: conformed
work: operations-fixture
title: Operations fixture
scenes:
  - {{ id: scene-01, duration_seconds: {shots}.0 }}
shots:
{shot_yaml}
"#
        ),
    )
    .unwrap();
}

fn write_voice_manifest(path: impl AsRef<Path>) {
    fs::write(
        path,
        r#"
manifest_version: reel.manifest.v0.2
profile: animatic
timing_status: conformed
work: voice-ledger-fixture
title: Voice ledger fixture
scenes:
  - { id: scene-01, duration_seconds: 4.0 }
shots:
  - { id: shot-01, scene_id: scene-01, start_seconds: 0.0, duration_seconds: 1.0, narration_cue_ids: [cue-01] }
  - { id: shot-02, scene_id: scene-01, start_seconds: 1.0, duration_seconds: 1.0, narration_cue_ids: [cue-02] }
  - { id: shot-03, scene_id: scene-01, start_seconds: 2.0, duration_seconds: 1.0, narration_cue_ids: [cue-03] }
  - { id: shot-04, scene_id: scene-01, start_seconds: 3.0, duration_seconds: 1.0, narration_cue_ids: [cue-04] }
speakers:
  - { id: narrator }
narration_cues:
  - { id: cue-01, speaker_id: narrator, text: one, source_refs: [source-01], shot_ids: [shot-01], start_seconds: 0.0, duration_seconds: 1.0 }
  - { id: cue-02, speaker_id: narrator, text: two, source_refs: [source-02], shot_ids: [shot-02], start_seconds: 1.0, duration_seconds: 1.0 }
  - { id: cue-03, speaker_id: narrator, text: three, source_refs: [source-03], shot_ids: [shot-03], start_seconds: 2.0, duration_seconds: 1.0 }
  - { id: cue-04, speaker_id: narrator, text: four, source_refs: [source-04], shot_ids: [shot-04], start_seconds: 3.0, duration_seconds: 1.0 }
source_ranges:
  - { id: source-01, start: 1, end: 1 }
  - { id: source-02, start: 2, end: 2 }
  - { id: source-03, start: 3, end: 3 }
  - { id: source-04, start: 4, end: 4 }
"#,
    )
    .unwrap();
}

#[test]
fn generation_and_materialization_are_strict_verified_and_path_free() {
    let directory = tempdir().unwrap();
    let manifest = directory.path().join("manifest.yaml");
    write_picture_manifest(&manifest, 1);
    let manifest_hash = hash(&manifest);
    let generation_input = directory.path().join("generation.json");
    write_json(
        &generation_input,
        json!({
            "schema": "reel.generation-plan-input.v0.1",
            "production_manifest_sha256": manifest_hash,
            "tool_version": "fixture-1",
            "units": [{
                "unit_id": "unit-01",
                "shot_id": "shot-01",
                "prompt_sha256": evidence('a'),
                "input_hashes": [{"id": "reference", "sha256": evidence('b')}],
                "expected_output": {"media_type": "image/png", "width": 2, "height": 2}
            }]
        }),
    );
    let plan_path = directory.path().join("plan.json");
    let plan = reel::production_operations::write_generation_plan(
        &manifest,
        &generation_input,
        &plan_path,
    )
    .unwrap();
    assert!(!plan.provider_execution_requested);
    assert!(
        !fs::read_to_string(&plan_path)
            .unwrap()
            .contains(&directory.path().display().to_string())
    );
    assert!(
        reel::production_operations::write_generation_plan(
            &manifest,
            &generation_input,
            &plan_path
        )
        .unwrap_err()
        .to_string()
        .contains("refusing to overwrite")
    );

    let picture = directory.path().join("output.png");
    write_picture(&picture, 2, 2);
    let result_input = directory.path().join("result.json");
    write_json(
        &result_input,
        json!({
            "schema": "reel.materialization-result-input.v0.1",
            "generation_plan_sha256": hash(&plan_path),
            "production_manifest_sha256": manifest_hash,
            "outputs": [{
                "unit_id": "unit-01",
                "path": picture,
                "sha256": hash(&picture),
                "bytes": fs::metadata(&picture).unwrap().len(),
                "width": 2,
                "height": 2
            }]
        }),
    );
    let receipt_path = directory.path().join("receipt.json");
    let receipt = reel::production_operations::write_materialization_receipt(
        &plan_path,
        &result_input,
        &receipt_path,
    )
    .unwrap();
    assert!(receipt.all_outputs_verified);
    assert!(!receipt.provider_executed_by_reel);
    assert!(
        !fs::read_to_string(&receipt_path)
            .unwrap()
            .contains(&directory.path().display().to_string())
    );

    let tampered = directory.path().join("tampered.json");
    let mut value: Value =
        serde_json::from_str(&fs::read_to_string(&result_input).unwrap()).unwrap();
    value["outputs"][0]["sha256"] = Value::String(evidence('c'));
    write_json(&tampered, value);
    assert!(
        reel::production_operations::write_materialization_receipt(
            &plan_path,
            &tampered,
            directory.path().join("tampered-receipt.json")
        )
        .unwrap_err()
        .to_string()
        .contains("hash mismatch")
    );

    let unknown = directory.path().join("unknown.json");
    let mut value: Value =
        serde_json::from_str(&fs::read_to_string(&generation_input).unwrap()).unwrap();
    value["credential"] = Value::String("must-be-rejected".to_string());
    write_json(&unknown, value);
    assert!(
        reel::production_operations::write_generation_plan(
            &manifest,
            &unknown,
            directory.path().join("unknown-plan.json")
        )
        .is_err()
    );
}

#[test]
fn promotion_chain_rejects_skips_reversals_and_stale_assets() {
    let directory = tempdir().unwrap();
    let asset = directory.path().join("asset.bin");
    fs::write(&asset, b"candidate bytes").unwrap();
    let asset_hash = hash(&asset);
    let candidate_input = directory.path().join("candidate-input.json");
    write_json(
        &candidate_input,
        json!({
            "schema": "reel.asset-promotion-input.v0.1",
            "asset_id": "asset-01",
            "asset": {"path": asset, "sha256": asset_hash},
            "state": "candidate",
            "review_evidence_sha256": [evidence('a')]
        }),
    );
    let candidate_path = directory.path().join("candidate.json");
    let candidate =
        reel::production_operations::write_asset_promotion(&candidate_input, &candidate_path)
            .unwrap();
    assert!(!candidate.publication_approved);
    assert!(!candidate.rights_approved);

    let skipped_input = directory.path().join("skipped.json");
    write_json(
        &skipped_input,
        json!({
            "schema": "reel.asset-promotion-input.v0.1",
            "asset_id": "asset-01",
            "asset": {"path": asset, "sha256": asset_hash},
            "state": "approved",
            "prior_record": {"path": candidate_path, "sha256": hash(&candidate_path)},
            "review_evidence_sha256": [evidence('b')]
        }),
    );
    assert!(
        reel::production_operations::write_asset_promotion(
            &skipped_input,
            directory.path().join("skipped-record.json")
        )
        .unwrap_err()
        .to_string()
        .contains("transition")
    );

    let selected_input = directory.path().join("selected-input.json");
    write_json(
        &selected_input,
        json!({
            "schema": "reel.asset-promotion-input.v0.1",
            "asset_id": "asset-01",
            "asset": {"path": asset, "sha256": asset_hash},
            "state": "selected",
            "prior_record": {"path": candidate_path, "sha256": hash(&candidate_path)},
            "review_evidence_sha256": [evidence('b')]
        }),
    );
    let selected_path = directory.path().join("selected.json");
    reel::production_operations::write_asset_promotion(&selected_input, &selected_path).unwrap();

    let reversed_input = directory.path().join("reversed-input.json");
    write_json(
        &reversed_input,
        json!({
            "schema": "reel.asset-promotion-input.v0.1",
            "asset_id": "asset-01",
            "asset": {"path": asset, "sha256": asset_hash},
            "state": "candidate",
            "prior_record": {"path": selected_path, "sha256": hash(&selected_path)},
            "review_evidence_sha256": [evidence('c')]
        }),
    );
    assert!(
        reel::production_operations::write_asset_promotion(
            &reversed_input,
            directory.path().join("reversed.json")
        )
        .unwrap_err()
        .to_string()
        .contains("must not cite a prior record")
    );

    let approved_input = directory.path().join("approved-input.json");
    write_json(
        &approved_input,
        json!({
            "schema": "reel.asset-promotion-input.v0.1",
            "asset_id": "asset-01",
            "asset": {"path": asset, "sha256": asset_hash},
            "state": "approved",
            "prior_record": {"path": selected_path, "sha256": hash(&selected_path)},
            "prior_chain": [{"path": candidate_path, "sha256": hash(&candidate_path)}],
            "review_evidence_sha256": [evidence('c')]
        }),
    );
    let approved = reel::production_operations::write_asset_promotion(
        &approved_input,
        directory.path().join("approved.json"),
    )
    .unwrap();
    assert_eq!(
        approved.state,
        reel::production_operations::PromotionState::Approved
    );
    assert!(!approved.publication_approved);
    assert!(!approved.rights_approved);

    fs::write(&asset, b"changed bytes").unwrap();
    assert!(
        reel::production_operations::write_asset_promotion(
            &approved_input,
            directory.path().join("stale.json")
        )
        .unwrap_err()
        .to_string()
        .contains("hash mismatch")
    );
}

#[test]
fn picture_plan_reports_incremental_states_and_blocks_proxy_delivery() {
    let directory = tempdir().unwrap();
    let manifest = directory.path().join("manifest.yaml");
    write_picture_manifest(&manifest, 5);
    let manifest_hash = hash(&manifest);
    let input_path = directory.path().join("picture-input.json");
    let recipes = (1..=5)
        .map(|index| {
            json!({
                "shot_id": format!("shot-{index:02}"),
                "prompt_sha256": evidence(char::from_digit(index, 10).unwrap()),
                "input_hashes": [],
                "recipe_sha256": evidence('a')
            })
        })
        .collect::<Vec<_>>();
    write_json(
        &input_path,
        json!({
            "schema": "reel.picture-plan-input.v0.1",
            "production_manifest_sha256": manifest_hash,
            "tool_version": "fixture-1",
            "review_profile": "internal-picture-review",
            "disclosure": "REVIEW PROXY - NOT DELIVERY",
            "output_profile": {
                "id": "review-2x2",
                "width": 2,
                "height": 2,
                "media_type": "image/png",
                "purpose": "delivery"
            },
            "shots": recipes
        }),
    );
    let initial =
        reel::production_operations::picture_plan(&manifest, &input_path, None, None).unwrap();
    let keys = initial
        .shots
        .iter()
        .map(|shot| (shot.shot_id.clone(), shot.recipe_key.clone().unwrap()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let picture = directory.path().join("cached.png");
    write_picture(&picture, 2, 2);
    let picture_hash = hash(&picture);
    let picture_bytes = fs::metadata(&picture).unwrap().len();
    let cache_path = directory.path().join("cache.json");
    write_json(
        &cache_path,
        json!({
            "schema": "reel.picture-cache-index.v0.1",
            "production_manifest_sha256": manifest_hash,
            "entries": [
                {
                    "shot_id": "shot-01",
                    "recipe_key": keys["shot-01"],
                    "output_sha256": picture_hash,
                    "bytes": picture_bytes,
                    "width": 2,
                    "height": 2,
                    "local_path": picture
                },
                {
                    "shot_id": "shot-02",
                    "recipe_key": keys["shot-02"],
                    "output_sha256": picture_hash,
                    "bytes": picture_bytes,
                    "width": 2,
                    "height": 2
                },
                {
                    "shot_id": "shot-03",
                    "recipe_key": evidence('f'),
                    "output_sha256": picture_hash,
                    "bytes": picture_bytes,
                    "width": 2,
                    "height": 2
                }
            ]
        }),
    );
    let report = reel::production_operations::picture_plan(
        &manifest,
        &input_path,
        Some(&cache_path),
        Some(&directory.path().join("picture-report.json")),
    )
    .unwrap();
    assert_eq!(report.exact_byte_reuse, 1);
    assert_eq!(report.recipe_equivalent_regeneration, 1);
    assert_eq!(report.stale, 1);
    assert_eq!(report.render, 2);
    assert_eq!(report.missing, 0);
    assert!(!report.delivery_ready);
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains(&directory.path().display().to_string())
    );

    let mut proxy_input: Value =
        serde_json::from_str(&fs::read_to_string(&input_path).unwrap()).unwrap();
    proxy_input["output_profile"]["purpose"] = Value::String("review-proxy".to_string());
    let proxy_path = directory.path().join("proxy.json");
    write_json(&proxy_path, proxy_input);
    let proxy_initial =
        reel::production_operations::picture_plan(&manifest, &proxy_path, None, None).unwrap();
    let proxy_cache = directory.path().join("proxy-cache.json");
    let entries = proxy_initial
        .shots
        .iter()
        .map(|shot| {
            json!({
                "shot_id": shot.shot_id,
                "recipe_key": shot.recipe_key,
                "output_sha256": picture_hash,
                "bytes": picture_bytes,
                "width": 2,
                "height": 2,
                "local_path": picture
            })
        })
        .collect::<Vec<_>>();
    write_json(
        &proxy_cache,
        json!({
            "schema": "reel.picture-cache-index.v0.1",
            "production_manifest_sha256": manifest_hash,
            "entries": entries
        }),
    );
    let proxy =
        reel::production_operations::picture_plan(&manifest, &proxy_path, Some(&proxy_cache), None)
            .unwrap();
    assert_eq!(proxy.exact_byte_reuse, 5);
    assert!(proxy.proxy);
    assert!(!proxy.delivery_ready);
}

#[test]
fn review_queue_and_portfolio_audit_reject_invalid_authority_inputs() {
    let directory = tempdir().unwrap();
    let manifest = directory.path().join("manifest.yaml");
    write_picture_manifest(&manifest, 2);
    let manifest_hash = hash(&manifest);
    let findings = directory.path().join("findings.json");
    write_json(
        &findings,
        json!({
            "schema": "reel.timecoded-review-findings.v0.1",
            "production_manifest_sha256": manifest_hash,
            "findings": [
                {
                    "id": "minor-open",
                    "shot_id": "shot-02",
                    "start_ms": 1000,
                    "end_ms": 1500,
                    "severity": "minor",
                    "owner": "editor",
                    "status": "open",
                    "evidence_sha256": [evidence('a')]
                },
                {
                    "id": "blocker-open",
                    "shot_id": "shot-01",
                    "start_ms": 0,
                    "end_ms": 500,
                    "severity": "blocker",
                    "owner": "animation",
                    "status": "in-progress",
                    "evidence_sha256": [evidence('b')]
                },
                {
                    "id": "resolved",
                    "shot_id": "shot-01",
                    "start_ms": 500,
                    "end_ms": 900,
                    "severity": "major",
                    "owner": "sound",
                    "status": "resolved",
                    "evidence_sha256": [evidence('c')]
                }
            ]
        }),
    );
    let queue = reel::production_operations::repair_queue(&manifest, &findings, None).unwrap();
    assert_eq!(queue.open_count, 2);
    assert_eq!(queue.open_findings[0].finding_id, "blocker-open");
    assert!(queue.human_decision_required);
    assert!(!queue.approvals_inferred);

    let invalid = directory.path().join("invalid-findings.json");
    let mut value: Value = serde_json::from_str(&fs::read_to_string(&findings).unwrap()).unwrap();
    value["findings"][0]["shot_id"] = Value::String("unknown".to_string());
    write_json(&invalid, value);
    assert!(
        reel::production_operations::repair_queue(&manifest, &invalid, None)
            .unwrap_err()
            .to_string()
            .contains("unknown shot")
    );
    let invalid_range = directory.path().join("invalid-range.json");
    let mut value: Value = serde_json::from_str(&fs::read_to_string(&findings).unwrap()).unwrap();
    value["findings"][0]["end_ms"] = Value::Number(2500.into());
    write_json(&invalid_range, value);
    assert!(
        reel::production_operations::repair_queue(&manifest, &invalid_range, None)
            .unwrap_err()
            .to_string()
            .contains("invalid or out-of-shot")
    );

    let index = directory.path().join("index.json");
    write_json(
        &index,
        json!({
            "schema": "reel.production-state-index.v0.1",
            "manifests": [
                {"id": "current", "manifest_path": "manifest.yaml", "manifest_sha256": manifest_hash},
                {"id": "stale", "manifest_path": "manifest.yaml", "manifest_sha256": evidence('f')}
            ]
        }),
    );
    let audit = reel::production_operations::production_state_audit(&index, None).unwrap();
    assert_eq!(audit.total, 2);
    assert_eq!(audit.valid, 1);
    assert_eq!(audit.stale_hashes, 1);
    assert!(
        !serde_json::to_string(&audit)
            .unwrap()
            .contains(&directory.path().display().to_string())
    );
}

#[test]
fn voice_retake_queue_only_contains_rejected_and_missing_spans() {
    let directory = tempdir().unwrap();
    let manifest = directory.path().join("manifest.yaml");
    write_voice_manifest(&manifest);
    let manifest_hash = hash(&manifest);
    let voice_plan = directory.path().join("voice-plan.json");
    fs::write(&voice_plan, b"{}").unwrap();
    let selected_audio = directory.path().join("selected.wav");
    let rejected_audio = directory.path().join("rejected.wav");
    let pending_audio = directory.path().join("pending.wav");
    fs::write(&selected_audio, b"selected").unwrap();
    fs::write(&rejected_audio, b"rejected").unwrap();
    fs::write(&pending_audio, b"pending").unwrap();
    let input_path = directory.path().join("takes.json");
    write_json(
        &input_path,
        json!({
            "schema": "reel.voice-take-ledger-input.v0.1",
            "production_manifest_sha256": manifest_hash,
            "voice_plan": {"path": voice_plan, "sha256": hash(&voice_plan)},
            "takes": [
                {
                    "cue_id": "cue-01",
                    "take_id": "take-selected",
                    "audio": {"path": selected_audio, "sha256": hash(&selected_audio)},
                    "start_ms": 0,
                    "end_ms": 1000,
                    "disposition": "available",
                    "evidence_sha256": [evidence('a')]
                },
                {
                    "cue_id": "cue-02",
                    "take_id": "take-rejected",
                    "audio": {"path": rejected_audio, "sha256": hash(&rejected_audio)},
                    "start_ms": 1000,
                    "end_ms": 2000,
                    "disposition": "rejected",
                    "evidence_sha256": [evidence('b')]
                },
                {
                    "cue_id": "cue-04",
                    "take_id": "take-pending",
                    "audio": {"path": pending_audio, "sha256": hash(&pending_audio)},
                    "start_ms": 3000,
                    "end_ms": 4000,
                    "disposition": "available",
                    "evidence_sha256": [evidence('c')]
                }
            ],
            "selections": [{
                "cue_id": "cue-01",
                "take_id": "take-selected",
                "evidence_sha256": [evidence('d')]
            }]
        }),
    );
    let report =
        reel::production_operations::voice_take_ledger(&manifest, &input_path, None).unwrap();
    assert_eq!(report.selected_takes["cue-01"], "take-selected");
    assert_eq!(report.retake_queue.len(), 2);
    assert_eq!(report.retake_queue[0].cue_id, "cue-02");
    assert_eq!(report.retake_queue[1].cue_id, "cue-03");
    assert_eq!(report.awaiting_selection, vec!["cue-04"]);
    assert!(!report.synthesis_requested);
    assert!(!report.voice_approval_inferred);

    let invalid = directory.path().join("invalid-selection.json");
    let mut value: Value = serde_json::from_str(&fs::read_to_string(&input_path).unwrap()).unwrap();
    value["selections"][0]["cue_id"] = Value::String("cue-02".to_string());
    value["selections"][0]["take_id"] = Value::String("take-rejected".to_string());
    write_json(&invalid, value);
    assert!(
        reel::production_operations::voice_take_ledger(&manifest, &invalid, None)
            .unwrap_err()
            .to_string()
            .contains("cannot be selected")
    );
}

#[test]
fn music_provenance_requires_both_exact_comparison_variants() {
    let directory = tempdir().unwrap();
    let manifest = directory.path().join("manifest.yaml");
    write_picture_manifest(&manifest, 1);
    let manifest_hash = hash(&manifest);
    let score_plan = directory.path().join("score-plan.json");
    let scored = directory.path().join("scored.wav");
    let no_score = directory.path().join("no-score.wav");
    fs::write(&score_plan, b"score-plan").unwrap();
    fs::write(&scored, b"scored").unwrap();
    fs::write(&no_score, b"no-score").unwrap();
    let input_path = directory.path().join("music.json");
    write_json(
        &input_path,
        json!({
            "schema": "reel.music-provenance-input.v0.1",
            "production_manifest_sha256": manifest_hash,
            "score_plan": {"path": score_plan, "sha256": hash(&score_plan)},
            "variants": [
                {
                    "id": "scored",
                    "kind": "scored",
                    "audio": {"path": scored, "sha256": hash(&scored)},
                    "source": "original-commission",
                    "license": "documented",
                    "provenance": "human-authored",
                    "human_review_status": "reviewed",
                    "evidence_sha256": [evidence('a')]
                },
                {
                    "id": "explicit-no-score",
                    "kind": "no-score",
                    "audio": {"path": no_score, "sha256": hash(&no_score)},
                    "source": "no-score",
                    "license": "not-applicable",
                    "provenance": "no-score",
                    "human_review_status": "pending",
                    "evidence_sha256": [evidence('b')]
                }
            ],
            "comparison": {
                "scored_variant_sha256": hash(&scored),
                "no_score_variant_sha256": hash(&no_score)
            }
        }),
    );
    let report =
        reel::production_operations::music_provenance(&manifest, &input_path, None).unwrap();
    assert!(report.comparison_verified);
    assert!(!report.rights_approval_inferred);
    assert!(!report.creative_approval_inferred);
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains(&directory.path().display().to_string())
    );

    let invalid = directory.path().join("invalid-music.json");
    let mut value: Value = serde_json::from_str(&fs::read_to_string(&input_path).unwrap()).unwrap();
    value["comparison"]["no_score_variant_sha256"] = Value::String(evidence('f'));
    write_json(&invalid, value);
    assert!(
        reel::production_operations::music_provenance(&manifest, &invalid, None)
            .unwrap_err()
            .to_string()
            .contains("both exact")
    );
}

#[test]
fn sprite_coverage_reports_unresolved_requests_without_guessing() {
    let directory = tempdir().unwrap();
    let library =
        reel::sprite_library::load_library("manifests/fixtures/sprite-library/library.yaml")
            .unwrap();
    let profile =
        reel::sprite_library::load_profile("manifests/fixtures/sprite-library/profile.yaml")
            .unwrap();
    let original_cast =
        reel::sprite_library::load_cast("manifests/fixtures/sprite-library/cast.yaml").unwrap();
    let cache_plan =
        reel::sprite_library::resolve_cast(&library, &profile, &original_cast).unwrap();
    let cache_plan_path = directory.path().join("cache-plan.json");
    reel::sprite_library::write_cache_plan(&cache_plan, &cache_plan_path).unwrap();
    let cache_coverage = reel::sprite_library::coverage_from_cache_plan(&cache_plan_path).unwrap();
    assert_eq!(cache_coverage.exact, 3);
    assert_eq!(cache_coverage.unresolved, 0);

    let cast_path = directory.path().join("cast.yaml");
    let source = fs::read_to_string("manifests/fixtures/sprite-library/cast.yaml")
        .unwrap()
        .replace("action: carry", "action: unavailable");
    fs::write(&cast_path, source).unwrap();
    let cast = reel::sprite_library::load_cast(&cast_path).unwrap();
    let report = reel::sprite_library::coverage_from_cast(&library, &profile, &cast).unwrap();
    assert_eq!(report.exact, 2);
    assert_eq!(report.unresolved, 1);
    assert!(!report.complete);
    assert!(report.characters.iter().any(|character| {
        character.requests.iter().any(|request| {
            request.coverage == reel::sprite_library::RequestCoverage::Unresolved
                && request.binding.is_none()
                && request.pose.is_none()
        })
    }));
}

#[test]
fn cli_help_exposes_all_production_operations_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    for command in [
        "generation-plan",
        "materialization-result",
        "asset-promote",
        "picture-plan",
        "review-repair-queue",
        "production-state-audit",
        "voice-take-ledger",
        "music-provenance",
        "sprite-coverage",
    ] {
        assert!(help.contains(command), "missing {command} from CLI help");
    }
}
