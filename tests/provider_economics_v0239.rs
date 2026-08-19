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

fn lifecycle(state: &str, offset_seconds: u32) -> Value {
    let submitted = format!("2026-08-19T10:00:{offset_seconds:02}Z");
    let running = format!("2026-08-19T10:00:{:02}Z", offset_seconds + 2);
    let terminal = format!("2026-08-19T10:00:{:02}Z", offset_seconds + 7);
    match state {
        "failed" => json!([
            {"state": "submitted", "observed_at_utc": submitted},
            {"state": "running", "observed_at_utc": running},
            {"state": "failed", "observed_at_utc": terminal}
        ]),
        "completed" => json!([
            {"state": "submitted", "observed_at_utc": submitted},
            {"state": "running", "observed_at_utc": running},
            {"state": "completed", "observed_at_utc": terminal}
        ]),
        _ => unreachable!(),
    }
}

fn attempt_input(
    attempt_id: &str,
    sequence: u32,
    operation: &str,
    state: &str,
    offset_seconds: u32,
) -> Value {
    let mut value = json!({
        "schema": "reel.provider-attempt-input.v0.1",
        "intent_id": "economics-s1e01-shot-01",
        "attempt_id": attempt_id,
        "attempt_sequence": sequence,
        "production_manifest_sha256": digest('a'),
        "operation_kind": operation,
        "scope": {"shot_id": "shot-01"},
        "generation_plan_sha256": digest('b'),
        "requested_policy_sha256": digest('c'),
        "resolved_configuration_sha256": digest('d'),
        "provider_identifier": "fixture-provider",
        "provider_job_id_sha256": digest('e'),
        "lifecycle_state": state,
        "lifecycle_observations": lifecycle(state, offset_seconds),
        "replay_grade": "best-effort-provider-replay"
    });
    if state == "failed" {
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

fn denomination(code: &str) -> Value {
    json!({"kind": "currency", "code": code})
}

fn reported(amount_micros: u64) -> Value {
    json!({
        "availability": "reported",
        "amount": {
            "denomination": denomination("USD"),
            "amount_micros": amount_micros
        },
        "evidence_sha256": digest('f')
    })
}

fn unavailable(availability: &str) -> Value {
    json!({"availability": availability})
}

fn economics_attempt(receipt: &Path, quote: u64, reservation: u64, realized: u64) -> Value {
    json!({
        "receipt_file": {"path": receipt, "sha256": hash(receipt)},
        "quote": reported(quote),
        "reservation": reported(reservation),
        "realized_charge": reported(realized)
    })
}

fn complete_chain(directory: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let failed = write_attempt(
        directory,
        "failed",
        &attempt_input("attempt-01", 1, "initial", "failed", 0),
    );
    let picture = directory.join("captured.png");
    RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255]))
        .save(&picture)
        .unwrap();
    let mut retry = attempt_input("attempt-02", 2, "retry", "completed", 10);
    retry["parent_receipt"] = json!({"path": failed, "sha256": hash(&failed)});
    retry["artifact"] = json!({
        "path": picture,
        "sha256": hash(&picture),
        "bytes": fs::metadata(&picture).unwrap().len(),
        "media_type": "image/png",
        "width": 2,
        "height": 2
    });
    retry["replay_grade"] = Value::String("exact-byte-reuse".to_string());
    let completed = write_attempt(directory, "completed", &retry);
    (failed, completed)
}

fn economics_input(failed: &Path, completed: &Path) -> Value {
    let mut second = economics_attempt(completed, 2_000_000, 2_200_000, 1_800_000);
    second["artifact_captured_at_utc"] = Value::String("2026-08-19T10:00:19Z".to_string());
    json!({
        "schema": "reel.provider-economics-input.v0.1",
        "report_id": "s1e01-provider-economics-01",
        "intent_id": "economics-s1e01-shot-01",
        "production_manifest_sha256": digest('a'),
        "attempts": [
            economics_attempt(failed, 1_000_000, 1_200_000, 500_000),
            second
        ],
        "budget_policy": {
            "policy_id": "s1e01-private-preview-budget",
            "denomination": denomination("USD"),
            "max_total_quote_micros": 3_000_000,
            "max_total_reservation_micros": 3_400_000,
            "max_total_realized_charge_micros": 2_300_000,
            "max_retry_attempts": 1,
            "max_total_observed_latency_ms": 16000,
            "require_realized_charges": true
        }
    })
}

