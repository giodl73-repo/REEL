//! Explicit close and same-slot replacement with an owned resolution worker.
use super::*;
use reel_workflow_native::{
    admission::Prepared, closing, graph, handoff, resolution_job::Request, retention,
};

fn load<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> T {
    decode(&fs::read(path).unwrap()).unwrap()
}
fn save(path: impl AsRef<Path>, value: &impl serde::Serialize) {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
    file.sync_all().unwrap();
}
fn run(exe: &Path, args: &[&std::ffi::OsStr]) {
    let output = Command::new(exe).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
#[ignore = "Existing retained resolution only; explicit close, same-Index slot reuse, owned automatic dispatch/retention/replay and original retained history; preserves roots"]
fn retained_resolution_slot_reuses_for_automatic_worker_and_preserves_history() {
    let original = PathBuf::from(
        std::env::var_os("REEL_RESOLUTION_RECOVERY_ROOT").expect("Set retained resolution root"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_RESOLUTION_AUTOMATIC_EVIDENCE_DIRECTORY")
            .expect("Set new evidence directory"),
    );
    let exe = PathBuf::from(
        std::env::var_os("REEL_NATIVE_HANDOFF_TOOL").expect("Set frozen corrected CLI"),
    );
    let old_packet: Packet = load(original.join("packet.json"));
    let old_profile: handoff::Profile = load(original.join("profile.json"));
    let old_result: ReturnedResult = load(original.join("recovery-result.json"));
    let old_ack: retention::Acknowledgement = load(old_profile.acknowledgements.join(format!(
        "retained-{}-{}.json",
        old_packet.task_key, old_profile.binding.admission.attempt_id
    )));
    let request: Request = load(original.join("request.json"));
    let parents = [
        &request.baseline.admission.index_root,
        &request.corrected["es"].admission.index_root,
        &request.productions["comic-render"]
            .profile
            .admission
            .index_root,
    ];
    let parent_claims: Vec<_> = parents
        .iter()
        .map(|p| fs::read(p.join("claims.json")).unwrap())
        .collect();
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
    let mut task = old_packet.task.clone();
    task.id = "automatic-resolution".into();
    let tool = task.tool.as_mut().unwrap();
    tool.executable.sha256 = runtime.sha256.clone();
    tool.executable.bytes = runtime.bytes;
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: old_packet.project.clone(),
        source_commit: old_packet.source_commit.clone(),
        tasks: vec![task],
    };
    let packet = make_packet(&plan, "automatic-resolution", BTreeMap::new()).unwrap();
    let source: reel_workflow_native::NativeProfile =
        decode(&serde_json::to_vec(&old_profile.source).unwrap()).unwrap();
    let mut admission = old_profile.admission.clone();
    admission.scope.operation = tig_store::Key::parse("resolution-automatic-replacement").unwrap();
    let marker = json!({"root":root,"original":original,"old_task_key":old_packet.task_key,"new_task_key":packet.task_key,"original_runtime":old_profile.runtime,"runtime":runtime,"same_index":admission.index_root});
    save(root.join("original-operation.json"), &marker);
    save(evidence.join("original-operation-roots.json"), &marker);
    save(root.join("packet.json"), &packet);
    save(root.join("source.json"), &source);
    save(root.join("admission.json"), &admission);
    drop(Prepared::open(&packet, &source, &admission, "attempt-2").unwrap());
    // Closing has its own explicit pinned runtime/operation. The original
    // handoff profile and source runtime are never retargeted.
    let target = root.join("close-target.json");
    save(&target, &old_profile.binding.target);
    let ticket = root.join("close-ticket.json");
    run(
        &exe,
        &[
            "prepare-close".as_ref(),
            original.join("journal").as_os_str(),
            original.join("profile.json").as_os_str(),
            target.as_os_str(),
            ticket.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let pin = digest(&fs::read(&ticket).unwrap());
    save(root.join("close-ticket-pin.json"), &pin);
    let closed_path = root.join("closed.json");
    run(
        &exe,
        &[
            "close-job".as_ref(),
            ticket.as_os_str(),
            pin.as_ref(),
            closed_path.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let closed: closing::Outcome = load(&closed_path);
    assert!(closed.slot_idle && closed.original_clear_complete && closed.applied);
    let prepared = Prepared::open(&packet, &source, &admission, "attempt-2").unwrap();
    save(root.join("preview.json"), &prepared.preview().unwrap());
    let receipt = prepared.accept().unwrap();
    save(root.join("receipt.json"), &receipt);
    assert_ne!(receipt.activation, old_profile.binding.admission.activation);
    assert!(
        Prepared::open(
            &old_packet,
            &old_profile.source,
            &old_profile.admission,
            &old_profile.binding.admission.attempt_id
        )
        .unwrap()
        .reopen(&old_profile.binding.admission)
        .is_err()
    );
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
        runtime,
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
    let bindings = graph::Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::from([(
            packet.task.id.clone(),
            graph::Binding {
                journal,
                profile: graph::PinnedFile {
                    path: profile_path.clone(),
                    sha256: digest(&fs::read(profile_path).unwrap()),
                },
                staging_ticket: None,
            },
        )]),
    };
    let mut controls = Vec::new();
    automatic_dispatch::run_graph_root(
        root,
        &packet,
        &profile,
        &exe,
        &root.join("result.json"),
        &mut controls,
        &BTreeMap::new(),
        &plan,
        &bindings,
    );
    for (name, raw) in controls {
        fs::write(root.join(name), raw).unwrap();
    }
    let result: ReturnedResult = load(root.join("result.json"));
    let ack: retention::Acknowledgement = load(
        profile
            .acknowledgements
            .join(format!("retained-{}-attempt-2.json", packet.task_key)),
    );
    save(root.join("ack.json"), &ack);
    let proof = retention::verify(
        &packet,
        &profile.source,
        &profile.admission,
        &receipt,
        &result,
        &ack,
    )
    .unwrap();
    let proposal = proof.resolution_proposal().unwrap();
    assert_eq!(proposal.proposal.finding_keys.len(), 1);
    let expected: serde_json::Value =
        load(original.join("manual-resolution-recovery/resolution.json"));
    assert_eq!(serde_json::to_value(proposal).unwrap(), expected);
    drop(proof);
    let claims = fs::read(profile.admission.index_root.join("claims.json")).unwrap();
    let replay = root.join("old-close-after-reuse.json");
    run(
        &exe,
        &[
            "recover-close".as_ref(),
            ticket.as_os_str(),
            pin.as_ref(),
            replay.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let replay: closing::Outcome = load(replay);
    assert!(replay.original_clear_complete && !replay.slot_idle);
    assert_eq!(
        claims,
        fs::read(profile.admission.index_root.join("claims.json")).unwrap()
    );
    let historical = retention::verify(
        &old_packet,
        &old_profile.source,
        &old_profile.admission,
        &old_profile.binding.admission,
        &old_result,
        &old_ack,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(historical.resolution_proposal().unwrap()).unwrap(),
        expected
    );
    drop(historical);
    assert_eq!(
        claims,
        fs::read(profile.admission.index_root.join("claims.json")).unwrap()
    );
    for (index, before) in parents.iter().zip(parent_claims) {
        assert_eq!(before, fs::read(index.join("claims.json")).unwrap());
    }
    let outcome = json!({"root":root,"original":original,"old_task_key":old_packet.task_key,"task_key":packet.task_key,"same_index_slot_reused":true,"owned_dispatch_retained":true,"typed_native_proof_reopened":true,"automatic_replay_claims_unchanged":true,"stale_close_did_not_clear_replacement":true,"original_retained_history_reopened":true,"parent_claims_unchanged":true,"new_index_created":false,"provider_called":false,"issue_closed":false,"selected":false,"scratch_retired":false});
    save(root.join("qualification-result.json"), &outcome);
    save(evidence.join("qualification-result.json"), &outcome);
}
