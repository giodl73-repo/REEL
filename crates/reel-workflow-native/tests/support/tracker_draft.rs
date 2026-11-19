//! Native retained report -> explicit minimal issue draft, without sending it.
use super::*;
#[test]
#[ignore = "Existing retained synthetic structured report only; private draft export, no tracker or native publication effects"]
fn issue_draft_exports_only_explicit_text_from_an_actual_retained_finding() {
    let original = PathBuf::from(
        std::env::var_os("REEL_TRACKER_REPORT_FIXTURE").expect("Set retained report root"),
    );
    let executable =
        PathBuf::from(std::env::var_os("REEL_NATIVE_HANDOFF_TOOL").expect("Set pinned CLI"));
    let evidence = PathBuf::from(
        std::env::var_os("REEL_TRACKER_DRAFT_EVIDENCE_DIRECTORY").expect("Set evidence directory"),
    );
    let packet: Packet = decode(&fs::read(original.join("packet.json")).unwrap()).unwrap();
    let source: reel_workflow_native::NativeProfile =
        decode(&fs::read(original.join("source.json")).unwrap()).unwrap();
    let profile: reel_workflow_native::admission::Profile =
        decode(&fs::read(original.join("admission.json")).unwrap()).unwrap();
    let receipt = decode(&fs::read(original.join("receipt.json")).unwrap()).unwrap();
    let result = decode(&fs::read(original.join("result.json")).unwrap()).unwrap();
    let ack = decode(&fs::read(original.join("ack.json")).unwrap()).unwrap();
    let guard = reel_workflow_native::retention::verify(
        &packet, &source, &profile, &receipt, &result, &ack,
    )
    .unwrap();
    let report = guard.review_report().unwrap();
    let key = report.findings().keys().next().unwrap().clone();
    drop(guard);
    let mut temp = tempfile::tempdir().unwrap();
    temp.disable_cleanup(true);
    let root = temp.path();
    let marker = json!({"root":root,"original":original,"task_key":packet.task_key,"finding_key":key,"runtime":executable});
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
    let routes = review::Routes {
        project: "example/project".into(),
        reel: "example/reel".into(),
        tig: "example/tig".into(),
    };
    let mut text = tracker::Text {
        schema: "reel.workflow-issue-text.v1".into(),
        finding_key: key,
        repository: "example/project".into(),
        title: "Synthetic issue export".into(),
        body: "Explicit synthetic reproduction text only.".into(),
        synthetic_reproduction: None,
    };
    let routes_path = root.join("routes.json");
    let text_path = root.join("text.json");
    fs::write(&routes_path, serde_json::to_vec(&routes).unwrap()).unwrap();
    fs::write(&text_path, serde_json::to_vec(&text).unwrap()).unwrap();
    let claims = fs::read(profile.index_root.join("claims.json")).unwrap();
    let invoke = |output: &Path| {
        let mut child = Command::new(&executable);
        child.arg("export-issue-draft");
        for file in [
            "packet.json",
            "source.json",
            "admission.json",
            "receipt.json",
            "result.json",
            "ack.json",
        ] {
            child.arg(original.join(file));
        }
        child
            .arg(&routes_path)
            .arg(&text_path)
            .arg(output)
            .arg("--apply")
            .output()
            .unwrap()
    };
    let path = root.join("draft.json");
    let output = invoke(&path);
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fs::write(root.join("export-stdout.json"), output.stdout).unwrap();
    let draft: tracker::Draft = decode(&fs::read(&path).unwrap()).unwrap();
    draft.check().unwrap();
    assert_eq!(draft.title, text.title);
    assert_eq!(draft.body, text.body);
    let payload = draft.payload().unwrap();
    let payload: serde_json::Value = decode(&payload).unwrap();
    assert!(payload.get("report_sha256").is_none());
    assert!(payload.get("finding_key").is_none());
    assert!(payload.get("copy").is_none());
    assert_eq!(payload["repository"], "example/project");
    text.repository = "example/other".into();
    fs::write(&text_path, serde_json::to_vec(&text).unwrap()).unwrap();
    let refused = root.join("refused.json");
    let output = invoke(&refused);
    assert!(!output.status.success());
    assert!(!refused.exists());
    fs::write(root.join("refused-stdout.json"), output.stdout).unwrap();
    assert_eq!(
        claims,
        fs::read(profile.index_root.join("claims.json")).unwrap()
    );
    let outcome = json!({"root":root,"original":original,"retained_finding_bound":true,"explicit_text_only":true,"private_context_not_forwarded":true,"wrong_repository_refused_without_output":true,"claims_unchanged":true,"tracker_called":false,"task_completed":false,"approved":false});
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
