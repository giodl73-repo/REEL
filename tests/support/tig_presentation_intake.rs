use super::*;
use reel::tig_presentation_registration as intake;
use std::io::Cursor;
use tig_store::existing::{ByteSource, Error};

struct MetadataOnly {
    nodes: BTreeMap<tig_core::Digest, Vec<u8>>,
    opened: Vec<tig_core::Digest>,
}
impl ByteSource for MetadataOnly {
    type Reader = Cursor<Vec<u8>>;
    fn open_verified(&mut self, object: &Object) -> Result<Self::Reader, Error> {
        self.opened.push(object.sha256.clone());
        let bytes = self.nodes.get(&object.sha256).ok_or(Error::Missing)?;
        if Object::from_bytes(bytes) != *object {
            return Err(Error::Integrity);
        }
        Ok(Cursor::new(bytes.clone()))
    }
}
fn fixture(existing: bool) -> (Fixture, Value, MetadataOnly, Request) {
    let (mut f, request) = setup();
    let existing_profile = f.profile.clone();
    let source = f.source();
    let compiled = tig_presentation::compile(&source, &request).unwrap();
    let mut provider = MetadataOnly {
        nodes: BTreeMap::new(),
        opened: Vec::new(),
    };
    for (pin, bytes) in compiled.metadata_files() {
        provider.nodes.insert(
            tig_core::Digest::parse(&pin.sha256).unwrap(),
            bytes.to_vec(),
        );
        fs::write(f.root().join("cache/objects").join(&pin.sha256), bytes).unwrap();
    }
    let manifest = Object {
        sha256: tig_core::Digest::parse(&compiled.manifest().sha256).unwrap(),
        bytes: compiled.manifest().bytes,
    };
    let schema_path = f.root().join("definitions/authored.json");
    let schema = Object::from_bytes(&fs::read(&schema_path).unwrap());
    let mut definitions = serde_json::Map::new();
    let mut nodes = serde_json::Map::new();
    for role in [
        "source", "template", "timing", "clock", "evidence", "primary",
    ] {
        let id = format!("types/intake-{role}");
        let path = format!("definitions/intake-{role}.json");
        let fields = if role == "primary" {
            vec![]
        } else {
            vec!["document"]
        };
        let object = write(
            f.root(),
            &path,
            &json!({"schema":"tig.definition.design-1","id":id,"authored_schema":schema,"authoring":{"properties":fields,"required":fields,"defaults":{}},"semantic_rules":{},"stream_rules":{}}),
        );
        f.profile["admitted"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":id,"object":object,"payload":{"kind":"selected"}}));
        definitions.insert(role.into(), json!({"definition":{"path":f.root().join(&path),"sha256":object.sha256,"bytes":object.bytes},"authored_schema":{"path":schema_path,"sha256":schema.sha256,"bytes":schema.bytes}}));
        if role != "primary" {
            nodes.insert(
                role.into(),
                json!({"id":format!("import/{role}"),"filename":format!("{role}.tig")}),
            );
        }
    }
    let mut bindings = serde_json::Map::new();
    for (key, id, filename) in [
        ("picture/0", "cel/a", "cel-a.tig"),
        ("picture/1", "cel/b", "cel-b.tig"),
        ("audio", "cue/main", "audio.tig"),
        ("layer", "presentation/layer", "layer.tig"),
    ] {
        bindings.insert(
            key.into(),
            if existing {
                json!({"id":id,"existing":true})
            } else {
                json!({"id":format!("import/{id}"),"filename":filename})
            },
        );
    }
    let profile = json!({"schema":intake::SCHEMA,"byte_source":f.profile["byte_source"],"manifest":manifest,"definitions":definitions,"nodes":nodes,"bindings":bindings,"primary_selection_state":"selected-private-production","existing_source":if existing {existing_profile} else {Value::Null},"existing_primary_definitions":if existing {json!(["types/media"])} else {json!([])},"extra_audio":null,"max_output_bytes":65536});
    (f, profile, provider, request)
}
fn install(f: &mut Fixture, prepared: &reel::tig_native_registration::Prepared) {
    prepared.write(&f.root().join("intake")).unwrap();
    git(f.root(), &["add", "intake"]);
    commit(f);
    f.profile["records"]
        .as_array_mut()
        .unwrap()
        .push(json!("intake"));
}

#[test]
fn retained_v2_intake_opens_only_metadata_and_replays_native_clock_and_ids() {
    let (mut f, value, mut provider, _) = fixture(false);
    let profile = serde_json::from_value(value).unwrap();
    let (prepared, request) = intake::prepare(&profile, &mut provider).unwrap();
    assert_eq!(provider.opened.len(), 5);
    install(&mut f, &prepared);
    assert_eq!(
        fs::read_dir(f.root().join("intake/inputs"))
            .unwrap()
            .count(),
        4
    );
    let source = f.source();
    let compiled = tig_presentation::compile(&source, &request).unwrap();
    assert_eq!(compiled.primary_files().count(), 4);
    assert_eq!(compiled.metadata_files().count(), 5);
    assert_eq!(source.byte_requests(), 0);
    let clock: Value =
        serde_json::from_slice(&fs::read(f.root().join("intake/clock.tig")).unwrap()).unwrap();
    assert_eq!(
        clock["authored"]["document"]["source"]["id"],
        "import/cue/main"
    );
    assert_eq!(clock["authored"]["document"]["total_samples"], 48000);
    assert!(prepared.write(&f.root().join("intake")).is_err());
}

#[test]
fn existing_primary_intake_preserves_original_records_and_requires_matching_scope() {
    let (mut f, mut value, mut provider, _) = fixture(true);
    let original = fs::read(f.root().join("records/cel-a.tig")).unwrap();
    let (prepared, request) = intake::prepare(
        &serde_json::from_value(value.clone()).unwrap(),
        &mut provider,
    )
    .unwrap();
    install(&mut f, &prepared);
    assert_eq!(
        fs::read_dir(f.root().join("intake/inputs"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        fs::read(f.root().join("records/cel-a.tig")).unwrap(),
        original
    );
    assert_eq!(
        tig_presentation::compile(&f.source(), &request)
            .unwrap()
            .primary_files()
            .count(),
        4
    );
    value["bindings"]["picture/0"]["id"] = json!("cel/b");
    assert!(intake::prepare(&serde_json::from_value(value).unwrap(), &mut provider).is_err());
}

#[test]
fn intake_refuses_alias_collisions_unused_bindings_and_wrong_definition_before_writes() {
    let (f, value, mut provider, _) = fixture(false);
    for case in [
        "shared-id",
        "filename",
        "control-id",
        "unused",
        "definition",
        "budget",
    ] {
        let mut wrong = value.clone();
        match case {
            "shared-id" => {
                wrong["bindings"]["picture/1"]["id"] = wrong["bindings"]["picture/0"]["id"].clone()
            }
            "filename" => wrong["bindings"]["picture/1"]["filename"] = json!("CEL-A.tig"),
            "control-id" => wrong["bindings"]["audio"]["id"] = json!("import/source"),
            "unused" => wrong["bindings"]["unused"] = wrong["bindings"]["audio"].clone(),
            "definition" => {
                wrong["definitions"]["primary"] = wrong["definitions"]["source"].clone()
            }
            _ => wrong["max_output_bytes"] = json!(1),
        }
        assert!(
            intake::prepare(&serde_json::from_value(wrong).unwrap(), &mut provider).is_err(),
            "{case}"
        );
        assert!(!f.root().join("intake").exists());
    }
}

#[test]
fn v1_picture_only_plan_can_declare_a_new_audio_contract_without_rewriting_history() {
    let (mut f, mut value, mut provider, _) = fixture(false);
    let pin: Object = serde_json::from_value(value["manifest"].clone()).unwrap();
    let original = provider.nodes[&pin.sha256].clone();
    let mut native: Value = serde_json::from_slice(&original).unwrap();
    native["schema"] = json!("reel.presentation-still-sequence.v1");
    native.as_object_mut().unwrap().remove("picture_timing");
    let audio: Object = serde_json::from_value(
        json!({"sha256":native["audio"]["sha256"],"bytes":native["audio"]["bytes"]}),
    )
    .unwrap();
    native.as_object_mut().unwrap().remove("audio");
    let template = serde_json::to_vec(&json!({"schema":"reel.presentation-still-template.v1","template_id":"fixed","width":64,"height":64,"frame_rate":24,"sample_rate":48000,"picture_count":2,"frames_per_picture":12})).unwrap();
    let template_pin = Object::from_bytes(&template);
    provider.nodes.insert(template_pin.sha256.clone(), template);
    native["template"] = json!({"path":"fixed-template.json","sha256":template_pin.sha256,"bytes":template_pin.bytes,"cache_uri":format!("cache://sha256/{}",template_pin.sha256.as_str())});
    let bytes = serde_json::to_vec(&native).unwrap();
    let manifest = Object::from_bytes(&bytes);
    provider.nodes.insert(manifest.sha256.clone(), bytes);
    value["manifest"] = json!(manifest);
    value["extra_audio"] = json!(audio);
    let (prepared, request) =
        intake::prepare(&serde_json::from_value(value).unwrap(), &mut provider).unwrap();
    assert_eq!(provider.nodes[&pin.sha256], original);
    install(&mut f, &prepared);
    assert_eq!(fs::read_dir(f.root().join("intake")).unwrap().count(), 3); // source/template/inputs
    let compiled = tig_presentation::compile(&f.source(), &request).unwrap();
    assert_eq!(compiled.metadata_files().count(), 2);
    assert_eq!(compiled.primary_files().count(), 4);
}

#[test]
fn registration_cli_emits_candidate_request_and_refuses_output_reuse() {
    let (f, value, _, _) = fixture(false);
    write(f.root(), "intake-profile.json", &value);
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_reel-tig-register"))
            .arg("--presentation-source")
            .arg(f.root().join("intake-profile.json"))
            .arg(f.root().join("cli-intake"))
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["primary_media_opened"], 0);
    assert_eq!(result["adopted"], false);
    assert_eq!(result["source_request"]["presentation_id"], "import/source");
    assert!(!run().status.success());
}

#[test]
fn intake_cannot_shadow_unadmitted_existing_ids_or_embed_large_asset_data() {
    let (f, value, mut provider, _) = fixture(true);
    let mut collision = value.clone();
    collision["existing_source"]["admitted"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| entry["id"] != "types/media");
    collision["bindings"]["picture/0"] = json!({"id":"cel/a","filename":"clobber.tig"});
    let error = intake::prepare(&serde_json::from_value(collision).unwrap(), &mut provider)
        .err()
        .unwrap();
    assert!(error.to_string().contains("identity already exists"));
    let mut control = value.clone();
    control["nodes"]["source"]["id"] = json!("cel/a");
    assert!(
        intake::prepare(&serde_json::from_value(control).unwrap(), &mut provider)
            .err()
            .unwrap()
            .to_string()
            .contains("control identity already exists")
    );
    let mut large = value;
    large["metadata"] = json!({"large-asset-data":"x".repeat(65536)});
    assert!(
        intake::prepare(&serde_json::from_value(large).unwrap(), &mut provider)
            .err()
            .unwrap()
            .to_string()
            .contains("retain large metadata as an asset")
    );
    assert!(!f.root().join("intake").exists());
}

#[test]
fn floating_measurement_report_stays_an_exact_asset_and_cannot_drift_from_its_link() {
    let (mut f, mut profile, mut provider, _) = fixture(false);
    let decode = |provider: &MetadataOnly, reference: &Value| -> Value {
        let digest = tig_core::Digest::parse(reference["sha256"].as_str().unwrap()).unwrap();
        serde_json::from_slice(&provider.nodes[&digest]).unwrap()
    };
    let insert = |provider: &mut MetadataOnly, value: &Value, path: &str| -> Value {
        let raw = serde_json::to_vec(value).unwrap();
        let object = Object::from_bytes(&raw);
        provider.nodes.insert(object.sha256.clone(), raw);
        json!({"path":path,"sha256":object.sha256,"bytes":object.bytes,"cache_uri":format!("cache://sha256/{}",object.sha256.as_str())})
    };
    let mut manifest = decode(&provider, &profile["manifest"]);
    let mut timing = decode(&provider, &manifest["picture_timing"]);
    let mut clock = decode(&provider, &timing["clock"]);
    let evidence = insert(
        &mut provider,
        &json!({"measured_seconds":0.125,"report":"x".repeat(100_000)}),
        "large-measurement.json",
    );
    clock["evidence"] = evidence.clone();
    timing["clock"] = insert(&mut provider, &clock, "measured-clock.json");
    manifest["picture_timing"] = insert(&mut provider, &timing, "measured-timing.json");
    let manifest = insert(&mut provider, &manifest, "original-manifest.json");
    profile["manifest"] = json!({"sha256":manifest["sha256"],"bytes":manifest["bytes"]});
    let (prepared, request) =
        intake::prepare(&serde_json::from_value(profile).unwrap(), &mut provider).unwrap();
    install(&mut f, &prepared);
    let mut record: Value =
        serde_json::from_slice(&fs::read(f.root().join("intake/evidence.tig")).unwrap()).unwrap();
    assert_eq!(record["selected"]["sha256"], evidence["sha256"]);
    assert!(
        fs::metadata(f.root().join("intake/evidence.tig"))
            .unwrap()
            .len()
            < 2048
    );
    assert_eq!(
        record["authored"]["document"]["schema"],
        "reel.presentation-clock-evidence-reference.v1"
    );
    assert!(tig_presentation::compile(&f.source(), &request).is_ok());
    record["selected"] = json!(Object::from_bytes(b"different measurement report"));
    write(f.root(), "intake/evidence.tig", &record);
    git(f.root(), &["add", "intake"]);
    commit(&mut f);
    let source = f.source();
    assert!(
        tig_presentation::compile(&source, &request)
            .err()
            .unwrap()
            .to_string()
            .contains("evidence asset differs")
    );
    assert_eq!(source.byte_requests(), 0);
}

#[cfg(windows)]
#[test]
fn imported_source_reproduces_the_retained_master_from_guarded_original_primaries() {
    use reel::{presentation_still_sequence as producer, scene_delivery::FileRef};
    let (mut f, value, mut provider, original_request) = fixture(false);
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    let mut original_profile = f.profile.clone();
    original_profile["admitted"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| !entry["id"].as_str().unwrap().starts_with("types/intake-"));
    let mut source = Source::open(serde_json::from_value(original_profile).unwrap()).unwrap();
    let original = tig_presentation::compile(&source, &original_request).unwrap();
    let inputs = original.prepare(&mut source).unwrap();
    let output = f.root().join("original-native-master");
    producer::build_with_control_files(original.manifest(), &original, &inputs, &output).unwrap();
    let selected = Object::from_bytes(&fs::read(output.join("master.mkv")).unwrap());
    drop(inputs);
    drop(source);
    let (prepared, request) =
        intake::prepare(&serde_json::from_value(value).unwrap(), &mut provider).unwrap();
    install(&mut f, &prepared);
    let mut source = f.source();
    let imported = tig_presentation::compile(&source, &request).unwrap();
    let inputs = imported.prepare(&mut source).unwrap();
    let master = FileRef {
        path: "master.mkv".into(),
        sha256: selected.sha256.as_str().into(),
        bytes: selected.bytes,
    };
    let proof = producer::compare_master_to_primaries(
        imported.manifest(),
        &imported,
        &inputs,
        &output,
        &master,
        f.root(),
        1024 * 1024,
    )
    .unwrap();
    assert_eq!(proof.master().sha256, selected.sha256.as_str());
    assert_eq!(provider.opened.len(), 5);
    #[cfg(feature = "tig-publication")]
    {
        drop(inputs);
        drop(source);
        check_cli_master(&mut f, &request, &selected, &output);
    }
}

#[cfg(feature = "tig-publication")]
#[test]
fn check_cli_inspects_committed_import_without_runtime_or_media() {
    let (mut f, value, mut provider, _) = fixture(false);
    let (prepared, request) =
        intake::prepare(&serde_json::from_value(value).unwrap(), &mut provider).unwrap();
    install(&mut f, &prepared);
    let profile = json!({"schema":"reel.tig-presentation-check-profile.experimental-1","source":f.profile,"request":request,"comparison":null});
    write(f.root(), "check-profile.json", &profile);
    let result = Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
        .args(["inspect-presentation"])
        .arg(f.root().join("check-profile.json"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["primary_media_opened"], 0);
    assert_eq!(report["primary_names"], 4);
    assert_eq!(report["adopted"], false);
    let result = Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
        .args(["compare-presentation"])
        .arg(f.root().join("check-profile.json"))
        .output()
        .unwrap();
    assert!(!result.status.success());
}

#[cfg(all(windows, feature = "tig-publication"))]
fn check_cli_master(f: &mut Fixture, request: &Request, selected: &Object, output: &Path) {
    let definition_path = "definitions/presentation-output.json";
    let mut definition: Value =
        serde_json::from_slice(&fs::read(f.root().join(definition_path)).unwrap()).unwrap();
    definition["authoring"]["properties"] = json!([
        "document",
        "presentation",
        "episode_id",
        "language",
        "source_segment_id",
        "component_kind"
    ]);
    let definition = write(f.root(), definition_path, &definition);
    for admission in f.profile["admitted"].as_array_mut().unwrap() {
        if admission["id"] == "types/presentation-output" {
            admission["object"] = json!(definition);
        }
    }
    write(
        f.root(),
        "records/checked-output.tig",
        &json!({"schema":"tig.instance.design-1","id":"checked/output","definition":{"id":"types/presentation-output","sha256":definition.sha256,"bytes":definition.bytes},"authored":{"document":{},"presentation":{"id":request.presentation_id},"episode_id":"fixture","language":"es","source_segment_id":"opening","component_kind":"episode-presentation"},"selected":selected,"streams":[],"status":{},"metadata":{}}),
    );
    fs::copy(
        output.join("master.mkv"),
        f.root()
            .join("cache/objects")
            .join(selected.sha256.as_str()),
    )
    .unwrap();
    commit(f);
    let tool = PathBuf::from(env!("CARGO_BIN_EXE_reel-tig-job"));
    let program = |name: &str| {
        let path = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|root| root.join(format!("{name}.exe")))
            .find(|path| path.is_file())
            .unwrap()
            .canonicalize()
            .unwrap();
        let identity = reel::media_runtime::ByteIdentity::observe_file(&path).unwrap();
        json!({"path":path,"sha256":identity.sha256,"bytes":identity.bytes})
    };
    let mut profile = json!({"schema":"reel.tig-presentation-check-profile.experimental-1","source":f.profile,"request":request,"comparison":{"output_id":"checked/output","output_definition":"types/presentation-output","runtime":{"host":reel::media_runtime::ByteIdentity::observe_file(&tool).unwrap(),"ffmpeg":program("ffmpeg"),"ffprobe":program("ffprobe"),"dependencies":{}},"stage_parent":f.root(),"max_staged_bytes":1048576,"max_objects":8192,"max_logical_bytes":2147483648u64}});
    let run = |profile: &Value| {
        write(f.root(), "check-profile.json", profile);
        Command::new(&tool)
            .arg("compare-presentation")
            .arg(f.root().join("check-profile.json"))
            .output()
            .unwrap()
    };
    let result = run(&profile);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["equal"], true);
    assert_eq!(report["adopted"], false);
    assert_eq!(report["production_current"], false);
    assert_eq!(report["master"]["sha256"], selected.sha256.as_str());
    profile["comparison"]["max_objects"] = json!(1);
    let refused = run(&profile);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("resources exceed admission"));
}
