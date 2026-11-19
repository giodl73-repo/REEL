//! Actual runner waves, retained parent completion, and protected waiting jobs.
use super::*;
use reel_workflow_native::{NativeProfile, advancement, graph, staging};

fn run(executable: &Path, args: &[&std::ffi::OsStr]) -> serde_json::Value {
    let output = Command::new(executable).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
#[ignore = "Explicit existing native pool: runner waves, staged prompt batch, synthetic retained response and newly unlocked dependent; preserves roots"]
fn recipe_runner_unlocks_a_new_wave_after_retained_return_and_preserves_other_waiting_jobs() {
    runner_case(false);
}
#[test]
#[ignore = "Existing declared native pool: automatic runner staging, retained manual return and next-wave protected handoff; preserves roots"]
fn recipe_runner_automatically_stages_handoffs_and_the_unlocked_next_wave() {
    runner_case(true);
}
fn runner_case(automatic: bool) {
    let descriptor =
        std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").expect("Set REEL_NATIVE_INPUT_FIXTURE");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(descriptor).unwrap()).unwrap();
    let object: Object = serde_json::from_value(value["object"].clone()).unwrap();
    let native: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(value["source"].clone()).unwrap();
    let mut fixture = Fixture::with_object(object);
    fixture.temp.disable_cleanup(true);
    let bytes = fs::read(&native.pool.git).unwrap();
    fixture.profile.git = Tool {
        path: native.pool.git.clone(),
        sha256: digest(&bytes),
        bytes: bytes.len() as u64,
    };
    let mut case = AdmissionCase::with_extra_slot(&fixture, native.store.clone(), None, true);
    case.temp.disable_cleanup(true);
    let control = case.source.source.pool.control.clone();
    case.source = NativeProfile {
        snapshot: fixture.profile.clone(),
        source: native,
        annex: serde_json::from_value(value["annex"].clone()).unwrap(),
    };
    case.source.source.pool.control = control;
    let root = case.temp.path();
    eprintln!(
        "Native recipe runner: {}; source: {}",
        root.display(),
        fixture.temp.path().display()
    );
    let evidence = std::env::var_os("REEL_RUNNER_EVIDENCE_DIRECTORY").map(PathBuf::from);
    if let Some(directory) = &evidence {
        fs::write(
            directory.join("original-operation-roots.json"),
            serde_json::to_vec(&json!({"root":root,"source":fixture.temp.path()})).unwrap(),
        )
        .unwrap();
    }
    let executable = std::env::var_os("REEL_NATIVE_HANDOFF_TOOL")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_reel-workflow-native")));
    let bytes = fs::read(&executable).unwrap();
    let handoff = advancement::Config {
        schema: "reel.workflow-advance-config.v1".into(),
        runtime: Tool {
            path: executable.clone(),
            sha256: digest(&bytes),
            bytes: bytes.len() as u64,
        },
        target: retained_target(),
    };
    let (recipe, mut config, bindings) =
        frozen_expansion::runner_configuration(&fixture, &case, &handoff);
    if automatic {
        let parent = root.join("automatic-inputs");
        fs::create_dir(&parent).unwrap();
        for step in config.steps.values_mut() {
            step.execution = Some(reel_workflow_native::dispatch::Config {
                provider: None,
                schema: "reel.workflow-dispatch.v1".into(),
                input_staging: staging::Profile {
                    schema: "reel.workflow-staging.v1".into(),
                    parent: parent.clone(),
                    maximum_bytes: 8 * 1024 * 1024,
                    capacity_policy: reel_assets::admission::Policy {
                        minimum_free_bytes: 65536,
                        minimum_free_basis_points: 0,
                        max_object_bytes: 512 * 1024 * 1024,
                        ..Default::default()
                    },
                },
                worker_staging: None,
            });
        }
    }
    for (name, bytes) in [
        ("recipe.json", serde_json::to_vec(&recipe).unwrap()),
        ("base.json", serde_json::to_vec(&recipe.base).unwrap()),
        ("config.json", serde_json::to_vec(&config).unwrap()),
        ("base-bindings.json", serde_json::to_vec(&bindings).unwrap()),
    ] {
        fs::write(root.join(name), bytes).unwrap();
    }
    let runner = root.join("runner");
    let invoke = || {
        run(
            &executable,
            &[
                "advance-recipe".as_ref(),
                root.join("recipe.json").as_os_str(),
                root.join("base.json").as_os_str(),
                root.join("base-bindings.json").as_os_str(),
                root.join("config.json").as_os_str(),
                runner.as_os_str(),
                "16".as_ref(),
                "--apply".as_ref(),
            ],
        )
    };
    let first = invoke();
    assert_eq!(first["summary"]["advanced"], json!(["one", "two"]));
    if automatic {
        assert_eq!(first["summary"]["dispatched"].as_array().unwrap().len(), 2);
        assert!(runner.join("task-001/execution/ready.json").is_file());
        assert!(runner.join("task-002/execution/ready.json").is_file());
    }
    let header = fs::read(case.profile.index_root.join("claims.json")).unwrap();
    assert_eq!(invoke()["summary"]["advanced"], json!([]));
    assert_eq!(
        header,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(runner.join("state-002.json")).unwrap()).unwrap();
    let plan: Plan = serde_json::from_value(state["plan"].clone()).unwrap();
    let mut bindings: graph::Bindings = serde_json::from_value(state["bindings"].clone()).unwrap();
    let scratch = root.join("scratch");
    fs::create_dir(&scratch).unwrap();
    for id in ["one", "two"] {
        let binding = &bindings.tasks[id];
        let profile: reel_workflow_native::handoff::Profile =
            serde_json::from_slice(&fs::read(&binding.profile.path).unwrap()).unwrap();
        let packet = reel_workflow::handoff::Journal::open(&binding.journal)
            .unwrap()
            .ticket()
            .packet
            .clone();
        let ticket = staging::prepare(
            &packet,
            &profile.source,
            &profile.admission,
            &profile.binding.admission,
            &staging::Profile {
                schema: "reel.workflow-staging.v1".into(),
                parent: scratch.clone(),
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
        let bytes = ticket.canonical_bytes().unwrap();
        let path = root.join(format!("{id}-stage.json"));
        fs::write(&path, &bytes).unwrap();
        bindings.tasks.get_mut(id).unwrap().staging_ticket = Some(graph::PinnedFile {
            path,
            sha256: digest(&bytes),
        });
    }
    let plan_path = root.join("staged-plan.json");
    let bindings_path = root.join("staged-bindings.json");
    fs::write(&plan_path, serde_json::to_vec(&plan).unwrap()).unwrap();
    fs::write(&bindings_path, serde_json::to_vec(&bindings).unwrap()).unwrap();
    let batch = root.join("ready.json");
    assert_eq!(
        graph::export_ready(&plan, &bindings, &["one".into(), "two".into()], &batch).unwrap(),
        2
    );
    let packet = reel_workflow::handoff::Journal::open(&bindings.tasks["one"].journal)
        .unwrap()
        .ticket()
        .packet
        .clone();
    let (result, returns) = response_fixture(&packet, &root.join("returns"));
    let result_path = root.join("result.json");
    let returns_path = root.join("returns.json");
    fs::write(&result_path, serde_json::to_vec(&result).unwrap()).unwrap();
    fs::write(&returns_path, serde_json::to_vec(&returns).unwrap()).unwrap();
    let (import_plan, import_bindings, import_batch) = if automatic {
        let directory = runner.join("task-001/execution");
        (
            directory.join("plan.json"),
            directory.join("bindings.json"),
            directory.join("ready.json"),
        )
    } else {
        (plan_path.clone(), bindings_path.clone(), batch.clone())
    };
    run(
        &executable,
        &[
            "import-ready".as_ref(),
            import_plan.as_os_str(),
            import_bindings.as_os_str(),
            import_batch.as_os_str(),
            digest(&fs::read(&import_batch).unwrap()).as_ref(),
            "one".as_ref(),
            result_path.as_os_str(),
            returns_path.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    assert_eq!(
        reel_workflow::handoff::Journal::open(&bindings.tasks["one"].journal)
            .unwrap()
            .state(),
        reel_workflow::handoff::State::Retained
    );
    let second = invoke();
    assert_eq!(second["summary"]["advanced"], json!(["after-one"]));
    assert_eq!(second["summary"]["checkpoint"], 3);
    if automatic {
        assert!(runner.join("task-003/execution/ready.json").is_file());
    }
    assert_eq!(
        reel_workflow::handoff::Journal::open(&bindings.tasks["two"].journal)
            .unwrap()
            .state(),
        reel_workflow::handoff::State::Waiting
    );
    let header = fs::read(case.profile.index_root.join("claims.json")).unwrap();
    assert_eq!(invoke()["summary"]["advanced"], json!([]));
    assert_eq!(
        header,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
    if let Some(directory) = evidence {
        fs::write(directory.join("qualification-result.json"),serde_json::to_vec(&json!({"root":root,"source":fixture.temp.path(),"first_wave":first,"second_wave":second,"automatic_staging":automatic,"protected_batch_tasks":2,"retained_parent":"one","other_waiting_job_preserved":true,"unchanged_claims_on_noop":true,"real_provider_invoked":false,"scratch_retired":false})).unwrap()).unwrap();
    }
}
