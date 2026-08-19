use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(feature = "test-fixtures")]
use std::sync::{Mutex, OnceLock};

use ed25519_dalek::SigningKey;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::{TempDir, tempdir};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn file_sha(path: &Path) -> String {
    sha256_hex(&fs::read(path).unwrap())
}

fn digest(character: char) -> String {
    character.to_string().repeat(64)
}

fn write_json(path: &Path, value: &Value) {
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(value).unwrap()),
    )
    .unwrap();
}

struct Approver {
    seed: [u8; 32],
    public_key: String,
    key_id: String,
}

fn make_approver(seed_byte: u8) -> Approver {
    let seed = [seed_byte; 32];
    let signing_key = SigningKey::from_bytes(&seed);
    let public_key_bytes = signing_key.verifying_key().to_bytes();
    Approver {
        seed,
        public_key: hex(&public_key_bytes),
        key_id: sha256_hex(&public_key_bytes),
    }
}

fn entry(
    approver: &Approver,
    approver_id: &str,
    role: &str,
    scopes: &[&str],
    decisions: &[&str],
) -> Value {
    json!({
        "approver_id": approver_id,
        "role": role,
        "public_key": approver.public_key,
        "key_id": approver.key_id,
        "scopes": scopes,
        "decisions": decisions,
    })
}

struct Fixture {
    dir: TempDir,
    registry_path: PathBuf,
    registry_sha: String,
    target_path: PathBuf,
    target_sha: String,
    key_path: PathBuf,
    approver: Approver,
}

impl Fixture {
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}

fn fixture() -> Fixture {
    let dir = tempdir().unwrap();
    let approver = make_approver(7);
    let key_path = dir.path().join("signing.key");
    fs::write(&key_path, approver.seed).unwrap();
    let registry = json!({
        "schema": "reel.approval-authority-registry.v0.1",
        "owner_id": "studio-owner",
        "registry_id": "authority-registry-01",
        "entries": [entry(
            &approver,
            "director-01",
            "final-approver",
            &["picture-lock", "audio-lock"],
            &["approved", "rejected", "revoked"],
        )],
    });
    let registry_path = dir.path().join("registry.json");
    write_json(&registry_path, &registry);
    let registry_sha = file_sha(&registry_path);
    let target_path = dir.path().join("target.bin");
    fs::write(&target_path, b"the-approved-cut-bytes").unwrap();
    let target_sha = file_sha(&target_path);
    Fixture {
        dir,
        registry_path,
        registry_sha,
        target_path,
        target_sha,
        key_path,
        approver,
    }
}

#[allow(clippy::too_many_arguments)]
fn auth_input(
    fx: &Fixture,
    decision: &str,
    scope: &str,
    sequence: u64,
    issued: &str,
    expires: &str,
    prior: Option<(&Path, &str)>,
) -> Value {
    let mut value = json!({
        "schema": "reel.approval-authorization-input.v0.1",
        "owner_id": "studio-owner",
        "registry": {"path": fx.registry_path, "sha256": fx.registry_sha},
        "signing_key_path": fx.key_path,
        "authority_context_sha256": digest('c'),
        "approver_id": "director-01",
        "role": "final-approver",
        "scope": scope,
        "decision": decision,
        "target_kind": "video",
        "target": {"path": fx.target_path, "sha256": fx.target_sha},
        "policy_sha256": digest('b'),
        "sequence": sequence,
        "issued_at_utc": issued,
        "expires_at_utc": expires,
    });
    if let Some((path, hash)) = prior {
        value["prior_attestation"] = json!({"path": path, "sha256": hash});
    }
    value
}

fn sign(fx: &Fixture, input: &Value, name: &str) -> PathBuf {
    let input_path = fx.path(&format!("{name}-input.json"));
    write_json(&input_path, input);
    let output_path = fx.path(&format!("{name}.json"));
    reel::approval_attestation::sign_attestation(&input_path, &output_path).unwrap();
    output_path
}

fn sign_result(fx: &Fixture, input: &Value, name: &str) -> anyhow::Result<()> {
    let input_path = fx.path(&format!("{name}-input.json"));
    write_json(&input_path, input);
    let output_path = fx.path(&format!("{name}.json"));
    reel::approval_attestation::sign_attestation(&input_path, &output_path).map(|_| ())
}

