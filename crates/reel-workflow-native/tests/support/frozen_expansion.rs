//! Git metadata and claim lifecycle only; no native payload availability.
use super::*;
use reel_workflow::expansion::{Input, TaskTemplate, Template};
use reel_workflow_native::{
    expansion::{self, Frozen},
    graph,
};

fn prepare() -> (Fixture, AdmissionCase, Plan, Frozen) {
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let task = &fixture.packet.task;
    let base = Plan {
        schema: PLAN_SCHEMA.into(),
        project: fixture.packet.project.clone(),
        source_commit: fixture.packet.source_commit.clone(),
        tasks: vec![task.clone()],
    };
    let template = Template {
        schema: "reel.workflow-expansion-template.v1".into(),
        base_plan_sha256: canonical_digest(&base).unwrap(),
        task: TaskTemplate {
            id: "expanded".into(),
            operation: task.operation,
            language: task.language.clone(),
            prompt: task.prompt.clone(),
            depends_on: vec![],
            inputs: task
                .inputs
                .iter()
                .map(|a| Input::Fixed {
                    artifact: a.clone(),
                })
                .collect(),
            outputs: task.outputs.clone(),
            tool: None,
        },
    };
    let bindings = graph::Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::new(),
    };
    let source = serde_json::from_slice(&serde_json::to_vec(&case.source).unwrap()).unwrap();
    let before = fs::read(case.profile.metadata_root.join("metadata.json")).unwrap();
    let frozen = expansion::prepare(
        &base,
        &bindings,
        &template,
        source,
        case.profile.clone(),
        "attempt-1",
    )
    .unwrap();
    assert_eq!(
        fs::read(case.profile.metadata_root.join("metadata.json")).unwrap(),
        before
    );
    assert!(
        case.protected(&Object::from_bytes(b"synthetic input"))
            .is_empty()
    );
    (fixture, case, base, frozen)
}
#[test]
fn expanded_source_freeze_accept_and_original_receipt_recovery_preserve_one_operation() {
    let (_fixture, case, base, frozen) = prepare();
    assert!(frozen.expansion.packet.task.native_inputs.is_some());
    assert_eq!(frozen.preview.total_actor_limit, 16);
    assert_eq!(frozen.preview.changed_path_limit, 128);
    let output = case.temp.path().join("frozen.json");
    frozen.export(&output).unwrap();
    assert_eq!(
        expansion::read_pinned(&output, &"0".repeat(64))
            .err()
            .unwrap()
            .kind,
        ErrorKind::Integrity
    );
    let (frozen, _pin): (Frozen, _) =
        expansion::read_pinned(&output, &digest(&fs::read(&output).unwrap())).unwrap();
    frozen.check(&base).unwrap();
    let receipt = frozen
        .accept_to(&base, &case.temp.path().join("accepted.json"))
        .unwrap();
    assert_eq!(
        case.protected(&Object::from_bytes(b"synthetic input")),
        vec![case.profile.scope.actor.clone()]
    );
    let recovered = frozen
        .recover_to(&base, &case.temp.path().join("recovered.json"))
        .unwrap();
    assert_eq!(
        receipt.canonical_bytes().unwrap(),
        recovered.canonical_bytes().unwrap()
    );
    assert_eq!(receipt.work, recovered.work);
    assert_eq!(receipt.activation, recovered.activation);
    assert_eq!(frozen.attempt_id, recovered.attempt_id);
}
#[test]
fn frozen_expansion_refuses_changed_base_preview_packet_and_existing_outputs() {
    let (_fixture, case, mut base, mut frozen) = prepare();
    let output = case.temp.path().join("existing.json");
    fs::write(&output, b"preserve original").unwrap();
    assert_eq!(
        frozen.export(&output).unwrap_err().kind,
        ErrorKind::Conflict
    );
    assert_eq!(fs::read(&output).unwrap(), b"preserve original");
    assert_eq!(
        frozen
            .export(&case.profile.index_root.join("forbidden.json"))
            .unwrap_err()
            .kind,
        ErrorKind::Denied
    );
    base.tasks[0].prompt.push_str(" changed");
    assert_eq!(frozen.accept(&base).unwrap_err().kind, ErrorKind::Stale);
    let original_len = base.tasks[0].prompt.len() - 8;
    base.tasks[0].prompt.truncate(original_len);
    frozen.preview.changed_path_limit += 1;
    assert_eq!(
        frozen.accept(&base).unwrap_err().code,
        "frozen-expansion-preview"
    );
    assert!(
        case.protected(&Object::from_bytes(b"synthetic input"))
            .is_empty()
    );
    frozen.preview.changed_path_limit -= 1;
    frozen.expansion.packet.task.prompt.push_str(" changed");
    assert_eq!(frozen.accept(&base).unwrap_err().kind, ErrorKind::Stale);
    assert!(
        case.protected(&Object::from_bytes(b"synthetic input"))
            .is_empty()
    );
}
