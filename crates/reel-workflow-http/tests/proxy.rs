use reel_workflow::*;
use reel_workflow_http::proxy::Spec;
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub fn fixture_spec() -> Spec {
    let plan:Plan=decode(&serde_json::to_vec(&json!({"schema":PLAN_SCHEMA,"project":"synthetic-film","source_commit":"a".repeat(40),"tasks":[{"id":"note","operation":"text-proposal","language":"es","prompt":"Return a synthetic editorial note.","inputs":[],"depends_on":[],"outputs":{"note.txt":{"media_type":"text/plain","max_bytes":1024,"required":true},"extra.txt":{"media_type":"text/plain","max_bytes":8,"required":false}}}]})).unwrap()).unwrap();
    Spec {
        packet: make_packet(&plan, "note", BTreeMap::new()).unwrap(),
        attempt_id: "attempt.1".into(),
        provider: "synthetic".into(),
        model: "synthetic-v1".into(),
    }
}
pub fn reply(spec: &Spec) -> Value {
    json!({"schema":"reel.workflow-proxy-response.v1","operation_id":spec.operation_id().unwrap(),"provider":spec.provider,"model":spec.model,"outputs":{"note.txt":{"media_type":"text/plain","base64":"aGVsbG8="}}})
}
#[test]
fn direct_and_manual_candidates_use_the_same_exact_task_and_byte_contract() {
    let spec = fixture_spec();
    let candidate = spec
        .normalize(&serde_json::to_vec(&reply(&spec)).unwrap())
        .unwrap();
    assert_eq!(candidate.result.provenance.mode, Mode::Direct);
    check_output(&spec.packet, &candidate.result, "note.txt", b"hello").unwrap();
    let mut manual = candidate.result.clone();
    manual.provenance = Provenance {
        mode: Mode::Handoff,
        provider: None,
        model: None,
        executable: None,
    };
    check_output(&spec.packet, &manual, "note.txt", b"hello").unwrap();
    let mut other = fixture_spec();
    other.attempt_id = "attempt.2".into();
    assert_eq!(
        other
            .normalize(&serde_json::to_vec(&reply(&spec)).unwrap())
            .err()
            .unwrap()
            .kind,
        ErrorKind::Conflict
    );
    let mut changed = fixture_spec();
    changed.model = "other-v2".into();
    assert_eq!(
        changed.operation_id().unwrap(),
        spec.operation_id().unwrap()
    );
    assert!(
        changed
            .normalize(&serde_json::to_vec(&reply(&spec)).unwrap())
            .is_err()
    );
}
#[test]
fn proxy_refuses_duplicate_unknown_missing_and_over_budget_outputs() {
    let spec = fixture_spec();
    let original = reply(&spec);
    for value in [
        {
            let mut v = original.clone();
            v["outputs"] = json!({});
            v
        },
        {
            let mut v = original.clone();
            v["outputs"]["unknown.txt"] = v["outputs"]["note.txt"].clone();
            v
        },
        {
            let mut v = original.clone();
            v["outputs"]["note.txt"]["media_type"] = json!("image/png");
            v
        },
        {
            let mut v = original.clone();
            v["outputs"]["note.txt"]["base64"] = json!("bad!");
            v
        },
        {
            let mut v = original.clone();
            v["outputs"]["extra.txt"] =
                json!({"media_type":"text/plain","base64":"MDEyMzQ1Njc4OQ=="});
            v
        },
    ] {
        assert!(
            spec.normalize(&serde_json::to_vec(&value).unwrap())
                .is_err()
        );
    }
    let original = serde_json::to_string(&original).unwrap();
    let duplicate = original.replace(
        "\"outputs\":{",
        "\"outputs\":{\"note.txt\":{\"media_type\":\"text/plain\",\"base64\":\"aGVsbG8=\"},",
    );
    assert_eq!(
        spec.normalize(duplicate.as_bytes()).err().unwrap().code,
        "proxy-response-json"
    );
}
#[test]
fn only_explicit_matching_inputs_are_encoded_and_voice_operations_require_an_adapter() {
    let mut spec = fixture_spec();
    let artifact = Artifact {
        logical_id: "reference/text".into(),
        sha256: digest(b"reference"),
        bytes: 9,
        media_type: "text/plain".into(),
    };
    let mut plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: spec.packet.project.clone(),
        source_commit: spec.packet.source_commit.clone(),
        task_snapshots: BTreeMap::new(),
        tasks: vec![spec.packet.task.clone()],
    };
    plan.tasks[0].inputs.push(artifact);
    spec.packet = make_packet(&plan, "note", BTreeMap::new()).unwrap();
    let unsent: Value = serde_json::from_slice(&spec.compose(&BTreeMap::new()).unwrap()).unwrap();
    assert_eq!(unsent["inputs"], json!({}));
    let sent: Value = serde_json::from_slice(
        &spec
            .compose(&BTreeMap::from([(
                "reference/text".into(),
                b"reference".to_vec(),
            )]))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(sent["inputs"]["reference/text"]["base64"], "cmVmZXJlbmNl");
    assert!(
        spec.compose(&BTreeMap::from([("other".into(), vec![1])]))
            .is_err()
    );
    assert!(
        spec.compose(&BTreeMap::from([("reference/text".into(), vec![1])]))
            .is_err()
    );
    plan.tasks[0].operation = Operation::SpeechCandidate;
    spec.packet = make_packet(&plan, "note", BTreeMap::new()).unwrap();
    assert_eq!(spec.check().unwrap_err().kind, ErrorKind::Unsupported);
}