fn verify_input(
    fx: &Fixture,
    expected_registry_sha: &str,
    target_sha: &str,
    verification_time: &str,
    chain: &[(&Path, &str)],
) -> Value {
    json!({
        "schema": "reel.approval-verification-input.v0.1",
        "expected_registry_sha256": expected_registry_sha,
        "registry": {"path": fx.registry_path, "sha256": fx.registry_sha},
        "target": {"path": fx.target_path, "sha256": target_sha},
        "verification_time_utc": verification_time,
        "chain": chain
            .iter()
            .map(|(path, hash)| json!({"path": path, "sha256": hash}))
            .collect::<Vec<_>>(),
    })
}

fn verify(
    fx: &Fixture,
    input: &Value,
    name: &str,
) -> anyhow::Result<reel::approval_attestation::ApprovalVerificationReport> {
    let input_path = fx.path(&format!("{name}-verify-input.json"));
    write_json(&input_path, input);
    let output_path = fx.path(&format!("{name}-report.json"));
    reel::approval_attestation::verify_attestation(&input_path, Some(&output_path))
}

#[test]
fn signs_and_verifies_owner_issued_approval() {
    let fx = fixture();
    let input = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-20T10:00:00Z",
        None,
    );
    let attestation_path = sign(&fx, &input, "approved");
    let attestation_hash = file_sha(&attestation_path);
    let text = fs::read_to_string(&attestation_path).unwrap();
    assert!(!text.contains(&fx.dir.path().display().to_string()));
    assert!(!text.contains("\"path\""));
    assert!(
        !text.contains(&hex(&fx.approver.seed)),
        "private key leaked into attestation"
    );
    assert!(text.contains(&fx.approver.public_key));

    let verification = verify_input(
        &fx,
        &fx.registry_sha,
        &fx.target_sha,
        "2026-08-19T12:00:00Z",
        &[(&attestation_path, &attestation_hash)],
    );
    let report = verify(&fx, &verification, "approved").unwrap();
    assert_eq!(report.current_decision, "approved");
    assert_eq!(report.signed_human_decision, "approved");
    assert!(report.cryptographic_signature_valid);
    assert!(report.registry_authorized);
    assert!(report.target_integrity_verified);
    assert!(report.time_valid_at_verification);
    assert!(report.authenticated_from_origin);
    assert_eq!(
        report.current_status_basis,
        "full-hash-pinned-origin-chain-head"
    );
    assert!(!report.implies_rights && !report.implies_publication && !report.implies_release);
    assert!(report.human_authority_required);

    let report_text = fs::read_to_string(fx.path("approved-report.json")).unwrap();
    assert!(!report_text.contains(&fx.dir.path().display().to_string()));
    assert!(!report_text.contains("\"path\""));
    assert!(!report_text.contains(&hex(&fx.approver.seed)));
}

