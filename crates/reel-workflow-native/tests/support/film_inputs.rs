//! Existing synthetic scene -> admitted native guards -> existing REEL renderer.
use super::*;
use reel::scene_delivery::{self, FileRef, InputFiles};
use reel_workflow_native::{
    NativeInputs, NativeProfile,
    admission::Prepared,
    film_inputs::{Inputs, Programs},
};
#[test]
fn renderer_file_bridge_refuses_unadmitted_reader_before_program_or_payload_access() {
    let fixture = Fixture::new();
    let mut case = AdmissionCase::new(&fixture);
    // A byte-valid installed-tool descriptor is enough to construct a metadata
    // reader. No Annex command may run before live authority is checked.
    case.source.annex = fixture.profile.git.clone();
    case.source.source.pool.executable = case.source.annex.path.clone();
    let native = NativeInputs::open(&fixture.packet, &case.source).unwrap();
    let program = Tool {
        path: PathBuf::from("missing.exe"),
        sha256: "a".repeat(64),
        bytes: 1,
    };
    let err = Inputs::prepare(
        &fixture.packet,
        native,
        &BTreeMap::from([("source.json".into(), "input/source".into())]),
        Programs {
            ffmpeg: program.clone(),
            ffprobe: program,
        },
    )
    .err()
    .unwrap();
    assert_eq!(err.code, "film-input-live-authority-required");
}
#[test]
#[ignore = "Explicit existing native pool and previously qualified synthetic film files; protected path/renderer bridge only, no output publication"]
fn admitted_native_inputs_render_review_ids_without_staging_or_path_fallback() {
    run(false);
}
#[test]
#[ignore = "Explicit existing native pool and synthetic film files; task-bound movie candidate packet only, preserves original admission and owned capacity"]
fn movie_task_binds_native_controls_programs_and_complete_candidate_outputs() {
    run(true);
}
#[test]
#[ignore = "Explicit original admitted movie failure and unchanged pinned runtime; resumes original owned capacity, never creates another job"]
fn original_movie_metadata_binding_failure_resumes_existing_custody_and_capacity() {
    let root = PathBuf::from(
        std::env::var_os("REEL_MOVIE_RECOVERY_FIXTURE").expect("Set REEL_MOVIE_RECOVERY_FIXTURE"),
    );
    let packet: Packet =
        serde_json::from_slice(&fs::read(root.join("packet.json")).unwrap()).unwrap();
    let source: NativeProfile =
        serde_json::from_slice(&fs::read(root.join("source.json")).unwrap()).unwrap();
    let profile: reel_workflow_native::admission::Profile =
        serde_json::from_slice(&fs::read(root.join("admission-profile.json")).unwrap()).unwrap();
    let receipt: reel_workflow_native::admission::Receipt =
        serde_json::from_slice(&fs::read(root.join("admission-receipt.json")).unwrap()).unwrap();
    let ticket: reel_workflow_native::staging::Ticket =
        serde_json::from_slice(&fs::read(root.join("movie-owned-ticket.json")).unwrap()).unwrap();
    let programs: Programs =
        serde_json::from_slice(&fs::read(root.join("programs.json")).unwrap()).unwrap();
    let runtime_path = PathBuf::from(
        std::env::var_os("REEL_NATIVE_HANDOFF_TOOL")
            .expect("Set original REEL_NATIVE_HANDOFF_TOOL"),
    );
    let raw = fs::read(&runtime_path).unwrap();
    let runtime = Tool {
        path: runtime_path,
        sha256: digest(&raw),
        bytes: raw.len() as u64,
    };
    let before = fs::read(profile.index_root.join("claims.json")).unwrap();
    let owner = ticket
        .resume_workspace(&packet, &source, &profile, &receipt)
        .unwrap();
    let workspace = owner.path().join("movie");
    assert!(workspace.is_dir());
    assert!(
        fs::read_dir(&workspace).unwrap().next().is_none(),
        "original render already has effects; reconcile instead"
    );
    let read = NativeInputs::open_admitted(&packet, &source, &profile, &receipt).unwrap();
    let generated = reel_workflow_native::movie::generate(
        &packet,
        read,
        &BTreeMap::from([
            ("ffmpeg".into(), programs.ffmpeg),
            ("ffprobe".into(), programs.ffprobe),
        ]),
        &runtime,
        &workspace,
    )
    .unwrap();
    assert_eq!(generated.outputs.len(), 12);
    assert_eq!(generated.files.len(), 13);
    let delivery: scene_delivery::Receipt =
        serde_json::from_slice(&generated.files["delivery/receipt.json"]).unwrap();
    assert_eq!(delivery.delivery_frames, 48);
    assert_eq!(delivery.plan.review_ids[1].start_frame, 24);
    for (name, artifact) in &generated.outputs {
        let raw = &generated.files[&generated.paths[name]];
        assert_eq!(artifact.sha256, digest(raw));
        assert_eq!(artifact.bytes, raw.len() as u64);
    }
    assert_eq!(
        before,
        fs::read(profile.index_root.join("claims.json")).unwrap()
    );
    assert!(!generated.receipt.human_approval);
    assert!(!generated.receipt.retained_owner_acknowledged);
    if let Some(e) = std::env::var_os("REEL_FILM_EVIDENCE_DIRECTORY").map(PathBuf::from) {
        fs::write(e.join("qualification-result.json"),serde_json::to_vec(&json!({"root":root,"source":source.snapshot.repository,"workspace":workspace,
            "task_key":packet.task_key,"outputs":12,"packet_files":13,"frames":48,"cut_frame":24,"claims_unchanged":true,
            "original_admission_reused":true,"original_owned_capacity_reopened":true,"runtime":runtime,
            "output_retained":false,"task_completed":false,"selected":false,"scratch_retired":false})).unwrap()).unwrap();
    }
}
fn run(movie: bool) {
    let descriptor =
        std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").expect("Set REEL_NATIVE_INPUT_FIXTURE");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(descriptor).unwrap()).unwrap();
    let source_value: serde_json::Value = serde_json::from_slice(
        &fs::read(
            std::env::var_os("REEL_FILM_SOURCE_FIXTURE").expect("Set REEL_FILM_SOURCE_FIXTURE"),
        )
        .unwrap(),
    )
    .unwrap();
    let programs: Programs = serde_json::from_value(source_value["programs"].clone()).unwrap();
    let native: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(value["source"].clone()).unwrap();
    let mut fixture = Fixture::new();
    fixture.temp.disable_cleanup(true);
    let root = fixture.temp.path();
    let template: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("records/input.tig")).unwrap()).unwrap();
    fixture.profile.records.clear();
    fixture.profile.bindings.clear();
    fixture.profile.input_streams.clear();
    fixture.packet.task.inputs.clear();
    let mut aliases: BTreeMap<String, String> = BTreeMap::new();
    let mut raw_inputs = Vec::new();
    let mut refs = BTreeMap::new();
    let mut items = source_value["files"].as_array().unwrap().clone();
    let request = reel_workflow::film_render::Request {
        schema: "reel.workflow-film-render.v1".into(),
        job_path: "job.json".into(),
        production_path: "production.json".into(),
        output_paths: [
            ("picture", "delivery/picture.mkv"),
            ("dialogue", "delivery/D.wav"),
            ("music", "delivery/M.wav"),
            ("effects", "delivery/E.wav"),
            ("mix", "delivery/mix.wav"),
            ("master", "delivery/master.mkv"),
            ("review", "delivery/review.mp4"),
            ("review-ids", "delivery/review-ids.json"),
            ("review-font", "delivery/review-id-font.ttf"),
            ("delivery-receipt", "delivery/receipt.json"),
            ("execution-request", "render-request.json"),
            ("execution-result", "render-result.json"),
        ]
        .into_iter()
        .map(|(name, path)| (name.into(), path.into()))
        .collect(),
        limits: reel_workflow::film_render::Limits {
            timeout_seconds: 120,
            memory_bytes: 4 * 1024 * 1024 * 1024,
            processes: 8,
            workspace_bytes: 128 * 1024 * 1024,
            output_bytes: 32 * 1024 * 1024,
            frames: 48,
            samples: 96000,
            raw_rgb_bytes: 64 * 1024 * 1024,
        },
    };
    let runtime = movie.then(|| {
        let path = PathBuf::from(
            std::env::var_os("REEL_NATIVE_HANDOFF_TOOL").expect("Set REEL_NATIVE_HANDOFF_TOOL"),
        );
        let raw = fs::read(&path).unwrap();
        Tool {
            path,
            sha256: digest(&raw),
            bytes: raw.len() as u64,
        }
    });
    if movie {
        let raw = serde_json::to_vec(&request).unwrap();
        let path = root.join("movie-request.json");
        fs::write(&path, &raw).unwrap();
        items.push(json!({"alias":"request.json","path":path,"sha256":digest(&raw),"bytes":raw.len(),"media_type":"application/json"}));
    }
    for (i, item) in items.iter().enumerate() {
        let name = item["alias"].as_str().unwrap();
        let bytes = fs::read(item["path"].as_str().unwrap()).unwrap();
        assert_eq!(digest(&bytes), item["sha256"]);
        assert_eq!(bytes.len() as u64, item["bytes"].as_u64().unwrap());
        let object = Object::from_bytes(&bytes);
        let id = format!("input/{i:03}");
        let leaf = format!("payloads/film-{i}.bin");
        let record = format!("records/film-{i}.tig");
        let a = Artifact {
            logical_id: id.clone(),
            sha256: digest(&bytes),
            bytes: bytes.len() as u64,
            media_type: item["media_type"].as_str().unwrap().into(),
        };
        let mut v = template.clone();
        v["id"] = json!(id);
        v["selected"] = json!(object);
        v["streams"] = json!([]);
        write(root, &record, v);
        fs::write(
            root.join(&leaf),
            Key::from_object(&object).unlocked_pointer(),
        )
        .unwrap();
        fixture.profile.records.push(record);
        fixture.profile.input_streams.insert(id.clone(), None);
        fixture.profile.bindings.insert(
            id.clone(),
            Binding {
                selected: LogicalPath::parse(&leaf).unwrap(),
                streams: BTreeMap::new(),
            },
        );
        aliases.insert(name.into(), id);
        refs.insert(
            name.to_owned(),
            FileRef {
                path: name.into(),
                sha256: a.sha256.clone(),
                bytes: a.bytes,
            },
        );
        fixture.packet.task.inputs.push(a);
        raw_inputs.push((format!("input-{i}.bin"), bytes));
    }
    if let Some(runtime) = &runtime {
        let artifact = |id: &str, tool: &Tool| Artifact {
            logical_id: id.into(),
            sha256: tool.sha256.clone(),
            bytes: tool.bytes,
            media_type: "application/octet-stream".into(),
        };
        let selected = |name: &str| {
            fixture
                .packet
                .task
                .inputs
                .iter()
                .find(|a| a.logical_id == aliases[name])
                .unwrap()
                .clone()
        };
        fixture.packet.task.tool = Some(ToolContract {
            schema: "reel.workflow-tool.v1".into(),
            capability: ToolCapability::FilmRender,
            executable: artifact("tools/native", runtime),
            auxiliary_executables: BTreeMap::from([
                ("ffmpeg".into(), artifact("tools/ffmpeg", &programs.ffmpeg)),
                (
                    "ffprobe".into(),
                    artifact("tools/ffprobe", &programs.ffprobe),
                ),
            ]),
            publication: selected("job.json"),
            request: selected("request.json"),
            edition_id: "synthetic-es".into(),
            source_bindings: aliases
                .iter()
                .filter(|(name, _)| name.as_str() != "request.json")
                .map(|(a, b)| (a.clone(), b.clone()))
                .collect(),
            part: None,
        });
        fixture.packet.task.operation = Operation::DeterministicTool;
        fixture.packet.task.prompt.clear();
        fixture.packet.task.outputs = request
            .output_paths
            .iter()
            .map(|(name, path)| {
                (
                    name.clone(),
                    OutputContract {
                        media_type: reel_workflow::film_render::media_type(path).unwrap().into(),
                        max_bytes: 4 * 1024 * 1024,
                        required: true,
                    },
                )
            })
            .collect();
    }
    git(root, &["add", "."]);
    git(
        root,
        &["commit", "-m", "synthetic protected film input source"],
    );
    fixture.profile.commit = git(root, &["rev-parse", "HEAD"]);
    let raw = fs::read(&native.pool.git).unwrap();
    fixture.profile.git = Tool {
        path: native.pool.git.clone(),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
    };
    fixture.repin();
    let mut case = AdmissionCase::with_store(&fixture, native.store.clone());
    case.temp.disable_cleanup(true);
    let control = case.source.source.pool.control.clone();
    case.source = NativeProfile {
        snapshot: fixture.profile.clone(),
        source: native,
        annex: serde_json::from_value(value["annex"].clone()).unwrap(),
    };
    case.source.source.pool.control = control;
    let root = case.temp.path();
    let evidence = std::env::var_os("REEL_FILM_EVIDENCE_DIRECTORY").map(PathBuf::from);
    eprintln!(
        "Native renderer input root: {}; source: {}",
        root.display(),
        fixture.temp.path().display()
    );
    if let Some(e) = &evidence {
        fs::write(
            e.join("original-operation-roots.json"),
            serde_json::to_vec(&json!({"root":root,"source":fixture.temp.path()})).unwrap(),
        )
        .unwrap();
    }
    for (name, raw) in [
        ("packet.json", serde_json::to_vec(&fixture.packet).unwrap()),
        ("source.json", serde_json::to_vec(&case.source).unwrap()),
        (
            "admission-profile.json",
            serde_json::to_vec(&case.profile).unwrap(),
        ),
        ("programs.json", serde_json::to_vec(&programs).unwrap()),
        ("aliases.json", serde_json::to_vec(&aliases).unwrap()),
    ] {
        fs::write(root.join(name), raw).unwrap();
    }
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    fs::write(
        root.join("admission-receipt.json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    let ingress = root.join("synthetic-inputs");
    fs::create_dir(&ingress).unwrap();
    for (name, bytes) in &raw_inputs {
        fs::write(ingress.join(name), bytes).unwrap();
        tig_git::annex::workflow::publication::publish(
            &tig_git::annex::workflow::publication::Profile {
                schema: "tig.annex-file-publication.experimental-1".into(),
                pool: case.source.source.pool.clone(),
                store: case.profile.scope.store.clone(),
                source_root: ingress.clone(),
                source: LogicalPath::parse(name).unwrap(),
                source_owned: true,
                expected: Some(Object::from_bytes(bytes)),
                include_private_location: false,
                maximum_bytes: 2 * 1024 * 1024,
                minimum_free_bytes: 100 * 1024 * 1024,
            },
        )
        .unwrap();
    }
    let before = fs::read(case.profile.index_root.join("claims.json")).unwrap();
    if let Some(runtime) = runtime {
        let parent = root.join("movie-owned-parent");
        fs::create_dir(&parent).unwrap();
        let ticket = reel_workflow_native::staging::prepare(
            &fixture.packet,
            &case.source,
            &case.profile,
            &receipt,
            &reel_workflow_native::staging::Profile {
                schema: "reel.workflow-staging.v1".into(),
                parent,
                maximum_bytes: 256 * 1024 * 1024,
                capacity_policy: reel_assets::admission::Policy {
                    minimum_free_bytes: 100 * 1024 * 1024,
                    minimum_free_basis_points: 0,
                    max_object_bytes: 512 * 1024 * 1024,
                    ..Default::default()
                },
            },
        )
        .unwrap();
        fs::write(
            root.join("movie-owned-ticket.json"),
            ticket.canonical_bytes().unwrap(),
        )
        .unwrap();
        let owner = ticket
            .resume_workspace(&fixture.packet, &case.source, &case.profile, &receipt)
            .unwrap();
        let workspace = owner.path().join("movie");
        fs::create_dir(&workspace).unwrap();
        let read =
            NativeInputs::open_admitted(&fixture.packet, &case.source, &case.profile, &receipt)
                .unwrap();
        let generated = reel_workflow_native::movie::generate(
            &fixture.packet,
            read,
            &BTreeMap::from([
                ("ffmpeg".into(), programs.ffmpeg.clone()),
                ("ffprobe".into(), programs.ffprobe.clone()),
            ]),
            &runtime,
            &workspace,
        )
        .unwrap();
        assert_eq!(generated.outputs.len(), 12);
        assert_eq!(generated.files.len(), 13);
        assert_eq!(generated.paths, request.output_paths);
        assert!(!generated.receipt.human_approval);
        assert!(!generated.receipt.retained_owner_acknowledged);
        assert_eq!(generated.receipt.task_key, fixture.packet.task_key);
        let delivery: scene_delivery::Receipt =
            serde_json::from_slice(&generated.files["delivery/receipt.json"]).unwrap();
        assert_eq!(delivery.delivery_frames, 48);
        assert_eq!(delivery.plan.review_ids[1].start_frame, 24);
        for (name, artifact) in &generated.outputs {
            let raw = &generated.files[&generated.paths[name]];
            assert_eq!(artifact.sha256, digest(raw));
            assert_eq!(artifact.bytes, raw.len() as u64);
        }
        assert_eq!(
            before,
            fs::read(case.profile.index_root.join("claims.json")).unwrap()
        );
        if let Some(e) = evidence {
            fs::write(e.join("qualification-result.json"),serde_json::to_vec(&json!({"root":root,"source":fixture.temp.path(),"workspace":workspace,
                "task_key":fixture.packet.task_key,"outputs":12,"packet_files":13,"frames":48,"cut_frame":24,"claims_unchanged":true,
                "owned_capacity_preserved":true,"output_retained":false,"task_completed":false,"selected":false,"scratch_retired":false})).unwrap()).unwrap();
        }
        return;
    }
    let read = NativeInputs::open_admitted(&fixture.packet, &case.source, &case.profile, &receipt)
        .unwrap();
    let inputs = Inputs::prepare(&fixture.packet, read, &aliases, programs.clone()).unwrap();
    assert_eq!(inputs.objects(), raw_inputs.len());
    let selected = inputs.verified_path(&refs["red.ppm"]).unwrap();
    assert!(selected.starts_with(&case.source.source.pool.repository));
    assert!(
        std::fs::OpenOptions::new()
            .write(true)
            .open(&selected)
            .is_err(),
        "native source can mutate while renderer reads"
    );
    let mut unknown = refs["red.ppm"].clone();
    unknown.path = "not-admitted".into();
    assert!(inputs.verified_path(&unknown).is_err());
    let mut stale = refs["red.ppm"].clone();
    stale.sha256 = "0".repeat(64);
    assert!(inputs.verified_path(&stale).is_err());
    let output = root.join("review-film");
    let produced = scene_delivery::render_with_control_files(
        &refs["job.json"],
        &refs["production.json"],
        &inputs,
        &inputs,
        &output,
    )
    .unwrap();
    let rendered_receipt = fs::read(output.join("receipt.json")).unwrap();
    let pin = FileRef {
        path: "receipt.json".into(),
        sha256: digest(&rendered_receipt),
        bytes: rendered_receipt.len() as u64,
    };
    scene_delivery::check_with_all_files(
        &refs["job.json"],
        &refs["production.json"],
        &inputs,
        &output,
        &pin,
        &inputs,
    )
    .unwrap();
    assert_eq!(produced.plan.review_ids.len(), 2);
    assert_eq!(produced.delivery_frames, 48);
    assert_eq!(produced.plan.review_ids[1].start_frame, 24);
    assert_eq!(
        before,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
    if let Some(e) = evidence {
        fs::write(e.join("qualification-result.json"),serde_json::to_vec(&json!({"root":root,"source":fixture.temp.path(),"output":output,"native_objects":inputs.objects(),"frames":48,"cut_frame":24,"program_fingerprint":inputs.program_fingerprint().unwrap(),"native_source_mutation_refused":true,"stale_and_unknown_refs_refused":true,"claims_unchanged":true,"source_staging_used":false,"output_retained":false,"task_completed":false})).unwrap()).unwrap();
    }
}
