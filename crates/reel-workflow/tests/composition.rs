//! Synthetic complete authored closures; no native media or admission.
use reel_workflow::{
    composition::*,
    expansion::{Input, TaskTemplate},
    *,
};
use std::collections::BTreeMap;
fn fixture() -> Declaration {
    let commit = "a".repeat(40);
    let remaining = 7_936_104_893u64 - 3_360_000_000 - 2_900_000_000;
    let mut resources = BTreeMap::new();
    let mut assets = Vec::new();
    for i in 0..600 {
        let bytes = match i {
            0 => 3_360_000_000,
            1 => 2_900_000_000,
            599 => remaining / 598 + remaining % 598,
            _ => remaining / 598,
        };
        let asset = Artifact {
            logical_id: format!("source/{i}"),
            sha256: digest(format!("synthetic-{i}").as_bytes()),
            bytes,
            media_type: "application/octet-stream".into(),
        };
        resources.insert(
            format!("resource-{i}"),
            Resource {
                context_commit: commit.clone(),
                artifact: asset.clone(),
            },
        );
        assets.push(asset);
    }
    let outputs = |bytes| {
        BTreeMap::from([(
            "master".into(),
            OutputContract {
                media_type: "application/octet-stream".into(),
                max_bytes: bytes,
                required: true,
            },
        )])
    };
    let seed = Task {
        id: "seed".into(),
        operation: Operation::TextProposal,
        tool: None,
        language: None,
        prompt: "Synthetic existing anchor.".into(),
        inputs: vec![assets[599].clone()],
        native_inputs: None,
        upstream_inputs: None,
        depends_on: vec![],
        outputs: outputs(100),
    };
    let base = Plan {
        schema: PLAN_SCHEMA.into(),
        task_snapshots: Default::default(),
        project: "synthetic-composition".into(),
        source_commit: commit,
        tasks: vec![seed],
    };
    let mut steps = Vec::new();
    for part in 0..7 {
        steps.push(TaskTemplate {
            id: format!("part-{part}"),
            operation: Operation::TextProposal,
            language: Some("es".into()),
            prompt: "Synthetic component intent; no producer.".into(),
            depends_on: vec!["seed".into()],
            inputs: assets[..599]
                .iter()
                .enumerate()
                .filter(|(i, _)| i % 7 == part)
                .map(|(_, a)| Input::Fixed {
                    artifact: a.clone(),
                })
                .collect(),
            outputs: outputs(4 * 1024 * 1024 * 1024),
            tool: None,
            streaming: None,
        });
    }
    let components: Vec<_> = steps.iter().map(|s| s.id.clone()).collect();
    steps.push(TaskTemplate {
        id: "episode-es".into(),
        operation: Operation::TextProposal,
        language: Some("es".into()),
        prompt: "Synthetic composition intent; no producer conformance.".into(),
        depends_on: components.clone(),
        inputs: components
            .iter()
            .map(|id| Input::Parent {
                reference: upstream::Reference {
                    task_id: id.clone(),
                    artifact: upstream::Selector::Output {
                        name: "master".into(),
                    },
                },
            })
            .collect(),
        outputs: outputs(8 * 1024 * 1024 * 1024),
        tool: None,
        streaming: Some(upstream::Streaming {
            schema: "reel.workflow-upstream-streaming.v1".into(),
            input_bytes: 32 * 1024 * 1024 * 1024,
            input_object_bytes: 4 * 1024 * 1024 * 1024,
        }),
    });
    Declaration {
        schema: "reel.workflow-composition.v1".into(),
        format: Format::Film,
        recipe: recipe::Recipe {
            schema: "reel.workflow-recipe.v2".into(),
            base,
            steps,
        },
        resources,
        editions: BTreeMap::from([(
            "es".into(),
            Edition {
                language: "es".into(),
                components,
                controls: vec![],
                final_task: "episode-es".into(),
            },
        )]),
        limits: Limits {
            source_bytes: 8 * 1024 * 1024 * 1024,
            input_bytes: 32 * 1024 * 1024 * 1024,
            input_object_bytes: 4 * 1024 * 1024 * 1024,
            inputs_per_task: 96,
        },
    }
}
fn clone_declaration(value: &Declaration) -> Declaration {
    decode(&serde_json::to_vec(value).unwrap()).unwrap()
}
#[test]
fn complete_six_hundred_resource_recipe_has_explicit_bounded_components() {
    let declaration = fixture();
    let report = check(&declaration).unwrap();
    assert_eq!(report.resources, 600);
    assert_eq!(report.logical_source_bytes, 7_936_104_893);
    assert_eq!(report.distinct_objects, 600);
    assert_eq!(report.stages.len(), 9);
    assert!(report.stages.iter().all(|s| s.input_count <= 96));
    assert_eq!(
        report
            .stages
            .iter()
            .find(|s| s.task_id == "episode-es")
            .unwrap()
            .input_ceiling_bytes,
        28 * 1024 * 1024 * 1024
    );
    assert!(
        !report.native_bindings_verified
            && !report.production_admitted
            && !report.producer_order_verified
            && !report.task_completed
    );
    let mut changed = clone_declaration(&declaration);
    changed
        .editions
        .get_mut("es")
        .unwrap()
        .components
        .swap(0, 1);
    let reordered = check(&changed).unwrap();
    assert_ne!(report.declaration_sha256, reordered.declaration_sha256);
    assert_eq!(report.recipe_sha256, reordered.recipe_sha256);
    assert!(!reordered.producer_order_verified);
}
#[test]
fn missing_extra_duplicate_and_stale_context_resources_refuse() {
    let base = fixture();
    for mutation in ["missing", "unused", "duplicate", "context", "length"] {
        let mut d = clone_declaration(&base);
        match mutation {
            "missing" => {
                d.resources.remove("resource-123");
            }
            "unused" => {
                d.resources.insert(
                    "unused".into(),
                    Resource {
                        context_commit: "a".repeat(40),
                        artifact: Artifact {
                            logical_id: "unused/asset".into(),
                            sha256: digest(b"unused"),
                            bytes: 1,
                            media_type: "text/plain".into(),
                        },
                    },
                );
            }
            "duplicate" => {
                d.resources
                    .insert("duplicate".into(), d.resources["resource-0"].clone());
            }
            "context" => d.resources.get_mut("resource-0").unwrap().context_commit = "b".repeat(40),
            _ => {
                let sha = d.resources["resource-0"].artifact.sha256.clone();
                d.resources.get_mut("resource-1").unwrap().artifact.sha256 = sha;
            }
        }
        assert!(check(&d).is_err(), "{mutation}");
    }
}
#[test]
fn disconnected_stages_language_errors_and_dependency_only_components_refuse() {
    let base = fixture();
    let mut orphan = clone_declaration(&base);
    let mut extra = orphan.recipe.steps[0].clone();
    extra.id = "orphan".into();
    orphan.recipe.steps.push(extra);
    assert_eq!(
        check(&orphan).err().unwrap().code,
        "composition-unconsumed-step"
    );
    let mut d = clone_declaration(&base);
    d.recipe.steps[0].language = Some("en".into());
    assert_eq!(check(&d).err().unwrap().code, "composition-component-scope");
    let mut d = clone_declaration(&base);
    d.recipe.steps.last_mut().unwrap().inputs.remove(0);
    assert_eq!(
        check(&d).err().unwrap().code,
        "composition-edition-parent-set"
    );
    let mut d = clone_declaration(&base);
    d.limits.inputs_per_task = 64;
    assert_eq!(check(&d).err().unwrap().code, "composition-stage-inputs");
    let mut d = clone_declaration(&base);
    d.recipe.steps.last_mut().unwrap().streaming = None;
    assert_eq!(check(&d).err().unwrap().code, "composition-upstream-budget");
}
#[test]
fn shared_bytes_keep_distinct_bindings_and_scoped_task_versions_keep_their_contexts() {
    let mut d = fixture();
    let original = check(&d).unwrap();
    let shared = d.resources["resource-2"].artifact.clone();
    d.resources.get_mut("resource-3").unwrap().artifact.sha256 = shared.sha256.clone();
    for step in &mut d.recipe.steps {
        for input in &mut step.inputs {
            if let Input::Fixed { artifact } = input {
                if artifact.logical_id == "source/3" {
                    artifact.sha256 = shared.sha256.clone();
                }
            }
        }
    }
    let report = check(&d).unwrap();
    assert_eq!(report.resources, 600);
    assert_eq!(report.distinct_objects, 599);
    assert_eq!(report.logical_source_bytes, original.logical_source_bytes);
    assert_eq!(
        report.distinct_object_bytes,
        original.distinct_object_bytes - shared.bytes
    );
    d.recipe.base.schema = SCOPED_PLAN_SCHEMA.into();
    d.recipe.base.source_commit = "b".repeat(40);
    d.recipe
        .base
        .task_snapshots
        .insert("seed".into(), "a".repeat(40));
    for (binding, resource) in &mut d.resources {
        if binding != "resource-599" {
            resource.context_commit = "b".repeat(40);
        }
    }
    let newer = Artifact {
        logical_id: "source/599".into(),
        sha256: digest(b"new version"),
        bytes: 11,
        media_type: "application/octet-stream".into(),
    };
    d.recipe.steps[0].inputs.push(Input::Fixed {
        artifact: newer.clone(),
    });
    d.resources.insert(
        "new-version".into(),
        Resource {
            context_commit: "b".repeat(40),
            artifact: newer,
        },
    );
    let report = check(&d).unwrap();
    assert_eq!(report.resources, 601);
    assert_eq!(
        report
            .stages
            .iter()
            .find(|s| s.task_id == "seed")
            .unwrap()
            .context_commit,
        "a".repeat(40)
    );
    assert_eq!(
        report
            .stages
            .iter()
            .find(|s| s.task_id == "part-0")
            .unwrap()
            .context_commit,
        "b".repeat(40)
    );
}
#[test]
fn cli_checks_only_authored_controls_and_exports_a_schedulable_recipe_without_overwrite() {
    let persistent =
        std::env::var_os("REEL_COMPOSITION_EVIDENCE_DIRECTORY").map(std::path::PathBuf::from);
    let temp = persistent.is_none().then(|| tempfile::tempdir().unwrap());
    let root = persistent.unwrap_or_else(|| temp.as_ref().unwrap().path().to_path_buf());
    assert!(root.is_absolute() && root.is_dir());
    let cli = std::env::var_os("REEL_COMPOSITION_CLI")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_reel-workflow").into());
    let declaration = fixture();
    let path = root.join("composition.json");
    use std::io::Write;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    output
        .write_all(&serde_json::to_vec(&declaration).unwrap())
        .unwrap();
    output.sync_all().unwrap();
    drop(output);
    let checked = std::process::Command::new(&cli)
        .arg("check-composition")
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stdout)
    );
    let flags: serde_json::Value = decode(&checked.stdout).unwrap();
    assert_eq!(flags["report"]["resources"], 600);
    let recipe = root.join("recipe.json");
    let report = root.join("report.json");
    let export = || {
        std::process::Command::new(&cli)
            .arg("export-composition")
            .arg(&path)
            .arg(&recipe)
            .arg(&report)
            .output()
            .unwrap()
    };
    assert!(export().status.success());
    let raw = std::fs::read(&recipe).unwrap();
    let restored: recipe::Recipe = decode(&raw).unwrap();
    recipe::check(&restored).unwrap();
    assert_eq!(
        canonical_digest(&restored).unwrap(),
        canonical_digest(&declaration.recipe).unwrap()
    );
    let repeat = export();
    assert!(!repeat.status.success());
    assert_eq!(std::fs::read(&recipe).unwrap(), raw);
    if temp.is_none() {
        let summary = serde_json::json!({"schema":"reel.workflow-composition-cli-qualification.v1","resources":600,
            "logical_source_bytes":flags["report"]["logical_source_bytes"],"recipe_sha256":digest(&raw),
            "report_sha256":digest(&std::fs::read(&report).unwrap()),"check_stdout_sha256":digest(&checked.stdout),
            "overwrite_refused":true,"production_admitted":false,"native_bindings_verified":false,"producer_order_verified":false});
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("qualification-result.json"))
            .unwrap();
        output
            .write_all(&serde_json::to_vec(&summary).unwrap())
            .unwrap();
        output.sync_all().unwrap();
    }
}