#[test]
fn reconciles_complete_chain_cost_latency_retry_and_budget_without_authority() {
    let directory = tempdir().unwrap();
    let (failed, completed) = complete_chain(directory.path());
    let input_path = directory.path().join("economics-input.json");
    let output_path = directory.path().join("economics-report.json");
    write_json(&input_path, &economics_input(&failed, &completed));

    let report =
        reel::production_operations::write_provider_economics_report(&input_path, &output_path)
            .unwrap();
    assert_eq!(report.operation_counts.initial, 1);
    assert_eq!(report.operation_counts.retry, 1);
    assert_eq!(report.operation_counts.retake, 0);
    assert_eq!(report.totals.reported_quote_micros, 3_000_000);
    assert_eq!(report.totals.reported_reservation_micros, 3_400_000);
    assert_eq!(report.totals.reported_realized_charge_micros, 2_300_000);
    assert_eq!(report.totals.total_observed_latency_ms, 16_000);
    assert_eq!(report.attempts[0].latency.queue_ms, Some(2_000));
    assert_eq!(report.attempts[0].latency.execution_ms, Some(5_000));
    assert_eq!(
        report.attempts[1].latency.terminal_to_capture_ms,
        Some(2_000)
    );
    assert_eq!(
        report.budget_evaluation.overall,
        reel::production_operations::ProviderBudgetDisposition::Pass
    );
    assert!(!report.provider_executed_by_reel);
    assert!(!report.spending_authority_granted);
    assert!(report.human_authority_required);
    assert!(!report.output_selected);
    assert!(!report.creative_approved);

    let text = fs::read_to_string(&output_path).unwrap();
    assert!(!text.contains(&directory.path().display().to_string()));
    assert!(!text.contains("\"path\""));
    assert!(!text.contains("prompt"));
}

#[test]
fn missing_realized_charge_warns_and_is_never_inferred_from_quote() {
    let directory = tempdir().unwrap();
    let (failed, completed) = complete_chain(directory.path());
    let mut input = economics_input(&failed, &completed);
    input["attempts"][1]["realized_charge"] = unavailable("unavailable");
    let input_path = directory.path().join("missing-actual-input.json");
    let output_path = directory.path().join("missing-actual-report.json");
    write_json(&input_path, &input);

    let report =
        reel::production_operations::write_provider_economics_report(&input_path, &output_path)
            .unwrap();
    assert_eq!(report.totals.reported_realized_charge_micros, 500_000);
    assert!(!report.totals.all_realized_charges_reported);
    assert_eq!(
        report.budget_evaluation.realized_charge,
        reel::production_operations::ProviderBudgetDisposition::Warn
    );
    assert_eq!(
        report.budget_evaluation.overall,
        reel::production_operations::ProviderBudgetDisposition::Warn
    );
}

#[test]
fn known_partial_charge_above_limit_blocks_even_when_another_charge_is_unavailable() {
    let directory = tempdir().unwrap();
    let (failed, completed) = complete_chain(directory.path());
    let mut input = economics_input(&failed, &completed);
    input["attempts"][1]["realized_charge"] = unavailable("unavailable");
    input["budget_policy"]["max_total_realized_charge_micros"] = json!(499_999);
    let input_path = directory.path().join("partial-over-limit-input.json");
    let output_path = directory.path().join("partial-over-limit-report.json");
    write_json(&input_path, &input);

    let report =
        reel::production_operations::write_provider_economics_report(&input_path, &output_path)
            .unwrap();
    assert!(!report.totals.all_realized_charges_reported);
    assert_eq!(
        report.budget_evaluation.realized_charge,
        reel::production_operations::ProviderBudgetDisposition::Block
    );
    assert_eq!(
        report.budget_evaluation.overall,
        reel::production_operations::ProviderBudgetDisposition::Block
    );
}

#[test]
fn blocks_only_after_a_reported_total_exceeds_the_owner_limit() {
    let directory = tempdir().unwrap();
    let (failed, completed) = complete_chain(directory.path());
    let mut input = economics_input(&failed, &completed);
    input["budget_policy"]["max_total_realized_charge_micros"] = json!(2_299_999);
    let input_path = directory.path().join("over-budget-input.json");
    let output_path = directory.path().join("over-budget-report.json");
    write_json(&input_path, &input);

    let report =
        reel::production_operations::write_provider_economics_report(&input_path, &output_path)
            .unwrap();
    assert_eq!(
        report.budget_evaluation.realized_charge,
        reel::production_operations::ProviderBudgetDisposition::Block
    );
    assert_eq!(
        report.budget_evaluation.overall,
        reel::production_operations::ProviderBudgetDisposition::Block
    );
    assert!(!report.spending_authority_granted);
}

