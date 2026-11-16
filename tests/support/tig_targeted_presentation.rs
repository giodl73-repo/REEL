use super::*;

fn load(root: &Path, path: &str) -> Value {
    serde_json::from_slice(&fs::read(root.join(path)).unwrap()).unwrap()
}
fn cache(root: &Path, value: &Value) -> Object {
    let bytes = serde_json::to_vec(value).unwrap();
    let pin = Object::from_bytes(&bytes);
    fs::write(root.join("cache/objects").join(pin.sha256.as_str()), bytes).unwrap();
    pin
}
fn selected(root: &Path, record: &Value) -> Value {
    load(
        root,
        &format!(
            "cache/objects/{}",
            record["selected"]["sha256"].as_str().unwrap()
        ),
    )
}
fn fixture() -> (Fixture, reel::tig_scene::Request) {
    let (mut f, mut request) = super::presentation::presentation_fixture(false);
    let root = f.root().to_path_buf();
    let root = root.as_path();
    let ass = f.add(
        "presentation/ass",
        b"[Script Info]\nScriptType: v4.00+\n[Events]\n",
    );
    let mut ass_record = load(root, "records/presentation-ass.tig");
    ass_record["status"]["selection_state"] = json!("selected-private-production");
    write(root, "records/presentation-ass.tig", &ass_record);
    let mut parent = load(root, "records/scene.tig");
    let mut scene = selected(root, &parent);
    scene["presentation"]["role"] = json!("chapter-title");
    f.add(
        "presentation/font",
        b"synthetic selected font; metadata tests only",
    );
    let mut font = load(root, "records/presentation-font.tig");
    font["status"]["selection_state"] = json!("selected-private-production");
    write(root, "records/presentation-font.tig", &font);
    parent["authored"]["binding_ids"]["font"] = json!({"id":"presentation/font"});
    scene["presentation"]["asset_binding"] = json!("font");
    parent["selected"] = json!(cache(root, &scene));
    let mut projection = scene.clone();
    projection["presentation"]
        .as_object_mut()
        .unwrap()
        .remove("asset_binding");
    projection["scene_id"] = json!("chapter-overlay");
    projection["languages"]["es"]["cues"] = json!([]);
    projection["languages"]["es"]["events"] = json!([]);
    projection["presentation"]["content"]
        .as_object_mut()
        .unwrap()
        .remove("ass_layer_bindings");
    projection["presentation"]["content"]
        .as_object_mut()
        .unwrap()
        .remove("template_receipt_bindings");
    projection["presentation"]["placement"] = json!({
        "mode":"overlay-on-target-scene-start","target_scene_id":"scene-1",
        "target_picture_slot_id":scene["languages"]["es"]["events"][0]["picture_slot_id"]});
    let projection = cache(root, &projection);
    parent["streams"].as_array_mut().unwrap().push(json!({"slot":"chapter-source","role":"native-control","group":"control","object":projection}));
    for stream in parent["streams"].as_array_mut().unwrap() {
        if stream["slot"] == "catalog.json" {
            let mut catalog = load(
                root,
                &format!(
                    "cache/objects/{}",
                    stream["object"]["sha256"].as_str().unwrap()
                ),
            );
            catalog["templates"][0]["kind"] = json!("chapter-title");
            stream["object"] = json!(cache(root, &catalog));
        }
    }
    write(root, "records/scene.tig", &parent);
    request.controls.presentation_overlay = Some(Box::new(
        reel::scene_delivery_compile::TargetedPresentation {
            scene: "chapter-source".into(),
            attachment_id: "chapter-dated".into(),
            title_id: "dated-chapter".into(),
            reason: "Retained chapter over narration".into(),
        },
    ));
    super::presentation::replace_receipt(&mut f, |receipt| {
        receipt["scene_id"] = json!("chapter-overlay");
        receipt["sample_rate"] = json!(100);
        receipt["duration_samples"] = json!(40);
        receipt["source_text_state"] = json!("project-draft-review-held");
        receipt["ass_sha256"] = json!(ass.sha256);
        receipt["ass_bytes"] = json!(ass.bytes);
    });
    // The receipt helper commits its leaf; also commit the changed parent.
    f.commit();
    (f, request)
}

