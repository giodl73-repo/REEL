use std::{fs, path::Path, process::Command};

use image::{Rgba, RgbaImage};
use serde_json::{Value, json};
use tempfile::tempdir;

fn hash(path: impl AsRef<Path>) -> String {
    reel::production::sha256_path(path).unwrap()
}

fn digest(byte: char) -> String {
    byte.to_string().repeat(64)
}

fn write_json(path: impl AsRef<Path>, value: &Value) {
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(value).unwrap()),
    )
    .unwrap();
}

fn write_picture(path: impl AsRef<Path>, rgba: [u8; 4]) {
    RgbaImage::from_pixel(3, 2, Rgba(rgba)).save(path).unwrap();
}

fn base_attempt(
    attempt_id: &str,
    attempt_sequence: u32,
    operation_kind: &str,
    lifecycle_state: &str,
) -> Value {
    let lifecycle_observations = match lifecycle_state {
        "submitted" => json!([
            {"state": "submitted", "observed_at_utc": "2026-08-19T10:00:00Z"}
        ]),
        "running" => json!([
            {"state": "submitted", "observed_at_utc": "2026-08-19T10:00:00Z"},
            {"state": "running", "observed_at_utc": "2026-08-19T10:00:05Z"}
        ]),
        "completed" => json!([
            {"state": "submitted", "observed_at_utc": "2026-08-19T10:00:00Z"},
            {"state": "running", "observed_at_utc": "2026-08-19T10:00:05Z"},
            {"state": "completed", "observed_at_utc": "2026-08-19T10:00:10Z"}
        ]),
        "failed" => json!([
            {"state": "submitted", "observed_at_utc": "2026-08-19T10:00:00Z"},
            {"state": "failed", "observed_at_utc": "2026-08-19T10:00:06Z"}
        ]),
        _ => unreachable!(),
    };
    let mut value = json!({
        "schema": "reel.provider-attempt-input.v0.1",
        "intent_id": "still-intent-s1e01-shot-01",
        "attempt_id": attempt_id,
        "attempt_sequence": attempt_sequence,
        "production_manifest_sha256": digest('a'),
        "operation_kind": operation_kind,
        "scope": {
            "shot_id": "shot-01",
            "timed_span": {"start_ms": 0, "end_ms": 1500}
        },
        "generation_plan_sha256": digest('b'),
        "requested_policy_sha256": digest('c'),
        "resolved_configuration_sha256": digest('d'),
        "provider_identifier": "fixture-provider",
        "provider_job_id_sha256": digest('e'),
        "lifecycle_state": lifecycle_state,
        "lifecycle_observations": lifecycle_observations,
        "replay_grade": "best-effort-provider-replay"
    });
    if lifecycle_state == "failed" {
        value["failure_classification"] = Value::String("provider-unavailable".to_string());
    }
    value
}

fn write_attempt(directory: &Path, name: &str, value: &Value) -> std::path::PathBuf {
    let input = directory.join(format!("{name}-input.json"));
    let receipt = directory.join(format!("{name}-receipt.json"));
    write_json(&input, value);
    reel::production_operations::write_provider_attempt_receipt(&input, &receipt).unwrap();
    receipt
}

fn resume_input(receipts: &[&Path]) -> Value {
    json!({
        "schema": "reel.provider-attempt-resume-input.v0.1",
        "intent_id": "still-intent-s1e01-shot-01",
        "production_manifest_sha256": digest('a'),
        "generation_plan_sha256": digest('b'),
        "requested_policy_sha256": digest('c'),
        "resolved_configuration_sha256": digest('d'),
        "receipt_files": receipts
            .iter()
            .map(|path| json!({"path": path, "sha256": hash(path)}))
            .collect::<Vec<_>>()
    })
}