#[test]
fn rejects_denomination_mismatch_tampering_invalid_capture_and_clobbering() {
    let directory = tempdir().unwrap();
    let (failed, completed) = complete_chain(directory.path());

    let mut mismatch = economics_input(&failed, &completed);
    mismatch["attempts"][0]["quote"]["amount"]["denomination"] = denomination("EUR");
    let mismatch_path = directory.path().join("mismatch.json");
    write_json(&mismatch_path, &mismatch);
    assert!(
        reel::production_operations::write_provider_economics_report(
            &mismatch_path,
            directory.path().join("mismatch-report.json")
        )
        .unwrap_err()
        .to_string()
        .contains("denomination")
    );

    let mut tampered = economics_input(&failed, &completed);
    tampered["attempts"][0]["receipt_file"]["sha256"] = Value::String(digest('0'));
    let tampered_path = directory.path().join("tampered.json");
    write_json(&tampered_path, &tampered);
    assert!(
        reel::production_operations::write_provider_economics_report(
            &tampered_path,
            directory.path().join("tampered-report.json")
        )
        .unwrap_err()
        .to_string()
        .contains("hash mismatch")
    );

    let mut invalid_capture = economics_input(&failed, &completed);
    invalid_capture["attempts"][1]["artifact_captured_at_utc"] =
        Value::String("2026-08-19T10:00:16Z".to_string());
    let invalid_capture_path = directory.path().join("invalid-capture.json");
    write_json(&invalid_capture_path, &invalid_capture);
    assert!(
        reel::production_operations::write_provider_economics_report(
            &invalid_capture_path,
            directory.path().join("invalid-capture-report.json")
        )
        .unwrap_err()
        .to_string()
        .contains("precedes")
    );

    let valid_path = directory.path().join("valid.json");
    let output_path = directory.path().join("report.json");
    write_json(&valid_path, &economics_input(&failed, &completed));
    reel::production_operations::write_provider_economics_report(&valid_path, &output_path)
        .unwrap();
    assert!(
        reel::production_operations::write_provider_economics_report(&valid_path, &output_path)
            .unwrap_err()
            .to_string()
            .contains("overwrite")
    );
}

#[test]
fn rejects_duplicate_attempts_unknown_fields_and_amounts_for_unavailable_values() {
    let directory = tempdir().unwrap();
    let (failed, completed) = complete_chain(directory.path());

    let mut duplicate = economics_input(&failed, &completed);
    duplicate["attempts"][1] = duplicate["attempts"][0].clone();
    let duplicate_path = directory.path().join("duplicate.json");
    write_json(&duplicate_path, &duplicate);
    assert!(
        reel::production_operations::write_provider_economics_report(
            &duplicate_path,
            directory.path().join("duplicate-report.json")
        )
        .unwrap_err()
        .to_string()
        .contains("duplicate attempt")
    );

    let mut unknown = economics_input(&failed, &completed);
    unknown["provider_api_key"] = Value::String("secret".to_string());
    let unknown_path = directory.path().join("unknown.json");
    write_json(&unknown_path, &unknown);
    assert!(
        reel::production_operations::write_provider_economics_report(
            &unknown_path,
            directory.path().join("unknown-report.json")
        )
        .is_err()
    );

    let mut invalid_unavailable = economics_input(&failed, &completed);
    invalid_unavailable["attempts"][0]["realized_charge"] = json!({
        "availability": "unavailable",
        "amount": {
            "denomination": denomination("USD"),
            "amount_micros": 1
        }
    });
    let invalid_unavailable_path = directory.path().join("invalid-unavailable.json");
    write_json(&invalid_unavailable_path, &invalid_unavailable);
    assert!(
        reel::production_operations::write_provider_economics_report(
            &invalid_unavailable_path,
            directory.path().join("invalid-unavailable-report.json")
        )
        .unwrap_err()
        .to_string()
        .contains("must not contain an amount")
    );
}

#[test]
fn cli_help_exposes_provider_economics_report() {
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("provider-economics-report"));
}
