//! Explicit recovery of an already protected job, never a new admission.
use super::*;
use crate::{admission, handoff, response, retention};
use reel_workflow::{Mode, Provenance, ReturnedResult};
use std::{fs, io::Write, path::Path, process::Command};

fn load<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> T {
    reel_workflow::decode(&fs::read(path).unwrap()).unwrap()
}
fn save(path: impl AsRef<Path>, value: &impl Serialize) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
    file.sync_all().unwrap();
}
#[test]
#[ignore = "Explicit existing protected resolution root only; manual candidate recovery with original pinned lifecycle runtime, no new admission or automatic worker claim"]
fn recover_original_resolution_through_manual_return() {
    let root = std::path::PathBuf::from(
        std::env::var_os("REEL_RESOLUTION_RECOVERY_ROOT").expect("Set preserved resolution root"),
    );
    let packet: Packet = load(root.join("packet.json"));
    let profile: handoff::Profile = load(root.join("profile.json"));
    let receipt: admission::Receipt = load(root.join("receipt.json"));
    assert_eq!(
        serde_json::to_value(&profile.binding.admission).unwrap(),
        serde_json::to_value(&receipt).unwrap()
    );
    assert_eq!(
        packet.task.tool.as_ref().unwrap().executable.sha256,
        profile.runtime.sha256
    );
    let _runtime = crate::hold_tool(&profile.runtime).unwrap();
    let observed = handoff::observe(&root.join("journal"), &profile).unwrap();
    assert_eq!(observed.state, reel_workflow::handoff::State::Waiting);
    assert!(observed.result.is_none());
    assert!(!root.join("automatic-dispatch").exists());
    let original_packet = fs::read(root.join("packet.json")).unwrap();
    let original_profile = fs::read(root.join("profile.json")).unwrap();
    let original_receipt = fs::read(root.join("receipt.json")).unwrap();
    let request: Request = load(root.join("request.json"));
    let parents = [
        (
            "baseline-claims-before.bin",
            &request.baseline.admission.index_root,
        ),
        (
            "follow-up-claims-before.bin",
            &request.corrected["es"].admission.index_root,
        ),
        (
            "producer-claims-before.bin",
            &request.productions["comic-render"]
                .profile
                .admission
                .index_root,
        ),
    ];
    for (before, index) in parents {
        assert_eq!(
            fs::read(root.join(before)).unwrap(),
            fs::read(index.join("claims.json")).unwrap()
        );
    }
    // Prepare data only. The original lifecycle executable independently
    // recomputes these exact bytes during import and retained reopening.
    let mut inputs =
        NativeInputs::open_admitted(&packet, &profile.source, &profile.admission, &receipt)
            .unwrap();
    let generated = generate(&packet, &mut inputs).unwrap();
    drop(inputs);
    let staging = root.join("manual-resolution-recovery");
    fs::create_dir(&staging).unwrap();
    for (name, bytes) in &generated.files {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(staging.join(name))
            .unwrap();
        file.write_all(bytes).unwrap();
        file.sync_all().unwrap();
    }
    let raw = &generated.files["receipt.json"];
    let result = ReturnedResult {
        schema: reel_workflow::RESULT_SCHEMA.into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: receipt.attempt_id.clone(),
        artifacts: generated.outputs,
        provenance: Provenance {
            mode: Mode::Tool,
            provider: None,
            model: None,
            executable: Some(packet.task.tool.as_ref().unwrap().executable.clone()),
        },
        worker_receipt: Some(Artifact {
            logical_id: "workflow/worker/receipt".into(),
            sha256: reel_workflow::digest(raw),
            bytes: raw.len() as u64,
            media_type: "application/json".into(),
        }),
    };
    let returns = response::Returns {
        schema: "reel.workflow-returns.v1".into(),
        root: staging,
        files: result
            .artifacts
            .keys()
            .map(|n| (n.clone(), n.clone()))
            .collect(),
    };
    let result_path = root.join("recovery-result.json");
    let returns_path = root.join("recovery-returns.json");
    save(&result_path, &result);
    save(&returns_path, &returns);
    save(
        root.join("recovery-intent.json"),
        &serde_json::json!({"task_key":packet.task_key,"runtime":profile.runtime,"mode":"manual-deterministic-return","automatic_dispatch":false,"new_admission":false}),
    );
    let invoke = || {
        Command::new(&profile.runtime.path)
            .arg("import-handoff")
            .arg(root.join("journal"))
            .arg(root.join("profile.json"))
            .arg(&result_path)
            .arg(&returns_path)
            .arg("--apply")
            .output()
            .unwrap()
    };
    let imported = invoke();
    save(
        root.join("recovery-import.json"),
        &serde_json::json!({"exit":imported.status.code(),"stdout":String::from_utf8_lossy(&imported.stdout),"stderr":String::from_utf8_lossy(&imported.stderr)}),
    );
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stdout)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&imported.stdout).unwrap()["task_completed"],
        true
    );
    let ack: retention::Acknowledgement = load(profile.acknowledgements.join(format!(
        "retained-{}-{}.json",
        packet.task_key, receipt.attempt_id
    )));
    let proof = retention::verify(
        &packet,
        &profile.source,
        &profile.admission,
        &receipt,
        &result,
        &ack,
    )
    .unwrap();
    let typed = proof.resolution_proposal().unwrap();
    assert_eq!(
        serde_json::to_value(typed).unwrap(),
        serde_json::to_value(&generated.checked).unwrap()
    );
    assert_eq!(typed.proposal.finding_keys.len(), 1);
    assert!(!typed.proposal.remote_issues_closed);
    drop(proof);
    let claims = fs::read(profile.admission.index_root.join("claims.json")).unwrap();
    let replay = Command::new(&profile.runtime.path)
        .arg("resume-handoff")
        .arg(root.join("journal"))
        .arg(root.join("profile.json"))
        .arg("-")
        .arg("--apply")
        .output()
        .unwrap();
    assert!(
        replay.status.success(),
        "{}",
        String::from_utf8_lossy(&replay.stdout)
    );
    assert_eq!(
        claims,
        fs::read(profile.admission.index_root.join("claims.json")).unwrap()
    );
    for (before, index) in parents {
        assert_eq!(
            fs::read(root.join(before)).unwrap(),
            fs::read(index.join("claims.json")).unwrap()
        );
    }
    assert_eq!(original_packet, fs::read(root.join("packet.json")).unwrap());
    assert_eq!(
        original_profile,
        fs::read(root.join("profile.json")).unwrap()
    );
    assert_eq!(
        original_receipt,
        fs::read(root.join("receipt.json")).unwrap()
    );
    save(
        root.join("recovery-qualified.json"),
        &serde_json::json!({"task_key":packet.task_key,"original_runtime":profile.runtime,"manual_return_retained":true,"typed_native_proof_reopened":true,"replay_claims_unchanged":true,"parent_claims_unchanged":true,"original_controls_unchanged":true,"automatic_dispatch":false,"new_admission":false,"issue_closed":false,"selected":false,"scratch_retired":false}),
    );
}