#[test]
fn signs_and_verifies_rejection() {
    let fx = fixture();
    let input = auth_input(
        &fx,
        "rejected",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    let path = sign(&fx, &input, "rejected");
    let hash = file_sha(&path);
    let verification = verify_input(
        &fx,
        &fx.registry_sha,
        &fx.target_sha,
        "2026-08-19T12:00:00Z",
        &[(&path, &hash)],
    );
    let report = verify(&fx, &verification, "rejected").unwrap();
    assert_eq!(report.current_decision, "rejected");
}

#[test]
fn verifies_full_revocation_chain() {
    let fx = fixture();
    let approved = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    let approved_path = sign(&fx, &approved, "chain-approved");
    let approved_hash = file_sha(&approved_path);
    let revoked = auth_input(
        &fx,
        "revoked",
        "picture-lock",
        2,
        "2026-08-19T11:00:00Z",
        "2026-08-25T10:00:00Z",
        Some((&approved_path, &approved_hash)),
    );
    let revoked_path = sign(&fx, &revoked, "chain-revoked");
    let revoked_hash = file_sha(&revoked_path);

    let verification = verify_input(
        &fx,
        &fx.registry_sha,
        &fx.target_sha,
        "2026-08-19T12:00:00Z",
        &[
            (&approved_path, &approved_hash),
            (&revoked_path, &revoked_hash),
        ],
    );
    let report = verify(&fx, &verification, "chain").unwrap();
    assert_eq!(report.current_decision, "revoked");
    assert_eq!(report.historical_decisions, vec!["approved", "revoked"]);
    assert_eq!(report.chain_length, 2);
    assert!(report.authenticated_from_origin);
}

#[test]
fn verification_requires_a_complete_sequence_one_lineage() {
    let fx = fixture();
    let approved = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    let approved_path = sign(&fx, &approved, "complete-approved");
    let approved_hash = file_sha(&approved_path);
    let revoked = auth_input(
        &fx,
        "revoked",
        "picture-lock",
        2,
        "2026-08-19T11:00:00Z",
        "2026-08-25T10:00:00Z",
        Some((&approved_path, &approved_hash)),
    );
    let revoked_path = sign(&fx, &revoked, "complete-revoked");
    let revoked_hash = file_sha(&revoked_path);

    let partial = verify_input(
        &fx,
        &fx.registry_sha,
        &fx.target_sha,
        "2026-08-19T12:00:00Z",
        &[(&revoked_path, &revoked_hash)],
    );
    assert!(
        verify(&fx, &partial, "partial-lineage")
            .unwrap_err()
            .to_string()
            .contains("sequence 1 origin")
    );
}

#[test]
fn revocation_requires_a_prior_attestation() {
    let fx = fixture();
    let input = auth_input(
        &fx,
        "revoked",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    let error = sign_result(&fx, &input, "orphan-revoke")
        .unwrap_err()
        .to_string();
    assert!(error.contains("revocation requires an exact validated prior attestation"));
}

#[test]
fn signing_rejects_origin_and_child_sequence_gaps() {
    let fx = fixture();
    let origin_gap = auth_input(
        &fx,
        "approved",
        "picture-lock",
        2,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    assert!(
        sign_result(&fx, &origin_gap, "origin-gap")
            .unwrap_err()
            .to_string()
            .contains("origin attestation must use sequence 1")
    );

    let origin = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    let origin_path = sign(&fx, &origin, "sequence-origin");
    let origin_hash = file_sha(&origin_path);
    let child_gap = auth_input(
        &fx,
        "revoked",
        "picture-lock",
        3,
        "2026-08-19T11:00:00Z",
        "2026-08-25T10:00:00Z",
        Some((&origin_path, &origin_hash)),
    );
    assert!(
        sign_result(&fx, &child_gap, "child-gap")
            .unwrap_err()
            .to_string()
            .contains("child attestation sequence must equal prior sequence plus one")
    );
}

#[test]
fn rejects_tampered_target_bytes() {
    let fx = fixture();
    let input = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    let path = sign(&fx, &input, "tt");
    let hash = file_sha(&path);
    fs::write(&fx.target_path, b"a-different-cut").unwrap();
    let tampered_sha = file_sha(&fx.target_path);
    let verification = verify_input(
        &fx,
        &fx.registry_sha,
        &tampered_sha,
        "2026-08-19T12:00:00Z",
        &[(&path, &hash)],
    );
    let error = verify(&fx, &verification, "tt").unwrap_err().to_string();
    assert!(error.contains("does not bind the current target bytes"));
}

#[test]
fn rejects_stale_pinned_target_digest() {
    let fx = fixture();
    let input = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    let path = sign(&fx, &input, "stale-target");
    let hash = file_sha(&path);
    fs::write(&fx.target_path, b"a-different-cut").unwrap();
    let verification = verify_input(
        &fx,
        &fx.registry_sha,
        &fx.target_sha,
        "2026-08-19T12:00:00Z",
        &[(&path, &hash)],
    );
    let error = verify(&fx, &verification, "stale-target")
        .unwrap_err()
        .to_string();
    assert!(error.contains("does not match the pinned digest"));
}

#[test]
fn rejects_tampered_payload_and_signature() {
    let fx = fixture();
    let input = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    let path = sign(&fx, &input, "tamper");
    let original = fs::read_to_string(&path).unwrap();

    let payload = original.replace("\"approved\"", "\"rejected\"");
    let payload_path = fx.path("tamper-payload.json");
    fs::write(&payload_path, &payload).unwrap();
    let payload_hash = file_sha(&payload_path);
    let payload_verification = verify_input(
        &fx,
        &fx.registry_sha,
        &fx.target_sha,
        "2026-08-19T12:00:00Z",
        &[(&payload_path, &payload_hash)],
    );
    let payload_error = verify(&fx, &payload_verification, "tamper-payload")
        .unwrap_err()
        .to_string();
    assert!(payload_error.contains("not cryptographically valid"));

    let mut value: Value = serde_json::from_str(&original).unwrap();
    let signature = value["signature"].as_str().unwrap();
    let flipped = if let Some(rest) = signature.strip_prefix('a') {
        format!("b{rest}")
    } else {
        format!("a{}", &signature[1..])
    };
    value["signature"] = json!(flipped);
    let signature_path = fx.path("tamper-signature.json");
    write_json(&signature_path, &value);
    let signature_hash = file_sha(&signature_path);
    let signature_verification = verify_input(
        &fx,
        &fx.registry_sha,
        &fx.target_sha,
        "2026-08-19T12:00:00Z",
        &[(&signature_path, &signature_hash)],
    );
    let signature_error = verify(&fx, &signature_verification, "tamper-signature")
        .unwrap_err()
        .to_string();
    assert!(signature_error.contains("not cryptographically valid"));
}

#[test]
fn rejects_wrong_registry_digest() {
    let fx = fixture();
    let input = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    let path = sign(&fx, &input, "wrong-registry");
    let hash = file_sha(&path);
    let verification = verify_input(
        &fx,
        &digest('d'),
        &fx.target_sha,
        "2026-08-19T12:00:00Z",
        &[(&path, &hash)],
    );
    let error = verify(&fx, &verification, "wrong-registry")
        .unwrap_err()
        .to_string();
    assert!(error.contains("independently trusted digest"));
}

#[test]
fn rejects_unauthorized_key_scope_role_and_approver() {
    let fx = fixture();

    let unknown = make_approver(9);
    let unknown_key = fx.path("unknown.key");
    fs::write(&unknown_key, unknown.seed).unwrap();
    let mut unknown_input = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    unknown_input["signing_key_path"] = json!(unknown_key);
    assert!(
        sign_result(&fx, &unknown_input, "unknown-key")
            .unwrap_err()
            .to_string()
            .contains("no registry entry authorizes")
    );

    let scope_input = auth_input(
        &fx,
        "approved",
        "release-window",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    assert!(
        sign_result(&fx, &scope_input, "wrong-scope")
            .unwrap_err()
            .to_string()
            .contains("no registry entry authorizes")
    );

    let mut role_input = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    role_input["role"] = json!("assistant-editor");
    assert!(
        sign_result(&fx, &role_input, "wrong-role")
            .unwrap_err()
            .to_string()
            .contains("no registry entry authorizes")
    );

    let mut approver_input = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    approver_input["approver_id"] = json!("director-99");
    assert!(
        sign_result(&fx, &approver_input, "wrong-approver")
            .unwrap_err()
            .to_string()
            .contains("no registry entry authorizes")
    );
}

#[test]
fn rejects_expired_and_future_attestations() {
    let fx = fixture();
    let input = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-19T11:00:00Z",
        None,
    );
    let path = sign(&fx, &input, "clock");
    let hash = file_sha(&path);

    let expired = verify_input(
        &fx,
        &fx.registry_sha,
        &fx.target_sha,
        "2026-08-19T12:00:00Z",
        &[(&path, &hash)],
    );
    assert!(
        verify(&fx, &expired, "expired")
            .unwrap_err()
            .to_string()
            .contains("expired at the verification time")
    );

    let future = verify_input(
        &fx,
        &fx.registry_sha,
        &fx.target_sha,
        "2026-08-19T09:00:00Z",
        &[(&path, &hash)],
    );
    assert!(
        verify(&fx, &future, "future")
            .unwrap_err()
            .to_string()
            .contains("not yet valid at the verification time")
    );
}

#[test]
fn rejects_impossible_and_year_zero_utc_timestamps() {
    let fx = fixture();
    let impossible_date = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-02-29T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    assert!(
        sign_result(&fx, &impossible_date, "impossible-date")
            .unwrap_err()
            .to_string()
            .contains("out-of-range field")
    );
    let year_zero = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "0000-01-01T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    assert!(
        sign_result(&fx, &year_zero, "year-zero")
            .unwrap_err()
            .to_string()
            .contains("out-of-range field")
    );
}

#[test]
fn rejects_parent_hash_mismatch() {
    let fx = fixture();
    let approved = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    let approved_path = sign(&fx, &approved, "pt-a");
    let approved_hash = file_sha(&approved_path);
    let revoked = auth_input(
        &fx,
        "revoked",
        "picture-lock",
        2,
        "2026-08-19T11:00:00Z",
        "2026-08-25T10:00:00Z",
        Some((&approved_path, &approved_hash)),
    );
    let revoked_path = sign(&fx, &revoked, "pt-b");
    let revoked_hash = file_sha(&revoked_path);

    let decoy = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-08-19T09:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    let decoy_path = sign(&fx, &decoy, "pt-decoy");
    let decoy_hash = file_sha(&decoy_path);

    let verification = verify_input(
        &fx,
        &fx.registry_sha,
        &fx.target_sha,
        "2026-08-19T12:00:00Z",
        &[(&decoy_path, &decoy_hash), (&revoked_path, &revoked_hash)],
    );
    let error = verify(&fx, &verification, "pt").unwrap_err().to_string();
    assert!(error.contains("parent hash"));
}

#[test]
fn signing_rejects_context_mismatch_in_child() {
    let fx = fixture();
    let picture = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    let picture_path = sign(&fx, &picture, "cm-picture");
    let picture_hash = file_sha(&picture_path);
    let error = sign_result(
        &fx,
        &json!({
            "schema": "reel.approval-authorization-input.v0.1",
            "owner_id": "studio-owner",
            "registry": {"path": fx.registry_path, "sha256": fx.registry_sha},
            "signing_key_path": fx.key_path,
            "authority_context_sha256": digest('c'),
            "approver_id": "director-01",
            "role": "final-approver",
            "scope": "audio-lock",
            "decision": "approved",
            "target_kind": "video",
            "target": {"path": fx.target_path, "sha256": fx.target_sha},
            "policy_sha256": digest('b'),
            "sequence": 2,
            "issued_at_utc": "2026-08-19T11:00:00Z",
            "expires_at_utc": "2026-08-25T10:00:00Z",
            "prior_attestation": {"path": picture_path, "sha256": picture_hash},
        }),
        "cm-audio",
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("prior attestation does not share the exact authority context"));
}

#[test]
fn rejects_duplicate_and_unknown_json_fields() {
    let fx = fixture();
    let input = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    let path = sign(&fx, &input, "strict");
    let original = fs::read_to_string(&path).unwrap();

    let duplicate = original.replacen('{', "{\n  \"scope\": \"picture-lock\",", 1);
    let duplicate_path = fx.path("duplicate.json");
    fs::write(&duplicate_path, &duplicate).unwrap();
    let duplicate_hash = file_sha(&duplicate_path);
    let duplicate_verification = verify_input(
        &fx,
        &fx.registry_sha,
        &fx.target_sha,
        "2026-08-19T12:00:00Z",
        &[(&duplicate_path, &duplicate_hash)],
    );
    assert!(
        verify(&fx, &duplicate_verification, "duplicate")
            .unwrap_err()
            .to_string()
            .contains("duplicate JSON fields")
    );

    let mut value: Value = serde_json::from_str(&original).unwrap();
    value["surprise_field"] = json!(true);
    let unknown_path = fx.path("unknown.json");
    write_json(&unknown_path, &value);
    let unknown_hash = file_sha(&unknown_path);
    let unknown_verification = verify_input(
        &fx,
        &fx.registry_sha,
        &fx.target_sha,
        "2026-08-19T12:00:00Z",
        &[(&unknown_path, &unknown_hash)],
    );
    assert!(
        verify(&fx, &unknown_verification, "unknown")
            .unwrap_err()
            .to_string()
            .contains("strict JSON")
    );
}

#[test]
fn refuses_to_clobber_existing_attestation() {
    let fx = fixture();
    let input = auth_input(
        &fx,
        "approved",
        "picture-lock",
        1,
        "2026-08-19T10:00:00Z",
        "2026-08-25T10:00:00Z",
        None,
    );
    let input_path = fx.path("noclobber-input.json");
    write_json(&input_path, &input);
    let output_path = fx.path("noclobber.json");
    reel::approval_attestation::sign_attestation(&input_path, &output_path).unwrap();
    let error = reel::approval_attestation::sign_attestation(&input_path, &output_path)
        .unwrap_err()
        .to_string();
    assert!(error.contains("refusing to overwrite"));
}

// --- C2PA verification -------------------------------------------------------

#[cfg(feature = "test-fixtures")]
fn fake_c2patool() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake_c2patool"))
}

#[cfg(feature = "test-fixtures")]
fn c2pa_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(feature = "test-fixtures")]
fn c2pa_input(
    dir: &Path,
    scenario: &str,
    expected_version: Option<&str>,
    executable_hash: Option<&str>,
) -> PathBuf {
    let extension = if scenario == "extension" {
        "mp4"
    } else {
        "bin"
    };
    let target = dir.join(format!("{scenario}-target.{extension}"));
    fs::write(&target, format!("{scenario}\n")).unwrap();
    let target_sha = file_sha(&target);
    let executable = fake_c2patool();
    let executable_sha = executable_hash
        .map(str::to_string)
        .unwrap_or_else(|| file_sha(&executable));
    let mut input = json!({
        "schema": "reel.c2pa-verification-input.v0.1",
        "c2patool_path": executable,
        "expected_c2patool_sha256": executable_sha,
        "target": {"path": target, "sha256": target_sha},
    });
    if let Some(version) = expected_version {
        input["expected_tool_version"] = json!(version);
    }
    let input_path = dir.join(format!("{scenario}-input.json"));
    write_json(&input_path, &input);
    input_path
}