#[test]
fn completed_capture_checks_and_resumes_exact_bytes_path_free() {
    let directory = tempdir().unwrap();
    let picture = directory.path().join("captured.png");
    write_picture(&picture, [10, 20, 30, 255]);
    let mut input = base_attempt("attempt-01", 1, "initial", "completed");
    input["artifact"] = json!({
        "path": picture,
        "sha256": hash(&picture),
        "bytes": fs::metadata(&picture).unwrap().len(),
        "media_type": "image/png",
        "width": 3,
        "height": 2
    });
    input["replay_grade"] = Value::String("exact-byte-reuse".to_string());
    let receipt_path = write_attempt(directory.path(), "completed", &input);
    let receipt_text = fs::read_to_string(&receipt_path).unwrap();
    assert!(!receipt_text.contains(&directory.path().display().to_string()));
    assert!(!receipt_text.contains("\"path\""));
    assert!(!receipt_text.contains("prompt"));
    let receipt: Value = serde_json::from_str(&receipt_text).unwrap();
    assert_eq!(receipt["provider_executed_by_reel"], false);
    assert_eq!(receipt["human_authority_required"], true);
    assert_eq!(receipt["creative_approved"], false);
    assert_eq!(receipt["rights_approved"], false);
    assert_eq!(receipt["publication_approved"], false);
    assert_eq!(receipt["release_approved"], false);

    let check_path = directory.path().join("check.json");
    let check = reel::production_operations::check_provider_attempt_receipt(
        &receipt_path,
        Some(&picture),
        Some(&check_path),
    )
    .unwrap();
    assert!(check.captured_artifact_verified);
    assert!(!check.provider_executed_by_reel);
    assert!(check.human_authority_required);

    let resume_path = directory.path().join("resume-input.json");
    let mut resume = resume_input(&[&receipt_path]);
    resume["captured_artifact"] = json!({"path": picture, "sha256": hash(&picture)});
    write_json(&resume_path, &resume);
    let plan_path = directory.path().join("resume-plan.json");
    let plan =
        reel::production_operations::plan_provider_attempt_resume(&resume_path, Some(&plan_path))
            .unwrap();
    assert_eq!(
        plan.decision,
        reel::production_operations::ProviderAttemptResumeDecision::ReuseCaptured
    );
    assert_eq!(
        plan.reason_code,
        reel::production_operations::ProviderAttemptResumeReasonCode::CapturedOutputVerified
    );
    assert!(!plan.provider_executed_by_reel);
    assert!(plan.human_authority_required);
    for output in [check_path, plan_path] {
        let text = fs::read_to_string(output).unwrap();
        assert!(!text.contains(&directory.path().display().to_string()));
        assert!(!text.contains("\"path\""));
    }
}

#[test]
fn running_polls_and_completed_pending_capture_requests_capture() {
    let directory = tempdir().unwrap();
    let running = write_attempt(
        directory.path(),
        "running",
        &base_attempt("attempt-running", 1, "initial", "running"),
    );
    let running_resume = directory.path().join("running-resume.json");
    write_json(&running_resume, &resume_input(&[&running]));
    let poll =
        reel::production_operations::plan_provider_attempt_resume(&running_resume, None).unwrap();
    assert_eq!(
        poll.decision,
        reel::production_operations::ProviderAttemptResumeDecision::PollExisting
    );

    let pending = write_attempt(
        directory.path(),
        "pending",
        &base_attempt("attempt-pending", 1, "initial", "completed"),
    );
    let pending_resume = directory.path().join("pending-resume.json");
    write_json(&pending_resume, &resume_input(&[&pending]));
    let capture =
        reel::production_operations::plan_provider_attempt_resume(&pending_resume, None).unwrap();
    assert_eq!(
        capture.decision,
        reel::production_operations::ProviderAttemptResumeDecision::CaptureOutput
    );
}

