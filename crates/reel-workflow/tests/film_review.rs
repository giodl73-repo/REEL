use reel_workflow::{ErrorKind, digest, film_review::*};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, process::Command};
#[path = "support/film_review_fixture.rs"]
mod fixture;
use fixture::*;
#[test]
fn exact_targets_stable_keys_and_private_drafts_do_not_claim_viewing_or_submission() {
    let (mut report, mut reader) = fixture();
    let drafts = draft(&report, &mut reader).unwrap();
    assert_eq!(reader.verified, vec![report.review.logical_id.clone()]);
    assert_eq!(drafts[0].target.cel_id, "C100");
    assert_eq!(drafts[1].target.cel_id, "C101");
    assert_eq!(drafts[0].target.selected_picture.sha256, "7".repeat(64));
    assert!(drafts[0].body.contains("Conservar el texto español."));
    assert!(!drafts[0].body.contains("picture.png"));
    for d in &drafts {
        assert!(
            !d.viewing_verified
                && !d.authority_verified
                && !d.native_retention_verified
                && !d.issue_submitted
                && !d.selection_applied
        );
        assert!(d.body.ends_with(&format!(
            "<!-- reel-film-finding:{} -->\n",
            d.finding_identity
        )));
    }
    report.findings.reverse();
    let reversed = draft(&report, &mut reader).unwrap();
    assert_eq!(drafts[0].key, reversed[1].key);
    report.findings.remove(0);
    let subset = draft(&report, &mut reader).unwrap();
    assert_eq!(drafts[0].key, subset[0].key);
    report.findings[0]
        .observation
        .push_str(" Additional reported detail.");
    let revised = draft(&report, &mut reader).unwrap();
    assert_ne!(drafts[0].key, revised[0].key);
    assert_eq!(drafts[0].finding_identity, revised[0].finding_identity);
}
#[test]
fn board_reference_resolves_target_and_rejects_a_stale_drawing_before_movie_reads() {
    let (mut report, mut reader) = fixture();
    let explicit = draft(&report, &mut reader).unwrap();
    report.findings[0].target_id.clear();
    report.findings[0].board_reference = Some(serde_json::from_value(json!({
        "project": "synthetic-film",
        "source_commit": "b".repeat(40),
        "story_id": "story-1",
        "display_id": "C100",
        "semantic_id": "arrival",
        "language": "es",
        "version": {
            "id": "v1",
            "designation": "selected",
            "asset": {"logical_id":"picture/arrival/v1","sha256":"7".repeat(64),"bytes":100,"media_type":"image/png"}
        }
    })).unwrap());
    let drafts = draft(&report, &mut reader).unwrap();
    assert_eq!(drafts[0].target.attachment_id, "p-a");
    assert_eq!(drafts[0].finding_identity, explicit[0].finding_identity);
    assert_eq!(drafts[0].key, explicit[0].key);
    reader.verified.clear();
    report.findings[0]
        .board_reference
        .as_mut()
        .unwrap()
        .version
        .asset
        .sha256 = "8".repeat(64);
    assert_eq!(
        draft(&report, &mut reader).unwrap_err().code,
        "film-findings-board-reference"
    );
    assert!(reader.verified.is_empty());
}

