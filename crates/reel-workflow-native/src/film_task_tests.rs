use super::*;
use reel_workflow::{Operation, OutputContract, Plan, Task, ToolContract, make_packet};

struct Controls(BTreeMap<String, Vec<u8>>);
impl InputReader for Controls {
    fn read(&mut self, artifact: &Artifact, _: u64) -> Result<Vec<u8>> {
        self.0
            .get(&artifact.logical_id)
            .cloned()
            .ok_or_else(|| error(ErrorKind::Missing, "fixture-control"))
    }
}
fn fixture(change: impl FnOnce(&mut Request)) -> (Packet, Controls) {
    let mut request = Request {
        schema: "reel.workflow-film-task.v1".into(),
        job_path: "job.json".into(),
        production_path: "production.json".into(),
        output_paths: BTreeMap::from([
            ("execution-request".into(), "render-request.json".into()),
            ("execution-result".into(), "render-result.json".into()),
            ("delivery-receipt".into(), "delivery/receipt.json".into()),
            ("master".into(), "delivery/master.mkv".into()),
        ]),
        limits: Limits {
            timeout_seconds: 120,
            memory_bytes: 1024 * 1024 * 1024,
            processes: 8,
            workspace_bytes: 64 * 1024 * 1024,
            output_bytes: 32 * 1024 * 1024,
            frames: 24,
            samples: 48000,
            raw_rgb_bytes: 1024 * 1024,
        },
    };
    change(&mut request);
    let controls = Controls(BTreeMap::from([
        (
            "controls/request".into(),
            serde_json::to_vec(&request).unwrap(),
        ),
        ("controls/job".into(), b"{}".to_vec()),
        ("controls/production".into(), b"{}".to_vec()),
    ]));
    let inputs: BTreeMap<_, _> = controls
        .0
        .iter()
        .map(|(id, raw)| (id.clone(), artifact(id.clone(), raw)))
        .collect();
    let executable = artifact("runtime/producer".into(), b"fixture runtime");
    let task = Task {
        id: "scene".into(),
        native_inputs: None,
        upstream_inputs: None,
        operation: Operation::DeterministicTool,
        language: Some("es".into()),
        prompt: String::new(),
        inputs: inputs.values().cloned().collect(),
        depends_on: vec![],
        tool: Some(ToolContract {
            schema: "reel.workflow-tool.v1".into(),
            capability: ToolCapability::FilmRender,
            executable: executable.clone(),
            auxiliary_executables: BTreeMap::from([
                ("ffmpeg".into(), executable.clone()),
                ("ffprobe".into(), executable),
            ]),
            publication: inputs["controls/job"].clone(),
            request: inputs["controls/request"].clone(),
            edition_id: "synthetic".into(),
            source_bindings: BTreeMap::from([
                ("job.json".into(), "controls/job".into()),
                ("production.json".into(), "controls/production".into()),
                ("request.json".into(), "controls/request".into()),
            ]),
            part: None,
        }),
        outputs: request
            .output_paths
            .iter()
            .map(|(name, path)| {
                (
                    name.clone(),
                    OutputContract {
                        media_type: media(path).unwrap_or("application/octet-stream").into(),
                        max_bytes: 1024 * 1024,
                        required: true,
                    },
                )
            })
            .collect(),
    };
    (
        make_packet(
            &Plan {
                schema: reel_workflow::PLAN_SCHEMA.into(),
                project: "synthetic".into(),
                source_commit: "a".repeat(40),
                task_snapshots: BTreeMap::new(),
                tasks: vec![task],
            },
            "scene",
            BTreeMap::new(),
        )
        .unwrap(),
        controls,
    )
}
#[test]
fn task_binding_checks_complete_evidence_paths_and_exact_controls_without_dispatch() {
    let (packet, mut reader) = fixture(|_| {});
    assert_eq!(
        bind(&packet, &mut reader).unwrap().task_key,
        packet.task_key
    );
    reader.0.insert("controls/job".into(), b"changed".to_vec());
    assert_eq!(
        bind(&packet, &mut reader).err().unwrap().code,
        "film-task-control-pin"
    );
}
#[test]
fn task_binding_refuses_unsafe_paths_missing_evidence_and_byte_identical_aliases() {
    for path in ["../master.mkv", "delivery/NUL.mkv", "delivery/receipt.json"] {
        let (packet, mut reader) = fixture(|request| {
            request.output_paths.insert("master".into(), path.into());
        });
        assert!(bind(&packet, &mut reader).is_err(), "{path}");
    }
    let (packet, mut reader) = fixture(|request| {
        request.output_paths.remove("execution-result");
    });
    assert_eq!(
        bind(&packet, &mut reader).err().unwrap().code,
        "film-task-required-evidence"
    );
    let (mut packet, mut reader) = fixture(|_| {});
    let tool = packet.task.tool.as_mut().unwrap();
    tool.source_bindings
        .insert("job.json".into(), "controls/production".into());
    tool.source_bindings
        .insert("production.json".into(), "controls/job".into());
    packet = make_packet(
        &Plan {
            schema: reel_workflow::PLAN_SCHEMA.into(),
            project: packet.project,
            source_commit: packet.source_commit,
            task_snapshots: BTreeMap::new(),
            tasks: vec![packet.task],
        },
        "scene",
        BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(
        bind(&packet, &mut reader).err().unwrap().code,
        "film-task-request-binding"
    );
}
#[test]
fn lost_metadata_can_be_recreated_but_partial_or_changed_controls_are_preserved() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("result.json");
    drop(persist(&path, b"exact", true).unwrap());
    drop(persist(&path, b"exact", true).unwrap());
    assert!(persist(&path, b"changed", true).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"exact");
}
