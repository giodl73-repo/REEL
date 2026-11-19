//! Close an existing completed movie; retain history across unrelated slot reuse.
use super::*;
use reel_workflow_native::{admission::Prepared, closing::Outcome, handoff, retention};

fn save(root: &Path, name: &str, value: &impl serde::Serialize) {
    fs::write(root.join(name), serde_json::to_vec(value).unwrap()).unwrap();
}
fn phase(root: &Path, name: &str) {
    save(root, "phase.json", &json!({"phase":name}));
}
fn run(executable: &Path, args: &[&std::ffi::OsStr]) {
    let output = Command::new(executable).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}: {} {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "Existing completed movie only; closes original slot, preserves native bytes and admits unrelated replacement generation"]
fn completed_movie_survives_interrupted_close_and_unrelated_slot_reuse() {
    let original = PathBuf::from(
        std::env::var_os("REEL_COMPLETED_MOVIE_FIXTURE").expect("Set completed movie fixture"),
    );
    let packet: Packet =
        serde_json::from_slice(&fs::read(original.join("packet.json")).unwrap()).unwrap();
    let profile: handoff::Profile =
        serde_json::from_slice(&fs::read(original.join("profile.json")).unwrap()).unwrap();
    let result: ReturnedResult =
        serde_json::from_slice(&fs::read(original.join("result.json")).unwrap()).unwrap();
    let receipt = &profile.binding.admission;
    let ack_path = profile
        .acknowledgements
        .join(format!("retained-{}-attempt-1.json", packet.task_key));
    let acknowledgement = serde_json::from_slice(&fs::read(&ack_path).unwrap()).unwrap();
    let mut temp = tempfile::tempdir().unwrap();
    temp.disable_cleanup(true);
    let root = temp.path();
    let marker = json!({"root":root,"original":original,"task_key":packet.task_key,"admission":receipt,"runtime":profile.runtime});
    save(root, "original-operation.json", &marker);
    let evidence = std::env::var_os("REEL_MOVIE_CLOSE_EVIDENCE_DIRECTORY").map(PathBuf::from);
    if let Some(e) = &evidence {
        save(e, "original-operation-roots.json", &marker);
    }

    // Prepare the unrelated packet before the first native effect. Its selected
    // object is deliberately absent from the movie's input closure.
    let mut next_source: reel_workflow_native::NativeProfile =
        serde_json::from_value(serde_json::to_value(&profile.source).unwrap()).unwrap();
    next_source.movie_verifier = None;
    next_source.snapshot.records = vec!["records/input.tig".into()];
    next_source.snapshot.bindings = BTreeMap::from([(
        "input/source".into(),
        Binding {
            selected: LogicalPath::parse("payloads/source.bin").unwrap(),
            streams: BTreeMap::from([(
                "preview".into(),
                LogicalPath::parse("payloads/preview.bin").unwrap(),
            )]),
        },
    )]);
    next_source.snapshot.input_streams = BTreeMap::from([("input/source".into(), None)]);
    let selected = Object::from_bytes(b"synthetic input");
    assert!(
        packet
            .task
            .inputs
            .iter()
            .all(|a| a.sha256 != selected.sha256.as_str())
    );
    let next_task = Task {
        id: "unrelated-slot-review".into(),
        upstream_inputs: None,
        native_inputs: Some(Fixture::declaration(&next_source.snapshot)),
        operation: Operation::ReviewFinding,
        tool: None,
        language: Some("es".into()),
        prompt: "Review the unrelated synthetic input after movie close.".into(),
        inputs: vec![Artifact {
            logical_id: "input/source".into(),
            sha256: selected.sha256.as_str().into(),
            bytes: selected.bytes,
            media_type: "application/octet-stream".into(),
        }],
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
    let next_packet = make_packet(
        &Plan {
            schema: PLAN_SCHEMA.into(),
            project: packet.project.clone(),
            source_commit: packet.source_commit.clone(),
            tasks: vec![next_task],
        },
        "unrelated-slot-review",
        BTreeMap::new(),
    )
    .unwrap();
    let mut next_profile = profile.admission.clone();
    next_profile.scope.operation = tig_store::Key::parse("movie-slot-replacement").unwrap();
    save(root, "replacement-packet.json", &next_packet);
    save(root, "replacement-source.json", &next_source);
    save(root, "replacement-profile.json", &next_profile);
    drop(Prepared::open(&next_packet, &next_source, &next_profile, "attempt-2").unwrap());

    let executable = &profile.runtime.path;
    let ticket = root.join("close-ticket.json");
    let target = root.join("close-target.json");
    save(root, "close-target.json", &profile.binding.target);
    let claims = profile.admission.index_root.join("claims.json");
    let before = fs::read(&claims).unwrap();
    phase(root, "prepare-close");
    run(
        executable,
        &[
            "prepare-close".as_ref(),
            original.join("journal").as_os_str(),
            original.join("profile.json").as_os_str(),
            target.as_os_str(),
            ticket.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    assert_eq!(before, fs::read(&claims).unwrap());
    let pin = digest(&fs::read(&ticket).unwrap());
    save(root, "close-ticket-pin.json", &pin);
    let staged_file = root.join("close-staged.json");
    phase(root, "stage-close");
    run(
        executable,
        &[
            "stage-close".as_ref(),
            ticket.as_os_str(),
            pin.as_ref(),
            staged_file.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let staged: Outcome = serde_json::from_slice(&fs::read(staged_file).unwrap()).unwrap();
    assert!(!staged.slot_idle && !staged.original_clear_complete);
    phase(root, "interrupt-original-handoff-prepared");
    {
        use tig_store::{
            metadata::MetadataStore,
            source_claims::protection::{self, Index, work},
        };
        let mut nodes = MetadataStore::open(
            &profile.admission.metadata_root,
            &staged.metadata_checkpoint,
        )
        .unwrap();
        let mut index =
            Index::open(&profile.admission.index_root, &staged.index_checkpoint).unwrap();
        let record = work::read(&mut index, staged.handoff.as_ref().unwrap(), &nodes).unwrap();
        let mut interrupted = false;
        assert!(
            index
                .update_hook(&record.update, &mut nodes, |stage| {
                    if stage == protection::Stage::Prepared {
                        interrupted = true;
                        Err(protection::Error::Unknown)
                    } else {
                        Ok(())
                    }
                })
                .is_err()
        );
        assert!(interrupted);
        nodes.finish().unwrap();
    }
    phase(root, "recover-original-close");
    let closed_file = root.join("closed.json");
    run(
        executable,
        &[
            "recover-close".as_ref(),
            ticket.as_os_str(),
            pin.as_ref(),
            closed_file.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let closed: Outcome = serde_json::from_slice(&fs::read(closed_file).unwrap()).unwrap();
    assert!(closed.slot_idle && closed.original_clear_complete && closed.applied);
    assert!(
        Prepared::open(&packet, &profile.source, &profile.admission, "attempt-1")
            .unwrap()
            .reopen(receipt)
            .is_err()
    );
    phase(root, "admit-unrelated-replacement");
    let next = Prepared::open(&next_packet, &next_source, &next_profile, "attempt-2")
        .unwrap()
        .accept()
        .unwrap();
    save(root, "replacement-receipt.json", &next);
    assert_ne!(next.activation, receipt.activation);
    let before_replay = fs::read(&claims).unwrap();
    phase(root, "replay-old-close-after-reuse");
    let replay_file = root.join("close-after-reuse.json");
    run(
        executable,
        &[
            "recover-close".as_ref(),
            ticket.as_os_str(),
            pin.as_ref(),
            replay_file.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let replay: Outcome = serde_json::from_slice(&fs::read(replay_file).unwrap()).unwrap();
    assert!(replay.original_clear_complete && !replay.slot_idle);
    assert_eq!(replay.handoff, closed.handoff);
    assert_eq!(replay.clear, closed.clear);
    assert_eq!(before_replay, fs::read(&claims).unwrap());
    drop(
        Prepared::open(&next_packet, &next_source, &next_profile, "attempt-2")
            .unwrap()
            .reopen(&next)
            .unwrap(),
    );
    assert!(
        Prepared::open(&packet, &profile.source, &profile.admission, "attempt-1")
            .unwrap()
            .reopen(receipt)
            .is_err()
    );
    phase(root, "verify-retained-movie-after-slot-reuse");
    drop(
        retention::verify(
            &packet,
            &profile.source,
            &profile.admission,
            receipt,
            &result,
            &acknowledgement,
        )
        .unwrap(),
    );
    assert_eq!(before_replay, fs::read(&claims).unwrap());
    phase(root, "verified");
    let report = json!({"root":root,"original":original,"task_key":packet.task_key,"original_close_recovered":true,"unrelated_replacement_survived_stale_close":true,"old_activation_refused":true,"retained_movie_verified_after_slot_reuse":true,"history_read_claims_unchanged":true,"replacement_executed":false,"selected":false,"scratch_retired":false,"payloads_deleted":false});
    save(root, "qualification-result.json", &report);
    if let Some(e) = evidence {
        save(&e, "qualification-result.json", &report);
    }
}