#[test]
fn final_control_parents_and_native_binding_descriptors_cannot_be_hidden() {
    let mut d = fixture();
    let final_task = d.recipe.steps.last_mut().unwrap();
    final_task.depends_on.push("seed".into());
    final_task.inputs.push(Input::Parent {
        reference: upstream::Reference {
            task_id: "seed".into(),
            artifact: upstream::Selector::Output {
                name: "master".into(),
            },
        },
    });
    assert_eq!(
        check(&d).err().unwrap().code,
        "composition-edition-parent-set"
    );
    d.editions.get_mut("es").unwrap().controls = vec!["seed".into()];
    check(&d).unwrap();
    let binding = Artifact {
        logical_id: "control/native-bindings".into(),
        sha256: digest(b"{}"),
        bytes: 2,
        media_type: "application/json".into(),
    };
    d.recipe.base.tasks[0].native_inputs = Some(binding.clone());
    assert_eq!(
        check(&d).err().unwrap().code,
        "composition-resource-binding"
    );
    d.resources.insert(
        "native-binding".into(),
        Resource {
            context_commit: "a".repeat(40),
            artifact: binding,
        },
    );
    let report = check(&d).unwrap();
    assert_eq!(report.resources, 601);
    assert!(!report.producer_closure_verified);
}

