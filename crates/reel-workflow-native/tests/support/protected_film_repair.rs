//! Explicit real-pool qualification using ONLY previously retained synthetic
//! fixtures. Each new operation is persisted before effects and kept on failure.
use super::*;
use reel::scene_delivery::{self, repair};
use reel_workflow_native::{
    InputReader, NativeInputs, NativeProfile, admission, closing, film_cli, film_controls,
    film_rebuild as native, film_task, handoff, retention, upstream,
};
use serde::{Serialize, de::DeserializeOwned};

fn load<T: DeserializeOwned>(path: impl AsRef<Path>) -> T {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn copy<T: Serialize + DeserializeOwned>(v: &T) -> T {
    serde_json::from_slice(&serde_json::to_vec(v).unwrap()).unwrap()
}
fn save(path: impl AsRef<Path>, v: &impl Serialize) {
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    f.write_all(&serde_json::to_vec(v).unwrap()).unwrap();
    f.sync_all().unwrap();
}
fn tool(path: PathBuf) -> Tool {
    let raw = fs::read(&path).unwrap();
    Tool {
        path,
        sha256: digest(&raw),
        bytes: raw.len() as u64,
    }
}
fn call(runtime: &Tool, args: &[&Path]) -> serde_json::Value {
    let out = Command::new(&runtime.path).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "{} refused: {}",
        args[0].display(),
        String::from_utf8_lossy(&out.stdout)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn dep(result: &ReturnedResult) -> DependencyResult {
    DependencyResult {
        result_sha256: canonical_digest(result).unwrap(),
        artifacts: result.artifacts.clone(),
    }
}
fn pin(source: &NativeProfile) -> Artifact {
    let raw = source.snapshot.declaration_bytes().unwrap();
    Artifact {
        logical_id: "repair/native-inputs".into(),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    }
}
struct Lane<'a> {
    evidence: &'a Path,
    runtime: &'a Tool,
    admission: &'a admission::Profile,
    retained: &'a retention::Target,
}
fn exchange(
    lane: Lane<'_>,
    packet: Packet,
    source: NativeProfile,
    parents: Option<upstream::Parents>,
    name: &str,
    raw: &[u8],
    output: Artifact,
) -> upstream::ParentControl {
    fs::create_dir(lane.evidence).unwrap();
    let mut profile = lane.admission.clone();
    profile.scope.operation =
        tig_store::Key::parse(&format!("protected-repair-{name}-v1")).unwrap();
    save(lane.evidence.join("packet.json"), &packet);
    save(lane.evidence.join("source.json"), &source);
    save(lane.evidence.join("admission-profile.json"), &profile);
    if let Some(parents) = &parents {
        save(lane.evidence.join("parents.json"), parents);
    }
    // Admission receipt is a create-only CLI output, so an interruption never
    // leaves an admitted operation known only to this test's process memory.
    let mut args = vec![
        Path::new(if parents.is_some() {
            "accept-upstream-admission"
        } else {
            "accept-admission"
        })
        .to_owned(),
        lane.evidence.join("packet.json"),
        lane.evidence.join("source.json"),
        lane.evidence.join("admission-profile.json"),
    ];
    if parents.is_some() {
        args.push(lane.evidence.join("parents.json"));
    }
    args.extend([
        "repair-attempt-1".into(),
        lane.evidence.join("admission.json"),
        "--apply".into(),
    ]);
    call(
        lane.runtime,
        &args.iter().map(PathBuf::as_path).collect::<Vec<_>>(),
    );
    let receipt: admission::Receipt = load(lane.evidence.join("admission.json"));
    fs::create_dir(lane.evidence.join("returns")).unwrap();
    // Exact external return, not an assertion of model or deterministic-worker
    // execution. PNG bytes reuse a retained synthetic drawing; controls are
    // produced by the shared Rust transform outside this manual job.
    fs::write(lane.evidence.join("returns/output.bin"), raw).unwrap();
    let result = ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: receipt.attempt_id.clone(),
        worker_receipt: None,
        artifacts: BTreeMap::from([("output".into(), output)]),
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
    };
    let returns = reel_workflow_native::response::Returns {
        schema: "reel.workflow-returns.v1".into(),
        root: lane.evidence.join("returns"),
        files: BTreeMap::from([("output".into(), "output.bin".into())]),
    };
    fs::create_dir(lane.evidence.join("acknowledgements")).unwrap();
    let handoff = handoff::Profile {
        schema: "reel.workflow-native-handoff.v1".into(),
        source,
        admission: profile,
        binding: handoff::Binding {
            schema: "reel.workflow-native-handoff-binding.v1".into(),
            admission: receipt,
            target: lane.retained.clone(),
        },
        runtime: lane.runtime.clone(),
        acknowledgements: lane.evidence.join("acknowledgements"),
    };
    save(lane.evidence.join("handoff.json"), &handoff);
    save(lane.evidence.join("result.json"), &result);
    save(lane.evidence.join("returns.json"), &returns);
    call(
        lane.runtime,
        &[
            Path::new("create-handoff"),
            &lane.evidence.join("packet.json"),
            &lane.evidence.join("handoff.json"),
            &lane.evidence.join("journal"),
            Path::new("--apply"),
        ],
    );
    call(
        lane.runtime,
        &[
            Path::new("import-handoff"),
            &lane.evidence.join("journal"),
            &lane.evidence.join("handoff.json"),
            &lane.evidence.join("result.json"),
            &lane.evidence.join("returns.json"),
            Path::new("--apply"),
        ],
    );
    let ack: retention::Acknowledgement = load(handoff.acknowledgements.join(format!(
        "retained-{}-{}.json",
        handoff.binding.admission.task_key, handoff.binding.admission.attempt_id
    )));
    let journal = reel_workflow::handoff::Journal::open(&lane.evidence.join("journal")).unwrap();
    let close = closing::Request {
        schema: "reel.workflow-native-close.v1".into(),
        journal: lane.evidence.join("journal"),
        packet: packet.clone(),
        source: copy(&handoff.source),
        admission: handoff.admission.clone(),
        receipt: handoff.binding.admission.clone(),
        result: result.clone(),
        acknowledgement: ack.clone(),
        operation: tig_store::Key::parse(&format!("protected-close-{name}-v1")).unwrap(),
        original_lifecycle_tool: journal.ticket().lifecycle_tool.clone(),
        runtime: lane.runtime.clone(),
    };
    save(lane.evidence.join("close.json"), &close);
    let sha = digest(&fs::read(lane.evidence.join("close.json")).unwrap());
    call(
        lane.runtime,
        &[
            Path::new("close-retained"),
            &lane.evidence.join("close.json"),
            Path::new(&sha),
            &lane.evidence.join("closed.json"),
            Path::new("--apply"),
        ],
    );
    let closed: closing::Outcome = load(lane.evidence.join("closed.json"));
    assert!(closed.slot_idle && closed.original_clear_complete);
    drop(
        retention::verify(
            &packet,
            &handoff.source,
            &handoff.admission,
            &handoff.binding.admission,
            &result,
            &ack,
        )
        .unwrap(),
    );
    upstream::ParentControl {
        packet,
        result,
        profile: handoff,
        acknowledgement: ack,
    }
}

