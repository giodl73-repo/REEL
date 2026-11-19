//! Selected recording -> native deterministic semantic controls -> retained replay.
use super::*;
use reel_workflow_native::{NativeProfile, admission::Prepared};
#[test]
#[ignore = "Existing declared native pool: synthetic scene control retention/replay only; preserves original roots"]
fn selected_takes_drive_retained_language_local_scene_controls() {
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
    if let Some(e) = evidence {
        for (name, raw) in controls {
            fs::write(e.join(name), raw).unwrap();
        }
        fs::write(e.join("qualification-result.json"),serde_json::to_vec(&json!({"root":root,"source":fixture.temp.path(),"workspace":workspace,"retained_outputs":5,"language":"es","cut_sample":2400,"cue_end_sample":8000,"replay_without_effects":true,"rendered":false,"selected":false,"scratch_retired":false})).unwrap()).unwrap();
    }
}
