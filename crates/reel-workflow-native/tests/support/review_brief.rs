//! Add model instructions/result packing to an existing protected Waiting job.
use super::*;
#[test]
#[ignore = "Existing protected synthetic structured-review job only; exports/validates local controls, no provider or import effects"]
fn protected_review_brief_and_result_packer_preserve_the_original_waiting_job() {
    let original = PathBuf::from(
        std::env::var_os("REEL_REVIEW_BRIEF_FIXTURE").expect("Set Waiting review root"),
    );
    let executable = PathBuf::from(
        std::env::var_os("REEL_NATIVE_HANDOFF_TOOL").expect("Set current pinned CLI"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_REVIEW_BRIEF_EVIDENCE_DIRECTORY").expect("Set evidence directory"),
    );
    let packet: Packet = decode(&fs::read(original.join("packet.json")).unwrap()).unwrap();
    let profile: reel_workflow_native::handoff::Profile =
        decode(&fs::read(original.join("profile.json")).unwrap()).unwrap();
    let mut temp = tempfile::tempdir().unwrap();
    temp.disable_cleanup(true);
    let root = temp.path();
    let marker = json!({"root":root,"original":original,"task_key":packet.task_key,"admission":profile.binding.admission,"current_cli":executable,"original_lifecycle_runtime":profile.runtime});
    fs::write(
        root.join("original-operation.json"),
        serde_json::to_vec(&marker).unwrap(),
    )
    .unwrap();
    fs::write(
        evidence.join("original-operation-roots.json"),
        serde_json::to_vec(&marker).unwrap(),
    )
    .unwrap();
    let claims = fs::read(profile.admission.index_root.join("claims.json")).unwrap();
    let journal = original.join("journal");
    let waiting = fs::read(journal.join("00000.json")).unwrap();
    assert_eq!(
        handoff::Journal::open(&journal).unwrap().state(),
        handoff::State::Waiting
    );
    let invoke = |command: &str, report: Option<&Path>, output: &Path| {
        let mut child = Command::new(&executable);
        child.arg(command);
        for file in [
            "packet.json",
            "source.json",
            "admission.json",
            "receipt.json",
        ] {
            child.arg(original.join(file));
        }
        if let Some(report) = report {
            child.arg(report);
        }
        child.arg(output).arg("--apply").output().unwrap()
    };
    let brief_path = root.join("review-brief.json");
    let output = invoke("export-review-brief", None, &brief_path);
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fs::write(root.join("export-stdout.json"), output.stdout).unwrap();
    let brief: review::Brief = decode(&fs::read(&brief_path).unwrap()).unwrap();
    assert_eq!(brief.task_key, packet.task_key);
    assert_eq!(brief.attempt_id, profile.binding.admission.attempt_id);
    assert_eq!(brief.units.len(), 3);
    let mut raw = serde_json::to_vec_pretty(&brief.empty_report).unwrap();
    raw.push(b'\n');
    let report = root.join("empty-report.json");
    fs::write(&report, &raw).unwrap();
    let result_path = root.join("result.json");
    let output = invoke("pack-review-result", Some(&report), &result_path);
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let status: serde_json::Value = decode(&output.stdout).unwrap();
    assert_eq!(status["task_completed"], false);
    assert_eq!(status["retained_owner_acknowledged"], false);
    fs::write(root.join("pack-stdout.json"), output.stdout).unwrap();
    let result: ReturnedResult = decode(&fs::read(&result_path).unwrap()).unwrap();
    assert_eq!(result.task_key, packet.task_key);
    assert_eq!(result.attempt_id, profile.binding.admission.attempt_id);
    assert_eq!(result.provenance.mode, Mode::Handoff);
    assert_eq!(result.artifacts["report"].sha256, digest(&raw));
    assert_eq!(result.artifacts["report"].bytes, raw.len() as u64);
    let mut invalid = brief.empty_report;
    invalid.copy_sha256 = "b".repeat(64);
    let invalid_path = root.join("invalid-report.json");
    fs::write(&invalid_path, serde_json::to_vec(&invalid).unwrap()).unwrap();
    let refused = root.join("refused-result.json");
    let output = invoke("pack-review-result", Some(&invalid_path), &refused);
    assert!(!output.status.success());
    assert!(!refused.exists());
    fs::write(root.join("refused-stdout.json"), output.stdout).unwrap();
    let before = fs::read(&brief_path).unwrap();
    assert!(
        !invoke("export-review-brief", None, &brief_path)
            .status
            .success()
    );
    assert_eq!(before, fs::read(&brief_path).unwrap());
    assert_eq!(
        claims,
        fs::read(profile.admission.index_root.join("claims.json")).unwrap()
    );
    assert_eq!(waiting, fs::read(journal.join("00000.json")).unwrap());
    assert_eq!(fs::read_dir(&journal).unwrap().count(), 1);
    let outcome = json!({"root":root,"original":original,"brief_exported":true,"unit_ids":3,"exact_pretty_report_bytes_hashed":true,"stale_candidate_refused_without_output":true,"existing_brief_preserved":true,"claims_and_waiting_journal_unchanged":true,"original_runtime_not_retargeted":true,"report_is_empty_technical_fixture":true,"provider_called":false,"candidate_registered":false,"task_completed":false,"approved":false});
    fs::write(
        root.join("qualification-result.json"),
        serde_json::to_vec(&outcome).unwrap(),
    )
    .unwrap();
    fs::write(
        evidence.join("qualification-result.json"),
        serde_json::to_vec(&outcome).unwrap(),
    )
    .unwrap();
}
