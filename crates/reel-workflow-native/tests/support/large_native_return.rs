//! Explicit existing-pool large result qualification. Every root is preserved.
use super::*;
use reel_workflow::publication::InputReader;
use reel_workflow_native::{
    NativeInputs, SnapshotBudget, admission::Prepared, closing, handoff, response, retention,
};
fn load<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> T {
    decode(&fs::read(path).unwrap()).unwrap()
}
fn save(path: impl AsRef<Path>, value: &impl serde::Serialize) {
    use std::io::Write;
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    out.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
    out.sync_all().unwrap();
}
fn save_exact(path: impl AsRef<Path>, value: &impl serde::Serialize) {
    if path.as_ref().exists() {
        assert_eq!(fs::read(path).unwrap(), serde_json::to_vec(value).unwrap());
    } else {
        save(path, value);
    }
}
fn run(exe: &Path, root: &Path, label: &str, args: &[&std::ffi::OsStr]) {
    let output = Command::new(exe).args(args).output().unwrap();
    let log = (0..8)
        .map(|i| root.join(format!("{label}-command-{i}.json")))
        .find(|p| !p.exists())
        .expect("Bounded original-command observations exhausted");
    save(
        log,
        &json!({"exit_code":output.status.code(),
        "stdout":String::from_utf8_lossy(&output.stdout),"stderr":String::from_utf8_lossy(&output.stderr)}),
    );
    assert!(
        output.status.success(),
        "{label}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn env_path(name: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(name).expect(name));
    assert!(path.is_absolute());
    path
}
fn complete(root: &Path, exe: &Path, first_import: bool) {
    let journal = root.join("journal");
    let profile_path = root.join("profile.json");
    if first_import {
        run(
            exe,
            root,
            "import",
            &[
                "import-handoff".as_ref(),
                journal.as_os_str(),
                profile_path.as_os_str(),
                root.join("result.json").as_os_str(),
                root.join("returns.json").as_os_str(),
                "--apply".as_ref(),
            ],
        );
    } else {
        run(
            exe,
            root,
            "resume",
            &[
                "resume-handoff".as_ref(),
                journal.as_os_str(),
                profile_path.as_os_str(),
                root.join("returns.json").as_os_str(),
                "--apply".as_ref(),
            ],
        );
    }
    let packet: Packet = load(root.join("packet.json"));
    let profile: handoff::Profile = load(&profile_path);
    let result: ReturnedResult = load(root.join("result.json"));
    let job = reel_workflow::handoff::Journal::open(&journal).unwrap();
    assert_eq!(job.state(), reel_workflow::handoff::State::Retained);
    assert_eq!(job.result(), Some(&result));
    let before = (
        job.ticket().clone(),
        job.state(),
        job.result().cloned(),
        job.acknowledgement().cloned(),
    );
    drop(job);
    let ack_path = profile.acknowledgements.join(format!(
        "retained-{}-{}.json",
        packet.task_key, profile.binding.admission.attempt_id
    ));
    let ack: retention::Acknowledgement = load(&ack_path);
    let staged: response::Candidate = load(root.join("candidate.json"));
    assert_eq!(ack.candidate, staged);
    save_exact(root.join("ack.json"), &ack);
    drop(
        retention::verify(
            &packet,
            &profile.source,
            &profile.admission,
            &profile.binding.admission,
            &result,
            &ack,
        )
        .unwrap(),
    );
    run(
        exe,
        root,
        "replay",
        &[
            "resume-handoff".as_ref(),
            journal.as_os_str(),
            profile_path.as_os_str(),
            "-".as_ref(),
            "--apply".as_ref(),
        ],
    );
    let after = reel_workflow::handoff::Journal::open(&journal).unwrap();
    assert_eq!(
        (
            after.ticket().clone(),
            after.state(),
            after.result().cloned(),
            after.acknowledgement().cloned()
        ),
        before
    );
    save_exact(
        root.join("completed.json"),
        &json!({"schema":"reel.workflow-large-native-result-qualification.v1",
        "task_key":packet.task_key,"attempt_id":result.attempt_id,"artifacts":result.artifacts,
            "candidate_ack":ack.candidate,"retained":true,"journal_state":after.state(),
        "selected":false,"consumer_adoption":false,"original_work_preserved":true}),
    );
}
#[test]
#[ignore = "Existing retained synthetic movie slot and existing reserved 3.6 GB source only; closes original slot into retained coverage, admits one declared large-result job, publishes/retains/replays under original identity; preserves all roots"]
fn existing_large_return_publishes_and_retains_under_one_managed_operation() {
    let old = env_path("REEL_LARGE_RETURN_PREVIOUS_JOB");
    let root = env_path("REEL_LARGE_NATIVE_ROOT");
    let exe = env_path("REEL_NATIVE_HANDOFF_TOOL");
    let large = env_path("REEL_LARGE_RETURN_SOURCE");
    assert!(root.is_dir() && large.is_file());
    assert_eq!(fs::metadata(&large).unwrap().len(), 3_600_000_001);
    let old_packet: Packet = load(old.join("packet.json"));
    let old_profile: handoff::Profile = load(old.join("profile.json"));
    drop(
        Prepared::open(
            &old_packet,
            &old_profile.source,
            &old_profile.admission,
            &old_profile.binding.admission.attempt_id,
        )
        .unwrap()
        .reopen(&old_profile.binding.admission)
        .unwrap(),
    );
    let raw = fs::read(&exe).unwrap();
    let runtime = Tool {
        path: exe.clone(),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
    };
    let mut source: reel_workflow_native::NativeProfile =
        decode(&serde_json::to_vec(&old_profile.source).unwrap()).unwrap();
    source.movie_verifier = None;
    source.source.maximum_bytes = 4 * 1024 * 1024 * 1024;
    source.snapshot.schema = "reel.workflow-native-snapshot.v2".into();
    source.snapshot.budget = Some(SnapshotBudget {
        schema: "reel.workflow-snapshot-budget.v1".into(),
        input_bytes: 64 * 1024 * 1024,
        input_object_bytes: 64 * 1024 * 1024,
        projection_bytes: 8 * 1024 * 1024 * 1024,
        returns: Some(response::Budget {
            schema: "reel.workflow-return-budget.v1".into(),
            total_bytes: 4 * 1024 * 1024 * 1024,
            object_bytes: 4 * 1024 * 1024 * 1024,
            verify_millis: 300_000,
        }),
    });
    let declaration = source.snapshot.declaration_bytes().unwrap();
    let task=Task {id:"large-native-result".into(),operation:Operation::TextProposal,tool:None,language:Some("es".into()),
        prompt:"Retain the explicitly supplied existing synthetic binary; no model call, creative production or adoption.".into(),
        inputs:old_packet.task.inputs.clone(),upstream_inputs:None,native_inputs:Some(Artifact {logical_id:"control/large-native-bindings".into(),
            sha256:digest(&declaration),bytes:declaration.len() as u64,media_type:"application/json".into()}),depends_on:vec![],
        outputs:BTreeMap::from([("master".into(),OutputContract {media_type:"application/octet-stream".into(),max_bytes:4*1024*1024*1024,required:true})])};
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        task_snapshots: Default::default(),
        project: "synthetic-large-native-result".into(),
        source_commit: old_packet.source_commit.clone(),
        tasks: vec![task],
    };
    let packet = make_packet(&plan, "large-native-result", BTreeMap::new()).unwrap();
    let mut admission = old_profile.admission.clone();
    admission.scope.operation = tig_store::Key::parse("large-native-result-original").unwrap();
    drop(Prepared::open(&packet, &source, &admission, "large-attempt-1").unwrap());
    // Read named existing inputs before changing any original native job state.
    let mut inputs = NativeInputs::open(&packet, &source).unwrap();
    for asset in &packet.task.inputs {
        inputs.read(asset, asset.bytes).unwrap();
    }
    drop(inputs);
    save(
        root.join("original-operation.json"),
        &json!({"previous_job":old,"old_task_key":old_packet.task_key,
        "task_key":packet.task_key,"attempt":"large-attempt-1","runtime":runtime,"large_source":large,
        "index":admission.index_root,"original_operation":admission.scope.operation}),
    );
    save(root.join("plan.json"), &plan);
    save(root.join("packet.json"), &packet);
    save(root.join("source.json"), &source);
    save(root.join("admission.json"), &admission);
    save(root.join("close-target.json"), &old_profile.binding.target);
    let close = root.join("close-ticket.json");
    run(
        &exe,
        &root,
        "prepare-close",
        &[
            "prepare-close".as_ref(),
            old.join("journal").as_os_str(),
            old.join("profile.json").as_os_str(),
            root.join("close-target.json").as_os_str(),
            close.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let pin = digest(&fs::read(&close).unwrap());
    save(root.join("close-ticket-pin.json"), &pin);
    run(
        &exe,
        &root,
        "close",
        &[
            "close-job".as_ref(),
            close.as_os_str(),
            pin.as_ref(),
            root.join("closed.json").as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let closed: closing::Outcome = load(root.join("closed.json"));
    assert!(closed.slot_idle && closed.original_clear_complete);
    let prepared = Prepared::open(&packet, &source, &admission, "large-attempt-1").unwrap();
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
            admission: receipt,
            target: old_profile.binding.target,
        },
        runtime,
        acknowledgements,
    };
    save(root.join("profile.json"), &profile);
    run(
        &exe,
        &root,
        "create",
        &[
            "create-handoff".as_ref(),
            root.join("packet.json").as_os_str(),
            root.join("profile.json").as_os_str(),
            root.join("journal").as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let result = ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: "large-attempt-1".into(),
        artifacts: BTreeMap::from([(
            "master".into(),
            Artifact {
                logical_id: "synthetic/large-native-master".into(),
                sha256: "1800c39a6a2baaedb3760b6be6035276d6ca7602430e0f91fc671be1ee22a509".into(),
                bytes: 3_600_000_001,
                media_type: "application/octet-stream".into(),
            },
        )]),
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
        worker_receipt: None,
    };
    save(root.join("result.json"), &result);
    save(
        root.join("returns.json"),
        &response::Returns {
            schema: "reel.workflow-returns.v1".into(),
            root: large.parent().unwrap().into(),
            files: BTreeMap::from([(
                "master".into(),
                large.file_name().unwrap().to_str().unwrap().into(),
            )]),
        },
    );
    run(
        &exe,
        &root,
        "stage-response",
        &[
            "stage-response".as_ref(),
            root.join("packet.json").as_os_str(),
            root.join("source.json").as_os_str(),
            root.join("admission.json").as_os_str(),
            root.join("receipt.json").as_os_str(),
            root.join("result.json").as_os_str(),
            root.join("returns.json").as_os_str(),
            root.join("candidate.json").as_os_str(),
            "--apply".as_ref(),
        ],
    );
    complete(&root, &exe, true);
}
#[test]
#[ignore = "Resume only the existing large-result ImportPending job with its original runtime/profile/journal and return identity; no new admission or close"]
fn resume_original_large_native_result() {
    let root = env_path("REEL_LARGE_NATIVE_ROOT");
    let profile: handoff::Profile = load(root.join("profile.json"));
    let marker: serde_json::Value = load(root.join("original-operation.json"));
    assert_eq!(
        serde_json::to_value(&profile.runtime).unwrap(),
        marker["runtime"]
    );
    complete(&root, &profile.runtime.path, false);
}

#[test]
#[ignore = "Read-only reconciliation of the existing large-result operation: exact journal, candidate work, output claims and one native content-location query; no byte ingestion, retry, admission or close"]
fn inspect_original_large_native_result() {
    use tig_core::tree::NodeStore;
    use tig_git::annex::{
        identity::Key,
        process::{Command as NativeCommand, Runner},
    };
    use tig_store::{
        metadata::MetadataStore,
        source_claims::protection::{Index, work},
    };
    let root = env_path("REEL_LARGE_NATIVE_ROOT");
    let profile: handoff::Profile = load(root.join("profile.json"));
    let packet: Packet = load(root.join("packet.json"));
    let result: ReturnedResult = load(root.join("result.json"));
    let candidate: response::Candidate = load(root.join("candidate.json"));
    reel_workflow::check_result(&packet, &result).unwrap();
    assert_eq!(candidate.task_key, packet.task_key);
    assert_eq!(candidate.attempt_id, result.attempt_id);
    let journal = reel_workflow::handoff::Journal::open(&root.join("journal")).unwrap();
    assert_eq!(journal.ticket().packet, packet);
    assert_eq!(journal.result(), Some(&result));
    drop(
        Prepared::open(
            &packet,
            &profile.source,
            &profile.admission,
            &result.attempt_id,
        )
        .unwrap()
        .reopen(&profile.binding.admission)
        .unwrap(),
    );
    let mut nodes = MetadataStore::open(
        &profile.admission.metadata_root,
        &profile.binding.admission.metadata_checkpoint,
    )
    .unwrap();
    let mut index = Index::open(
        &profile.admission.index_root,
        &profile.binding.admission.index_checkpoint,
    )
    .unwrap();
    let before = index.checkpoint();
    let record = work::read(&mut index, &candidate.work, &nodes).unwrap();
    assert_eq!(record.evidence.as_ref(), Some(&candidate.registration));
    let registration: serde_json::Value =
        decode(&nodes.get(&candidate.registration).unwrap().unwrap()).unwrap();
    assert_eq!(
        registration["result"],
        serde_json::to_value(&result).unwrap()
    );
    let completed = work::complete(&mut index, &candidate.work).unwrap();
    let object = Object {
        sha256: tig_core::Digest::parse(&result.artifacts["master"].sha256).unwrap(),
        bytes: result.artifacts["master"].bytes,
    };
    let protected = index
        .inspect(&mut nodes, &object, &profile.admission.scope.store)
        .unwrap()
        .protected_by;
    assert!(protected.contains(&profile.admission.scope.actor));
    let runner = Runner::new(profile.source.source.pool.clone()).unwrap();
    let output = runner
        .run(NativeCommand::ContentLocationBatch(&Key::from_object(
            &object,
        )))
        .unwrap();
    assert!(output.success);
    let missing = matches!(output.stdout.as_slice(), b"\n" | b"\r\n");
    if !missing {
        let location = std::str::from_utf8(&output.stdout)
            .unwrap()
            .trim_end_matches(['\r', '\n']);
        assert!(!location.is_empty() && !location.contains(['\r', '\n']));
    }
    let after = Index::open(&profile.admission.index_root, &before)
        .unwrap()
        .checkpoint();
    assert_eq!(before, after);
    let report = (0..4)
        .map(|i| root.join(format!("inspection-{i}.json")))
        .find(|p| !p.exists())
        .expect("Bounded read-only inspections exhausted");
    save(
        report,
        &json!({"schema":"reel.workflow-large-native-inspection.v1",
        "task_key":packet.task_key,"attempt_id":result.attempt_id,"journal_state":journal.state(),
        "candidate":candidate,"candidate_work_complete":completed,"protected_by":protected,
        "native_location_reported":!missing,"location_stdout_sha256":digest(&output.stdout),
        "index_before":before,"index_after":after,"payload_verified":false,
        "publication_retried":false,"native_admission":false}),
    );
}