#[test]
fn captured_receipt_without_current_artifact_requires_capture_again() {
    let directory = tempdir().unwrap();
    let picture = directory.path().join("captured.png");
    write_picture(&picture, [8, 9, 10, 255]);
    let mut input = base_attempt("attempt-01", 1, "initial", "completed");
    input["artifact"] = json!({
        "path": picture,
        "sha256": hash(&picture),
        "bytes": fs::metadata(&picture).unwrap().len(),
        "media_type": "image/png",
        "width": 3,
        "height": 2
    });
    input["replay_grade"] = Value::String("exact-byte-reuse".to_string());
    let receipt = write_attempt(directory.path(), "captured-missing", &input);
    let resume_path = directory.path().join("captured-missing-resume.json");
    write_json(&resume_path, &resume_input(&[&receipt]));
    let plan =
        reel::production_operations::plan_provider_attempt_resume(&resume_path, None).unwrap();
    assert_eq!(
        plan.decision,
        reel::production_operations::ProviderAttemptResumeDecision::CaptureOutput
    );
    assert_eq!(
        plan.reason_code,
        reel::production_operations::ProviderAttemptResumeReasonCode::CapturedArtifactNotAvailable
    );

    write_picture(&picture, [11, 12, 13, 255]);
    let mut tampered = resume_input(&[&receipt]);
    tampered["captured_artifact"] = json!({"path": picture, "sha256": hash(&picture)});
    write_json(&resume_path, &tampered);
    assert!(
        reel::production_operations::plan_provider_attempt_resume(&resume_path, None)
            .unwrap_err()
            .to_string()
            .contains("does not match latest receipt")
    );
}

#[test]
fn failed_retry_chain_is_verified_before_retry_terminal_decision() {
    let directory = tempdir().unwrap();
    let failed = write_attempt(
        directory.path(),
        "failed-initial",
        &base_attempt("attempt-01", 1, "initial", "failed"),
    );
    let mut retry = base_attempt("attempt-02", 2, "retry", "failed");
    retry["failure_classification"] = Value::String("timeout".to_string());
    retry["parent_receipt"] = json!({"path": failed, "sha256": hash(&failed)});
    let retry_receipt = write_attempt(directory.path(), "failed-retry", &retry);
    let resume = directory.path().join("retry-resume.json");
    write_json(&resume, &resume_input(&[&failed, &retry_receipt]));
    let plan = reel::production_operations::plan_provider_attempt_resume(&resume, None).unwrap();
    assert_eq!(plan.latest_attempt_sequence, 2);
    assert_eq!(
        plan.decision,
        reel::production_operations::ProviderAttemptResumeDecision::RetryTerminal
    );
}

#[test]
fn stale_manifest_blocks_after_the_complete_chain_is_validated() {
    let directory = tempdir().unwrap();
    let running = write_attempt(
        directory.path(),
        "stale",
        &base_attempt("attempt-01", 1, "initial", "running"),
    );
    let mut input = resume_input(&[&running]);
    input["production_manifest_sha256"] = Value::String(digest('f'));
    let resume = directory.path().join("stale-resume.json");
    write_json(&resume, &input);
    let plan = reel::production_operations::plan_provider_attempt_resume(&resume, None).unwrap();
    assert_eq!(
        plan.decision,
        reel::production_operations::ProviderAttemptResumeDecision::BlockedStaleInput
    );
    assert_eq!(
        plan.reason_code,
        reel::production_operations::ProviderAttemptResumeReasonCode::StaleProductionManifest
    );
}

