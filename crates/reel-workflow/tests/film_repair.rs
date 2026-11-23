use reel_workflow::{film_repair::*, film_review, *};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, process::Command};
#[path = "support/film_review_fixture.rs"]
mod support;
use support::*;

fn fixture() -> (Plan, film_review::Report, Spec, Memory) {
    let (mut report, mut memory) = support::fixture();
    let picture = artifact(
        &mut memory,
        "picture/selected",
        b"old exact synthetic picture".to_vec(),
        "image/png",
    );
    let continuity = control(&mut memory, "continuity/card", json!({"synthetic":true}));
    let job = control(
        &mut memory,
        "scene/job",
        json!({"synthetic":"original delivery job"}),
    );
    let request = control(
        &mut memory,
        "scene/task",
        json!({"synthetic":"native task wrapper"}),
    );
    let executable = artifact(
        &mut memory,
        "tools/renderer",
        b"synthetic executable pin".to_vec(),
        "application/octet-stream",
    );
    let mut map: Value = serde_json::from_slice(&memory.files[&report.map.logical_id]).unwrap();
    map["targets"][0]["selected_picture"] =
        json!({"path":"picture.png","sha256":picture.sha256,"bytes":picture.bytes});
    map["job"] = json!({"path":"job.json","sha256":job.sha256,"bytes":job.bytes});
    report.map = control(&mut memory, "review/map", map);
    let mut receipt: Value =
        serde_json::from_slice(&memory.files[&report.receipt.logical_id]).unwrap();
    receipt["outputs"]["review-map.json"] =
        json!({"path":"review-map.json","sha256":report.map.sha256,"bytes":report.map.bytes});
    report.receipt = control(&mut memory, "review/receipt", receipt);
    let output = OutputContract {
        media_type: "video/mp4".into(),
        max_bytes: 1024,
        required: true,
    };
    let render = Task {
        id: "render-es".into(),
        native_inputs: None,
        upstream_inputs: None,
        operation: Operation::DeterministicTool,
        tool: Some(ToolContract {
            schema: "reel.workflow-tool.v1".into(),
            capability: ToolCapability::FilmRender,
            executable,
            auxiliary_executables: BTreeMap::new(),
            publication: job.clone(),
            request: request.clone(),
            edition_id: "scene-es".into(),
            source_bindings: BTreeMap::from([
                ("picture.png".into(), picture.logical_id.clone()),
                ("job.json".into(), job.logical_id.clone()),
                ("task.json".into(), request.logical_id.clone()),
                ("continuity.json".into(), continuity.logical_id.clone()),
            ]),
            part: None,
        }),
        language: Some("es".into()),
        prompt: String::new(),
        inputs: vec![picture, job, request, continuity],
        depends_on: vec![],
        outputs: BTreeMap::from([("movie".into(), output)]),
    };
    let mut en = render.clone();
    en.id = "render-en".into();
    en.language = Some("en".into());
    let mut other = render.clone();
    other.id = "different-record".into();
    other.language = Some("fr".into());
    other.inputs[0].logical_id = "picture/another-record-same-bytes".into();
    other
        .tool
        .as_mut()
        .unwrap()
        .source_bindings
        .insert("picture.png".into(), other.inputs[0].logical_id.clone());
    let consumer = |id: &str, language: &str, deps: &[&str]| Task {
        id: id.into(),
        native_inputs: None,
        upstream_inputs: None,
        operation: Operation::TextProposal,
        tool: None,
        language: Some(language.into()),
        prompt: "Synthetic downstream fixture only".into(),
        inputs: vec![],
        depends_on: deps.iter().map(|s| s.to_string()).collect(),
        outputs: BTreeMap::from([(
            "note".into(),
            OutputContract {
                media_type: "text/plain".into(),
                max_bytes: 1024,
                required: true,
            },
        )]),
    };
    let baseline = Plan {
        schema: PLAN_SCHEMA.into(),
        project: "synthetic-film".into(),
        source_commit: "b".repeat(40),
        task_snapshots: BTreeMap::new(),
        tasks: vec![
            render,
            en,
            other,
            consumer("assembly-es", "es", &["render-es"]),
            consumer("assembly-en", "en", &["render-en"]),
            consumer("portable", "es", &["assembly-es", "assembly-en"]),
        ],
    };
    let key = film_review::draft(&report, &mut memory).unwrap()[0]
        .key
        .clone();
    let spec = Spec {
        schema: "reel.workflow-film-image-repair.v1".into(),
        reviewed_task: "render-es".into(),
        finding_key: key,
        task_id: "repair-C100".into(),
        instructions: "Correct the reported shore detail, preserving continuity.".into(),
        reference_inputs: vec!["continuity/card".into()],
        native_inputs: None,
        output: OutputContract {
            media_type: "image/png".into(),
            max_bytes: 1024 * 1024,
            required: true,
        },
        input_bytes: 1024 * 1024,
        object_bytes: 1024 * 1024,
    };
    (baseline, report, spec, memory)
}
#[test]
fn exact_original_drawing_creates_an_ordinary_image_task_and_tracks_both_consuming_languages() {
    let (plan, report, spec, mut memory) = fixture();
    let before = canonical_digest(&plan).unwrap();
    let result = prepare(&plan, &report, &spec, &mut memory).unwrap();
    assert_eq!(result.old_subject, plan.tasks[0].inputs[0]);
    assert_eq!(result.finding.target.cel_id, "C100");
    assert_eq!(
        result.impact.directly_affected,
        vec!["render-en", "render-es"]
    );
    assert_eq!(result.impact.rebuild.len(), 5);
    assert_eq!(result.impact.unaffected, vec!["different-record"]);
    assert_eq!(result.impact.languages, vec!["en", "es"]);
    assert_eq!(result.packet.task.operation, Operation::ImageCandidate);
    assert_eq!(result.packet.task.inputs.len(), 2);
    assert!(
        result
            .packet
            .task
            .prompt
            .contains("Conservar el texto español.")
    );
    assert!(
        !result
            .packet
            .task
            .inputs
            .iter()
            .any(|a| a.logical_id == report.review.logical_id)
    );
    assert!(
        !result.candidate_generated
            && !result.baseline_provenance_verified
            && !result.native_retention_verified
            && !result.selection_applied
            && !result.approvals_transferred
            && !result.consumers_rebuilt
    );
    assert_eq!(before, canonical_digest(&plan).unwrap());
}
#[test]
fn stale_finding_job_record_namespace_language_and_snapshot_refuse() {
    for case in 0..7 {
        let (mut plan, report, mut spec, mut memory) = fixture();
        match case {
            0 => spec.finding_key = "f".repeat(64),
            1 => plan.source_commit = "c".repeat(40),
            2 => plan.tasks[0].language = Some("en".into()),
            3 => plan.tasks[0]
                .tool
                .as_mut()
                .unwrap()
                .source_bindings
                .remove("picture.png")
                .map(|_| ())
                .unwrap(),
            4 => plan.tasks[0].tool.as_mut().unwrap().publication.sha256 = "f".repeat(64),
            5 => plan.tasks[0].inputs[0].sha256 = "f".repeat(64),
            _ => {
                spec.finding_key = film_review::draft(&report, &mut memory).unwrap()[1]
                    .key
                    .clone()
            }
        }
        // Keep ordinary task references internally consistent so refusal tests
        // exercise review binding rather than unrelated plan validation.
        if case == 4 {
            plan.tasks[0].inputs[1].sha256 = "f".repeat(64);
        }
        let error = prepare(&plan, &report, &spec, &mut memory).err().unwrap();
        assert!(
            matches!(
                error.kind,
                ErrorKind::Stale | ErrorKind::Missing | ErrorKind::Unsupported
            ),
            "{case}: {error}"
        );
    }
}
#[test]
fn only_explicit_declared_references_and_input_allowances_can_enter_the_candidate() {
    let (plan, report, mut spec, mut memory) = fixture();
    spec.reference_inputs = vec!["unrelated/private-file".into()];
    assert_eq!(
        prepare(&plan, &report, &spec, &mut memory)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Denied
    );
    spec.reference_inputs = vec![plan.tasks[0].inputs[0].logical_id.clone()];
    assert_eq!(
        prepare(&plan, &report, &spec, &mut memory)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Conflict
    );
    spec.reference_inputs.clear();
    spec.input_bytes = 1;
    spec.object_bytes = 1;
    assert_eq!(
        prepare(&plan, &report, &spec, &mut memory)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Budget
    );
}
#[test]
fn scoped_baselines_use_the_reviewed_tasks_exact_historical_snapshot() {
    let (mut plan, report, spec, mut memory) = fixture();
    plan.schema = SCOPED_PLAN_SCHEMA.into();
    plan.task_snapshots = plan
        .tasks
        .iter()
        .map(|t| (t.id.clone(), plan.source_commit.clone()))
        .collect();
    plan.source_commit = "c".repeat(40);
    assert!(prepare(&plan, &report, &spec, &mut memory).is_ok());
    plan.task_snapshots
        .insert("render-es".into(), "d".repeat(40));
    assert_eq!(
        prepare(&plan, &report, &spec, &mut memory)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Stale
    );
}
#[test]
fn cli_package_enters_existing_manual_capture_and_preserves_existing_or_partial_packages() {
    let (baseline, report, repair, memory) = fixture();
    let root = tempfile::tempdir().unwrap();
    let mut files = BTreeMap::new();
    for (index, (id, bytes)) in memory.files.iter().enumerate() {
        let path = root.path().join(format!("input-{index}"));
        fs::write(&path, bytes).unwrap();
        files.insert(id.clone(), path);
    }
    let request = Request {
        schema: "reel.workflow-film-image-repair-export.v1".into(),
        baseline,
        report,
        repair,
        attempt_id: "attempt-1".into(),
        files,
    };
    let raw = serde_json::to_vec(&request).unwrap();
    let path = root.path().join("request.json");
    fs::write(&path, &raw).unwrap();
    let destination = root.path().join("repair");
    let invoke = |pin: &str| {
        Command::new(env!("CARGO_BIN_EXE_reel-workflow"))
            .arg("prepare-film-repair")
            .arg(&path)
            .arg(pin)
            .arg(&destination)
            .output()
            .unwrap()
    };
    assert!(!invoke(&"f".repeat(64)).status.success());
    assert!(!destination.exists());
    let output = invoke(&digest(&raw));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let packet: Packet = manual::read_control(&destination.join("work/packet.json")).unwrap();
    let picture = root.path().join("new-picture.png");
    fs::write(&picture, b"synthetic returned candidate bytes").unwrap();
    let candidate = root.path().join("candidate");
    let bindings = BTreeMap::from([("picture".into(), picture)]);
    assert!(
        !manual::capture_work(
            &destination.join("work"),
            "attempt-1",
            &bindings,
            &candidate
        )
        .unwrap()
    );
    assert!(
        manual::capture_work(
            &destination.join("work"),
            "attempt-1",
            &bindings,
            &candidate
        )
        .unwrap()
    );
    assert_eq!(packet.task.id, "repair-C100");
    let old = fs::read(destination.join("prepared.json")).unwrap();
    assert!(!invoke(&digest(&raw)).status.success());
    assert_eq!(fs::read(destination.join("prepared.json")).unwrap(), old);
    fs::remove_file(destination.join("prepared.json")).unwrap();
    assert!(!invoke(&digest(&raw)).status.success());
    assert!(!destination.join("prepared.json").exists());
}
