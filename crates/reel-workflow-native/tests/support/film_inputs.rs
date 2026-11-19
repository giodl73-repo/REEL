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
    let mut aliases = BTreeMap::new();
    let mut raw_inputs = Vec::new();
    let mut refs = BTreeMap::new();
    for (i, item) in source_value["files"].as_array().unwrap().iter().enumerate() {
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