#[test]
fn checker_rejects_missing_extra_tampered_artifacts_and_authority() {
    let directory = tempdir().unwrap();
    let picture = directory.path().join("captured.png");
    write_picture(&picture, [1, 2, 3, 255]);
    let mut input = base_attempt("attempt-01", 1, "initial", "completed");
    input["artifact"] = json!({
        "path": picture,
        "sha256": hash(&picture),
        "bytes": fs::metadata(&picture).unwrap().len(),
        "media_type": "image/png",
        "width": 3,
        "height": 2
    });
    input["replay_grade"] = Value::String("exact-byte-reuse".to_string());
    let captured = write_attempt(directory.path(), "captured", &input);
    assert!(
        reel::production_operations::check_provider_attempt_receipt(&captured, None, None)
            .unwrap_err()
            .to_string()
            .contains("requires an artifact")
    );
    let captured_value: Value =
        serde_json::from_str(&fs::read_to_string(&captured).unwrap()).unwrap();
    let mut wrong_hash = captured_value.clone();
    wrong_hash["captured_image"]["sha256"] = Value::String(digest('f'));
    let wrong_hash_path = directory.path().join("wrong-hash-receipt.json");
    write_json(&wrong_hash_path, &wrong_hash);
    assert!(
        reel::production_operations::check_provider_attempt_receipt(
            &wrong_hash_path,
            Some(&picture),
            None
        )
        .unwrap_err()
        .to_string()
        .contains("hash mismatch")
    );
    let mut wrong_dimensions = captured_value.clone();
    wrong_dimensions["captured_image"]["width"] = Value::Number(4.into());
    let wrong_dimensions_path = directory.path().join("wrong-dimensions-receipt.json");
    write_json(&wrong_dimensions_path, &wrong_dimensions);
    assert!(
        reel::production_operations::check_provider_attempt_receipt(
            &wrong_dimensions_path,
            Some(&picture),
            None
        )
        .unwrap_err()
        .to_string()
        .contains("dimensions mismatch")
    );
    let mut wrong_media = captured_value;
    wrong_media["captured_image"]["media_type"] = Value::String("image/jpeg".to_string());
    let wrong_media_path = directory.path().join("wrong-media-receipt.json");
    write_json(&wrong_media_path, &wrong_media);
    assert!(
        reel::production_operations::check_provider_attempt_receipt(
            &wrong_media_path,
            Some(&picture),
            None
        )
        .unwrap_err()
        .to_string()
        .contains("expected image/png")
    );
    write_picture(&picture, [4, 5, 6, 255]);
    assert!(
        reel::production_operations::check_provider_attempt_receipt(
            &captured,
            Some(&picture),
            None
        )
        .unwrap_err()
        .to_string()
        .contains("hash mismatch")
    );

    let running = write_attempt(
        directory.path(),
        "running-extra",
        &base_attempt("attempt-running", 1, "initial", "running"),
    );
    assert!(
        reel::production_operations::check_provider_attempt_receipt(&running, Some(&picture), None)
            .unwrap_err()
            .to_string()
            .contains("must not")
    );

    let mut tampered: Value = serde_json::from_str(&fs::read_to_string(&running).unwrap()).unwrap();
    tampered["creative_approved"] = Value::Bool(true);
    let tampered_path = directory.path().join("tampered-authority.json");
    write_json(&tampered_path, &tampered);
    assert!(
        reel::production_operations::check_provider_attempt_receipt(&tampered_path, None, None)
            .unwrap_err()
            .to_string()
            .contains("authority boundary")
    );
}

#[test]
fn strict_inputs_reject_unknown_fields_and_invalid_state_evidence() {
    let directory = tempdir().unwrap();
    let mut unknown = base_attempt("attempt-unknown", 1, "initial", "running");
    unknown["provider_url"] = Value::String("https://private.example/job".to_string());
    let unknown_input = directory.path().join("unknown-input.json");
    write_json(&unknown_input, &unknown);
    assert!(
        reel::production_operations::write_provider_attempt_receipt(
            &unknown_input,
            directory.path().join("unknown-receipt.json")
        )
        .is_err()
    );

    let picture = directory.path().join("invalid.png");
    write_picture(&picture, [1, 1, 1, 255]);
    let mut running_artifact = base_attempt("attempt-running", 1, "initial", "running");
    running_artifact["artifact"] = json!({
        "path": picture,
        "sha256": hash(&picture),
        "bytes": fs::metadata(&picture).unwrap().len(),
        "media_type": "image/png",
        "width": 3,
        "height": 2
    });
    let running_input = directory.path().join("running-artifact.json");
    write_json(&running_input, &running_artifact);
    assert!(
        reel::production_operations::write_provider_attempt_receipt(
            &running_input,
            directory.path().join("running-artifact-receipt.json")
        )
        .unwrap_err()
        .to_string()
        .contains("non-terminal")
    );

    let mut failed = base_attempt("attempt-failed", 1, "initial", "failed");
    failed
        .as_object_mut()
        .unwrap()
        .remove("failure_classification");
    let failed_input = directory.path().join("failed-without-classification.json");
    write_json(&failed_input, &failed);
    assert!(
        reel::production_operations::write_provider_attempt_receipt(
            &failed_input,
            directory
                .path()
                .join("failed-without-classification-receipt.json")
        )
        .unwrap_err()
        .to_string()
        .contains("requires a normalized failure")
    );

    let receipt = write_attempt(
        directory.path(),
        "strict-receipt",
        &base_attempt("attempt-strict", 1, "initial", "running"),
    );
    let mut receipt_value: Value =
        serde_json::from_str(&fs::read_to_string(&receipt).unwrap()).unwrap();
    receipt_value["raw_provider_payload"] = json!({"secret": true});
    let unknown_receipt = directory.path().join("unknown-receipt-field.json");
    write_json(&unknown_receipt, &receipt_value);
    assert!(
        reel::production_operations::check_provider_attempt_receipt(&unknown_receipt, None, None)
            .is_err()
    );
}

