use reel_workflow::*;
use std::collections::BTreeMap;
pub const NOTE: &[u8] = b"synthetic returned text\n";
pub fn artifact(id: &str, bytes: &[u8], media: &str) -> Artifact {
    Artifact {
        logical_id: id.into(),
        sha256: digest(bytes),
        bytes: bytes.len() as u64,
        media_type: media.into(),
    }
}
pub fn returned(commit: &str, inputs: Vec<Artifact>) -> (handoff::Ticket, Vec<u8>) {
    let task = Task {
        id: "review".into(),
        operation: Operation::ReviewFinding,
        tool: None,
        language: Some("en".into()),
        prompt: "Review a synthetic input.".into(),
        inputs,
        native_inputs: None,
        upstream_inputs: None,
        depends_on: vec![],
        outputs: BTreeMap::from([(
            "note".into(),
            OutputContract {
                media_type: "text/plain".into(),
                max_bytes: 1024,
                required: true,
            },
        )]),
    };
    let packet = make_packet(
        &Plan {
            schema: PLAN_SCHEMA.into(),
            task_snapshots: BTreeMap::new(),
            project: "synthetic".into(),
            source_commit: commit.into(),
            tasks: vec![task],
        },
        "review",
        BTreeMap::new(),
    )
    .unwrap();
    let result = ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: "attempt-1".into(),
        artifacts: BTreeMap::from([(
            "note".into(),
            artifact("candidate/note", NOTE, "text/plain"),
        )]),
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
        worker_receipt: None,
    };
    let ticket = handoff::Ticket {
        packet,
        attempt_id: result.attempt_id.clone(),
        native_binding: artifact("native/original", b"original admission", "application/json"),
        lifecycle_tool: artifact(
            "tools/lifecycle",
            b"qualified synthetic lifecycle",
            "application/octet-stream",
        ),
    };
    let mut raw = serde_json::to_vec_pretty(&result).unwrap();
    raw.extend_from_slice(b"\n\n");
    (ticket, raw)
}
