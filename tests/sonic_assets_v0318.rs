use std::{fs, path::Path};

use reel::sonic_assets;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn wav(samples: u32, channels: u16) -> Vec<u8> {
    let data_bytes = samples * u32::from(channels) * 3;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&48_000u32.to_le_bytes());
    bytes.extend_from_slice(&(48_000 * u32::from(channels) * 3).to_le_bytes());
    bytes.extend_from_slice(&(channels * 3).to_le_bytes());
    bytes.extend_from_slice(&24u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    bytes.resize(bytes.len() + data_bytes as usize, 0);
    bytes
}

fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn fixture(root: &Path, state: &str, fixture: bool) -> (Value, Value, Vec<u8>) {
    let audio = wav(48_000, 1);
    fs::write(root.join("motor.wav"), &audio).unwrap();
    let catalog = json!({
        "schema": "reel.sonic-asset-catalog.v0.1",
        "library_id": "synthetic-sfx",
        "library_version": "1",
        "assets": [{
            "asset_id": "SFX-MOTOR-01",
            "authority_state": state,
            "authority_receipt_sha256": sha(b"authority"),
            "license": {
                "license_id": "synthetic-test",
                "review_status": "test-only",
                "permits_production_use": true
            },
            "lineage_sha256": [sha(b"generator")],
            "variants": [{
                "variant_id": "approach",
                "locator": "motor.wav",
                "sha256": sha(&audio),
                "bytes": audio.len(),
                "geometry": {
                    "sample_rate_hz": 48000,
                    "bit_depth": 24,
                    "channels": 1,
                    "sample_count": 48000
                },
                "sync_markers": {"arrival": 40000}
            }]
        }],
        "pools": []
    });
    let request = json!({
        "schema": "reel.sonic-asset-request.v0.1",
        "request_id": "request-01",
        "consumer_manifest_sha256": sha(b"manifest"),
        "engineering_fixture": fixture,
        "bindings": [{
            "event_id": "motor-arrives",
            "selection": "exact",
            "asset_id": "SFX-MOTOR-01",
            "variant_id": "approach",
            "required_geometry": {"sample_rate_hz": 48000, "bit_depth": 24, "channels": 1, "sample_count": 48000},
            "required_sync_markers": ["arrival"]
        }]
    });
    (catalog, request, audio)
}

#[test]
fn resolves_checks_and_materializes_exact_selected_audio_without_path_leakage() {
    let temp = tempdir().unwrap();
    let manifest = b"manifest_version: reel.manifest.v0.2\nprofile: short-form\ntiming_status: ready\nwork: synthetic\ntitle: Synthetic\nformat: test\nduration_seconds: 1.0\naudio_events:\n  - id: motor-arrives\n    role: effect\n    source: unresolved.wav\n    start_seconds: 0.0\n    duration_seconds: 1.0\n";
    let (catalog, mut request, _) = fixture(temp.path(), "selected-private-production", false);
    request["consumer_manifest_sha256"] = json!(sha(manifest));
    let catalog_path = temp.path().join("catalog.json");
    let request_path = temp.path().join("request.json");
    let manifest_path = temp.path().join("manifest.yaml");
    write_json(&catalog_path, &catalog);
    write_json(&request_path, &request);
    fs::write(&manifest_path, manifest).unwrap();
    let loaded_catalog = sonic_assets::load_catalog(&catalog_path).unwrap();
    let loaded_request = sonic_assets::load_request(&request_path).unwrap();
    let (resolution, receipt) = sonic_assets::resolve(&loaded_catalog, &loaded_request).unwrap();
    assert!(!resolution.shareable);
    assert!(resolution.selections[0].resolved_path.contains("motor.wav"));
    let receipt_text = serde_json::to_string(&receipt).unwrap();
    assert!(receipt.path_free);
    assert!(!receipt_text.contains("motor.wav"));
    assert!(!receipt_text.contains(&temp.path().display().to_string()));
    assert!(!receipt.selects_creative_output && !receipt.grants_approval);
    let resolution_path = temp.path().join("resolution.json");
    let receipt_path = temp.path().join("receipt.json");
    sonic_assets::write_resolution_packet(&resolution, &receipt, &resolution_path, &receipt_path)
        .unwrap();
    let checked = sonic_assets::check(
        &catalog_path,
        &request_path,
        &resolution_path,
        &receipt_path,
    )
    .unwrap();
    assert!(checked.passed);
    let output_manifest = temp.path().join("materialized.yaml");
    let output_receipt = temp.path().join("materialized.receipt.json");
    let result = sonic_assets::materialize_manifest(
        &catalog_path,
        &request_path,
        &manifest_path,
        &resolution_path,
        &receipt_path,
        &output_manifest,
        &output_receipt,
    )
    .unwrap();
    assert_eq!(result.bound_events, 1);
    assert!(
        fs::read_to_string(output_manifest)
            .unwrap()
            .contains("motor.wav")
    );
    assert!(
        !fs::read_to_string(output_receipt)
            .unwrap()
            .contains("motor.wav")
    );
}

