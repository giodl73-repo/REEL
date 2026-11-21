use reel_workflow::{manual::*, *};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

fn packet() -> Packet {
    make_packet(
        &Plan {
            schema: PLAN_SCHEMA.into(),
            project: "synthetic-film".into(),
            source_commit: "a".repeat(40),
            task_snapshots: BTreeMap::new(),
            tasks: vec![Task {
                id: "cel-note".into(),
                operation: Operation::TextProposal,
                tool: None,
                language: Some("es".into()),
                prompt: "Devuelve una nota sintética.".into(),
                native_inputs: None,
                upstream_inputs: None,
                inputs: vec![],
                depends_on: vec![],
                outputs: BTreeMap::from([(
                    "note".into(),
                    OutputContract {
                        media_type: "text/plain".into(),
                        max_bytes: 32,
                        required: true,
                    },
                )]),
            }],
        },
        "cel-note",
        BTreeMap::new(),
    )
    .unwrap()
}
fn files(root: &Path) -> FileBindings {
    let path = root.join("response.txt");
    fs::write(&path, "Una nota.").unwrap();
    BTreeMap::from([("note".into(), path)])
}

#[test]
fn cli_capture_hashes_bytes_and_exact_replay_leaves_candidate_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let candidate = root.path().join("candidate");
    export_work(&packet(), "attempt-1", &work).unwrap();
    assert_eq!(
        fs::read(work.join("prompt.txt")).unwrap(),
        packet().task.prompt.as_bytes()
    );
    assert!(export_work(&packet(), "attempt-1", &work).is_err());
    let bindings = files(root.path());
    let binding_path = root.path().join("bindings.json");
    fs::write(&binding_path, serde_json::to_vec(&bindings).unwrap()).unwrap();
    let call = || {
        Command::new(env!("CARGO_BIN_EXE_reel-workflow"))
            .arg("capture-work")
            .arg(&work)
            .arg("attempt-1")
            .arg(&binding_path)
            .arg(&candidate)
            .output()
            .unwrap()
    };
    let first = call();
    assert!(first.status.success(), "{:?}", first);
    let report: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(report["task_completed"], false);
    assert_eq!(report["human_approval"], false);
    assert_eq!(report["native_retention_verified"], false);
    assert!(
        !String::from_utf8(first.stdout)
            .unwrap()
            .contains(&packet().task.prompt)
    );
    let result: ReturnedResult = read_control(&candidate.join("result.json")).unwrap();
    assert_eq!(
        result.artifacts["note"].sha256,
        digest("Una nota.".as_bytes())
    );
    assert_eq!(fs::read_dir(&candidate).unwrap().count(), 4);
    let before = fs::read(candidate.join("candidate.json")).unwrap();
    let time = fs::metadata(candidate.join("candidate.json"))
        .unwrap()
        .modified()
        .unwrap();
    let second = call();
    assert!(second.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&second.stdout).unwrap()["replay"],
        true
    );
    assert_eq!(fs::read(candidate.join("candidate.json")).unwrap(), before);
    assert_eq!(
        fs::metadata(candidate.join("candidate.json"))
            .unwrap()
            .modified()
            .unwrap(),
        time
    );
    fs::write(&bindings["note"], "Otra nota.").unwrap();
    assert!(!call().status.success());
    assert_eq!(fs::read(candidate.join("candidate.json")).unwrap(), before);
}

#[test]
fn tampered_packages_wrong_attempt_and_invalid_outputs_have_no_candidate_effect() {
    for change in [
        "prompt", "contract", "packet", "attempt", "missing", "extra", "oversize", "relative",
    ] {
        let root = tempfile::tempdir().unwrap();
        let work = root.path().join("work");
        let candidate = root.path().join("candidate");
        export_work(&packet(), "attempt-1", &work).unwrap();
        let mut bindings = files(root.path());
        let mut attempt = "attempt-1";
        match change {
            "prompt" => fs::write(work.join("prompt.txt"), "changed").unwrap(),
            "contract" => fs::write(work.join("response-contract.json"), "{}").unwrap(),
            "packet" => fs::write(work.join("packet.json"), "{}").unwrap(),
            "attempt" => attempt = "attempt-2",
            "missing" => bindings.clear(),
            "extra" => {
                bindings.insert("extra".into(), bindings["note"].clone());
            }
            "oversize" => fs::write(&bindings["note"], [0; 33]).unwrap(),
            "relative" => {
                bindings.insert("note".into(), "relative.txt".into());
            }
            _ => unreachable!(),
        }
        assert!(
            capture_work(&work, attempt, &bindings, &candidate).is_err(),
            "{change}"
        );
        assert!(!candidate.exists(), "{change}");
    }
}

#[test]
fn partial_foreign_and_changed_candidates_are_preserved_for_recovery() {
    for change in ["partial", "foreign", "result", "bindings"] {
        let root = tempfile::tempdir().unwrap();
        let work = root.path().join("work");
        let candidate = root.path().join("candidate");
        export_work(&packet(), "attempt-1", &work).unwrap();
        let bindings = files(root.path());
        capture_work(&work, "attempt-1", &bindings, &candidate).unwrap();
        match change {
            "partial" => fs::remove_file(candidate.join("candidate.json")).unwrap(),
            "foreign" => fs::write(candidate.join("unexpected"), "preserve").unwrap(),
            "result" => fs::write(candidate.join("result.json"), "{}").unwrap(),
            "bindings" => fs::write(candidate.join("files.json"), "{}").unwrap(),
            _ => unreachable!(),
        }
        let before: BTreeMap<_, _> = fs::read_dir(&candidate)
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (e.file_name(), fs::read(e.path()).unwrap())
            })
            .collect();
        assert!(capture_work(&work, "attempt-1", &bindings, &candidate).is_err());
        let after: BTreeMap<_, _> = fs::read_dir(&candidate)
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (e.file_name(), fs::read(e.path()).unwrap())
            })
            .collect();
        assert_eq!(before, after);
    }
}

#[test]
fn explicit_accept_requires_original_attempt_handoff_mode_and_exact_bytes() {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let captured = root.path().join("captured");
    export_work(&packet(), "attempt-1", &work).unwrap();
    let bindings = files(root.path());
    capture_work(&work, "attempt-1", &bindings, &captured).unwrap();
    let result: ReturnedResult = read_control(&captured.join("result.json")).unwrap();
    for change in ["attempt", "mode", "hash"] {
        let mut invalid = result.clone();
        match change {
            "attempt" => invalid.attempt_id = "attempt-2".into(),
            "mode" => invalid.provenance.mode = Mode::Direct,
            "hash" => invalid.artifacts.get_mut("note").unwrap().sha256 = "0".repeat(64),
            _ => unreachable!(),
        }
        let destination = root.path().join(change);
        assert!(accept_work(&work, "attempt-1", &invalid, &bindings, &destination).is_err());
        assert!(!destination.exists());
    }
    let accepted = root.path().join("accepted");
    assert!(!accept_work(&work, "attempt-1", &result, &bindings, &accepted).unwrap());
    assert!(accept_work(&work, "attempt-1", &result, &bindings, &accepted).unwrap());
}
