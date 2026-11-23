//! Real repeated native rendering with a full, unchanged source-identity guard.
use super::*;
use tig_store::source_claims::{Command as ClaimCommand, Error as ClaimError, MAX_SOURCES};

pub(super) fn exercise(
    p: &Project,
    binary: &Path,
    profile: &Value,
    first: &Value,
    journal: &mut SourceJournal,
) {
    let first_outputs = &first["result"]["outputs"];
    let original = journal
        .source(&key("bounded-episode-cli"))
        .unwrap()
        .unwrap();
    assert_eq!(original.revision, 6);
    let original_selection = original.current.as_ref().unwrap().selection.clone();
    let original_movie = Path::new(first_outputs["native/master.mkv"]["path"].as_str().unwrap());
    let original_identity = ByteIdentity::observe_file(original_movie).unwrap();
    let first_receipt = read(
        first_outputs["native/receipt.json"]["path"]
            .as_str()
            .unwrap(),
    );
    let first_report = read(first_outputs["worker.json"]["path"].as_str().unwrap());
    let mut filled = false;
    for index in 0..MAX_SOURCES {
        match journal.apply(
            key(&format!("capacity-reservation-{index}")),
            ClaimCommand::Prepare {
                source: key(&format!("capacity-source-{index}")),
                owner: key("capacity-fixture"),
                expected_revision: 0,
                selection: original_selection.clone(),
            },
        ) {
            Ok(_) => {}
            Err(ClaimError::Budget) => {
                filled = true;
                break;
            }
            Err(error) => panic!("unexpected capacity fixture error: {error}"),
        }
    }
    assert!(filled, "fixture must reach the existing source guard");
    let before = journal.checkpoint();
    let mut fresh = profile.clone();
    fresh["source_id"] = json!("capacity-fresh-episode");
    fresh["operation"] = json!("capacity-fresh-run");
    let fresh_path = p.data.path().join("capacity-fresh-profile.json");
    write(p.data.path(), "capacity-fresh-profile.json", &fresh);
    let refused = Command::new(binary)
        .arg("run-episode")
        .arg(fresh_path)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert_eq!(journal.checkpoint(), before);
    assert!(
        journal
            .source(&key("capacity-fresh-episode"))
            .unwrap()
            .is_none()
    );

    let mut next = profile.clone();
    next["expected_revision"] = json!(6);
    next["operation"] = json!("run-v2-reuse");
    next["reuse_children"] = serde_json::to_value(&original_selection).unwrap();
    next["journal_minimum"] = serde_json::to_value(journal.checkpoint()).unwrap();
    next["inputs"]["record_outputs"] = json!([]);
    next["inputs"]["git_handoff"] = Value::Null;
    let next_path = p.data.path().join("reused-episode-profile.json");
    for field in ["owner", "reuse_children"] {
        let mut changed = next.clone();
        if field == "owner" {
            changed[field] = json!("foreign");
        } else {
            changed[field]["context"] = json!(tig_core::Digest::blob(b"foreign handoff"));
        }
        write(p.data.path(), "reused-episode-profile.json", &changed);
        let before = journal.checkpoint();
        let refused = Command::new(binary)
            .arg("run-episode")
            .arg(&next_path)
            .output()
            .unwrap();
        assert!(!refused.status.success());
        assert_eq!(journal.checkpoint(), before);
    }
    write(p.data.path(), "reused-episode-profile.json", &next);
    let output = Command::new(binary)
        .arg("run-episode")
        .arg(&next_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}; {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["succeeded"], true);
    assert_eq!(result["approval"], false);
    assert_eq!(result["temporary_owner_cleanup_required"], false);
    let receipt = read(
        result["result"]["outputs"]["native/receipt.json"]["path"]
            .as_str()
            .unwrap(),
    );
    assert_eq!(receipt["total_frames"], first_receipt["total_frames"]);
    assert_eq!(receipt["total_samples"], first_receipt["total_samples"]);
    assert_eq!(receipt["decoded_master_matches_ordered_segments"], true);
    assert_eq!(receipt["timestamps_verified"], true);
    let old = first_receipt["segments"].as_array().unwrap();
    let new = receipt["segments"].as_array().unwrap();
    assert_eq!(old.len(), new.len());
    for (old, new) in old.iter().zip(new) {
        for field in [
            "id",
            "kind",
            "selected_master_sha256",
            "selected_master_bytes",
            "frames",
            "samples",
            "start_frame",
            "start_sample",
            "decoded_picture_sha256",
            "decoded_audio_sha256",
        ] {
            assert_eq!(old[field], new[field], "{field}");
        }
    }
    let report = read(
        result["result"]["outputs"]["worker.json"]["path"]
            .as_str()
            .unwrap(),
    );
    assert_eq!(
        report["temporary_sources"],
        first_report["temporary_sources"]
    );
    let root = journal
        .source(&key("bounded-episode-cli"))
        .unwrap()
        .unwrap();
    assert_eq!(root.revision, 12);
    assert!(root.pending.is_none());
    assert!(
        root.predecessors
            .iter()
            .any(|prior| prior.selection == original_selection)
    );
    for source in report["temporary_sources"].as_array().unwrap() {
        let state = journal
            .source(&key(source.as_str().unwrap()))
            .unwrap()
            .unwrap();
        assert_eq!(state.revision, 6);
        assert!(state.current.is_none() && state.pending.is_none());
    }
    assert_eq!(
        ByteIdentity::observe_file(original_movie).unwrap(),
        original_identity
    );
    let before = journal.checkpoint();
    let recovered = Command::new(binary)
        .arg("finish-episode")
        .arg(&next_path)
        .output()
        .unwrap();
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    assert_eq!(journal.checkpoint(), before);
    assert!(matches!(
        journal.apply(
            key("capacity-still-full"),
            ClaimCommand::Prepare {
                source: key("capacity-still-full"),
                owner: key("fixture"),
                expected_revision: 0,
                selection: original_selection
            }
        ),
        Err(ClaimError::Budget)
    ));
}
