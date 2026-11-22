//! Synthetic metadata and explicit native image-selection qualifications.
use super::*;
#[cfg(windows)]
fn image_selection_fixture(
    candidate_bytes: &[u8],
) -> (
    Fixture,
    ReturnedResult,
    reel_workflow_native::image_selection::Spec,
) {
    use reel_workflow_native::selection::{Change, Identity, Request};
    let mut fixture = Fixture::with_object(Object::from_bytes(b"P6\n1 1\n255\n\xff\0\0"));
    let root = fixture.temp.path().to_path_buf();
    let mut before: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("records/input.tig")).unwrap()).unwrap();
    before["metadata"]["story_id"] = json!("story-1");
    before["metadata"]["cel_id"] = json!("C101");
    before["status"] = json!({"approved_subject":fixture.packet.task.inputs[0].sha256});
    before["streams"] = json!([]);
    write(&root, "records/input.tig", before.clone());
    fixture
        .profile
        .bindings
        .get_mut("input/source")
        .unwrap()
        .streams
        .clear();
    fs::create_dir_all(root.join(".reel-workflow/decisions")).unwrap();
    fs::write(
        root.join(".reel-workflow/decisions/selection.json"),
        b"pending",
    )
    .unwrap();
    git(&root, &["add", "."]);
    git(
        &root,
        &[
            "commit",
            "-m",
            "explicit synthetic decision slot and stable identity",
        ],
    );
    fixture.profile.commit = git(&root, &["rev-parse", "HEAD"]);
    fixture.packet.task.operation = Operation::ImageCandidate;
    fixture.packet.task.inputs[0].media_type = "image/x-portable-pixmap".into();
    fixture.packet.task.outputs = BTreeMap::from([(
        "picture".into(),
        OutputContract {
            media_type: "image/png".into(),
            max_bytes: 1024 * 1024,
            required: true,
        },
    )]);
    fixture.repin();
    let candidate = Artifact {
        logical_id: "candidate/picture".into(),
        sha256: digest(candidate_bytes),
        bytes: candidate_bytes.len() as u64,
        media_type: "image/png".into(),
    };
    let old = fixture.packet.task.inputs[0].clone();
    let decision = serde_json::to_vec(&json!({"schema":"synthetic.selection-decision.v1","project":fixture.packet.project,"old_subject":old,"candidate":candidate})).unwrap();
    let mut after = before.clone();
    after["status"] = json!({});
    after["selected"] =
        json!({"sha256":candidate.sha256,"bytes":candidate.bytes,"media_type":"image/png"});
    let result = ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: fixture.packet.task.id.clone(),
        task_key: fixture.packet.task_key.clone(),
        attempt_id: "attempt-1".into(),
        artifacts: BTreeMap::from([("picture".into(), candidate.clone())]),
        worker_receipt: None,
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
    };
    let spec = reel_workflow_native::image_selection::Spec {
        schema: "reel.workflow-image-selection.v1".into(),
        old_subject: old.clone(),
        output_slot: "picture".into(),
        record_path: "records/input.tig".into(),
        old_record: fs::read(root.join("records/input.tig")).unwrap(),
        decision_path: ".reel-workflow/decisions/selection.json".into(),
        transaction: Request {
            schema: "reel.workflow-owned-selection.v1".into(),
            git: fixture.profile.git.clone(),
            repository: fixture.profile.repository.clone(),
            operation: "select-image-c101".into(),
            selection: "refs/reel-workflow/selections/image-fixture".into(),
            base: fixture.profile.commit.clone(),
            expected: None,
            identity: Identity {
                name: "Fixture".into(),
                email: "fixture@example.invalid".into(),
                unix_seconds: 1_700_000_000,
            },
            decision: Artifact {
                logical_id: "decision/image-fixture".into(),
                sha256: digest(&decision),
                bytes: decision.len() as u64,
                media_type: "application/json".into(),
            },
            changes: vec![
                Change {
                    path: "records/input.tig".into(),
                    expected: Object::from_bytes(&serde_json::to_vec(&before).unwrap()),
                    bytes: serde_json::to_vec(&after).unwrap(),
                },
                Change {
                    path: "payloads/source.bin".into(),
                    expected: Object::from_bytes(
                        &fs::read(root.join("payloads/source.bin")).unwrap(),
                    ),
                    bytes: Key::from_object(&Object::from_bytes(candidate_bytes))
                        .unlocked_pointer(),
                },
                Change {
                    path: ".reel-workflow/decisions/selection.json".into(),
                    expected: Object::from_bytes(b"pending"),
                    bytes: decision,
                },
            ],
        },
    };
    (fixture, result, spec)
}

