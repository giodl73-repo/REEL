//! Metadata fixtures plus an explicitly opted-in existing-pool advancement case.
use super::*;
use reel_workflow::expansion::{Input, TaskTemplate, Template};
use reel_workflow_native::{
    expansion::{self, Frozen},
    graph,
};

fn prepare() -> (Fixture, AdmissionCase, Plan, Frozen) {
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let (base, frozen) = freeze(&fixture, &case);
    (fixture, case, base, frozen)
}
fn freeze(fixture: &Fixture, case: &AdmissionCase) -> (Plan, Frozen) {
    let task = &fixture.packet.task;
    let base = Plan {
        schema: PLAN_SCHEMA.into(),
        project: fixture.packet.project.clone(),
        source_commit: fixture.packet.source_commit.clone(),
        tasks: vec![task.clone()],
    };
    let template = Template {
        schema: "reel.workflow-expansion-template.v1".into(),
        base_plan_sha256: canonical_digest(&base).unwrap(),
        task: TaskTemplate {
            id: "expanded".into(),
            operation: task.operation,
            language: task.language.clone(),
            prompt: task.prompt.clone(),
            depends_on: vec![],
            inputs: task
                .inputs
                .iter()
                .map(|a| Input::Fixed {
                    artifact: a.clone(),
                })
                .collect(),
            outputs: task.outputs.clone(),
            tool: None,
        },
    };
    let bindings = graph::Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::new(),
    };
    let source = serde_json::from_slice(&serde_json::to_vec(&case.source).unwrap()).unwrap();
    let before = fs::read(case.profile.metadata_root.join("metadata.json")).unwrap();
    let frozen = expansion::prepare(
        &base,
        &bindings,
        &template,
        source,
        case.profile.clone(),
        "attempt-1",
    )
    .unwrap();
    assert_eq!(
        fs::read(case.profile.metadata_root.join("metadata.json")).unwrap(),
        before
    );
    assert!(
        case.protected(&Object::from_bytes(b"synthetic input"))
            .is_empty()
    );
    (base, frozen)
}
#[test]
fn expanded_source_freeze_accept_and_original_receipt_recovery_preserve_one_operation() {
    let (_fixture, case, base, frozen) = prepare();
    assert!(frozen.expansion.packet.task.native_inputs.is_some());
    assert_eq!(frozen.preview.total_actor_limit, 16);
    assert_eq!(frozen.preview.changed_path_limit, 128);
    let output = case.temp.path().join("frozen.json");
    frozen.export(&output).unwrap();
    assert_eq!(
        expansion::read_pinned(&output, &"0".repeat(64))
            .err()
            .unwrap()
            .kind,
        ErrorKind::Integrity
    );
    let (frozen, _pin): (Frozen, _) =
        expansion::read_pinned(&output, &digest(&fs::read(&output).unwrap())).unwrap();
    frozen.check(&base).unwrap();
    let receipt = frozen
        .accept_to(&base, &case.temp.path().join("accepted.json"))
        .unwrap();
    assert_eq!(
        case.protected(&Object::from_bytes(b"synthetic input")),
        vec![case.profile.scope.actor.clone()]
    );
    let recovered = frozen
        .recover_to(&base, &case.temp.path().join("recovered.json"))
        .unwrap();
    assert_eq!(
        receipt.canonical_bytes().unwrap(),
        recovered.canonical_bytes().unwrap()
    );
    assert_eq!(receipt.work, recovered.work);
    assert_eq!(receipt.activation, recovered.activation);
    assert_eq!(frozen.attempt_id, recovered.attempt_id);
}
#[test]
fn frozen_expansion_refuses_changed_base_preview_packet_and_existing_outputs() {
    let (_fixture, case, mut base, mut frozen) = prepare();
    let output = case.temp.path().join("existing.json");
    fs::write(&output, b"preserve original").unwrap();
    assert_eq!(
        frozen.export(&output).unwrap_err().kind,
        ErrorKind::Conflict
    );
    assert_eq!(fs::read(&output).unwrap(), b"preserve original");
    assert_eq!(
        frozen
            .export(&case.profile.index_root.join("forbidden.json"))
            .unwrap_err()
            .kind,
        ErrorKind::Denied
    );
    base.tasks[0].prompt.push_str(" changed");
    assert_eq!(frozen.accept(&base).unwrap_err().kind, ErrorKind::Stale);
    let original_len = base.tasks[0].prompt.len() - 8;
    base.tasks[0].prompt.truncate(original_len);
    frozen.preview.changed_path_limit += 1;
    assert_eq!(
        frozen.accept(&base).unwrap_err().code,
        "frozen-expansion-preview"
    );
    assert!(
        case.protected(&Object::from_bytes(b"synthetic input"))
            .is_empty()
    );
    frozen.preview.changed_path_limit -= 1;
    frozen.expansion.packet.task.prompt.push_str(" changed");
    assert_eq!(frozen.accept(&base).unwrap_err().kind, ErrorKind::Stale);
    assert!(
        case.protected(&Object::from_bytes(b"synthetic input"))
            .is_empty()
    );
}

