//! One completed synthetic render -> controller restart -> file-backed worker.
use super::*;
use reel_workflow_native::{
    NativeInputs, admission::Prepared, closing, graph, handoff, movie, retention, staging, worker,
};
fn load<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> T {
    decode(&fs::read(path).unwrap()).unwrap()
}
fn save(path: impl AsRef<Path>, value: &impl serde::Serialize) {
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    f.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
    f.sync_all().unwrap();
}
fn run(exe: &Path, args: &[&std::ffi::OsStr]) -> std::process::Output {
    let out = Command::new(exe).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}
#[test]
#[ignore = "Existing retained synthetic resolution/movie inputs only; explicit same-Index slot reuse, one new synthetic render, completed-render adoption by original worker, native retention/replay/history; preserves roots"]
fn file_backed_movie_adopts_completed_render_and_retains_without_rerender() {
    let old = PathBuf::from(
        std::env::var_os("REEL_FILE_MOVIE_PREVIOUS_JOB").expect("Set retained resolution root"),
    );
    let bundle = PathBuf::from(
        std::env::var_os("REEL_FILE_MOVIE_SOURCE_BUNDLE").expect("Set original movie bundle"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_FILE_MOVIE_EVIDENCE_DIRECTORY").expect("Set evidence directory"),
    );
    let exe = PathBuf::from(std::env::var_os("REEL_NATIVE_HANDOFF_TOOL").expect("Set frozen CLI"));
    let old_packet: Packet = load(old.join("packet.json"));
    let old_profile: handoff::Profile = load(old.join("profile.json"));
    let old_result: ReturnedResult = load(old.join("result.json"));
    let old_ack: retention::Acknowledgement = load(old.join("ack.json"));
    let template: Packet = load(bundle.join("packet.json"));
    let mut source: reel_workflow_native::NativeProfile = load(bundle.join("source.json"));
    let old_movie_admission: reel_workflow_native::admission::Profile =
        load(bundle.join("admission.json"));
    let old_movie_claims = fs::read(old_movie_admission.index_root.join("claims.json")).unwrap();
    let expected: reel_workflow::review::Copy = load(bundle.join("review-copy.json"));
    let mut temp = tempfile::tempdir().unwrap();
    temp.disable_cleanup(true);
    let root = temp.path();
    let raw = fs::read(&exe).unwrap();
    let runtime = Tool {
        path: exe.clone(),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
    };
    assert_ne!(runtime.sha256, old_profile.runtime.sha256);
    let programs = source
        .movie_verifier
        .as_ref()
        .unwrap()
        .auxiliary_programs
        .clone();
    let checks = root.join("movie-checks");
    fs::create_dir(&checks).unwrap();
    let verifier = source.movie_verifier.as_mut().unwrap();
    verifier.runtime = runtime.clone();
    verifier.staging.parent = checks;
    source.source.pool.control = old_profile.source.source.pool.control.clone();
    let mut task = template.task.clone();
    task.id = "owned-file-movie".into();
    task.tool.as_mut().unwrap().executable.sha256 = runtime.sha256.clone();
    task.tool.as_mut().unwrap().executable.bytes = runtime.bytes;
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: template.project.clone(),
        source_commit: template.source_commit.clone(),
        tasks: vec![task],
    };
    let packet = make_packet(&plan, "owned-file-movie", BTreeMap::new()).unwrap();
    let mut admission = old_profile.admission.clone();
    admission.scope.operation = tig_store::Key::parse("owned-file-movie-replacement").unwrap();
    let marker = json!({"root":root,"previous_job":old,"movie_bundle":bundle,"task_key":packet.task_key,"runtime":runtime,"same_index":admission.index_root});
    save(root.join("original-operation.json"), &marker);
    save(evidence.join("original-operation-roots.json"), &marker);
    save(root.join("packet.json"), &packet);
    save(root.join("source.json"), &source);
    save(root.join("admission.json"), &admission);
    drop(Prepared::open(&packet, &source, &admission, "attempt-3").unwrap());
    let target = root.join("close-target.json");
    save(&target, &old_profile.binding.target);
    let close = root.join("close-ticket.json");
    run(
        &exe,
        &[
            "prepare-close".as_ref(),
            old.join("journal").as_os_str(),
            old.join("profile.json").as_os_str(),
            target.as_os_str(),
            close.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let close_pin = digest(&fs::read(&close).unwrap());
    save(root.join("close-ticket-pin.json"), &close_pin);
    let closed = root.join("closed.json");
    run(
        &exe,
        &[
            "close-job".as_ref(),
            close.as_os_str(),
            close_pin.as_ref(),
            closed.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let outcome: closing::Outcome = load(&closed);
    assert!(outcome.slot_idle && outcome.original_clear_complete);
    let prepared = Prepared::open(&packet, &source, &admission, "attempt-3").unwrap();
    save(root.join("preview.json"), &prepared.preview().unwrap());
    let receipt = prepared.accept().unwrap();
    save(root.join("receipt.json"), &receipt);
    let acknowledgements = root.join("acknowledgements");
    fs::create_dir(&acknowledgements).unwrap();
    let profile = handoff::Profile {
        schema: "reel.workflow-native-handoff.v1".into(),
        source,
        admission,
        binding: handoff::Binding {
            schema: "reel.workflow-native-handoff-binding.v1".into(),
            admission: receipt.clone(),
            target: old_profile.binding.target.clone(),
        },
        runtime: runtime.clone(),
        acknowledgements,
    };
    let profile_path = root.join("profile.json");
    save(&profile_path, &profile);
    let journal = root.join("journal");
    run(
        &exe,
        &[
            "create-handoff".as_ref(),
            root.join("packet.json").as_os_str(),
            profile_path.as_os_str(),
            journal.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let input_parent = root.join("input-staging");
    fs::create_dir(&input_parent).unwrap();
    let input_stage = staging::Profile {
        schema: "reel.workflow-staging.v1".into(),
        parent: input_parent,
        maximum_bytes: 64 * 1024 * 1024,
        capacity_policy: reel_assets::admission::Policy {
            minimum_free_bytes: 100 * 1024 * 1024,
            minimum_free_basis_points: 0,
            ..Default::default()
        },
    };
    let input_stage_path = root.join("input-profile.json");
    save(&input_stage_path, &input_stage);
    let input_ticket = root.join("input-ticket.json");
    run(
        &exe,
        &[
            "prepare-stage".as_ref(),
            root.join("packet.json").as_os_str(),
            root.join("source.json").as_os_str(),
            root.join("admission.json").as_os_str(),
            root.join("receipt.json").as_os_str(),
            input_stage_path.as_os_str(),
            input_ticket.as_os_str(),
        ],
    );
    let bindings = graph::Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::from([(
            packet.task.id.clone(),
            graph::Binding {
                journal,
                profile: graph::PinnedFile {
                    path: profile_path,
                    sha256: digest(&fs::read(root.join("profile.json")).unwrap()),
                },
                staging_ticket: Some(graph::PinnedFile {
                    path: input_ticket.clone(),
                    sha256: digest(&fs::read(input_ticket).unwrap()),
                }),
            },
        )]),
    };
    let plan_path = root.join("plan.json");
    let bind_path = root.join("bindings.json");
    save(&plan_path, &plan);
    save(&bind_path, &bindings);
    let ids = root.join("task-ids.json");
    save(&ids, &vec![packet.task.id.clone()]);
    let batch = root.join("ready.json");
    run(
        &exe,
        &[
            "export-ready".as_ref(),
            plan_path.as_os_str(),
            bind_path.as_os_str(),
            ids.as_os_str(),
            batch.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let batch_pin = digest(&fs::read(&batch).unwrap());
    let parent = root.join("worker");
    fs::create_dir(&parent).unwrap();
    let stage = staging::Profile {
        schema: "reel.workflow-staging.v1".into(),
        parent,
        maximum_bytes: 256 * 1024 * 1024,
        capacity_policy: reel_assets::admission::Policy {
            minimum_free_bytes: 100 * 1024 * 1024,
            minimum_free_basis_points: 0,
            ..Default::default()
        },
    };
    let stage_path = root.join("worker-profile.json");
    let programs_path = root.join("programs.json");
    save(&stage_path, &stage);
    save(&programs_path, &programs);
    let ticket_path = root.join("worker-ticket.json");
    run(
        &exe,
        &[
            "prepare-worker".as_ref(),
            plan_path.as_os_str(),
            bind_path.as_os_str(),
            batch.as_os_str(),
            batch_pin.as_ref(),
            packet.task.id.as_ref(),
            stage_path.as_os_str(),
            programs_path.as_os_str(),
            ticket_path.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let pin = digest(&fs::read(&ticket_path).unwrap());
    save(root.join("worker-ticket-pin.json"), &pin);
    let ticket: worker::Ticket = load(&ticket_path);
    let workspace = ticket.directory().join("packet");
    fs::create_dir(&workspace).unwrap();
    let native =
        NativeInputs::open_admitted(&packet, &profile.source, &profile.admission, &receipt)
            .unwrap();
    let produced = movie::generate_files(&packet, native, &programs, &runtime, &workspace).unwrap();
    assert_eq!(
        produced.files.len() + produced.controls.len(),
        packet.task.outputs.len() + 1
    );
    assert_eq!(
        produced
            .controls
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["receipt.json", "render-request.json", "render-result.json"]
    );
    assert!(!ticket.directory().join("complete.json").exists());
    assert!(!workspace.join("receipt.json").exists());
    let request = fs::read(workspace.join("render-request.json")).unwrap();
    let result = fs::read(workspace.join("render-result.json")).unwrap();
    let times = [
        fs::metadata(workspace.join("render-request.json"))
            .unwrap()
            .modified()
            .unwrap(),
        fs::metadata(workspace.join("render-result.json"))
            .unwrap()
            .modified()
            .unwrap(),
    ];
    drop(produced);
    save(
        root.join("completed-render-boundary.json"),
        &json!({"workspace":workspace,"task_key":packet.task_key,"render_request_sha256":digest(&request),"render_result_sha256":digest(&result),"worker_completion_present":false,"worker_receipt_present":false}),
    );
    // A new controller process must adopt the exact completed render, not render again.
    let completed = run(
        &exe,
        &[
            "run-worker".as_ref(),
            plan_path.as_os_str(),
            bind_path.as_os_str(),
            ticket_path.as_os_str(),
            pin.as_ref(),
            "--apply".as_ref(),
        ],
    );
    save(
        root.join("worker-completion.json"),
        &serde_json::from_slice::<serde_json::Value>(&completed.stdout).unwrap(),
    );
    assert_eq!(
        fs::read(workspace.join("render-request.json")).unwrap(),
        request
    );
    assert_eq!(
        fs::read(workspace.join("render-result.json")).unwrap(),
        result
    );
    assert_eq!(
        fs::metadata(workspace.join("render-request.json"))
            .unwrap()
            .modified()
            .unwrap(),
        times[0]
    );
    assert_eq!(
        fs::metadata(workspace.join("render-result.json"))
            .unwrap()
            .modified()
            .unwrap(),
        times[1]
    );
    let claims = fs::read(profile.admission.index_root.join("claims.json")).unwrap();
    run(
        &exe,
        &[
            "run-worker".as_ref(),
            plan_path.as_os_str(),
            bind_path.as_os_str(),
            ticket_path.as_os_str(),
            pin.as_ref(),
            "--apply".as_ref(),
        ],
    );
    assert_eq!(
        claims,
        fs::read(profile.admission.index_root.join("claims.json")).unwrap()
    );
    let result: ReturnedResult = load(ticket.directory().join("result.json"));
    save(root.join("result.json"), &result);
    let ack: retention::Acknowledgement = load(
        profile
            .acknowledgements
            .join(format!("retained-{}-attempt-3.json", packet.task_key)),
    );
    save(root.join("ack.json"), &ack);
    let review = retention::review_copy(
        &packet,
        &profile.source,
        &profile.admission,
        &receipt,
        &result,
        &ack,
    )
    .unwrap();
    assert_eq!(review.copy().units, expected.units);
    drop(review);
    drop(
        retention::verify(
            &old_packet,
            &old_profile.source,
            &old_profile.admission,
            &old_profile.binding.admission,
            &old_result,
            &old_ack,
        )
        .unwrap(),
    );
    assert_eq!(
        claims,
        fs::read(profile.admission.index_root.join("claims.json")).unwrap()
    );
    assert_eq!(
        old_movie_claims,
        fs::read(old_movie_admission.index_root.join("claims.json")).unwrap()
    );
    let report = json!({"root":root,"task_key":packet.task_key,"same_index_slot_reused":true,"new_render_count":1,"completed_render_adopted_after_controller_boundary":true,"original_render_controls_and_timestamps_unchanged":true,"file_backed_worker_retained":true,"replay_claims_unchanged":true,"review_ids_preserved":true,"previous_resolution_history_reopened":true,"original_movie_claims_unchanged":true,"new_index_created":false,"consumer_adopted":false,"selected":false,"scratch_retired":false});
    save(root.join("qualification-result.json"), &report);
    save(evidence.join("qualification-result.json"), &report);
}
