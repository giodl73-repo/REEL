//! Actual retained v2 report -> its independently retained producer. This
//! read-only qualification grants no contextual evaluation or finding closure.
use super::*;
use reel_workflow::{Plan, review};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tig_store::source_claims::protection::Index;

fn load<T: serde::de::DeserializeOwned>(root: &Path, name: &str) -> T {
    reel_workflow::decode(&fs::read(root.join(name)).unwrap()).unwrap()
}
#[test]
#[ignore = "Exact retained corrected v2 report/issue only; native report and producer proofs, derived manifest-ID correspondence and changed-media/witness refusals, actual Index unchanged; no proposal/issue closure or new native lifecycle effects"]
fn retained_bound_report_accepts_only_its_exact_producer_manifest_under_a_derived_id() {
    let env = |key| PathBuf::from(std::env::var_os(key).expect(key));
    let report_root = env("REEL_RESOLUTION_PROJECTION_REPORT");
    let producer_root = env("REEL_RESOLUTION_PROJECTION_PRODUCER");
    let evidence = env("REEL_RESOLUTION_PROJECTION_EVIDENCE");
    let profile: crate::handoff::Profile = load(&report_root, "profile.json");
    assert_eq!(
        profile.runtime.sha256,
        "5c237208ccf8bc9fe13e2cc2ccdc154430c4d4f8ef07c51a613f5ab41a40abe1"
    );
    let review = RetainedReview {
        packet: load(&report_root, "packet.json"),
        source: profile.source,
        admission: profile.admission,
        receipt: profile.binding.admission,
        result: load(&report_root, "result.json"),
        acknowledgement: load(&report_root, "retained.json"),
    };
    assert_eq!(
        review.packet.task_key,
        "230314b16bc5b4fdbce6df59a133112226d7416266e40412ae9252dfa3c207a5"
    );
    let checkpoint = || {
        Index::open(
            &review.admission.index_root,
            &review.acknowledgement.index_checkpoint,
        )
        .unwrap()
        .checkpoint()
    };
    let before = checkpoint();
    let (checked, _) = review.checked().unwrap();
    let witness = checked.production().unwrap();
    assert_eq!(
        witness.packet.task_key,
        "69d0d1a011e1a088c283dc64d3e29146a4be3c1a2b5fd931d8e334fb9c536ac9"
    );
    let plan: Plan = load(&report_root, "plan.json");
    let bindings: crate::graph::Bindings = load(&producer_root, "bindings.json");
    let produced = production_copy(
        &plan,
        &bindings,
        "issue-assemble",
        &witness.packet.task_key,
        &checked,
    )
    .unwrap();
    assert_eq!(
        produced.copy_sha256,
        reel_workflow::canonical_digest(checked.copy()).unwrap()
    );
    assert!(produced.producer_copy_sha256.is_some());
    assert_ne!(
        produced.producer_copy_sha256.as_ref().unwrap(),
        &produced.copy_sha256
    );
    let original: crate::handoff::Profile = load(&producer_root, "profile.json");
    let original_result: ReturnedResult = load(&producer_root, "result.json");
    let original_ack: retention::Acknowledgement = load(&producer_root, "retained.json");
    let direct = retention::review_copy(
        &witness.packet,
        &original.source,
        &original.admission,
        &original.binding.admission,
        &original_result,
        &original_ack,
    )
    .unwrap();
    assert_ne!(
        direct.copy().review.logical_id,
        checked.copy().review.logical_id
    );
    assert_eq!(direct.copy().review.sha256, checked.copy().review.sha256);
    assert_eq!(direct.copy().review.bytes, checked.copy().review.bytes);
    assert!(check_production_copy(direct.copy(), checked.copy()).is_err());
    for mode in [
        "hash",
        "bytes",
        "media",
        "publication",
        "language",
        "page-order",
        "unit",
        "attempt",
        "packet",
    ] {
        let mut copy = checked.copy().clone();
        let mut foreign = witness.clone();
        match mode {
            "hash" => copy.review.sha256 = "0".repeat(64),
            "bytes" => copy.review.bytes += 1,
            "media" => copy.review.media_type = "application/pdf".into(),
            "publication" => copy.publication.logical_id.push_str("-other"),
            "language" => copy.language = "en".into(),
            "page-order" => match &mut copy.format {
                review::Format::Comic { page_order } => page_order.reverse(),
                _ => panic!("comic required"),
            },
            "unit" => {
                copy.units.pop_first();
            }
            "attempt" => foreign.result.attempt_id.push_str("-other"),
            "packet" => foreign.packet.task_key = "0".repeat(64),
            _ => unreachable!(),
        }
        assert!(
            check_bound_production_copy(
                direct.copy(),
                &copy,
                &foreign,
                &witness.packet,
                &witness.result
            )
            .is_err(),
            "{mode}"
        );
    }
    drop(direct);
    let unbound = review::check_report(
        checked.copy(),
        &review::Report {
            schema: "reel.workflow-review-report.v1".into(),
            copy_sha256: reel_workflow::canonical_digest(checked.copy()).unwrap(),
            findings: vec![],
        },
    )
    .unwrap();
    assert_eq!(
        production_copy(
            &plan,
            &bindings,
            "issue-assemble",
            &witness.packet.task_key,
            &unbound
        )
        .err()
        .unwrap()
        .code,
        "resolution-production-review-copy"
    );
    assert_eq!(before, checkpoint());
    let output = evidence.join("qualification-result.json");
    let result = serde_json::json!({"schema":"reel.retained-report-producer-projection-qualification.v1","report_task_key":review.packet.task_key,"production":produced,"derived_manifest_id_only":true,"manifest_bytes_and_complete_context_equal":true,"legacy_unbound_namespace_change_refused":true,"changed_hash_length_media_publication_language_page_order_units_attempt_packet_refused":true,"actual_index_unchanged":true,"checkpoint_sha256":reel_workflow::canonical_digest(&before).unwrap(),"proposal_emitted":false,"finding_closed":false,"viewing_verified":false,"selected":false,"approved":false});
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(&serde_json::to_vec(&result).unwrap())?;
            file.sync_all()
        })
        .unwrap();
}
