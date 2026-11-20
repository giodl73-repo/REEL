//! Explicit read-only retained dependency stream qualification.
use super::*;
use reel_workflow_native::{SnapshotBudget, handoff, retention, upstream};
use sha2::{Digest as _, Sha256};
use tig_store::source_claims::protection::Index;

fn load<T: serde::de::DeserializeOwned>(path: &Path, held: &mut Vec<fs::File>) -> T {
    let (raw, guard) = reel_workflow_native::read_candidate_control(path).unwrap();
    held.push(guard);
    decode(&raw).unwrap()
}
fn pin(id: &str, raw: &[u8]) -> Artifact {
    Artifact {
        logical_id: id.into(),
        sha256: digest(raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    }
}
#[test]
#[ignore = "Read-only existing retained synthetic comic parent; exact dependency stream and Index checkpoint checks, no child admission, publication or new capacity"]
fn retained_parent_stream_preserves_exact_origin_native_guards_and_index_checkpoint() {
    let root = PathBuf::from(
        std::env::var_os("REEL_UPSTREAM_PARENT_ROOT").expect("Set original retained parent root"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_UPSTREAM_EVIDENCE_DIRECTORY").expect("Set owned evidence directory"),
    );
    assert!(root.is_absolute() && evidence.is_absolute() && evidence.is_dir());
    let mut controls = Vec::new();
    let packet: Packet = load(&root.join("packet.json"), &mut controls);
    let profile: handoff::Profile = load(&root.join("handoff-profile.json"), &mut controls);
    let result: ReturnedResult = load(&root.join("result.json"), &mut controls);
    let ack: retention::Acknowledgement = load(&root.join("ack.json"), &mut controls);
    let before = Index::open(
        &profile.admission.index_root,
        &profile.admission.index_minimum,
    )
    .unwrap()
    .checkpoint();
    let (output, asset) = result
        .artifacts
        .iter()
        .max_by_key(|(_, a)| a.bytes)
        .unwrap();
    assert!(asset.bytes > 0 && asset.bytes <= 16 * 1024 * 1024);
    let parents = BTreeMap::from([(
        packet.task.id.clone(),
        upstream::Parent {
            packet: &packet,
            result: &result,
            source: &profile.source,
            admission: &profile.admission,
            receipt: &profile.binding.admission,
            acknowledgement: &ack,
        },
    )]);
    let bindings = reel_workflow::upstream::Bindings {
        schema: "reel.workflow-upstream-inputs.v3".into(),
        streaming: Some(reel_workflow::upstream::Streaming {
            schema: "reel.workflow-upstream-streaming.v1".into(),
            input_bytes: 4 * 1024 * 1024 * 1024,
            input_object_bytes: 4 * 1024 * 1024 * 1024,
        }),
        inputs: BTreeMap::from([(
            asset.logical_id.clone(),
            reel_workflow::upstream::Reference {
                task_id: packet.task.id.clone(),
                artifact: reel_workflow::upstream::Selector::Output {
                    name: output.clone(),
                },
            },
        )]),
    };
    let mut snapshot = profile.source.snapshot.clone();
    snapshot.schema = "reel.workflow-native-composite-snapshot.v2".into();
    snapshot.records.clear();
    snapshot.definitions.clear();
    snapshot.bindings.clear();
    snapshot.input_streams.clear();
    snapshot.upstream = Some(upstream::Frozen::capture(&bindings, &parents));
    snapshot.budget = Some(SnapshotBudget {
        schema: "reel.workflow-snapshot-budget.v1".into(),
        input_bytes: 4 * 1024 * 1024 * 1024,
        input_object_bytes: 4 * 1024 * 1024 * 1024,
        projection_bytes: 8 * 1024 * 1024 * 1024,
        returns: None,
    });
    let placeholder = Task {
        id: packet.task.id.clone(),
        operation: Operation::TextProposal,
        tool: None,
        language: packet.task.language.clone(),
        prompt: "Synthetic dependency placeholder.".into(),
        inputs: vec![],
        native_inputs: None,
        upstream_inputs: None,
        depends_on: vec![],
        outputs: BTreeMap::from([(
            "note".into(),
            OutputContract {
                media_type: "text/plain".into(),
                max_bytes: 100,
                required: true,
            },
        )]),
    };
    let mut task = placeholder.clone();
    task.id = "streamed-dependency-read".into();
    task.inputs = vec![asset.clone()];
    task.depends_on = vec![packet.task.id.clone()];
    task.upstream_inputs = Some(pin(
        "control/streaming-upstream",
        &bindings.canonical_bytes().unwrap(),
    ));
    task.native_inputs = Some(pin(
        "control/native-binding",
        &snapshot.declaration_bytes().unwrap(),
    ));
    let child = make_packet(
        &Plan {
            schema: PLAN_SCHEMA.into(),
            task_snapshots: Default::default(),
            project: packet.project.clone(),
            source_commit: packet.source_commit.clone(),
            tasks: vec![placeholder, task.clone()],
        },
        &task.id,
        BTreeMap::from([(
            packet.task.id.clone(),
            DependencyResult {
                result_sha256: canonical_digest(&result).unwrap(),
                artifacts: result.artifacts.clone(),
            },
        )]),
    )
    .unwrap();
    assert_eq!(
        SnapshotInputs::open(&child, &snapshot)
            .unwrap()
            .input_count(),
        1
    );
    let mut borrowed = upstream::BorrowedInputs::open(&child, &bindings, parents).unwrap();
    assert_eq!(
        borrowed
            .with_stream(asset, asset.bytes - 1, |_| Ok(()))
            .unwrap_err()
            .kind,
        ErrorKind::Budget
    );
    let (actual, length) = borrowed
        .with_stream(asset, asset.bytes, |reader| {
            let mut hash = Sha256::new();
            let mut length = 0u64;
            let mut buffer = [0u8; 65536];
            loop {
                let count = match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        return Err(Error {
                            kind: ErrorKind::NeedsRecovery,
                            code: "fixture-stream-io".into(),
                        });
                    }
                };
                length += count as u64;
                hash.update(&buffer[..count]);
            }
            Ok((
                hash.finalize()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
                length,
            ))
        })
        .unwrap();
    assert_eq!(actual, asset.sha256);
    assert_eq!(length, asset.bytes);
    assert_eq!(
        borrowed
            .with_stream(asset, asset.bytes, |_| Ok(()))
            .unwrap_err()
            .code,
        "upstream-stream-alias-requires-local-reuse"
    );
    drop(borrowed);
    let after = Index::open(&profile.admission.index_root, &before)
        .unwrap()
        .checkpoint();
    assert_eq!(before, after);
    let report = serde_json::to_vec_pretty(
        &json!({"schema":"reel.workflow-upstream-stream-qualification.v1",
        "parent_task_key":packet.task_key,"child_task_key":child.task_key,"input":asset,
        "index_before":before,"index_after":after,"frozen_snapshot_resolved":true,
        "native_child_admitted":false,"publication":false,"new_capacity":false}),
    )
    .unwrap();
    use std::io::Write;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(evidence.join("stream-result.json"))
        .unwrap();
    output.write_all(&report).unwrap();
    output.sync_all().unwrap();
}