#[test]
fn rejects_diagnostic_unselected_and_wrong_geometry_assets() {
    let temp = tempdir().unwrap();
    let (catalog, request, _) = fixture(temp.path(), "diagnostic-placeholder", false);
    let catalog_path = temp.path().join("catalog.json");
    let request_path = temp.path().join("request.json");
    write_json(&catalog_path, &catalog);
    write_json(&request_path, &request);
    let error = sonic_assets::resolve(
        &sonic_assets::load_catalog(&catalog_path).unwrap(),
        &sonic_assets::load_request(&request_path).unwrap(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("not eligible"));

    let (mut catalog, _request, _) = fixture(temp.path(), "selected-private-production", false);
    catalog["assets"][0]["variants"][0]["geometry"]["sample_rate_hz"] = json!(44100);
    write_json(&temp.path().join("bad-catalog.json"), &catalog);
    let error = sonic_assets::resolve(
        &sonic_assets::load_catalog(temp.path().join("bad-catalog.json")).unwrap(),
        &sonic_assets::load_request(&request_path).unwrap(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("wrong required geometry"));
}

#[test]
fn detects_source_and_packet_tampering_and_refuses_overwrite() {
    let temp = tempdir().unwrap();
    let (catalog, request, _) = fixture(temp.path(), "fixture-only", true);
    let catalog_path = temp.path().join("catalog.json");
    let request_path = temp.path().join("request.json");
    write_json(&catalog_path, &catalog);
    write_json(&request_path, &request);
    let loaded_catalog = sonic_assets::load_catalog(&catalog_path).unwrap();
    let loaded_request = sonic_assets::load_request(&request_path).unwrap();
    let (resolution, receipt) = sonic_assets::resolve(&loaded_catalog, &loaded_request).unwrap();
    let resolution_path = temp.path().join("resolution.json");
    let receipt_path = temp.path().join("receipt.json");
    sonic_assets::write_resolution_packet(&resolution, &receipt, &resolution_path, &receipt_path)
        .unwrap();
    assert!(
        sonic_assets::write_resolution_packet(
            &resolution,
            &receipt,
            &resolution_path,
            &receipt_path
        )
        .unwrap_err()
        .to_string()
        .contains("overwrite")
    );
    let original_resolution = fs::read(&resolution_path).unwrap();
    fs::write(
        &resolution_path,
        [original_resolution.as_slice(), b" \n"].concat(),
    )
    .unwrap();
    assert!(
        sonic_assets::check(
            &catalog_path,
            &request_path,
            &resolution_path,
            &receipt_path
        )
        .is_err()
    );
    fs::write(&resolution_path, original_resolution).unwrap();
    fs::write(temp.path().join("motor.wav"), wav(47_999, 1)).unwrap();
    assert!(
        sonic_assets::check(
            &catalog_path,
            &request_path,
            &resolution_path,
            &receipt_path
        )
        .is_err()
    );
}

#[test]
fn approved_pool_choice_is_deterministic_and_pool_version_is_bound() {
    let temp = tempdir().unwrap();
    let (mut catalog, mut request, audio) = fixture(temp.path(), "approved-pool", false);
    let mut second = catalog["assets"][0].clone();
    second["asset_id"] = json!("SFX-MOTOR-02");
    second["variants"][0]["variant_id"] = json!("departure");
    second["variants"][0]["locator"] = json!("motor-2.wav");
    fs::write(temp.path().join("motor-2.wav"), &audio).unwrap();
    catalog["assets"].as_array_mut().unwrap().push(second);
    catalog["pools"] = json!([{
        "pool_id": "motor-pool",
        "pool_version": "3",
        "members": [
            {"asset_id": "SFX-MOTOR-01", "variant_id": "approach"},
            {"asset_id": "SFX-MOTOR-02", "variant_id": "departure"}
        ]
    }]);
    request["bindings"][0] = json!({
        "event_id": "motor-arrives",
        "selection": "approved-pool",
        "pool_id": "motor-pool",
        "pool_version": "3",
        "selection_key": "scene-01-low-salience"
    });
    let catalog_path = temp.path().join("catalog.json");
    let request_path = temp.path().join("request.json");
    write_json(&catalog_path, &catalog);
    write_json(&request_path, &request);
    let catalog = sonic_assets::load_catalog(&catalog_path).unwrap();
    let request = sonic_assets::load_request(&request_path).unwrap();
    let first = sonic_assets::resolve(&catalog, &request).unwrap().0;
    let second = sonic_assets::resolve(&catalog, &request).unwrap().0;
    assert_eq!(first.selections[0].asset_id, second.selections[0].asset_id);
    assert_eq!(
        first.selections[0].variant_id,
        second.selections[0].variant_id
    );
}

#[test]
#[ignore = "requires FFmpeg; exercised explicitly in CI"]
fn real_resolved_sonic_asset_renders_dme_stems_and_rechecks() {
    let temp = tempdir().unwrap();
    let manifest = b"manifest_version: reel.manifest.v0.2\nprofile: animatic\ntiming_status: conformed\nwork: synthetic-sonic-e2e\ntitle: Synthetic Sonic E2E\nformat: test\nstyle: synthetic\nduration_seconds: 1.0\nscenes:\n  - id: scene-1\n    duration_seconds: 1.0\n    purpose: Synthetic sonic resolver test.\n    story_beat: A generated tone is routed as an effect.\n    location: synthetic\nshots:\n  - id: shot-1\n    scene_id: scene-1\n    start_seconds: 0.0\n    duration_seconds: 1.0\n    camera: static\n    action: Synthetic test action.\n    visual_prompt: Synthetic blank frame.\n    visual_asset: unused.png\n    motion: static\n    transition_out: cut\n    source_refs: [fixture-source]\nsource_ranges:\n  - { id: fixture-source, start: 1, end: 1, label: synthetic }\naudio_events:\n  - id: motor-arrives\n    role: effect\n    source: unresolved.wav\n    start_seconds: 0.0\n    duration_seconds: 1.0\n";
    let (catalog, mut request, _) = fixture(temp.path(), "fixture-only", true);
    request["consumer_manifest_sha256"] = json!(sha(manifest));
    let catalog_path = temp.path().join("catalog.json");
    let request_path = temp.path().join("request.json");
    let manifest_path = temp.path().join("manifest.yaml");
    write_json(&catalog_path, &catalog);
    write_json(&request_path, &request);
    fs::write(&manifest_path, manifest).unwrap();
    let (resolution, receipt) = sonic_assets::resolve(
        &sonic_assets::load_catalog(&catalog_path).unwrap(),
        &sonic_assets::load_request(&request_path).unwrap(),
    )
    .unwrap();
    let resolution_path = temp.path().join("resolution.json");
    let receipt_path = temp.path().join("resolution.receipt.json");
    sonic_assets::write_resolution_packet(&resolution, &receipt, &resolution_path, &receipt_path)
        .unwrap();
    let materialized = temp.path().join("materialized.yaml");
    sonic_assets::materialize_manifest(
        &catalog_path,
        &request_path,
        &manifest_path,
        &resolution_path,
        &receipt_path,
        &materialized,
        temp.path().join("materialized.receipt.json"),
    )
    .unwrap();
    let output = temp.path().join("review.m4a");
    let stems = temp.path().join("stems");
    let report =
        reel::audio_preview::render_audio_preview(&reel::audio_preview::AudioPreviewOptions {
            manifest: materialized,
            asset_root: temp.path().to_path_buf(),
            output: output.clone(),
            dry_run: false,
            stems_dir: Some(stems.clone()),
            sample_rate_hz: 48_000,
            channels: 2,
        })
        .unwrap();
    assert_eq!(report.output_duration_ms, Some(1_000));
    assert!(stems.join("dialogue.pre-master.wav").exists());
    assert!(stems.join("music.pre-master.wav").exists());
    assert!(stems.join("effects.pre-master.wav").exists());
    assert!(stems.join("mix.mastered.wav").exists());
    let checked =
        reel::audio_preview::check_audio_preview(output.with_extension("audio-artifacts.json"))
            .unwrap();
    assert!(checked.verified);
}