#[test]
fn resume_rejects_tampered_receipts_broken_lineage_and_duplicates() {
    let directory = tempdir().unwrap();
    let failed = write_attempt(
        directory.path(),
        "chain-parent",
        &base_attempt("attempt-01", 1, "initial", "failed"),
    );
    let mut retry = base_attempt("attempt-02", 2, "retry", "running");
    retry["parent_receipt"] = json!({"path": failed, "sha256": hash(&failed)});
    let retry_receipt = write_attempt(directory.path(), "chain-child", &retry);

    let mut broken: Value =
        serde_json::from_str(&fs::read_to_string(&retry_receipt).unwrap()).unwrap();
    broken["parent_receipt_sha256"] = Value::String(digest('f'));
    let broken_path = directory.path().join("broken-child.json");
    write_json(&broken_path, &broken);
    let broken_input = directory.path().join("broken-resume.json");
    write_json(&broken_input, &resume_input(&[&failed, &broken_path]));
    assert!(
        reel::production_operations::plan_provider_attempt_resume(&broken_input, None)
            .unwrap_err()
            .to_string()
            .contains("broken parent lineage")
    );

    let duplicate_identity = directory.path().join("duplicate-identity.json");
    write_json(&duplicate_identity, &resume_input(&[&failed, &failed]));
    assert!(
        reel::production_operations::plan_provider_attempt_resume(&duplicate_identity, None)
            .unwrap_err()
            .to_string()
            .contains("duplicate attempt identity")
    );

    let mut duplicate_sequence: Value =
        serde_json::from_str(&fs::read_to_string(&failed).unwrap()).unwrap();
    duplicate_sequence["attempt_id"] = Value::String("attempt-other".to_string());
    let duplicate_sequence_path = directory.path().join("duplicate-sequence-receipt.json");
    write_json(&duplicate_sequence_path, &duplicate_sequence);
    let duplicate_sequence_input = directory.path().join("duplicate-sequence.json");
    write_json(
        &duplicate_sequence_input,
        &resume_input(&[&failed, &duplicate_sequence_path]),
    );
    assert!(
        reel::production_operations::plan_provider_attempt_resume(&duplicate_sequence_input, None)
            .unwrap_err()
            .to_string()
            .contains("duplicate attempt sequence")
    );

    let tampered_binding = directory.path().join("tampered-binding.json");
    let mut tampered_input = resume_input(&[&retry_receipt]);
    tampered_input["receipt_files"][0]["sha256"] = Value::String(digest('f'));
    write_json(&tampered_binding, &tampered_input);
    assert!(
        reel::production_operations::plan_provider_attempt_resume(&tampered_binding, None)
            .unwrap_err()
            .to_string()
            .contains("hash mismatch")
    );
}

#[test]
fn cli_help_exposes_provider_attempt_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    for command in [
        "provider-attempt-receipt",
        "provider-attempt-check",
        "provider-attempt-resume",
    ] {
        assert!(help.contains(command), "missing {command} from CLI help");
    }
}