#[test]
#[ignore = "Existing retained resolution only; continue the original qualification after a repeated-import refusal, using resume without return files or re-admission"]
fn resume_retained_resolution_after_import_replay_refusal() {
    let root = std::path::PathBuf::from(
        std::env::var_os("REEL_RESOLUTION_RECOVERY_ROOT").expect("Set preserved resolution root"),
    );
    let packet: Packet = load(root.join("packet.json"));
    let profile: handoff::Profile = load(root.join("profile.json"));
    let request: Request = load(root.join("request.json"));
    let observed = handoff::observe(&root.join("journal"), &profile).unwrap();
    assert_eq!(observed.state, reel_workflow::handoff::State::Retained);
    assert_eq!(observed.ticket.packet, packet);
    let bindings: crate::graph::Bindings = load(root.join("automatic-bindings.json"));
    let (_, _profile_pin) = bindings.tasks[&packet.task.id].profile.read().unwrap();
    let controls: Vec<_> = [
        "packet.json",
        "profile.json",
        "receipt.json",
        "recovery-result.json",
        "recovery-returns.json",
    ]
    .into_iter()
    .map(|p| (p, fs::read(root.join(p)).unwrap()))
    .collect();
    let parents = [
        (
            "baseline-claims-before.bin",
            &request.baseline.admission.index_root,
        ),
        (
            "follow-up-claims-before.bin",
            &request.corrected["es"].admission.index_root,
        ),
        (
            "producer-claims-before.bin",
            &request.productions["comic-render"]
                .profile
                .admission
                .index_root,
        ),
    ];
    for (before, index) in parents {
        assert_eq!(
            fs::read(root.join(before)).unwrap(),
            fs::read(index.join("claims.json")).unwrap()
        );
    }
    let claims = fs::read(profile.admission.index_root.join("claims.json")).unwrap();
    let journal_entries = fs::read_dir(root.join("journal")).unwrap().count();
    save(
        root.join("resume-qualification-intent.json"),
        &serde_json::json!({"task_key":packet.task_key,"original_runtime":profile.runtime,"command":"resume-handoff","returns":null,"new_admission":false}),
    );
    let output = Command::new(&profile.runtime.path)
        .arg("resume-handoff")
        .arg(root.join("journal"))
        .arg(root.join("profile.json"))
        .arg("-")
        .arg("--apply")
        .output()
        .unwrap();
    save(
        root.join("resume-qualification-output.json"),
        &serde_json::json!({"exit":output.status.code(),"stdout":String::from_utf8_lossy(&output.stdout),"stderr":String::from_utf8_lossy(&output.stderr)}),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let summary: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["state"], "retained");
    assert_eq!(summary["task_completed"], true);
    assert_eq!(
        claims,
        fs::read(profile.admission.index_root.join("claims.json")).unwrap()
    );
    assert_eq!(
        journal_entries,
        fs::read_dir(root.join("journal")).unwrap().count()
    );
    for (name, raw) in controls {
        assert_eq!(raw, fs::read(root.join(name)).unwrap());
    }
    for (before, index) in parents {
        assert_eq!(
            fs::read(root.join(before)).unwrap(),
            fs::read(index.join("claims.json")).unwrap()
        );
    }
    save(
        root.join("resume-qualified.json"),
        &serde_json::json!({"task_key":packet.task_key,"original_runtime":profile.runtime,"original_job_retained":true,"resume_without_return_files":true,"replay_claims_unchanged":true,"journal_unchanged":true,"parent_claims_unchanged":true,"controls_unchanged":true,"automatic_dispatch":false,"new_admission":false,"issue_closed":false,"selected":false,"scratch_retired":false}),
    );
}
