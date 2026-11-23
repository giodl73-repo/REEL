//! Native catalogue tests. Staged synthetic acknowledgements are deliberately
//! not live retained proofs; no payload pool is initialized by these tests.
use super::*;
use reel_workflow_native::{
    admission, film_rebuild as native, handoff, response, retention, upstream,
};
use std::collections::BTreeSet;

fn returned(packet: &Packet, output: Artifact) -> ReturnedResult {
    ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: "attempt-1".into(),
        worker_receipt: None,
        artifacts: BTreeMap::from([("output".into(), output)]),
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
    }
}
fn fixture() -> (Fixture, AdmissionCase, native::Request) {
    let mut f = Fixture::new();
    let root = f.temp.path();
    let mut record: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("records/input.tig")).unwrap()).unwrap();
    record["id"] = json!("controls/request");
    record["streams"] = json!([]);
    write(root, "records/request.tig", record);
    fs::copy(
        root.join("payloads/source.bin"),
        root.join("payloads/request.bin"),
    )
    .unwrap();
    f.profile.records.push("records/request.tig".into());
    f.profile.bindings.insert(
        "controls/request".into(),
        Binding {
            selected: LogicalPath::parse("payloads/request.bin").unwrap(),
            streams: BTreeMap::new(),
        },
    );
    f.profile
        .input_streams
        .insert("controls/request".into(), None);
    let mut request = f.packet.task.inputs[0].clone();
    request.logical_id = "controls/request".into();
    f.packet.task.inputs.push(request.clone());
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "synthetic request"]);
    f.profile.commit = git(root, &["rev-parse", "HEAD"]);
    f.repin();
    let a = f.packet.task.inputs[0].clone();
    let mut task = f.packet.task.clone();
    task.id = "scene-es".into();
    task.operation = Operation::DeterministicTool;
    task.prompt.clear();
    task.tool = Some(ToolContract {
        schema: "reel.workflow-tool.v1".into(),
        capability: ToolCapability::FilmRender,
        executable: Artifact {
            logical_id: "runtime/reel".into(),
            sha256: digest(b"runtime"),
            bytes: 7,
            media_type: "application/octet-stream".into(),
        },
        auxiliary_executables: BTreeMap::new(),
        publication: a.clone(),
        request: request.clone(),
        edition_id: "es".into(),
        source_bindings: BTreeMap::from([
            ("job.json".into(), a.logical_id.clone()),
            ("request.json".into(), request.logical_id),
        ]),
        part: None,
    });
    let mut en = task.clone();
    en.id = "scene-en".into();
    en.language = Some("en".into());
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: f.packet.project.clone(),
        source_commit: f.packet.source_commit.clone(),
        task_snapshots: BTreeMap::new(),
        tasks: vec![task, en],
    };
    let case = AdmissionCase::new(&f);
    // Stage ordinary synthetic metadata to construct origin shapes. The source
    // pool is absent; later live admission must refuse these origins.
    let receipt = admission::Prepared::open(&f.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let (r, returns) = response_fixture(&f.packet, &case.temp.path().join("returns"));
    response::register(
        &f.packet,
        &case.source,
        &case.profile,
        &receipt,
        &r,
        &returns,
    )
    .unwrap();
    let ack = retention::stage(
        &f.packet,
        &case.source,
        &case.profile,
        &receipt,
        &r,
        &retained_target(),
    )
    .unwrap();
    f.packet = make_packet(&plan, "scene-es", BTreeMap::new()).unwrap();
    let new = Artifact {
        logical_id: "candidate/C101".into(),
        sha256: digest(b"new picture"),
        bytes: 11,
        media_type: "image/png".into(),
    };
    let mut image = f.packet.task.clone();
    image.id = "image-fix".into();
    image.operation = Operation::ImageCandidate;
    image.tool = None;
    image.prompt = "Synthetic".into();
    image.outputs = BTreeMap::from([(
        "output".into(),
        OutputContract {
            media_type: "image/png".into(),
            max_bytes: 4096,
            required: true,
        },
    )]);
    image.native_inputs = None;
    let mut parents = BTreeMap::new();
    let ip = make_packet(
        &Plan {
            tasks: vec![image],
            ..plan.clone()
        },
        "image-fix",
        BTreeMap::new(),
    )
    .unwrap();
    let ir = returned(&ip, new.clone());
    let control_profile = || handoff::Profile {
        schema: "reel.workflow-native-handoff.v1".into(),
        source: serde_json::from_slice(&serde_json::to_vec(&case.source).unwrap()).unwrap(),
        admission: case.profile.clone(),
        binding: handoff::Binding {
            schema: "reel.workflow-native-handoff-binding.v1".into(),
            admission: receipt.clone(),
            target: ack.target.clone(),
        },
        runtime: f.profile.git.clone(),
        acknowledgements: case.temp.path().join("acknowledgements"),
    };
    parents.insert(
        ip.task.id.clone(),
        upstream::ParentControl {
            packet: ip.clone(),
            result: ir.clone(),
            profile: control_profile(),
            acknowledgement: ack.clone(),
        },
    );
    let mut controls = BTreeMap::new();
    let mut consumers = Vec::new();
    let mut baselines = BTreeMap::new();
    for lang in ["es", "en"] {
        let id = format!("control-{lang}");
        let mut t = ip.task.clone();
        t.id = id.clone();
        t.operation = Operation::TextProposal;
        t.language = Some(lang.into());
        t.inputs = vec![a.clone(), new.clone()];
        t.depends_on = vec![ip.task.id.clone()];
        t.outputs.get_mut("output").unwrap().media_type = "application/json".into();
        let p = make_packet(
            &Plan {
                tasks: vec![ip.task.clone(), t],
                ..plan.clone()
            },
            &id,
            BTreeMap::from([(
                ip.task.id.clone(),
                DependencyResult {
                    result_sha256: canonical_digest(&ir).unwrap(),
                    artifacts: ir.artifacts.clone(),
                },
            )]),
        )
        .unwrap();
        let result = returned(
            &p,
            Artifact {
                logical_id: format!("candidate/job-{lang}"),
                sha256: digest(lang.as_bytes()),
                bytes: lang.len() as u64,
                media_type: "application/json".into(),
            },
        );
        controls.insert(
            id.clone(),
            reel_workflow::film_rebuild::Parent {
                packet: p.clone(),
                result: result.clone(),
            },
        );
        parents.insert(
            id.clone(),
            upstream::ParentControl {
                packet: p,
                result,
                profile: control_profile(),
                acknowledgement: ack.clone(),
            },
        );
        let task_id = format!("scene-{lang}");
        baselines.insert(
            task_id.clone(),
            native::Baseline {
                packet: make_packet(&plan, &task_id, BTreeMap::new()).unwrap(),
                source: serde_json::from_slice(&serde_json::to_vec(&case.source).unwrap()).unwrap(),
                budget: reel_workflow_native::SnapshotBudget {
                    schema: "reel.workflow-native-input-budget.v1".into(),
                    input_bytes: 64 * 1024 * 1024,
                    input_object_bytes: 64 * 1024 * 1024,
                },
            },
        );
        consumers.push(reel_workflow::film_rebuild::Consumer {
            task_id,
            control_task: id,
            control_output: "output".into(),
            replacement_path: "replacement.png".into(),
            baseline_job_path: "baseline-job.json".into(),
            native_inputs: None,
        });
    }
    let q = native::Request {
        schema: "reel.workflow-native-film-rebuild.v1".into(),
        rebuild: reel_workflow::film_rebuild::Request {
            schema: "reel.workflow-film-rebuild.v1".into(),
            baseline: plan,
            old_subject: a,
            image: reel_workflow::film_rebuild::Parent {
                packet: ip,
                result: ir,
            },
            image_output: "output".into(),
            controls,
            consumers,
        },
        baselines,
        parents: upstream::Parents {
            schema: "reel.workflow-native-parents.v1".into(),
            parents,
        },
    };
    (f, case, q)
}
#[test]
fn exact_git_closure_binds_both_languages_without_granting_live_admission() {
    let (_f, case, q) = fixture();
    let before = fs::read(case.profile.metadata_root.join("metadata.json")).unwrap();
    let bound = native::bind(&q).unwrap();
    assert!(bound.prepared.native_binding_required.is_empty());
    assert_eq!(bound.prepared.affected_languages, vec!["en", "es"]);
    assert!(!bound.child_admitted && !bound.live_parent_custody_verified);
    for id in ["scene-es", "scene-en"] {
        let packet = &bound.packets[id];
        let source = &bound.sources[id];
        assert_eq!(
            SnapshotInputs::open(packet, &source.snapshot)
                .unwrap()
                .input_count(),
            4
        );
        assert_ne!(
            packet.task.native_inputs,
            q.baselines[id].packet.task.native_inputs
        );
        assert!(
            admission::Prepared::open(packet, source, &case.profile, "new-child")
                .unwrap()
                .accept()
                .is_err()
        );
    }
    assert_eq!(
        before,
        fs::read(case.profile.metadata_root.join("metadata.json")).unwrap()
    );
    assert!(!case.source.source.pool.repository.exists());
}
#[test]
fn stale_original_leaf_parent_and_incomplete_baselines_refuse() {
    let (_f, _case, q) = fixture();
    let raw = serde_json::to_vec(&q).unwrap();
    for fault in ["leaf", "packet", "parent", "missing", "budget", "reader"] {
        let mut q: native::Request = serde_json::from_slice(&raw).unwrap();
        match fault {
            "reader" => {
                q.baselines
                    .get_mut("scene-es")
                    .unwrap()
                    .source
                    .source
                    .maximum_bytes = 1
            }
            "leaf" => {
                q.baselines
                    .get_mut("scene-es")
                    .unwrap()
                    .source
                    .snapshot
                    .bindings
                    .get_mut("input/source")
                    .unwrap()
                    .selected = LogicalPath::parse("payloads/alias.bin").unwrap()
            }
            "packet" => {
                q.baselines
                    .get_mut("scene-es")
                    .unwrap()
                    .packet
                    .task
                    .language = Some("fr".into())
            }
            "parent" => {
                q.parents
                    .parents
                    .get_mut("image-fix")
                    .unwrap()
                    .result
                    .attempt_id = "different".into()
            }
            "missing" => {
                q.baselines.remove("scene-en");
            }
            "budget" => q.baselines.get_mut("scene-es").unwrap().budget.input_bytes = 1,
            _ => unreachable!(),
        }
        assert!(native::bind(&q).is_err(), "{fault}");
    }
}

