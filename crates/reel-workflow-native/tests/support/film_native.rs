//! Selected recording -> native deterministic semantic controls -> retained replay.
use super::*;
use reel_workflow_native::{NativeProfile, admission::Prepared};
#[test]
fn auxiliary_program_roles_and_pins_are_checked_without_opening_program_paths() {
    let mut f = film_fixture::Fixture::new("es");
    let mut artifact = f.packet.task.tool.as_ref().unwrap().executable.clone();
    artifact.logical_id = "tools/aux".into();
    f.packet
        .task
        .tool
        .as_mut()
        .unwrap()
        .auxiliary_executables
        .insert("ffmpeg".into(), artifact.clone());
    let packet = make_packet(
        &Plan {
            schema: PLAN_SCHEMA.into(),
            project: f.packet.project,
            source_commit: f.packet.source_commit,
            tasks: vec![f.packet.task],
        },
        "film-control",
        BTreeMap::new(),
    )
    .unwrap();
    let runtime = Tool {
        path: PathBuf::from("not-opened.exe"),
        sha256: artifact.sha256,
        bytes: artifact.bytes,
    };
    let mut programs = BTreeMap::from([("ffmpeg".into(), runtime.clone())]);
    assert!(reel_workflow_native::worker::validate_programs(&packet, &programs).is_ok());
    assert!(reel_workflow_native::worker::validate_programs(&packet, &BTreeMap::new()).is_err());
    programs.get_mut("ffmpeg").unwrap().sha256 = "0".repeat(64);
    assert!(reel_workflow_native::worker::validate_programs(&packet, &programs).is_err());
    programs.insert("ffmpeg".into(), runtime.clone());
    programs.get_mut("ffmpeg").unwrap().bytes += 1;
    assert!(reel_workflow_native::worker::validate_programs(&packet, &programs).is_err());
    programs.insert("ffmpeg".into(), runtime.clone());
    programs.insert("extra".into(), runtime);
    assert!(reel_workflow_native::worker::validate_programs(&packet, &programs).is_err());
}
#[test]
#[ignore = "Existing declared native pool: synthetic scene control retention/replay only; preserves original roots"]
fn selected_takes_drive_retained_language_local_scene_controls() {
    run_scene_controls(false);
}
#[test]
#[ignore = "Existing declared native pool: auxiliary runtime binding, retained scene controls and original-ticket replay; preserves roots"]
fn auxiliary_program_binding_survives_retention_and_refuses_tool_substitution() {
    run_scene_controls(true);
}
fn run_scene_controls(auxiliary: bool) {
    let descriptor =
        std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").expect("Set REEL_NATIVE_INPUT_FIXTURE");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(descriptor).unwrap()).unwrap();
    let native: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(value["source"].clone()).unwrap();
    let mut fixture = Fixture::new();
    fixture.temp.disable_cleanup(true);
    let film = film_fixture::Fixture::new("es");
    let executable = std::env::var_os("REEL_NATIVE_HANDOFF_TOOL")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_reel-workflow-native")));
    let raw = fs::read(&executable).unwrap();
    let mut task = film.packet.task.clone();
    task.tool.as_mut().unwrap().executable = Artifact {
        logical_id: "tools/native-film-control".into(),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
        media_type: "application/octet-stream".into(),
    };
    let root = fixture.temp.path();
    let template: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("records/input.tig")).unwrap()).unwrap();
    fixture.profile.records.clear();
    fixture.profile.bindings.clear();
    fixture.profile.input_streams.clear();
    for (i, (a, raw)) in film.values.values().enumerate() {
        let object = Object::from_bytes(raw);
        let record = format!("records/film-{i}.tig");
        let leaf = format!("payloads/film-{i}.bin");
        let mut v = template.clone();
        v["id"] = json!(a.logical_id);
        v["selected"] = json!(object);
        v["streams"] = json!([]);
        write(root, &record, v);
        fs::write(
            root.join(&leaf),
            Key::from_object(&object).unlocked_pointer(),
        )
        .unwrap();
        fixture.profile.records.push(record);
        fixture.profile.bindings.insert(
            a.logical_id.clone(),
            Binding {
                selected: LogicalPath::parse(&leaf).unwrap(),
                streams: BTreeMap::new(),
            },
        );
        fixture
            .profile
            .input_streams
            .insert(a.logical_id.clone(), None);
    }
    git(root, &["add", "."]);
    git(
        root,
        &["commit", "-m", "synthetic selected film scene inputs"],
    );
    fixture.profile.commit = git(root, &["rev-parse", "HEAD"]);
    let git_bytes = fs::read(&native.pool.git).unwrap();
    fixture.profile.git = Tool {
        path: native.pool.git.clone(),
        sha256: digest(&git_bytes),
        bytes: git_bytes.len() as u64,
    };
    if auxiliary {
        task.tool.as_mut().unwrap().auxiliary_executables.insert(
            "git-reader".into(),
            Artifact {
                logical_id: "tools/auxiliary-git".into(),
                sha256: digest(&git_bytes),
                bytes: git_bytes.len() as u64,
                media_type: "application/octet-stream".into(),
            },
        );
    }
    task.native_inputs = Some(Fixture::declaration(&fixture.profile));
    fixture.packet = make_packet(
        &Plan {
            schema: PLAN_SCHEMA.into(),
            project: film.packet.project,
            source_commit: fixture.profile.commit.clone(),
            tasks: vec![task],
        },
        "film-control",
        BTreeMap::new(),
    )
    .unwrap();
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
    let mut programs = BTreeMap::new();
    if auxiliary {
        let path = root.join("auxiliary-git.exe");
        fs::copy(&case.source.snapshot.git.path, &path).unwrap();
        programs.insert(
            "git-reader".into(),
            Tool {
                path,
                sha256: digest(&git_bytes),
                bytes: git_bytes.len() as u64,
            },
        );
    }
    let evidence = std::env::var_os("REEL_FILM_EVIDENCE_DIRECTORY").map(PathBuf::from);
    eprintln!(
        "Native film root: {}; source: {}",
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
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    if auxiliary {
        let mut nodes = tig_store::metadata::MetadataStore::open(
            &case.profile.metadata_root,
            &case.profile.metadata_minimum,
        )
        .unwrap();
        let entries = tig_core::tree::Tree::from_root(receipt.input.root.clone())
            .entries(&mut nodes, tig_core::tree::TreeLimits::default())
            .unwrap()
            .0;
        assert_eq!(
            entries[&LogicalPath::parse("runtime/aux/git-reader").unwrap()],
            Object::from_bytes(&git_bytes)
        );
    }
    let ingress = root.join("synthetic-inputs");
    fs::create_dir(&ingress).unwrap();
    for (i, (_, bytes)) in film.values.values().enumerate() {
        let name = format!("input-{i}.bin");
        fs::write(ingress.join(&name), bytes).unwrap();
        tig_git::annex::workflow::publication::publish(
            &tig_git::annex::workflow::publication::Profile {
                schema: "tig.annex-file-publication.experimental-1".into(),
                pool: case.source.source.pool.clone(),
                store: case.profile.scope.store.clone(),
                source_root: ingress.clone(),
                source: LogicalPath::parse(&name).unwrap(),
                source_owned: true,
                expected: Some(Object::from_bytes(bytes)),
                include_private_location: false,
                maximum_bytes: 1024 * 1024,
                minimum_free_bytes: 100 * 1024 * 1024,
            },
        )
        .unwrap();
    }
    let profile = handoff_profile(&case, receipt, executable.clone());
    let profile_path = root.join("profile.json");
    let packet_path = root.join("packet.json");
    fs::write(&profile_path, serde_json::to_vec(&profile).unwrap()).unwrap();
    fs::write(&packet_path, serde_json::to_vec(&fixture.packet).unwrap()).unwrap();
    let journal = root.join("journal");
    let created = Command::new(&executable)
        .arg("create-handoff")
        .arg(&packet_path)
        .arg(&profile_path)
        .arg(&journal)
        .arg("--apply")
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stdout)
    );
    let mut controls = Vec::new();
    let result_path = root.join("result.json");
    automatic_dispatch::run_comic(
        &case,
        &fixture.packet,
        &profile,
        &journal,
        &profile_path,
        &executable,
        &result_path,
        &mut controls,
        &programs,
    );
    let result: ReturnedResult = serde_json::from_slice(&fs::read(result_path).unwrap()).unwrap();
    assert_eq!(result.artifacts.len(), 5);
    assert_eq!(result.provenance.mode, Mode::Tool);
    assert_eq!(
        reel_workflow::handoff::Journal::open(&journal)
            .unwrap()
            .state(),
        reel_workflow::handoff::State::Retained
    );
    let locations: serde_json::Value = serde_json::from_slice(
        &controls
            .iter()
            .find(|(n, _)| *n == "automatic-locations.json")
            .unwrap()
            .1,
    )
    .unwrap();
    let workspace = PathBuf::from(locations["workspace"].as_str().unwrap());
    let map: serde_json::Value =
        serde_json::from_slice(&fs::read(workspace.join("packet/review-map.json")).unwrap())
            .unwrap();
    assert_eq!(map["events"][0]["end_sample"], 2400);
    assert_eq!(map["events"][1]["end_sample"], 8000);
    assert_eq!(map["events"][1]["cel"]["logical_id"], "film/two.svg");
    if auxiliary {
        let ticket_path = root.join("automatic-dispatch/worker-ticket.json");
        let ticket_raw = fs::read(&ticket_path).unwrap();
        let ticket: serde_json::Value = serde_json::from_slice(&ticket_raw).unwrap();
        assert_eq!(
            ticket["auxiliary_programs"]["git-reader"]["sha256"],
            digest(&git_bytes)
        );
        let before = fs::read(case.profile.index_root.join("claims.json")).unwrap();
        let invoke = || {
            Command::new(&executable)
                .arg("run-worker")
                .arg(root.join("automatic-plan.json"))
                .arg(root.join("automatic-dispatch/bindings.json"))
                .arg(&ticket_path)
                .arg(digest(&ticket_raw))
                .arg("--apply")
                .output()
                .unwrap()
        };
        let path = &programs["git-reader"].path;
        let mut bad = git_bytes.clone();
        bad[0] ^= 0xff;
        fs::write(root.join("refused-auxiliary-git.exe"), &bad).unwrap();
        fs::write(path, &bad).unwrap();
        let refused = invoke();
        assert!(!refused.status.success());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&refused.stdout).unwrap()["error"]["kind"],
            "integrity"
        );
        assert_eq!(
            before,
            fs::read(case.profile.index_root.join("claims.json")).unwrap()
        );
        let alternative = root.join("alternative-auxiliary-git.exe");
        fs::write(&alternative, &git_bytes).unwrap();
        let mut retargeted = ticket.clone();
        retargeted["auxiliary_programs"]["git-reader"]["path"] = json!(alternative);
        let retargeted = serde_json::to_vec(&retargeted).unwrap();
        let alternate_ticket = root.join("retargeted-worker-ticket.json");
        fs::write(&alternate_ticket, &retargeted).unwrap();
        let rejected = Command::new(&executable)
            .arg("run-worker")
            .arg(root.join("automatic-plan.json"))
            .arg(root.join("automatic-dispatch/bindings.json"))
            .arg(&alternate_ticket)
            .arg(digest(&retargeted))
            .arg("--apply")
            .output()
            .unwrap();
        assert!(!rejected.status.success());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&rejected.stdout).unwrap()["error"]["kind"],
            "conflict"
        );
        assert_eq!(
            before,
            fs::read(case.profile.index_root.join("claims.json")).unwrap()
        );
        assert_eq!(ticket_raw, fs::read(&ticket_path).unwrap());
        fs::write(path, &git_bytes).unwrap();
        let recovered = invoke();
        assert!(
            recovered.status.success(),
            "{}",
            String::from_utf8_lossy(&recovered.stdout)
        );
        assert_eq!(
            before,
            fs::read(case.profile.index_root.join("claims.json")).unwrap()
        );
        controls.extend([
            ("auxiliary-refused.json", refused.stdout),
            ("auxiliary-recovered.json", recovered.stdout),
            ("auxiliary-retarget-refused.json", rejected.stdout),
        ]);
    }
    if let Some(e) = evidence {
        for (name, raw) in controls {
            fs::write(e.join(name), raw).unwrap();
        }
        fs::write(e.join("qualification-result.json"),serde_json::to_vec(&json!({"root":root,"source":fixture.temp.path(),"workspace":workspace,"retained_outputs":5,"language":"es","cut_sample":2400,"cue_end_sample":8000,"replay_without_effects":true,"auxiliary_runtime":auxiliary,"tool_substitution_refused":auxiliary,"original_tool_restored":auxiliary,"rendered":false,"selected":false,"scratch_retired":false})).unwrap()).unwrap();
    }
}