#[cfg(windows)]
#[test]
fn image_selection_metadata_preserves_stable_identity_and_exact_old_approval_without_effects() {
    let (fixture, result, spec) = image_selection_fixture(b"metadata-only candidate bytes");
    let old = fs::read(fixture.temp.path().join("records/input.tig")).unwrap();
    let refs = git(fixture.temp.path(), &["show-ref"]);
    let checked = reel_workflow_native::image_selection::inspect(
        &fixture.packet,
        &fixture.profile,
        &result,
        &spec,
    )
    .unwrap();
    assert!(
        checked.status_cleared && !checked.native_retention_verified && !checked.selection_applied
    );
    assert_eq!(
        old,
        fs::read(fixture.temp.path().join("records/input.tig")).unwrap()
    );
    assert_eq!(refs, git(fixture.temp.path(), &["show-ref"]));
    assert!(
        serde_json::from_slice::<serde_json::Value>(&old).unwrap()["status"]["approved_subject"]
            .is_string()
    );
}

#[cfg(windows)]
#[test]
fn image_selection_metadata_refuses_identity_status_decision_leaf_and_record_alias_changes() {
    for mutation in [
        "identity",
        "status",
        "decision",
        "leaf",
        "record-alias",
        "cue",
        "subject",
    ] {
        let (fixture, result, mut spec) = image_selection_fixture(b"metadata-only candidate bytes");
        match mutation {
            "identity" | "status" => {
                let mut record: serde_json::Value =
                    serde_json::from_slice(&spec.transaction.changes[0].bytes).unwrap();
                if mutation == "identity" {
                    record["metadata"]["cel_id"] = json!("C102");
                } else {
                    record["status"] = json!({"approved":true});
                }
                spec.transaction.changes[0].bytes = serde_json::to_vec(&record).unwrap();
            }
            "decision" => spec.transaction.changes[2].bytes.push(b' '),
            "leaf" => spec.transaction.changes[1].path = "payloads/alias.bin".into(),
            "record-alias" => {
                spec.record_path = "records/alias.tig".into();
                spec.transaction.changes[0].path = spec.record_path.clone();
            }
            "cue" => spec
                .transaction
                .changes
                .push(reel_workflow_native::selection::Change {
                    path: "cue.json".into(),
                    expected: Object::from_bytes(b"old"),
                    bytes: b"new".to_vec(),
                }),
            "subject" => spec.old_subject.sha256 = digest(b"another version"),
            _ => unreachable!(),
        }
        assert!(
            reel_workflow_native::image_selection::inspect(
                &fixture.packet,
                &fixture.profile,
                &result,
                &spec
            )
            .is_err(),
            "{mutation}"
        );
        assert!(git(fixture.temp.path(), &["for-each-ref", "refs/reel-workflow"]).is_empty());
    }
}

#[cfg(windows)]
struct SyntheticSelectionAuthority;
#[cfg(windows)]
impl reel_workflow_native::image_selection::Authority for SyntheticSelectionAuthority {
    fn authorize(
        &mut self,
        context: &reel_workflow_native::image_selection::DecisionContext<'_>,
    ) -> Result<()> {
        let decision: serde_json::Value = serde_json::from_slice(context.decision_bytes).unwrap();
        if decision["schema"] != "synthetic.selection-decision.v1"
            || decision["project"] != context.project
            || decision["old_subject"] != serde_json::to_value(context.old_subject).unwrap()
            || decision["candidate"] != serde_json::to_value(context.candidate).unwrap()
        {
            return Err(Error {
                kind: ErrorKind::Denied,
                code: "synthetic-exact-selection-authority".into(),
            });
        }
        Ok(())
    }
}

#[cfg(windows)]
#[test]
fn image_selection_staged_acknowledgement_without_native_bytes_cannot_select() {
    use reel_workflow_native::{admission::Prepared, image_selection, response, retention};
    let (fixture, result, spec) = image_selection_fixture(b"metadata-only candidate bytes");
    let mut case = AdmissionCase::new(&fixture);
    case.source.source.maximum_bytes = 2 * 1024 * 1024;
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let returns_root = case.temp.path().join("returns");
    fs::create_dir(&returns_root).unwrap();
    fs::write(
        returns_root.join("picture.png"),
        b"metadata-only candidate bytes",
    )
    .unwrap();
    let returns = response::Returns {
        schema: "reel.workflow-returns.v1".into(),
        root: returns_root,
        files: BTreeMap::from([("picture".into(), "picture.png".into())]),
    };
    response::register(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &returns,
    )
    .unwrap();
    let ack = retention::stage(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &retained_target(),
    )
    .unwrap();
    let request = image_selection::Request {
        packet: fixture.packet.clone(),
        source: case.source,
        admission: case.profile,
        receipt,
        result,
        acknowledgement: ack,
        spec,
    };
    assert!(image_selection::run(&request, &mut SyntheticSelectionAuthority, true).is_err());
    assert!(git(fixture.temp.path(), &["for-each-ref", "refs/reel-workflow"]).is_empty());
}