#[test]
fn cli_exports_exact_per_consumer_parents_without_native_effects_or_overwrite() {
    let (_f, case, q) = fixture();
    let root = tempfile::tempdir().unwrap();
    let raw = serde_json::to_vec(&q).unwrap();
    let request = root.path().join("request.json");
    fs::write(&request, &raw).unwrap();
    let output = root.path().join("bound");
    let before = fs::read(case.profile.metadata_root.join("metadata.json")).unwrap();
    let call = |pin: &str| {
        Command::new(env!("CARGO_BIN_EXE_reel-workflow-native"))
            .arg("prepare-film-rebuild")
            .arg(&request)
            .arg(pin)
            .arg(&output)
            .output()
            .unwrap()
    };
    assert!(!call(&digest(b"wrong")).status.success());
    assert!(!output.exists());
    let status = call(&digest(&raw));
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stdout)
    );
    let summary: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    let manifest = fs::read(output.join("export.json")).unwrap();
    assert_eq!(summary["export_sha256"], digest(&manifest));
    let export: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    assert_eq!(export["request_sha256"], digest(&raw));
    let tasks = export["render_tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 2);
    let files = export["files"].as_object().unwrap();
    assert_eq!(files.len(), 7);
    for (name, pin) in files {
        let bytes = fs::read(output.join(name)).unwrap();
        assert_eq!(pin["sha256"], digest(&bytes));
        assert_eq!(pin["bytes"], bytes.len());
    }
    for lang in ["es", "en"] {
        assert!(tasks.contains(&serde_json::json!(format!("scene-{lang}"))));
        let parents: upstream::Parents = serde_json::from_slice(
            &fs::read(output.join(format!("scene-{lang}-parents.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(
            parents.parents.keys().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from(["image-fix".into(), format!("control-{lang}")])
        );
    }
    let saved = fs::read(output.join("bound.json")).unwrap();
    assert!(!call(&digest(&raw)).status.success());
    assert_eq!(saved, fs::read(output.join("bound.json")).unwrap());
    let export_pin = digest(&manifest);
    let verified = native::verify_export(&output, &digest(&raw), &export_pin).unwrap();
    assert_eq!(
        verified.into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from(["scene-en".into(), "scene-es".into()])
    );
    assert!(native::verify_export(&output, &digest(b"wrong"), &export_pin).is_err());
    let packet_path = output.join("scene-en-packet.json");
    let mut packet = fs::read(&packet_path).unwrap();
    packet[0] ^= 1;
    fs::write(packet_path, packet).unwrap();
    assert!(native::verify_export(&output, &digest(&raw), &export_pin).is_err());
    assert_eq!(
        before,
        fs::read(case.profile.metadata_root.join("metadata.json")).unwrap()
    );
}