#[test]
fn bilingual_comic_editions_share_art_without_merging_edition_identity_or_order() {
    let mut d = fixture();
    d.format = Format::ComicIssue;
    for resource in d.resources.values_mut() {
        resource.artifact.bytes = 1;
    }
    d.recipe.base.tasks[0].inputs[0].bytes = 1;
    for step in &mut d.recipe.steps {
        for input in &mut step.inputs {
            if let Input::Fixed { artifact } = input {
                artifact.bytes = 1;
            }
        }
        for contract in step.outputs.values_mut() {
            contract.max_bytes = 2 * 1024 * 1024;
        }
        step.streaming = None;
        if step.id.starts_with("part-") {
            step.language = None;
        }
    }
    let mut en = d.recipe.steps.last().unwrap().clone();
    en.id = "issue-en".into();
    en.language = Some("en".into());
    d.recipe.steps.push(en);
    let mut edition = d.editions["es"].clone();
    edition.language = "en".into();
    edition.final_task = "issue-en".into();
    edition.components.reverse();
    d.editions.insert("en".into(), edition);
    let report = check(&d).unwrap();
    assert_eq!(report.resources, 600);
    assert_eq!(report.logical_source_bytes, 600);
    assert_eq!(report.editions.len(), 2);
    assert_eq!(report.stages.len(), 10);
    assert_ne!(
        report.editions["es"].components,
        report.editions["en"].components
    );
    assert!(!report.producer_order_verified && !report.producer_closure_verified);
}