#[test]
#[ignore = "Explicit completed synthetic film and image fixtures only: new manual image/control jobs and owned repair render in the existing index; preserves original attempts, retained history, workspaces and capacity; no pool setup, production selection or adoption"]
fn retained_image_and_derived_controls_rebuild_original_film_with_owned_recovery() {
    let baseline = PathBuf::from(
        std::env::var_os("REEL_REPAIR_BASELINE")
            .expect("Set completed original synthetic film root"),
    );
    let image_root = PathBuf::from(
        std::env::var_os("REEL_REPAIR_IMAGE").expect("Set completed retained synthetic PNG root"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_REPAIR_EVIDENCE").expect("Set NEW absolute qualification root"),
    );
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(
        git(source_root, &["status", "--porcelain"]).is_empty(),
        "Commit the exact qualification source before native effects"
    );
    let source_commit = git(source_root, &["rev-parse", "HEAD"]);
    assert!(evidence.is_absolute());
    fs::create_dir(&evidence).unwrap();
    fs::copy(
        env!("CARGO_BIN_EXE_reel-workflow-native"),
        evidence.join("reel-workflow-native.exe"),
    )
    .unwrap();
    let runtime = tool(evidence.join("reel-workflow-native.exe"));
    save(
        evidence.join("source-version.json"),
        &json!({"commit":source_commit,"runtime":runtime,"scope":"synthetic-protected-film-repair","production_adopted":false}),
    );
    let locations: serde_json::Value = load(baseline.join("locations.json"));
    let original: Packet = serde_json::from_value(locations["packet"].clone()).unwrap();
    let source: NativeProfile = serde_json::from_value(locations["native"].clone()).unwrap();
    let profile: admission::Profile =
        serde_json::from_value(locations["admission_profile"].clone()).unwrap();
    let receipt: admission::Receipt = load(baseline.join("admission.json"));
    let completed: serde_json::Value = load(baseline.join("complete.json"));
    let old_result: ReturnedResult = serde_json::from_value(completed["result"].clone()).unwrap();
    let old_ack: retention::Acknowledgement =
        serde_json::from_value(completed["retained"].clone()).unwrap();
    let closed: closing::Outcome = load(baseline.join("closed.json"));
    assert!(closed.slot_idle);
    save(
        evidence.join("baseline.json"),
        &json!({"locations":locations,"receipt":receipt,"result":old_result,"acknowledgement":old_ack}),
    );
    drop(
        film_task::verify_retained(
            &original,
            &source,
            &profile,
            &receipt,
            &old_result,
            &old_ack,
        )
        .unwrap(),
    );
    eprintln!("Verified original retained film baseline");
    let image: reel_workflow_native::image_selection::Request =
        load(image_root.join("selection-request.json"));
    save(evidence.join("reused-image-origin.json"), &image);
    drop(
        retention::verify(
            &image.packet,
            &image.source,
            &image.admission,
            &image.receipt,
            &image.result,
            &image.acknowledgement,
        )
        .unwrap(),
    );
    let png = image.result.artifacts["picture"].clone();
    // Retained result is not a task input; reopen the exact output with the
    // qualified pinned native reader while its retained owner remains durable.
    use std::io::Read;
    let mut guarded = tig_git::annex::workflow::reader::open(
        &image.source.source,
        &Object {
            sha256: tig_core::Digest::parse(&png.sha256).unwrap(),
            bytes: png.bytes,
        },
    )
    .unwrap();
    let mut png_raw = Vec::new();
    guarded
        .by_ref()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut png_raw)
        .unwrap();
    drop(guarded);
    assert_eq!(digest(&png_raw), png.sha256);
    assert!(png_raw.len() <= 1024 * 1024);
    let mut input = NativeInputs::open(&original, &source).unwrap();
    let original_job = &original.task.tool.as_ref().unwrap().publication;
    let old_job_raw = input.read(original_job, 1024 * 1024).unwrap();
    drop(input);
    let old_job: scene_delivery::Job = serde_json::from_slice(&old_job_raw).unwrap();
    assert_eq!(old_job.pictures.len(), 1);
    let old_ref = &old_job.pictures[0].source;
    let old_id =
        &original.task.tool.as_ref().unwrap().source_bindings[old_ref.path.to_str().unwrap()];
    let old_picture = original
        .task
        .inputs
        .iter()
        .find(|a| &a.logical_id == old_id)
        .unwrap()
        .clone();
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: original.project.clone(),
        source_commit: original.source_commit.clone(),
        task_snapshots: BTreeMap::new(),
        tasks: vec![original.task.clone()],
    };
    let mut task = original.task.clone();
    task.id = "repair-image".into();
    task.operation = Operation::ImageCandidate;
    task.tool = None;
    task.prompt = "Synthetic manual reuse of an already retained green PNG".into();
    task.outputs = BTreeMap::from([(
        "output".into(),
        OutputContract {
            media_type: "image/png".into(),
            max_bytes: 1024 * 1024,
            required: true,
        },
    )]);
    let packet = make_packet(
        &Plan {
            tasks: vec![task],
            ..plan.clone()
        },
        "repair-image",
        BTreeMap::new(),
    )
    .unwrap();
    let picture = Artifact {
        logical_id: "repair/candidate-picture".into(),
        ..png
    };
    let image = exchange(
        Lane {
            evidence: &evidence.join("image"),
            runtime: &runtime,
            admission: &profile,
            retained: &old_ack.target,
        },
        packet,
        copy(&source),
        None,
        "image",
        &png_raw,
        picture.clone(),
    );
    save(evidence.join("image-parent.json"), &image);
    eprintln!("Retained and closed the new image candidate");
    let spec = repair::StillReplacement {
        schema: "reel.scene-still-replacement.v1".into(),
        attachments: vec![old_job.pictures[0].attachment_id.clone()],
        expected: old_ref.clone(),
        replacement: scene_delivery::FileRef {
            path: "replacement.png".into(),
            sha256: picture.sha256.clone(),
            bytes: picture.bytes,
        },
    };
    let new_job = repair::derive(&old_job, &spec).unwrap().job;
    let new_job_raw = serde_json::to_vec(&new_job).unwrap();
    let controls = upstream::Parents {
        schema: "reel.workflow-native-parents.v1".into(),
        parents: BTreeMap::from([(image.packet.task.id.clone(), copy(&image))]),
    };
    let origins = reel_workflow::upstream::Bindings {
        schema: "reel.workflow-upstream-inputs.v2".into(),
        streaming: None,
        inputs: BTreeMap::from([(
            picture.logical_id.clone(),
            reel_workflow::upstream::Reference {
                task_id: image.packet.task.id.clone(),
                artifact: reel_workflow::upstream::Selector::Output {
                    name: "output".into(),
                },
            },
        )]),
    };
    let mut control_source: NativeProfile = copy(&source);
    control_source.snapshot.schema = "reel.workflow-native-snapshot.v3".into();
    control_source.snapshot.budget = Some(reel_workflow_native::SnapshotBudget {
        schema: "reel.workflow-native-input-budget.v1".into(),
        input_bytes: 64 * 1024 * 1024,
        input_object_bytes: 64 * 1024 * 1024,
    });
    control_source.snapshot.upstream = Some(upstream::Frozen::capture(
        &origins,
        &controls.borrowed().unwrap(),
    ));
    let mut task = original.task.clone();
    task.id = "repair-controls".into();
    task.operation = Operation::TextProposal;
    task.tool = None;
    task.prompt="Exact shared Rust still transform returned through a manual handoff; not a deterministic worker assertion".into();
    task.inputs.push(picture.clone());
    task.depends_on = vec![image.packet.task.id.clone()];
    task.native_inputs = Some(pin(&control_source));
    let raw = origins.canonical_bytes().unwrap();
    task.upstream_inputs = Some(Artifact {
        logical_id: "repair/control-origins".into(),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    });
    task.outputs = BTreeMap::from([(
        "output".into(),
        OutputContract {
            media_type: "application/json".into(),
            max_bytes: 64 * 1024,
            required: true,
        },
    )]);
    let packet = make_packet(
        &Plan {
            tasks: vec![image.packet.task.clone(), task],
            ..plan.clone()
        },
        "repair-controls",
        BTreeMap::from([(image.packet.task.id.clone(), dep(&image.result))]),
    )
    .unwrap();
    let job = Artifact {
        logical_id: "repair/candidate-job".into(),
        sha256: digest(&new_job_raw),
        bytes: new_job_raw.len() as u64,
        media_type: "application/json".into(),
    };
    let control = exchange(
        Lane {
            evidence: &evidence.join("control"),
            runtime: &runtime,
            admission: &profile,
            retained: &old_ack.target,
        },
        packet,
        control_source,
        Some(controls),
        "controls",
        &new_job_raw,
        job,
    );
    save(evidence.join("control-parent.json"), &control);
    eprintln!("Retained and closed the derived scene controls");
    let request = native::Request {
        schema: "reel.workflow-native-film-rebuild.v1".into(),
        rebuild: reel_workflow::film_rebuild::Request {
            schema: "reel.workflow-film-rebuild.v1".into(),
            baseline: plan,
            old_subject: old_picture,
            image: reel_workflow::film_rebuild::Parent {
                packet: image.packet.clone(),
                result: image.result.clone(),
            },
            image_output: "output".into(),
            controls: BTreeMap::from([(
                control.packet.task.id.clone(),
                reel_workflow::film_rebuild::Parent {
                    packet: control.packet.clone(),
                    result: control.result.clone(),
                },
            )]),
            consumers: vec![reel_workflow::film_rebuild::Consumer {
                task_id: original.task.id.clone(),
                control_task: control.packet.task.id.clone(),
                control_output: "output".into(),
                replacement_path: "replacement.png".into(),
                baseline_job_path: "baseline-job.json".into(),
                native_inputs: None,
            }],
        },
        baselines: BTreeMap::from([(
            original.task.id.clone(),
            native::Baseline {
                packet: original.clone(),
                source: copy(&source),
                budget: reel_workflow_native::SnapshotBudget {
                    schema: "reel.workflow-native-input-budget.v1".into(),
                    input_bytes: 64 * 1024 * 1024,
                    input_object_bytes: 64 * 1024 * 1024,
                },
            },
        )]),
        parents: upstream::Parents {
            schema: "reel.workflow-native-parents.v1".into(),
            parents: BTreeMap::from([
                (image.packet.task.id.clone(), image),
                (control.packet.task.id.clone(), control),
            ]),
        },
    };
    save(evidence.join("rebuild-request.json"), &request);
    let sha = digest(&fs::read(evidence.join("rebuild-request.json")).unwrap());
    call(
        &runtime,
        &[
            Path::new("prepare-film-rebuild"),
            &evidence.join("rebuild-request.json"),
            Path::new(&sha),
            &evidence.join("bound"),
        ],
    );
    let id = &original.task.id;
    let packet: Packet = load(evidence.join(format!("bound/{id}-packet.json")));
    let child_source: NativeProfile = load(evidence.join(format!("bound/{id}-source.json")));
    let mut child_profile = profile.clone();
    child_profile.scope.operation = tig_store::Key::parse("protected-repair-film-v1").unwrap();
    save(evidence.join("film-admission-profile.json"), &child_profile);
    call(
        &runtime,
        &[
            Path::new("accept-upstream-admission"),
            &evidence.join(format!("bound/{id}-packet.json")),
            &evidence.join(format!("bound/{id}-source.json")),
            &evidence.join("film-admission-profile.json"),
            &evidence.join(format!("bound/{id}-parents.json")),
            Path::new("repair-film-attempt-1"),
            &evidence.join("film-admission.json"),
            Path::new("--apply"),
        ],
    );
    let child_receipt: admission::Receipt = load(evidence.join("film-admission.json"));
    fs::create_dir(evidence.join("workspaces")).unwrap();
    let request = film_cli::Request {
        schema: "reel.workflow-film-cli.v1".into(),
        packet,
        source: child_source,
        admission: child_profile,
        receipt: child_receipt,
        runtime: serde_json::from_value(locations["runtime"].clone()).unwrap(),
        programs: serde_json::from_value(locations["programs"].clone()).unwrap(),
        workspace: film_cli::WorkspaceProfile {
            parent: evidence.join("workspaces"),
            maximum_bytes: 772 * 1024 * 1024,
            capacity_policy: reel_assets::admission::Policy {
                minimum_free_bytes: 100 * 1024 * 1024,
                minimum_free_basis_points: 0,
                max_object_bytes: 1024 * 1024 * 1024,
                tier_minimum_free_bytes: BTreeMap::new(),
            },
        },
        retained: old_ack.target.clone(),
        close_operation: tig_store::Key::parse("protected-repair-film-close-v1").unwrap(),
        control_derivation: Some(film_controls::Derivation {
            schema: "reel.workflow-film-control-derivation.v1".into(),
            original_job: original_job.clone(),
            replacement: spec,
        }),
    };
    save(evidence.join("film-request.json"), &request);
    let sha = digest(&fs::read(evidence.join("film-request.json")).unwrap());
    call(
        &runtime,
        &[
            Path::new("prepare-film"),
            &evidence.join("film-request.json"),
            Path::new(&sha),
            &evidence.join("film-ticket.json"),
            Path::new("--apply"),
        ],
    );
    let ticket = fs::read(evidence.join("film-ticket.json")).unwrap();
    let ticket_sha = digest(&ticket);
    let action = |command: &str, apply: bool| {
        let mut args = vec![
            PathBuf::from(command),
            evidence.join("film-request.json"),
            PathBuf::from(&sha),
            evidence.join("film-ticket.json"),
            PathBuf::from(&ticket_sha),
        ];
        if apply {
            args.push("--apply".into());
        }
        call(
            &runtime,
            &args.iter().map(PathBuf::as_path).collect::<Vec<_>>(),
        )
    };
    action("run-film", true);
    eprintln!("Rendered and retained the protected film repair");
    let ticket: serde_json::Value = serde_json::from_slice(&ticket).unwrap();
    let workspace = evidence
        .join("workspaces")
        .join(ticket["root_name"].as_str().unwrap())
        .join("work");
    let result: ReturnedResult = load(workspace.join("workflow-result.json"));
    for name in ["D-wav", "M-wav", "E-wav", "mix-wav"] {
        assert_eq!(
            result.artifacts[name].sha256,
            old_result.artifacts[name].sha256
        );
        assert_eq!(
            result.artifacts[name].bytes,
            old_result.artifacts[name].bytes
        );
    }
    let baseline_work = PathBuf::from(locations["case_root"].as_str().unwrap()).join("film");
    let old_clock_raw = fs::read(baseline_work.join("delivery/receipt.json")).unwrap();
    let new_clock_raw = fs::read(workspace.join("delivery/receipt.json")).unwrap();
    assert_eq!(
        digest(&old_clock_raw),
        old_result.artifacts["delivery-receipt"].sha256
    );
    assert_eq!(
        digest(&new_clock_raw),
        result.artifacts["delivery-receipt"].sha256
    );
    let old_clock: serde_json::Value = serde_json::from_slice(&old_clock_raw).unwrap();
    let new_clock: serde_json::Value = serde_json::from_slice(&new_clock_raw).unwrap();
    for field in [
        "contract_sha256",
        "production_sha256",
        "compiled_sha256",
        "sample_rate",
        "fps_numerator",
        "fps_denominator",
        "duration_samples",
        "frame_count",
        "pictures",
        "audio",
    ] {
        assert!(
            !old_clock["plan"][field].is_null(),
            "Missing actual clock field {field}"
        );
        assert_eq!(
            old_clock["plan"][field], new_clock["plan"][field],
            "Changed actual clock {field}"
        );
    }
    for field in ["content_samples", "delivery_frames"] {
        assert_eq!(old_clock[field], new_clock[field]);
    }
    let modified = fs::metadata(workspace.join("delivery/picture.mkv"))
        .unwrap()
        .modified()
        .unwrap();
    action("resume-film", true);
    assert_eq!(
        modified,
        fs::metadata(workspace.join("delivery/picture.mkv"))
            .unwrap()
            .modified()
            .unwrap()
    );
    let decoded = Command::new(&request.programs.ffmpeg.path)
        .args(["-v", "error", "-i"])
        .arg(workspace.join("delivery/picture.mkv"))
        .args([
            "-vf",
            "scale=1:1",
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "pipe:1",
        ])
        .output()
        .unwrap();
    assert!(decoded.status.success());
    assert_eq!(decoded.stdout.len(), 3);
    assert!(decoded.stdout[1] > 150 && decoded.stdout[0] < 40 && decoded.stdout[2] < 60);
    fs::write(evidence.join("first-pixel.rgb"), &decoded.stdout).unwrap();
    action("verify-film", false);
    let closed = action("close-film", true);
    assert_eq!(closed["slot_idle"], true);
    action("verify-film", false);
    drop(
        film_task::verify_retained(
            &original,
            &source,
            &profile,
            &receipt,
            &old_result,
            &old_ack,
        )
        .unwrap(),
    );
    save(
        evidence.join("complete.json"),
        &json!({"actual_native_image_and_control_parents":true,"native_rebuild_binding":true,"control_payload_verified_under_child_claims":true,"same_original_recordings_and_clock":true,"actual_replacement_decoded":true,"owned_film_render_retained":true,"resume_preserved_picture":true,"child_slot_closed":true,"old_film_history_reopened":true,"bilingual_assembly_qualified":false,"selection_applied":false,"production_adopted":false,"capacity_released":false,"scratch_retired":false}),
    );
}
