use super::*;
use reel_workflow_native::{dispatch, graph, staging, worker};

#[allow(clippy::too_many_arguments)]
pub(super) fn run_comic(
    case: &AdmissionCase,
    packet: &Packet,
    profile: &reel_workflow_native::handoff::Profile,
    journal: &Path,
    profile_path: &Path,
    executable: &Path,
    result_path: &Path,
    controls: &mut Vec<(&'static str, Vec<u8>)>,
    auxiliary_programs: &BTreeMap<String, Tool>,
) -> std::process::Output {
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: packet.project.clone(),
        source_commit: packet.source_commit.clone(),
        tasks: vec![packet.task.clone()],
    };
    let bindings = graph::Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::from([(
            packet.task.id.clone(),
            graph::Binding {
                journal: journal.into(),
                profile: graph::PinnedFile {
                    path: profile_path.into(),
                    sha256: digest(&fs::read(profile_path).unwrap()),
                },
                staging_ticket: None,
            },
        )]),
    };
    run_graph(
        case,
        packet,
        profile,
        executable,
        result_path,
        controls,
        auxiliary_programs,
        &plan,
        &bindings,
    )
}
#[allow(clippy::too_many_arguments)]
pub(super) fn run_graph(
    case: &AdmissionCase,
    packet: &Packet,
    profile: &reel_workflow_native::handoff::Profile,
    executable: &Path,
    result_path: &Path,
    controls: &mut Vec<(&'static str, Vec<u8>)>,
    auxiliary_programs: &BTreeMap<String, Tool>,
    plan: &Plan,
    bindings: &graph::Bindings,
) -> std::process::Output {
    let root = case.temp.path();
    let stage = |name: &str| {
        let parent = root.join(name);
        fs::create_dir(&parent).unwrap();
        staging::Profile {
            schema: "reel.workflow-staging.v1".into(),
            parent,
            maximum_bytes: if packet
                .task
                .tool
                .as_ref()
                .is_some_and(|t| t.capability == ToolCapability::FilmRender)
            {
                256 * 1024 * 1024
            } else {
                64 * 1024 * 1024
            },
            capacity_policy: reel_assets::admission::Policy {
                minimum_free_bytes: 65536,
                minimum_free_basis_points: 0,
                max_object_bytes: 512 * 1024 * 1024,
                ..Default::default()
            },
        }
    };
    let config = dispatch::Config {
        auxiliary_programs: auxiliary_programs.clone(),
        provider: None,
        schema: "reel.workflow-dispatch.v1".into(),
        input_staging: stage("automatic-inputs"),
        worker_staging: Some(stage("automatic-worker")),
    };
    let plan_path = root.join("automatic-plan.json");
    let bindings_path = root.join("automatic-bindings.json");
    let config_path = root.join("automatic-config.json");
    for (path, raw) in [
        (&plan_path, serde_json::to_vec(&plan).unwrap()),
        (&bindings_path, serde_json::to_vec(&bindings).unwrap()),
        (&config_path, serde_json::to_vec(&config).unwrap()),
    ] {
        fs::write(path, raw).unwrap();
    }
    let directory = root.join("automatic-dispatch");
    let invoke = || {
        Command::new(executable)
            .arg("dispatch-task")
            .arg(&plan_path)
            .arg(&bindings_path)
            .arg(&packet.task.id)
            .arg(&config_path)
            .arg(&directory)
            .arg("--apply")
            .output()
            .unwrap()
    };
    let completed = invoke();
    assert!(
        completed.status.success(),
        "status {}; stdout: {}; stderr: {}",
        completed.status,
        String::from_utf8_lossy(&completed.stdout),
        String::from_utf8_lossy(&completed.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&completed.stdout).unwrap()["task_completed"],
        true
    );
    let header = fs::read(profile.admission.index_root.join("claims.json")).unwrap();
    let replay = invoke();
    assert!(
        replay.status.success(),
        "{}",
        String::from_utf8_lossy(&replay.stdout)
    );
    assert_eq!(
        header,
        fs::read(profile.admission.index_root.join("claims.json")).unwrap()
    );
    let ticket_raw = fs::read(directory.join("worker-ticket.json")).unwrap();
    let ticket: worker::Ticket = serde_json::from_slice(&ticket_raw).unwrap();
    fs::copy(ticket.directory().join("result.json"), result_path).unwrap();
    controls.extend([
        ("automatic-completion.json", completed.stdout.clone()),
        ("automatic-replay.json", replay.stdout),
        ("automatic-plan.json", fs::read(plan_path).unwrap()),
        (
            "automatic-bindings.json",
            fs::read(directory.join("bindings.json")).unwrap(),
        ),
        (
            "automatic-batch.json",
            fs::read(directory.join("ready.json")).unwrap(),
        ),
        ("automatic-worker-ticket.json", ticket_raw),
        (
            "automatic-locations.json",
            serde_json::to_vec(
                &json!({"root":root,"directory":directory,"workspace":ticket.directory()}),
            )
            .unwrap(),
        ),
    ]);
    completed
}