#[cfg(feature = "test-fixtures")]
fn run_c2pa(
    dir: &Path,
    scenario: &str,
    expected_version: Option<&str>,
    executable_hash: Option<&str>,
) -> anyhow::Result<reel::c2pa_verification::C2paVerificationReport> {
    let _guard = c2pa_test_lock().lock().expect("c2pa test lock poisoned");
    let input = c2pa_input(dir, scenario, expected_version, executable_hash);
    let output = dir.join(format!("{scenario}-report.json"));
    reel::c2pa_verification::verify_c2pa(&input, Some(&output))
}

#[cfg(feature = "test-fixtures")]
#[test]
fn c2pa_valid_report_is_path_free_and_trust_not_evaluated() {
    let dir = tempdir().unwrap();
    let report = run_c2pa(dir.path(), "valid", None, None).unwrap();
    assert_eq!(report.manifest_integrity, "valid");
    assert_eq!(report.certificate_trust, "not-evaluated");
    assert!(!report.trust_evaluated);
    assert_eq!(report.validation_state, "Valid");
    assert_eq!(report.tool_version, "9.9.9");
    assert_eq!(report.active_manifest_label, "urn:uuid:1");
    assert_eq!(report.verifier_settings_sha256.len(), 64);
    assert!(
        report
            .validation_status_codes
            .contains(&"timeStamp.untrusted".to_string())
    );
    assert!(!report.grants_identity);
    assert!(!report.grants_rights);
    assert!(!report.grants_publication);
    assert!(!report.grants_release);
    assert!(report.human_review_required);

    let text = fs::read_to_string(dir.path().join("valid-report.json")).unwrap();
    assert!(!text.contains(&dir.path().display().to_string()));
    assert!(!text.contains(&fake_c2patool().display().to_string()));
    assert!(!text.contains("\"path\""));
}

