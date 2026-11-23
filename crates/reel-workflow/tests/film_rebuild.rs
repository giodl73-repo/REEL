use reel_workflow::{film_rebuild::*, *};
use std::{collections::BTreeMap, fs, process::Command};
fn asset(id: &str, raw: &[u8], media: &str) -> Artifact {
    Artifact {
        logical_id: id.into(),
        sha256: digest(raw),
        bytes: raw.len() as u64,
        media_type: media.into(),
    }
}
fn task(id: &str) -> Task {
    Task {
        id: id.into(),
        native_inputs: None,
        upstream_inputs: None,
        operation: Operation::TextProposal,
        tool: None,
        language: Some("es".into()),
        prompt: "Synthetic control candidate".into(),
        inputs: vec![],
        depends_on: vec![],
        outputs: BTreeMap::from([(
            "job".into(),
            OutputContract {
                media_type: "application/json".into(),
                max_bytes: 4096,
                required: true,
            },
        )]),
    }
}
fn result(packet: &Packet, artifact: Artifact) -> ReturnedResult {
    ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: "a1".into(),
        worker_receipt: None,
        artifacts: BTreeMap::from([("job".into(), artifact)]),
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
    }
}
fn fixture() -> Request {
    let old = asset("picture/C101/v1", b"old picture", "image/png");
    let new = asset("candidate/C101", b"new picture", "image/png");
    let job = asset("scene/job/v1", b"original job", "application/json");
    let request = asset("scene/request", b"original request", "application/json");
    let runtime = asset("runtime/reel", b"runtime", "application/octet-stream");
    let mut render = task("render-es");
    render.operation = Operation::DeterministicTool;
    render.prompt.clear();
    render.inputs = vec![old.clone(), job.clone(), request.clone()];
    render.tool = Some(ToolContract {
        schema: "reel.workflow-tool.v1".into(),
        capability: ToolCapability::FilmRender,
        executable: runtime,
        auxiliary_executables: BTreeMap::new(),
        publication: job.clone(),
        request,
        edition_id: "es".into(),
        source_bindings: BTreeMap::from([
            ("picture.png".into(), old.logical_id.clone()),
            ("job.json".into(), job.logical_id.clone()),
            ("request.json".into(), "scene/request".into()),
        ]),
        part: None,
    });
    let mut en = render.clone();
    en.id = "render-en".into();
    en.language = Some("en".into());
    en.tool.as_mut().unwrap().edition_id = "en".into();
    let mut untouched = task("unaffected");
    untouched.language = Some("fr".into());
    let mut downstream = task("assemble");
    downstream.depends_on = vec![render.id.clone(), en.id.clone()];
    let baseline = Plan {
        schema: PLAN_SCHEMA.into(),
        project: "synthetic".into(),
        source_commit: "a".repeat(40),
        task_snapshots: BTreeMap::new(),
        tasks: vec![render, en, untouched, downstream],
    };
    let mut picture = task("fix-image");
    picture.operation = Operation::ImageCandidate;
    picture.inputs = vec![old.clone()];
    picture.outputs.get_mut("job").unwrap().media_type = "image/png".into();
    let picture_plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: "synthetic".into(),
        source_commit: "b".repeat(40),
        task_snapshots: BTreeMap::new(),
        tasks: vec![picture.clone()],
    };
    let packet = make_packet(&picture_plan, "fix-image", BTreeMap::new()).unwrap();
    let image = Parent {
        result: result(&packet, new.clone()),
        packet,
    };
    let mut controls = BTreeMap::new();
    let mut consumers = Vec::new();
    for language in ["es", "en"] {
        let id = format!("control-{language}");
        let mut control = task(&id);
        control.language = Some(language.into());
        control.inputs = vec![job.clone(), new.clone()];
        control.depends_on = vec!["fix-image".into()];
        let plan = Plan {
            schema: PLAN_SCHEMA.into(),
            project: "synthetic".into(),
            source_commit: "b".repeat(40),
            task_snapshots: BTreeMap::new(),
            tasks: vec![picture.clone(), control],
        };
        let packet = make_packet(
            &plan,
            &id,
            BTreeMap::from([(
                "fix-image".into(),
                DependencyResult {
                    result_sha256: canonical_digest(&image.result).unwrap(),
                    artifacts: image.result.artifacts.clone(),
                },
            )]),
        )
        .unwrap();
        let output = asset(
            &format!("derived/{language}/job"),
            language.as_bytes(),
            "application/json",
        );
        controls.insert(
            id.clone(),
            Parent {
                result: result(&packet, output),
                packet,
            },
        );
        consumers.push(Consumer {
            task_id: format!("render-{language}"),
            control_task: id,
            control_output: "job".into(),
            replacement_path: "replacement.png".into(),
            baseline_job_path: "baseline-job.json".into(),
            native_inputs: None,
        });
    }
    Request {
        schema: "reel.workflow-film-rebuild.v1".into(),
        baseline,
        old_subject: old,
        image,
        image_output: "job".into(),
        controls,
        consumers,
    }
}
#[test]
fn both_languages_get_new_exact_parent_inputs_and_stale_downstream_is_withheld() {
    let q = fixture();
    let old = canonical_digest(&q.baseline).unwrap();
    let prepared = prepare(&q).unwrap();
    assert_eq!(prepared.affected_languages, vec!["en", "es"]);
    assert_eq!(prepared.pending_consumers, vec!["assemble"]);
    assert_eq!(prepared.unaffected, vec!["unaffected"]);
    assert_eq!(prepared.native_binding_required.len(), 2);
    assert!(
        !prepared.selection_verified
            && !prepared.approvals_transferred
            && !prepared.consumers_rebuilt
    );
    assert!(!prepared.plan.tasks.iter().any(|t| t.id == "assemble"));
    for id in ["render-es", "render-en"] {
        let before = q.baseline.tasks.iter().find(|t| t.id == id).unwrap();
        let after = prepared.plan.tasks.iter().find(|t| t.id == id).unwrap();
        assert_eq!(
            before.tool.as_ref().unwrap().request,
            after.tool.as_ref().unwrap().request
        );
        assert_eq!(before.language, after.language);
        assert!(after.native_inputs.is_none());
        assert!(after.inputs.contains(&q.old_subject));
        assert!(after.inputs.contains(&q.image.result.artifacts["job"]));
        let deps = BTreeMap::from([
            (
                "fix-image".into(),
                DependencyResult {
                    result_sha256: canonical_digest(&q.image.result).unwrap(),
                    artifacts: q.image.result.artifacts.clone(),
                },
            ),
            (format!("control-{}", after.language.as_ref().unwrap()), {
                let p = &q.controls[&format!("control-{}", after.language.as_ref().unwrap())];
                DependencyResult {
                    result_sha256: canonical_digest(&p.result).unwrap(),
                    artifacts: p.result.artifacts.clone(),
                }
            }),
        ]);
        let packet = make_packet(&prepared.plan, id, deps).unwrap();
        assert_ne!(
            packet.task_key,
            make_packet(&q.baseline, id, BTreeMap::new())
                .unwrap()
                .task_key
        );
        let control = &q.controls[&format!("control-{}", after.language.as_ref().unwrap())];
        upstream::bind(
            &packet,
            &prepared.bindings[id],
            &BTreeMap::from([
                (
                    "fix-image".into(),
                    upstream::Parent {
                        packet: &q.image.packet,
                        result: &q.image.result,
                    },
                ),
                (
                    control.packet.task.id.clone(),
                    upstream::Parent {
                        packet: &control.packet,
                        result: &control.result,
                    },
                ),
            ]),
        )
        .unwrap();
    }
    let unaffected = q
        .baseline
        .tasks
        .iter()
        .find(|t| t.id == "unaffected")
        .unwrap();
    assert_eq!(
        *unaffected,
        *prepared
            .plan
            .tasks
            .iter()
            .find(|t| t.id == "unaffected")
            .unwrap()
    );
    assert_eq!(old, canonical_digest(&q.baseline).unwrap());
}
#[test]
fn incomplete_languages_stale_control_wrong_clock_lane_and_alias_collisions_refuse() {
    for mutation in [
        "missing-consumer",
        "wrong-language",
        "wrong-subject",
        "alias",
        "stale-parent",
        "existing-upstream",
        "affected-prerequisite",
    ] {
        let mut q = fixture();
        match mutation {
            "affected-prerequisite" => {
                let es = q.consumers[0].task_id.clone();
                let en = q.consumers[1].task_id.clone();
                q.baseline
                    .tasks
                    .iter_mut()
                    .find(|t| t.id == en)
                    .unwrap()
                    .depends_on
                    .push(es);
            }
            "missing-consumer" => {
                q.consumers.pop();
            }
            "wrong-language" => {
                q.consumers[0].control_task = "control-en".into();
            }
            "wrong-subject" => {
                q.old_subject.sha256 = digest(b"wrong");
            }
            "alias" => {
                q.consumers[0].replacement_path = "picture.png".into();
            }
            "stale-parent" => {
                q.image.result.attempt_id = "other".into();
            }
            "existing-upstream" => {
                q.baseline.tasks[0].upstream_inputs =
                    Some(asset("previous/origins", b"{}", "application/json"));
            }
            _ => unreachable!(),
        }
        assert!(prepare(&q).is_err(), "{mutation}");
    }
}
#[test]
fn cli_exports_bounded_candidate_plan_and_never_overwrites_or_dispatches() {
    let root = tempfile::tempdir().unwrap();
    let raw = serde_json::to_vec(&fixture()).unwrap();
    let request = root.path().join("request.json");
    fs::write(&request, &raw).unwrap();
    let output = root.path().join("prepared");
    let call = || {
        Command::new(env!("CARGO_BIN_EXE_reel-workflow"))
            .arg("prepare-film-rebuild")
            .arg(&request)
            .arg(digest(&raw))
            .arg(&output)
            .output()
            .unwrap()
    };
    let result = call();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap()["consumers_rebuilt"],
        false
    );
    assert!(output.join("render-es-origins.json").exists());
    assert!(!call().status.success());
}