#[test]
fn tool_roles_and_program_pins_belong_to_the_authored_closure() {
    let mut d = fixture();
    let control = |id: &str| Artifact {
        logical_id: id.into(),
        sha256: digest(b"{}"),
        bytes: 2,
        media_type: "application/json".into(),
    };
    let publication = control("control/publication");
    let request = control("control/request");
    let executable = Artifact {
        logical_id: "tools/renderer".into(),
        sha256: digest(b"synthetic tool"),
        bytes: 1024,
        media_type: "application/octet-stream".into(),
    };
    let auxiliary = Artifact {
        logical_id: "tools/auxiliary".into(),
        sha256: digest(b"synthetic auxiliary"),
        bytes: 2048,
        media_type: "application/octet-stream".into(),
    };
    let source = d.resources["resource-0"].artifact.clone();
    let task = &mut d.recipe.steps[0];
    task.operation = Operation::DeterministicTool;
    task.prompt.clear();
    task.tool = Some(expansion::Tool {
        capability: ToolCapability::ComicRender,
        executable: executable.clone(),
        auxiliary_executables: BTreeMap::from([("auxiliary".into(), auxiliary.clone())]),
        publication: Input::Fixed {
            artifact: publication.clone(),
        },
        request: Input::Fixed {
            artifact: request.clone(),
        },
        edition_id: "es".into(),
        sources: BTreeMap::from([("data/source.bin".into(), Input::Fixed { artifact: source })]),
        part: None,
    });
    assert_eq!(
        check(&d).err().unwrap().code,
        "composition-resource-binding"
    );
    for (name, artifact) in [
        ("publication", publication),
        ("request", request),
        ("renderer", executable),
        ("auxiliary", auxiliary),
    ] {
        d.resources.insert(
            name.into(),
            Resource {
                context_commit: "a".repeat(40),
                artifact,
            },
        );
    }
    let report = check(&d).unwrap();
    assert_eq!(report.resources, 604);
    let stage = report
        .stages
        .iter()
        .find(|s| s.task_id == "part-0")
        .unwrap();
    assert!(
        stage.fixed_bindings.contains(&"renderer".into())
            && stage.fixed_bindings.contains(&"auxiliary".into())
    );
    assert!(!report.producer_closure_verified);
}