#[cfg(windows)]
#[test]
#[ignore = "Explicit existing synthetic native pool and synthetic PNG only; publishes owned image fixtures, retains selection/decision/history and closes original slot; no project adoption or pool initialization"]
fn retained_image_selection_applies_exact_record_leaf_and_decision_and_recovers_without_reselection()
 {
    use reel_workflow_native::admission::Prepared;
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(
            std::env::var_os("REEL_NATIVE_INPUT_FIXTURE")
                .expect("Set existing synthetic pool descriptor"),
        )
        .unwrap(),
    )
    .unwrap();
    let native: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(value["source"].clone()).unwrap();
    let png =
        fs::read(std::env::var_os("REEL_SELECTION_PNG").expect("Set existing owned synthetic PNG"))
            .unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n") && png.len() < 1024 * 1024);
    let evidence = PathBuf::from(
        std::env::var_os("REEL_SELECTION_EVIDENCE").expect("Set NEW evidence directory"),
    );
    assert!(evidence.is_absolute());
    fs::create_dir(&evidence).unwrap();
    let (mut fixture, result, mut spec) = image_selection_fixture(&png);
    let git_raw = fs::read(&native.pool.git).unwrap();
    fixture.profile.git = Tool {
        path: native.pool.git.clone(),
        sha256: digest(&git_raw),
        bytes: git_raw.len() as u64,
    };
    spec.transaction.git = fixture.profile.git.clone();
    let mut case = AdmissionCase::with_store(&fixture, native.store.clone());
    let control = case.source.source.pool.control.clone();
    case.source.source = native;
    case.source.source.maximum_bytes = 2 * 1024 * 1024;
    case.source.source.pool.control = control;
    case.source.annex = serde_json::from_value(value["annex"].clone()).unwrap();
    fixture.temp.disable_cleanup(true);
    case.temp.disable_cleanup(true);
    let saved = |name: &str, value: &serde_json::Value| {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(evidence.join(name))
            .unwrap();
        file.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
        file.sync_all().unwrap();
    };
    saved(
        "locations.json",
        &json!({"source_root":fixture.temp.path(),"case_root":case.temp.path(),"packet":fixture.packet,"native":case.source,"profile":case.profile,"spec":spec}),
    );
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    saved("admission.json", &serde_json::to_value(&receipt).unwrap());
    let returns_root = case.temp.path().join("returns");
    fs::create_dir(&returns_root).unwrap();
    fs::write(returns_root.join("old.ppm"), b"P6\n1 1\n255\n\xff\0\0").unwrap();
    fs::write(returns_root.join("picture.png"), &png).unwrap();
    let published = tig_git::annex::workflow::publication::publish(
        &tig_git::annex::workflow::publication::Profile {
            schema: "tig.annex-file-publication.experimental-1".into(),
            pool: case.source.source.pool.clone(),
            store: case.source.source.store.clone(),
            source_root: returns_root.clone(),
            source: LogicalPath::parse("old.ppm").unwrap(),
            source_owned: true,
            expected: Some(Object::from_bytes(b"P6\n1 1\n255\n\xff\0\0")),
            include_private_location: false,
            maximum_bytes: 1024 * 1024,
            minimum_free_bytes: 100 * 1024 * 1024,
        },
    )
    .unwrap();
    assert!(published.report().reopened_verified);
    drop(published);
    let runtime = evidence.join("reel-workflow-native.exe");
    fs::copy(env!("CARGO_BIN_EXE_reel-workflow-native"), &runtime).unwrap();
    exercise_image_selection(NativeImageCase {
        root: fixture.temp.path().into(),
        case_root: case.temp.path().into(),
        evidence,
        packet: fixture.packet,
        source: case.source,
        admission: case.profile,
        receipt,
        result,
        spec,
        png,
    });
}