#[test]
fn targeted_chapter_independent_clock_preserves_narrative_and_declared_identity() {
    let (f, request) = fixture();
    let output = f.root().join("compiled");
    let receipt = reel::tig_scene::compile(&mut f.source(), &request, &output).unwrap();
    let contract = load(&output, "delivery/contract.json");
    assert_eq!(contract["sample_rate"], 48000);
    assert_eq!(contract["cues"][0]["duration_samples"], 48000);
    let typed: reel::cue_relative::Contract = serde_json::from_value(contract.clone()).unwrap();
    assert_eq!(typed.tail_duration_samples, 0);
    let title = contract["attachments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "chapter-dated")
        .unwrap();
    assert_eq!(
        title["target"],
        json!({"kind":"title","title_id":"dated-chapter"})
    );
    let job = load(&output, "delivery/job.json");
    let (plain, plain_request) = native_scene_fixture();
    let plain_output = plain.root().join("plain");
    reel::tig_scene::compile(&mut plain.source(), &plain_request, &plain_output).unwrap();
    let plain_job = load(&plain_output, "delivery/job.json");
    for field in [
        "pictures",
        "audio",
        "buses",
        "width",
        "height",
        "max_composition_samples",
    ] {
        assert_eq!(
            job[field], plain_job[field],
            "overlay changed narrative {field}"
        );
    }
    assert_eq!(
        contract["cues"],
        load(&plain_output, "delivery/contract.json")["cues"]
    );
    assert_eq!(job["pictures"].as_array().unwrap().len(), 2);
    assert_eq!(job["audio"].as_array().unwrap().len(), 1);
    assert_eq!(job["external_layers"][0]["attachment_id"], "chapter-dated");
    assert_eq!(
        job["external_layers"][0]["reason"],
        "Retained chapter over narration"
    );
    assert!(receipt["input_sha256"]["presentation_source"].is_string());
    assert!(receipt["targeted_presentation_compiler_sha256"].is_string());
    let inputs = load(&output, "native-source.json");
    assert!(inputs["control_objects"]["presentation-source.json"].is_object());
    assert_eq!(inputs["new_production_approval"], false);
    let assets = output.join("local-assets");
    let bindings = load(&output, "scene-bindings.json");
    for asset in bindings["assets"].as_object().unwrap().values() {
        let path = assets.join(asset["logical_id"].as_str().unwrap());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::copy(
            f.root()
                .join("cache/objects")
                .join(asset["sha256"].as_str().unwrap()),
            path,
        )
        .unwrap();
    }
    let check = || {
        Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
            .arg("check-inputs")
            .arg(&output)
            .arg("delivery/build.json")
            .arg("--asset-root")
            .arg(&assets)
            .output()
            .unwrap()
    };
    let checked = check();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let mut changed = load(&output, "presentation-source.json");
    changed["scene_id"] = json!("foreign-source");
    write(&output, "presentation-source.json", &changed);
    assert!(!check().status.success());
    let no_context = f.root().join("legacy-refused");
    let mut legacy = request;
    legacy.controls.presentation_overlay = None;
    assert!(reel::tig_scene::compile(&mut f.source(), &legacy, &no_context).is_err());
    assert!(!no_context.join("delivery/job.json").exists());
}

#[test]
fn targeted_chapter_rejects_foreign_receipts_and_invalid_clock() {
    for (field, value) in [
        ("scene_id", json!("another-overlay")),
        ("language", json!("en")),
        ("template_definition_sha256", json!("f".repeat(64))),
        ("ass_sha256", json!("f".repeat(64))),
        ("ass_bytes", json!(999)),
        ("source_text_sha256", json!("f".repeat(64))),
        ("duration_samples", json!(101)),
        ("duration_samples", json!(0)),
        ("sample_rate", json!(0)),
        ("sample_rate", json!(101)),
    ] {
        let (mut f, request) = fixture();
        super::presentation::replace_receipt(&mut f, |receipt| receipt[field] = value);
        let output = f.root().join("refused");
        assert!(
            reel::tig_scene::compile(&mut f.source(), &request, &output).is_err(),
            "accepted invalid {field}"
        );
        assert!(!output.join("delivery/job.json").exists());
    }
}

#[test]
fn targeted_chapter_rejects_wrong_target_content_and_unadmitted_source() {
    for (pointer, value) in [
        (
            "/presentation/placement/target_scene_id",
            json!("other-scene"),
        ),
        (
            "/presentation/placement/target_picture_slot_id",
            json!("other-picture"),
        ),
        (
            "/presentation/content/source_text_bindings/es",
            json!("other-text"),
        ),
        ("/source_authority_id", json!("other-authority")),
        ("/episode_id", json!("other-episode")),
        ("/scene_id", json!("scene-1")),
        ("/languages/es/cues", json!([{}])),
    ] {
        let (mut f, request) = fixture();
        let mut parent = load(f.root(), "records/scene.tig");
        let stream = parent["streams"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|s| s["slot"] == "chapter-source")
            .unwrap();
        let mut source = load(
            f.root(),
            &format!(
                "cache/objects/{}",
                stream["object"]["sha256"].as_str().unwrap()
            ),
        );
        *source.pointer_mut(pointer).unwrap() = value;
        stream["object"] = json!(cache(f.root(), &source));
        write(f.root(), "records/scene.tig", &parent);
        f.commit();
        let output = f.root().join("refused");
        assert!(
            reel::tig_scene::compile(&mut f.source(), &request, &output).is_err(),
            "accepted invalid {pointer}"
        );
        assert!(!output.join("delivery/job.json").exists());
    }
    let (f, mut request) = fixture();
    request
        .controls
        .presentation_overlay
        .as_mut()
        .unwrap()
        .scene = "not-declared".into();
    let output = f.root().join("unadmitted");
    assert!(reel::tig_scene::compile(&mut f.source(), &request, &output).is_err());
    assert!(!output.exists());
}

#[test]
fn targeted_chapter_archive_replays_without_git_or_primary_media() {
    use tig_core::tree::TreeLimits;
    use tig_store::metadata::{Limits, MetadataReader, MetadataStore};
    let (f, request) = fixture();
    // Remove every selected media/text/font payload before compilation. Only
    // admitted metadata, the selected receipt and measured alignment remain.
    for record in [
        "cel-a",
        "cel-b",
        "cue-main",
        "presentation-ass",
        "presentation-font",
        "presentation-source-text",
    ] {
        let record = load(f.root(), &format!("records/{record}.tig"));
        fs::remove_file(
            f.root()
                .join("cache/objects")
                .join(record["selected"]["sha256"].as_str().unwrap()),
        )
        .unwrap();
    }
    let metadata = f.root().join("metadata");
    let mut nodes = MetadataStore::create(
        &metadata,
        Limits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let mut source = f.source();
    let first = f.root().join("first");
    let context = reel::tig_scene::freeze_render_context(
        &mut source,
        &request,
        &first,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(source.byte_requests(), 8);
    let profile = serde_json::from_value(json!({"schema":reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA,
        "archive":context.archive(),"checkpoint":source.checkpoint(),
        "admitted":f.profile["admitted"],"byte_source":f.profile["byte_source"]}))
    .unwrap();
    let checkpoint = nodes.finish().unwrap();
    drop(nodes);
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut reader = MetadataReader::open(&metadata, &checkpoint).unwrap();
    let output = f.root().join("replayed");
    let (restored, replayed) = reel::tig_scene::restore_render_context(
        profile,
        &request,
        context.selection(),
        &output,
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(replayed.selection(), context.selection());
    assert_eq!(restored.byte_requests(), 8);
    assert_eq!(restored.active_readers(), 0);
    for file in [
        "delivery/job.json",
        "delivery/contract.json",
        "presentation-source.json",
    ] {
        assert_eq!(
            fs::read(first.join(file)).unwrap(),
            fs::read(output.join(file)).unwrap()
        );
    }
}

#[test]
fn targeted_chapter_local_inputs_refuse_tampering_oversize_and_path_escape() {
    let (f, request) = fixture();
    let output = f.root().join("local-inputs");
    reel::tig_scene::compile(&mut f.source(), &request, &output).unwrap();
    let mut compile: reel::scene_delivery_compile::CompileManifest =
        serde_json::from_value(load(&output, "compile.json")).unwrap();
    compile.output_dir = "refused".into();
    for (name, oversized) in [
        ("presentation-source.json", true),
        ("template-receipt.json", true),
        ("template-receipt.json", false),
    ] {
        let path = output.join(name);
        let original = fs::read(&path).unwrap();
        let mut bytes = original.clone();
        if oversized {
            bytes.resize(65_537, b' ');
        } else {
            bytes.push(b' ');
        }
        fs::write(&path, bytes).unwrap();
        assert!(reel::scene_delivery_compile::compile_to_dir(&output, &compile).is_err());
        assert!(!output.join("refused").exists());
        fs::write(path, original).unwrap();
    }
    compile.presentation_overlay.as_mut().unwrap().scene = "../outside.json".into();
    assert!(reel::scene_delivery_compile::compile_to_dir(&output, &compile).is_err());
    assert!(!output.join("refused").exists());
}

#[test]
fn targeted_chapter_independent_control_is_in_declared_and_language_requirements() {
    let (mut f, mut request) = fixture();
    let root = f.root();
    let mut definition = load(root, "definitions/scene.json");
    definition["authoring"]["properties"]
        .as_array_mut()
        .unwrap()
        .push(json!("control_ids"));
    let definition_pin = write(root, "definitions/scene.json", &definition);
    let mut parent = load(root, "records/scene.tig");
    parent["definition"]["sha256"] = json!(definition_pin.sha256);
    parent["definition"]["bytes"] = json!(definition_pin.bytes);
    let projection = parent["streams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["slot"] == "chapter-source")
        .unwrap()["object"]
        .clone();
    parent["streams"]
        .as_array_mut()
        .unwrap()
        .retain(|s| s["slot"] != "chapter-source");
    parent["authored"]["control_ids"] = json!({"chapter-source":{"id":"controls/chapter"}});
    let schema = Object::from_bytes(&fs::read(root.join("definitions/authored.json")).unwrap());
    let control = write(
        root,
        "definitions/control.json",
        &json!({
        "schema":"tig.definition.design-1","id":"types/control","authored_schema":schema,
        "authoring":{"properties":[],"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
    );
    write(
        root,
        "records/chapter-control.tig",
        &json!({"schema":"tig.instance.design-1","id":"controls/chapter",
        "definition":{"id":"types/control","sha256":control.sha256,"bytes":control.bytes},
        "authored":{},"selected":projection,"streams":[],"status":{"selection_state":"selected-private-production"},"metadata":{}}),
    );
    write(root, "records/scene.tig", &parent);
    for admitted in f.profile["admitted"].as_array_mut().unwrap() {
        if admitted["id"] == "types/scene" {
            admitted["object"] = json!(definition_pin);
        }
    }
    f.profile["admitted"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"types/control","object":control,"payload":{"kind":"selected"}}));
    request.control_definitions.push("types/control".into());
    git(f.root(), &["add", "definitions"]);
    f.commit();
    let required = reel::tig_scene::requirements(&mut f.source(), &request).unwrap();
    let projection: Object = serde_json::from_value(projection).unwrap();
    assert!(required.controls().contains(&projection));
    let output = f.root().join("independent-control");
    reel::tig_scene::compile(&mut f.source(), &request, &output).unwrap();
    let source = load(&output, "native-source.json");
    let scoped: std::collections::BTreeSet<Object> =
        serde_json::from_value(source["language_inputs"]["control_objects"].clone()).unwrap();
    assert!(scoped.contains(&projection));
    assert_eq!(
        source["control_objects"]["presentation-source.json"],
        json!(projection)
    );
}
