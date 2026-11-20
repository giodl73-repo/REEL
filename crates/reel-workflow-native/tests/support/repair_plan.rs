//! Existing retained report -> bounded private candidate plan, no execution.
use super::*;

#[test]
#[ignore = "Existing synthetic retained report only; candidate repair export, no admission, rendering, selection or closure"]
fn retained_finding_exports_a_revised_plan_and_refuses_a_stale_report() {
    let original = PathBuf::from(
        std::env::var_os("REEL_REPAIR_REPORT_FIXTURE").expect("Set retained report root"),
    );
    let executable =
        PathBuf::from(std::env::var_os("REEL_NATIVE_HANDOFF_TOOL").expect("Set pinned CLI"));
    let evidence = PathBuf::from(
        std::env::var_os("REEL_REPAIR_EVIDENCE_DIRECTORY").expect("Set evidence directory"),
    );
    let packet: Packet = decode(&fs::read(original.join("packet.json")).unwrap()).unwrap();
    let source = decode(&fs::read(original.join("source.json")).unwrap()).unwrap();
    let profile: reel_workflow_native::admission::Profile =
        decode(&fs::read(original.join("admission.json")).unwrap()).unwrap();
    let receipt = decode(&fs::read(original.join("receipt.json")).unwrap()).unwrap();
    let result = decode(&fs::read(original.join("result.json")).unwrap()).unwrap();
    let ack = decode(&fs::read(original.join("ack.json")).unwrap()).unwrap();
    let guard = reel_workflow_native::retention::verify(
        &packet, &source, &profile, &receipt, &result, &ack,
    )
    .unwrap();
    let checked = guard.review_report().unwrap();
    let before = checked.copy().publication.clone();
    let after_bytes = br#"{"synthetic":"corrected publication candidate"}"#;
    let after = Artifact {
        logical_id: before.logical_id.clone(),
        sha256: digest(after_bytes),
        bytes: after_bytes.len() as u64,
        media_type: before.media_type.clone(),
    };
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        task_snapshots: Default::default(),
        project: checked.copy().project.clone(),
        source_commit: checked.copy().source_commit.clone(),
        tasks: vec![Task {
            id: "synthetic-consumer".into(),
            native_inputs: None,
            upstream_inputs: None,
            operation: Operation::TextProposal,
            tool: None,
            language: Some(checked.copy().language.clone()),
            prompt: "Synthetic repair planning fixture".into(),
            inputs: vec![before.clone()],
            depends_on: vec![],
            outputs: BTreeMap::from([(
                "candidate".into(),
                OutputContract {
                    media_type: "application/json".into(),
                    max_bytes: 1024,
                    required: true,
                },
            )]),
        }],
    };
    let mut repair = review::Repair {
        roots: None,
        schema: "reel.workflow-repair.v1".into(),
        report_sha256: checked.report_sha256().into(),
        finding_keys: checked.findings().keys().cloned().collect(),
        edits: vec![review::Edit {
            before,
            after: after.clone(),
        }],
    };
    let revision = repair::Revision {
        schema: "reel.workflow-repair-revision.v1".into(),
        source_commit: "b".repeat(40),
        native_inputs: BTreeMap::new(),
    };
    let expected = repair::materialize(&plan, checked, &repair, &revision)
        .unwrap()
        .plan;
    drop(guard);
    let mut temp = tempfile::tempdir().unwrap();
    temp.disable_cleanup(true);
    let root = temp.path();
    fs::write(root.join("candidate-publication.json"), after_bytes).unwrap();
    let marker =
        json!({"root":root,"original":original,"task_key":packet.task_key,"runtime":executable});
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
    for (name, value) in [
        ("plan", serde_json::to_vec(&plan).unwrap()),
        ("repair", serde_json::to_vec(&repair).unwrap()),
        ("revision", serde_json::to_vec(&revision).unwrap()),
    ] {
        fs::write(root.join(name), value).unwrap();
    }
    let claims = fs::read(profile.index_root.join("claims.json")).unwrap();
    let invoke = |path: &Path| {
        let mut child = Command::new(&executable);
        child.arg("export-repair-plan");
        for name in [
            "packet.json",
            "source.json",
            "admission.json",
            "receipt.json",
            "result.json",
            "ack.json",
        ] {
            child.arg(original.join(name));
        }
        for name in ["plan", "repair", "revision"] {
            child.arg(root.join(name));
        }
        child.arg(path).arg("--apply").output().unwrap()
    };
    let output_path = root.join("candidate-plan.json");
    let output = invoke(&output_path);
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fs::write(root.join("export-stdout.json"), output.stdout).unwrap();
    let exported: Plan = decode(&fs::read(&output_path).unwrap()).unwrap();
    assert_eq!(exported, expected);
    assert_eq!(exported.tasks[0].inputs, [after]);
    check_plan(&exported).unwrap();
    repair.report_sha256 = "f".repeat(64);
    fs::write(root.join("repair"), serde_json::to_vec(&repair).unwrap()).unwrap();
    let refused = root.join("refused-plan.json");
    let output = invoke(&refused);
    assert!(!output.status.success());
    assert!(!refused.exists());
    fs::write(root.join("refused-stdout.json"), output.stdout).unwrap();
    assert_eq!(
        claims,
        fs::read(profile.index_root.join("claims.json")).unwrap()
    );
    let outcome = json!({"root":root,"original":original,"retained_finding_bound":true,"exact_replacement_exported":true,"candidate_passes_plan_checks":true,"stale_report_refused_before_output":true,"claims_unchanged":true,"new_snapshot_verified":false,"production_admitted":false,"rendered":false,"selected":false,"findings_closed":false});
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
