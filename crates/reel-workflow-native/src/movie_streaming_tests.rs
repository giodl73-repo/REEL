//! Existing retained movie only: streamed historical proof and review metadata.
use super::*;
use crate::{admission, retention};
use std::{fs, io::Write};

fn load<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> T {
    reel_workflow::decode(&fs::read(path).unwrap()).unwrap()
}
fn save(path: impl AsRef<Path>, value: &impl serde::Serialize) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
    file.sync_all().unwrap();
}
#[test]
#[ignore = "Existing retained synthetic movie bundle only; streamed reopening with/without review capture after source slot reuse, unchanged review IDs and claims; no render/admission/publication"]
fn streamed_movie_history_preserves_review_copy_and_source_activation_fencing() {
    let bundle = std::path::PathBuf::from(
        std::env::var_os("REEL_STREAMED_MOVIE_BUNDLE").expect("Set original retained movie bundle"),
    );
    let evidence = std::path::PathBuf::from(
        std::env::var_os("REEL_STREAMED_MOVIE_EVIDENCE_DIRECTORY")
            .expect("Set new evidence directory"),
    );
    let packet: Packet = load(bundle.join("packet.json"));
    let source: crate::NativeProfile = load(bundle.join("source.json"));
    let profile: admission::Profile = load(bundle.join("admission.json"));
    let receipt: admission::Receipt = load(bundle.join("receipt.json"));
    let result: ReturnedResult = load(bundle.join("result.json"));
    let ack: retention::Acknowledgement = load(bundle.join("ack.json"));
    let expected: serde_json::Value = load(bundle.join("review-copy.json"));
    let claims = fs::read(profile.index_root.join("claims.json")).unwrap();
    save(
        evidence.join("original-operation.json"),
        &serde_json::json!({"bundle":bundle,"task_key":packet.task_key,"activation":receipt.activation,"index":profile.index_root,"verifier":source.movie_verifier}),
    );
    // The original source slot was closed/reused by its qualified fixture.
    // Read through retained coverage, never re-admit or retarget that job.
    assert!(
        admission::Prepared::open(&packet, &source, &profile, &receipt.attempt_id)
            .unwrap()
            .reopen(&receipt)
            .is_err()
    );
    drop(retention::verify(&packet, &source, &profile, &receipt, &result, &ack).unwrap());
    let copy = retention::review_copy(&packet, &source, &profile, &receipt, &result, &ack).unwrap();
    assert_eq!(serde_json::to_value(copy.copy()).unwrap(), expected);
    assert!(!copy.copy().units.is_empty());
    drop(copy);
    assert_eq!(
        claims,
        fs::read(profile.index_root.join("claims.json")).unwrap()
    );
    assert!(
        admission::Prepared::open(&packet, &source, &profile, &receipt.attempt_id)
            .unwrap()
            .reopen(&receipt)
            .is_err()
    );
    save(
        evidence.join("qualification-result.json"),
        &serde_json::json!({"task_key":packet.task_key,"streamed_history_reopened":true,"review_metadata_capture_reopened":true,"review_copy_and_ids_unchanged":true,"source_activation_still_refused":true,"claims_unchanged":true,"new_admission":false,"new_render":false,"new_publication":false,"selected":false,"scratch_retired":false}),
    );
}