fn advance_config() -> reel_workflow_native::advancement::Config {
    let path = std::env::current_exe().unwrap();
    let bytes = fs::read(&path).unwrap();
    reel_workflow_native::advancement::Config {
        schema: "reel.workflow-advance-config.v1".into(),
        runtime: Tool {
            path,
            sha256: digest(&bytes),
            bytes: bytes.len() as u64,
        },
        target: retained_target(),
    }
}

#[test]
fn advancement_reconciles_preexisting_original_admission_before_creating_controls() {
    let (_fixture, case, base, frozen) = prepare();
    let original = frozen.accept(&base).unwrap();
    let header = fs::read(case.profile.index_root.join("claims.json")).unwrap();
    let root = case.temp.path().join("adopt-original");
    let summary =
        reel_workflow_native::advancement::advance(&base, &frozen, &advance_config(), &root)
            .unwrap();
    assert_eq!(
        summary.admission_identity,
        original.operation_identity().unwrap()
    );
    assert_eq!(
        fs::read(root.join("admission.json")).unwrap(),
        original.canonical_bytes().unwrap()
    );
    assert_eq!(
        header,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
}

#[test]
fn advancement_adopts_one_original_job_and_recovers_a_lost_receipt_without_reacceptance() {
    use reel_workflow_native::advancement::advance;
    let (_fixture, case, base, frozen) = prepare();
    let config = advance_config();
    let root = case.temp.path().join("advanced");
    let first = advance(&base, &frozen, &config, &root).unwrap();
    assert!(!first.already_adopted);
    assert_eq!(first.state, reel_workflow::handoff::State::Waiting);
    let original = fs::read(root.join("admission.json")).unwrap();
    let header = fs::read(case.profile.index_root.join("claims.json")).unwrap();
    let replay = advance(&base, &frozen, &config, &root).unwrap();
    assert!(replay.already_adopted);
    assert_eq!(replay.admission_identity, first.admission_identity);
    assert_eq!(
        header,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
    fs::rename(
        root.join("admission.json"),
        root.join("preserved-admission.json"),
    )
    .unwrap();
    let recovered = advance(&base, &frozen, &config, &root).unwrap();
    assert_eq!(recovered.admission_identity, first.admission_identity);
    assert_eq!(fs::read(root.join("admission.json")).unwrap(), original);
    assert_eq!(
        header,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
    let plan: Plan = serde_json::from_slice(&fs::read(root.join("plan.json")).unwrap()).unwrap();
    let bindings = serde_json::from_slice(&fs::read(root.join("bindings.json")).unwrap()).unwrap();
    let frontier = graph::inspect(&plan, &bindings).unwrap();
    assert!(frontier.waiting.contains_key("expanded"));
    assert!(!root.join("stage-ticket.json").exists());
}

#[test]
fn advancement_refuses_invalid_targets_foreign_directories_and_changed_controls() {
    use reel_workflow_native::advancement::advance;
    let (_fixture, case, base, frozen) = prepare();
    let mut config = advance_config();
    config.target.owner = tig_store::Key::parse("wrong-owner").unwrap();
    let invalid = case.temp.path().join("invalid-target");
    assert_eq!(
        advance(&base, &frozen, &config, &invalid).unwrap_err().kind,
        ErrorKind::Denied
    );
    assert!(
        case.protected(&Object::from_bytes(b"synthetic input"))
            .is_empty()
    );
    config.target = retained_target();
    let foreign = case.temp.path().join("foreign");
    fs::create_dir(&foreign).unwrap();
    fs::write(foreign.join("preserve"), b"original").unwrap();
    assert_eq!(
        advance(&base, &frozen, &config, &foreign).unwrap_err().code,
        "advance-original-intent-missing"
    );
    assert_eq!(fs::read(foreign.join("preserve")).unwrap(), b"original");
    let root = case.temp.path().join("advanced");
    advance(&base, &frozen, &config, &root).unwrap();
    let header = fs::read(case.profile.index_root.join("claims.json")).unwrap();
    config.target.owner = tig_store::Key::parse("changed").unwrap();
    assert_eq!(
        advance(&base, &frozen, &config, &root).unwrap_err().code,
        "advance-immutable-control"
    );
    assert_eq!(
        header,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
    config.target = retained_target();
    fs::write(root.join("bindings.json"), b"partial control").unwrap();
    assert_eq!(
        advance(&base, &frozen, &config, &root).unwrap_err().code,
        "advance-immutable-control"
    );
    assert_eq!(
        fs::read(root.join("bindings.json")).unwrap(),
        b"partial control"
    );
    assert_eq!(
        header,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
}

#[test]
#[ignore = "Existing declared native pool only: frozen advancement, replay and protected batch staging; keeps original roots"]
fn native_expansion_advances_through_cli_and_exports_protected_prompt_batch() {
    use reel_workflow_native::{NativeProfile, staging};
    let descriptor =
        std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").expect("Set REEL_NATIVE_INPUT_FIXTURE");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(descriptor).unwrap()).unwrap();
    let object: Object = serde_json::from_value(value["object"].clone()).unwrap();
    let native: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(value["source"].clone()).unwrap();
    let mut fixture = Fixture::with_object(object);
    fixture.temp.disable_cleanup(true);
    let git_bytes = fs::read(&native.pool.git).unwrap();
    fixture.profile.git = Tool {
        path: native.pool.git.clone(),
        sha256: digest(&git_bytes),
        bytes: git_bytes.len() as u64,
    };
    let mut case = AdmissionCase::with_store(&fixture, native.store.clone());
    case.temp.disable_cleanup(true);
    let control = case.source.source.pool.control.clone();
    case.source = NativeProfile {
        snapshot: fixture.profile.clone(),
        source: native,
        annex: serde_json::from_value(value["annex"].clone()).unwrap(),
    };
    case.source.source.pool.control = control;
    eprintln!(
        "Native advancement root: {}; source: {}",
        case.temp.path().display(),
        fixture.temp.path().display()
    );
    let evidence = std::env::var_os("REEL_ADVANCE_EVIDENCE_DIRECTORY").map(PathBuf::from);
    if let Some(evidence) = &evidence {
        fs::write(
            evidence.join("original-operation-roots.json"),
            serde_json::to_vec(&json!({"root":case.temp.path(),"source":fixture.temp.path()}))
                .unwrap(),
        )
        .unwrap();
    }
    let (base, frozen) = freeze(&fixture, &case);
    let root = case.temp.path();
    let executable = std::env::var_os("REEL_NATIVE_HANDOFF_TOOL")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_reel-workflow-native")));
    let exe_bytes = fs::read(&executable).unwrap();
    let config = reel_workflow_native::advancement::Config {
        schema: "reel.workflow-advance-config.v1".into(),
        runtime: Tool {
            path: executable.clone(),
            sha256: digest(&exe_bytes),
            bytes: exe_bytes.len() as u64,
        },
        target: retained_target(),
    };
    let base_path = root.join("base.json");
    let frozen_path = root.join("frozen.json");
    let config_path = root.join("config.json");
    fs::write(&base_path, serde_json::to_vec(&base).unwrap()).unwrap();
    frozen.export(&frozen_path).unwrap();
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let pin = digest(&fs::read(&frozen_path).unwrap());
    let adopted = root.join("adopted");
    let invoke = || {
        let output = Command::new(&executable)
            .arg("advance-expanded")
            .arg(&base_path)
            .arg(&frozen_path)
            .arg(&pin)
            .arg(&config_path)
            .arg(&adopted)
            .arg("--apply")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    assert_eq!(invoke()["summary"]["already_adopted"], false);
    let header = fs::read(case.profile.index_root.join("claims.json")).unwrap();
    assert_eq!(invoke()["summary"]["already_adopted"], true);
    assert_eq!(
        header,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
    let profile: reel_workflow_native::handoff::Profile =
        serde_json::from_slice(&fs::read(adopted.join("profile.json")).unwrap()).unwrap();
    let plan: Plan = serde_json::from_slice(&fs::read(adopted.join("plan.json")).unwrap()).unwrap();
    let mut bindings: graph::Bindings =
        serde_json::from_slice(&fs::read(adopted.join("bindings.json")).unwrap()).unwrap();
    let scratch = root.join("scratch");
    fs::create_dir(&scratch).unwrap();
    let stage = staging::prepare(
        &frozen.expansion.packet,
        &frozen.source,
        &frozen.admission,
        &profile.binding.admission,
        &staging::Profile {
            schema: "reel.workflow-staging.v1".into(),
            parent: scratch,
            maximum_bytes: 8 * 1024 * 1024,
            capacity_policy: reel_assets::admission::Policy {
                minimum_free_bytes: 65536,
                minimum_free_basis_points: 0,
                max_object_bytes: 512 * 1024 * 1024,
                ..Default::default()
            },
        },
    )
    .unwrap();
    let stage_path = root.join("staging-ticket.json");
    let bytes = stage.canonical_bytes().unwrap();
    fs::write(&stage_path, &bytes).unwrap();
    bindings.tasks.get_mut("expanded").unwrap().staging_ticket = Some(graph::PinnedFile {
        path: stage_path,
        sha256: digest(&bytes),
    });
    fs::write(
        root.join("staged-bindings.json"),
        serde_json::to_vec(&bindings).unwrap(),
    )
    .unwrap();
    let batch = root.join("ready.json");
    assert_eq!(
        graph::export_ready(&plan, &bindings, &["expanded".into()], &batch).unwrap(),
        1
    );
    assert!(stage.directory().join("input-bindings.json").is_file());
    assert_eq!(
        reel_workflow::handoff::Journal::open(&adopted.join("journal"))
            .unwrap()
            .state(),
        reel_workflow::handoff::State::Waiting
    );
    if let Some(evidence) = evidence {
        fs::copy(&batch, evidence.join("ready.json")).unwrap();
        fs::write(evidence.join("qualification-result.json"), serde_json::to_vec(&json!({"root":root,"source":fixture.temp.path(),"adopted":adopted,"workspace":stage.directory(),"same_operation_replayed":true,"native_inputs_staged":true,"ready_tasks":1,"provider_invoked":false,"claims_released":false,"scratch_retired":false})).unwrap()).unwrap();
    }
}