#[test]
fn stale_maps_wrong_frames_duplicate_ids_and_false_authority_refuse_before_movie_reads() {
    for case in [
        "frame",
        "target",
        "map-bytes",
        "receipt-claims",
        "receipt-movie",
        "receipt-map",
        "duplicate",
        "bound",
    ] {
        let (mut r, mut m) = fixture();
        match case {
            "frame" => r.findings[0].frame = 24,
            "target" => r.findings[0].target_id = "p-other".into(),
            "map-bytes" => m
                .files
                .insert(r.map.logical_id.clone(), b"different map".to_vec())
                .map(|_| ())
                .unwrap(),
            "receipt-claims" | "receipt-movie" | "receipt-map" => {
                let mut v: Value = serde_json::from_slice(&m.files[&r.receipt.logical_id]).unwrap();
                match case {
                    "receipt-claims" => v["visual_reviewed"] = json!(true),
                    "receipt-movie" => v["outputs"]["review.mp4"]["sha256"] = json!("0".repeat(64)),
                    _ => v["outputs"]["review-map.json"]["bytes"] = json!(1),
                }
                r.receipt = control(&mut m, "review/receipt", v);
            }
            "duplicate" => r.findings[1].id = r.findings[0].id.clone(),
            "bound" => {
                r.findings = (0..33)
                    .map(|i| {
                        let mut f = r.findings[0].clone();
                        f.id = format!("finding-{i}");
                        f
                    })
                    .collect();
            }
            _ => unreachable!(),
        }
        assert!(draft(&r, &mut m).is_err(), "{case}");
        assert!(m.verified.is_empty(), "{case}");
    }
}
#[test]
fn a_new_explicit_review_changes_the_draft_key_and_preserves_the_old_subject() {
    let (mut r, mut m) = fixture();
    let old = draft(&r, &mut m).unwrap();
    let mut map: Value = serde_json::from_slice(&m.files[&r.map.logical_id]).unwrap();
    map["targets"][0]["selected_picture"]["sha256"] = json!("9".repeat(64));
    r.map = control(&mut m, "review/map", map);
    let mut receipt: Value = serde_json::from_slice(&m.files[&r.receipt.logical_id]).unwrap();
    receipt["outputs"]["review-map.json"]["sha256"] = json!(r.map.sha256);
    receipt["outputs"]["review-map.json"]["bytes"] = json!(r.map.bytes);
    r.receipt = control(&mut m, "review/receipt", receipt);
    let new = draft(&r, &mut m).unwrap();
    assert_ne!(old[0].key, new[0].key);
    assert_eq!(old[0].target.selected_picture.sha256, "7".repeat(64));
    assert_eq!(new[0].target.selected_picture.sha256, "9".repeat(64));
}
#[test]
fn windows_authored_producer_paths_remain_readable_on_other_hosts() {
    let (mut r, mut m) = fixture();
    let mut map: Value = serde_json::from_slice(&m.files[&r.map.logical_id]).unwrap();
    map["delivery_receipt"]["path"] = json!("delivery\\receipt.json");
    map["source_film"]["path"] = json!("delivery\\review.mp4");
    r.map = control(&mut m, "review/map", map);
    let mut receipt: Value = serde_json::from_slice(&m.files[&r.receipt.logical_id]).unwrap();
    receipt["outputs"]["review-map.json"]["sha256"] = json!(r.map.sha256);
    receipt["outputs"]["review-map.json"]["bytes"] = json!(r.map.bytes);
    r.receipt = control(&mut m, "review/receipt", receipt);
    assert!(draft(&r, &mut m).is_ok());
}
#[test]
fn diagnostics_distinguish_schema_budget_and_conflicting_evidence_before_reads() {
    let (report, mut reader) = fixture();
    let mut bad = report.clone();
    bad.schema = "unknown".into();
    assert_eq!(
        draft(&bad, &mut reader).unwrap_err().kind,
        ErrorKind::Invalid
    );
    let mut bad = report.clone();
    bad.review.bytes = 17 * 1024 * 1024 * 1024;
    assert_eq!(
        draft(&bad, &mut reader).unwrap_err().kind,
        ErrorKind::Budget
    );
    let mut bad = report.clone();
    bad.findings[0].evidence.push(report.map.clone());
    assert_eq!(
        draft(&bad, &mut reader).unwrap_err().kind,
        ErrorKind::Conflict
    );
    assert!(reader.verified.is_empty());
}
fn exported_request(root: &Path) -> (std::path::PathBuf, String) {
    let (report, reader) = fixture();
    let mut files = BTreeMap::new();
    for (i, (id, raw)) in reader.files.into_iter().enumerate() {
        let path = root.join(format!("input-{i}"));
        fs::write(&path, raw).unwrap();
        files.insert(id, path);
    }
    let request = ExportRequest {
        schema: "reel.workflow-film-findings-export.v1".into(),
        report,
        files,
    };
    let raw = serde_json::to_vec(&request).unwrap();
    let pin = digest(&raw);
    let path = root.join("private-request.json");
    fs::write(&path, raw).unwrap();
    (path, pin)
}
#[test]
fn cli_exports_complete_replayable_packages_and_preserves_changed_or_partial_outputs() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let (path, pin) = exported_request(root);
    let out = root.join("drafts");
    let call = |pin: &str, out: &Path| {
        Command::new(env!("CARGO_BIN_EXE_reel-workflow"))
            .arg("draft-film-issues")
            .arg(&path)
            .arg(pin)
            .arg(out)
            .output()
            .unwrap()
    };
    let refused = root.join("refused");
    assert!(!call(&"0".repeat(64), &refused).status.success());
    assert!(!refused.exists());
    let result = call(&pin, &out);
    assert!(result.status.success(), "{result:?}");
    let status: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(status["issue_submitted"], false);
    assert_eq!(status["selection_applied"], false);
    assert_eq!(status["replay"], false);
    assert!(
        !String::from_utf8(result.stdout)
            .unwrap()
            .contains(root.to_str().unwrap())
    );
    assert_eq!(fs::read_dir(&out).unwrap().count(), 7);
    let second = call(&pin, &out);
    assert!(second.status.success());
    let status: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(status["replay"], true);
    let body = fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|s| s == "md"))
        .unwrap();
    fs::write(&body, "Changed package evidence").unwrap();
    assert!(!call(&pin, &out).status.success());
    assert_eq!(
        fs::read_to_string(&body).unwrap(),
        "Changed package evidence"
    );
    let partial = root.join("partial");
    fs::create_dir(&partial).unwrap();
    fs::write(partial.join("index.json"), "interrupted output").unwrap();
    assert!(!call(&pin, &partial).status.success());
    assert_eq!(
        fs::read_to_string(partial.join("index.json")).unwrap(),
        "interrupted output"
    );
}
#[test]
fn explicit_file_reader_detects_movie_and_evidence_changes_without_opening_an_issue() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let (path, pin) = exported_request(root);
    let request: ExportRequest = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let movie = &request.files[&request.report.review.logical_id];
    fs::write(movie, "bad movie").unwrap();
    assert_eq!(
        export(&path, &pin, &root.join("must-not-exist"))
            .unwrap_err()
            .kind,
        ErrorKind::Integrity
    );
    assert!(!root.join("must-not-exist").exists());
}
#[cfg(windows)]
#[test]
fn an_active_windows_writer_is_busy_without_creating_a_package() {
    use std::os::windows::fs::OpenOptionsExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let (path, pin) = exported_request(root);
    let request: ExportRequest = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let movie = &request.files[&request.report.review.logical_id];
    let writer = fs::OpenOptions::new()
        .write(true)
        .share_mode(0)
        .open(movie)
        .unwrap();
    let out = root.join("wait-for-original-writer");
    assert_eq!(export(&path, &pin, &out).unwrap_err().kind, ErrorKind::Busy);
    assert!(!out.exists());
    drop(writer);
    assert!(!export(&path, &pin, &out).unwrap());
}