#[cfg(windows)]
struct NativeImageCase {
    root: PathBuf,
    case_root: PathBuf,
    evidence: PathBuf,
    packet: Packet,
    source: reel_workflow_native::NativeProfile,
    admission: reel_workflow_native::admission::Profile,
    receipt: reel_workflow_native::admission::Receipt,
    result: ReturnedResult,
    spec: reel_workflow_native::image_selection::Spec,
    png: Vec<u8>,
}
#[cfg(windows)]
#[test]
#[ignore = "Continue only the preserved original synthetic image admission after pre-handoff runtime refusal; no new admission/pool/attempt or replacement lifecycle runtime"]
fn continue_original_image_selection_after_handoff_runtime_refusal() {
    let evidence = PathBuf::from(
        std::env::var_os("REEL_SELECTION_CONTINUE").expect("Set original preserved evidence root"),
    );
    let locations: serde_json::Value =
        serde_json::from_slice(&fs::read(evidence.join("locations.json")).unwrap()).unwrap();
    let packet: Packet = serde_json::from_value(locations["packet"].clone()).unwrap();
    let receipt: reel_workflow_native::admission::Receipt =
        serde_json::from_slice(&fs::read(evidence.join("admission.json")).unwrap()).unwrap();
    let case_root = PathBuf::from(locations["case_root"].as_str().unwrap());
    let png = fs::read(case_root.join("returns/picture.png")).unwrap();
    let spec: reel_workflow_native::image_selection::Spec =
        serde_json::from_value(locations["spec"].clone()).unwrap();
    // The refusal happened before any handoff or response registration. Rebuild
    // its exact deterministic result control, not a new task/attempt or payload.
    assert!(!case_root.join("image-journal").exists());
    let result = ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: receipt.attempt_id.clone(),
        worker_receipt: None,
        artifacts: BTreeMap::from([(
            spec.output_slot.clone(),
            Artifact {
                logical_id: "candidate/picture".into(),
                sha256: digest(&png),
                bytes: png.len() as u64,
                media_type: "image/png".into(),
            },
        )]),
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
    };
    exercise_image_selection(NativeImageCase {
        root: PathBuf::from(locations["source_root"].as_str().unwrap()),
        case_root,
        evidence,
        packet,
        source: serde_json::from_value(locations["native"].clone()).unwrap(),
        admission: serde_json::from_value(locations["profile"].clone()).unwrap(),
        receipt,
        result,
        spec,
        png,
    });
}
#[cfg(windows)]
fn exercise_image_selection(case: NativeImageCase) {
    use reel_workflow_native::{closing, handoff, image_selection};
    let NativeImageCase {
        root,
        case_root,
        evidence,
        packet,
        source,
        admission,
        receipt,
        result,
        spec,
        png,
    } = case;
    let saved = |name: &str, value: &serde_json::Value| {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(evidence.join(name))
            .unwrap();
        file.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
        file.sync_all().unwrap();
    };
    let runtime = evidence.join("reel-workflow-native.exe");
    let raw = fs::read(&runtime).unwrap();
    let acknowledgements = case_root.join("acknowledgements");
    if !acknowledgements.is_dir() {
        fs::create_dir(&acknowledgements).unwrap();
    }
    let profile = handoff::Profile {
        schema: "reel.workflow-native-handoff.v1".into(),
        source: serde_json::from_slice(&serde_json::to_vec(&source).unwrap()).unwrap(),
        admission: admission.clone(),
        binding: handoff::Binding {
            schema: "reel.workflow-native-handoff-binding.v1".into(),
            admission: receipt.clone(),
            target: retained_target(),
        },
        runtime: Tool {
            path: runtime,
            sha256: digest(&raw),
            bytes: raw.len() as u64,
        },
        acknowledgements,
    };
    let returns = reel_workflow_native::response::Returns {
        schema: "reel.workflow-returns.v1".into(),
        root: case_root.join("returns"),
        files: BTreeMap::from([("picture".into(), "picture.png".into())]),
    };
    saved(
        "packet-control.json",
        &serde_json::to_value(&packet).unwrap(),
    );
    saved(
        "profile-control.json",
        &serde_json::to_value(&profile).unwrap(),
    );
    saved(
        "result-control.json",
        &serde_json::to_value(&result).unwrap(),
    );
    saved(
        "returns-control.json",
        &serde_json::to_value(&returns).unwrap(),
    );
    let call = |args: Vec<std::ffi::OsString>| {
        let output = Command::new(&profile.runtime.path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    };

    let journal = case_root.join("image-journal");
    call(vec![
        "create-handoff".into(),
        evidence.join("packet-control.json").into(),
        evidence.join("profile-control.json").into(),
        journal.clone().into(),
        "--apply".into(),
    ]);
    call(vec![
        "import-handoff".into(),
        journal.clone().into(),
        evidence.join("profile-control.json").into(),
        evidence.join("result-control.json").into(),
        evidence.join("returns-control.json").into(),
        "--apply".into(),
    ]);
    let ack_path = fs::read_dir(&profile.acknowledgements)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "json"))
        .unwrap();
    let ack = serde_json::from_slice(&fs::read(ack_path).unwrap()).unwrap();
    let request = image_selection::Request {
        packet: packet.clone(),
        source: serde_json::from_slice(&serde_json::to_vec(&source).unwrap()).unwrap(),
        admission: admission.clone(),
        receipt: receipt.clone(),
        result: result.clone(),
        acknowledgement: ack,
        spec,
    };
    saved(
        "selection-request.json",
        &serde_json::to_value(&request).unwrap(),
    );
    struct Denied;
    impl image_selection::Authority for Denied {
        fn authorize(&mut self, _: &image_selection::DecisionContext<'_>) -> Result<()> {
            Err(Error {
                kind: ErrorKind::Denied,
                code: "synthetic-refused-selection".into(),
            })
        }
    }
    assert_eq!(
        image_selection::run(&request, &mut Denied, true)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Denied
    );
    assert!(git(&root, &["for-each-ref", "refs/reel-workflow"]).is_empty());
    let selected = image_selection::run(&request, &mut SyntheticSelectionAuthority, true).unwrap();
    assert!(
        selected.selection.applied
            && selected.selection.currently_selected
            && selected.native_retention_verified
            && selected.proposed_native_binding_verified
    );
    saved("selected.json", &serde_json::to_value(&selected).unwrap());
    let selected_record: serde_json::Value = serde_json::from_str(&git(
        &root,
        &[
            "show",
            &format!("{}:records/input.tig", selected.selection.proposal),
        ],
    ))
    .unwrap();
    assert_eq!(selected_record["metadata"]["cel_id"], "C101");
    assert_eq!(selected_record["status"], json!({}));
    assert_eq!(selected_record["selected"]["sha256"], digest(&png));
    let selected_decision = git(
        &root,
        &[
            "show",
            &format!(
                "{}:.reel-workflow/decisions/selection.json",
                selected.selection.proposal
            ),
        ],
    );
    assert_eq!(
        digest(selected_decision.as_bytes()),
        request.spec.transaction.decision.sha256
    );
    let replay = image_selection::run(&request, &mut SyntheticSelectionAuthority, true).unwrap();
    assert_eq!(replay.selection.proposal, selected.selection.proposal);
    fs::write(root.join("later.txt"), b"later unrelated authored change").unwrap();
    git(&root, &["add", "later.txt"]);
    git(&root, &["commit", "-m", "source advances independently"]);
    let replay = image_selection::run(&request, &mut SyntheticSelectionAuthority, true).unwrap();
    assert_eq!(replay.selection.proposal, selected.selection.proposal);
    assert_eq!(
        fs::read(root.join("records/input.tig")).unwrap(),
        request.spec.old_record
    );
    let original_lifecycle_tool = reel_workflow::handoff::Journal::open(&journal)
        .unwrap()
        .ticket()
        .lifecycle_tool
        .clone();
    let close = closing::Request {
        schema: "reel.workflow-native-close.v1".into(),
        journal,
        packet: request.packet.clone(),
        source: serde_json::from_slice(&serde_json::to_vec(&request.source).unwrap()).unwrap(),
        admission: request.admission.clone(),
        receipt: request.receipt.clone(),
        result: request.result.clone(),
        acknowledgement: request.acknowledgement.clone(),
        operation: tig_store::Key::parse("close-image-selection").unwrap(),
        original_lifecycle_tool,
        runtime: profile.runtime.clone(),
    };
    saved("close-request.json", &serde_json::to_value(&close).unwrap());
    let close_pin = digest(&fs::read(evidence.join("close-request.json")).unwrap());
    call(vec![
        "close-retained".into(),
        evidence.join("close-request.json").into(),
        close_pin.into(),
        evidence.join("closed.json").into(),
        "--apply".into(),
    ]);
    let closed: closing::Outcome =
        serde_json::from_slice(&fs::read(evidence.join("closed.json")).unwrap()).unwrap();
    assert!(closed.slot_idle && closed.original_clear_complete);
    for object in [
        Object::from_bytes(b"P6\n1 1\n255\n\xff\0\0"),
        Object::from_bytes(&png),
    ] {
        drop(tig_git::annex::workflow::reader::open(&source.source, &object).unwrap());
    }
    saved(
        "complete.json",
        &json!({"synthetic_authority_only":true,"selected":true,"original_replay":true,"source_advance_preserved":true,"old_and_new_native_bytes_reopened_after_close":true,"slot_idle":closed.slot_idle,"consumers_rebuilt":false,"production_adopted":false}),
    );
}