#[cfg(feature = "test-fixtures")]
#[test]
fn c2pa_rejects_uncontrolled_trust_and_does_not_infer_trust_from_codes() {
    let dir = tempdir().unwrap();
    assert!(
        run_c2pa(dir.path(), "trusted", None, None)
            .unwrap_err()
            .to_string()
            .contains("controlled no-trust run")
    );

    let untrusted = run_c2pa(dir.path(), "untrusted", None, None).unwrap();
    assert_eq!(untrusted.certificate_trust, "not-evaluated");
    assert!(!untrusted.trust_evaluated);
    assert_eq!(untrusted.manifest_integrity, "valid");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn c2pa_rejects_legacy_status_and_preserves_the_media_extension() {
    let dir = tempdir().unwrap();
    assert!(
        run_c2pa(dir.path(), "null-status", None, None)
            .unwrap_err()
            .to_string()
            .contains("missing validation_state")
    );

    let extension = run_c2pa(dir.path(), "extension", None, None).unwrap();
    assert_eq!(extension.manifest_integrity, "valid");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn c2pa_collects_unknown_codes_and_rejects_duplicate_json_keys() {
    let dir = tempdir().unwrap();
    let report = run_c2pa(dir.path(), "unknown-status", None, None).unwrap();
    assert_eq!(
        report.validation_status_codes,
        vec!["future.validation.success".to_string()]
    );
    assert!(
        run_c2pa(dir.path(), "duplicate", None, None)
            .unwrap_err()
            .to_string()
            .contains("duplicate JSON fields")
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn c2pa_bounds_active_manifest_labels_and_manifest_count() {
    let dir = tempdir().unwrap();
    assert!(
        run_c2pa(dir.path(), "overlong-label", None, None)
            .unwrap_err()
            .to_string()
            .contains("active manifest label exceeds")
    );
    assert!(
        run_c2pa(dir.path(), "many-manifests", None, None)
            .unwrap_err()
            .to_string()
            .contains("too many manifests")
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn c2pa_rejects_failure_missing_and_malformed_reports() {
    let dir = tempdir().unwrap();
    assert!(
        run_c2pa(dir.path(), "failure", None, None)
            .unwrap_err()
            .to_string()
            .contains("integrity validation failure")
    );
    assert!(
        run_c2pa(dir.path(), "xca-invalid", None, None)
            .unwrap_err()
            .to_string()
            .contains("validation_state Invalid")
    );
    assert!(
        run_c2pa(dir.path(), "missing", None, None)
            .unwrap_err()
            .to_string()
            .contains("no active manifest")
    );
    assert!(
        run_c2pa(dir.path(), "missing-state", None, None)
            .unwrap_err()
            .to_string()
            .contains("missing validation_state")
    );
    assert!(
        run_c2pa(dir.path(), "missing-results", None, None)
            .unwrap_err()
            .to_string()
            .contains("no validation_results")
    );
    assert!(
        run_c2pa(dir.path(), "malformed", None, None)
            .unwrap_err()
            .to_string()
            .contains("not valid JSON")
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn c2pa_rejects_oversize_output() {
    let dir = tempdir().unwrap();
    let error = run_c2pa(dir.path(), "oversize", None, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("exceeded the"));
}

#[cfg(feature = "test-fixtures")]
#[test]
fn c2pa_rejects_wrong_executable_hash() {
    let dir = tempdir().unwrap();
    let error = run_c2pa(dir.path(), "valid", None, Some(&digest('e')))
        .unwrap_err()
        .to_string();
    assert!(error.contains("executable hash does not match"));
}

#[cfg(feature = "test-fixtures")]
#[test]
fn c2pa_rejects_tool_failure_status() {
    let dir = tempdir().unwrap();
    let error = run_c2pa(dir.path(), "toolfail", None, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("failure status"));
}

#[cfg(feature = "test-fixtures")]
#[test]
fn c2pa_captures_and_pins_tool_version() {
    let dir = tempdir().unwrap();
    let matched = run_c2pa(dir.path(), "valid", Some("9.9.9"), None).unwrap();
    assert_eq!(matched.tool_version, "9.9.9");

    let error = run_c2pa(dir.path(), "valid", Some("1.2.3"), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("does not match the expected"));
}

#[test]
fn cli_help_exposes_signed_approval_and_c2pa_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    for command in ["approval-sign", "approval-verify", "c2pa-verify"] {
        assert!(help.contains(command), "missing {command} from CLI help");
    }

    let signed_help = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["approval-sign", "--help"])
        .output()
        .unwrap();
    assert!(signed_help.status.success());
    assert!(String::from_utf8_lossy(&signed_help.stdout).contains("attestation"));
}
