use super::*;

fn presentation_fixture(alias: bool) -> (Fixture, reel::tig_scene::Request) {
    let (mut f, mut request) = native_scene_fixture();
    let root = f.root();
    let load = |path: &str| -> Value {
        serde_json::from_slice(&fs::read(root.join(path)).unwrap()).unwrap()
    };
    let cached = |bytes: &[u8]| {
        let object = Object::from_bytes(bytes);
        fs::write(
            root.join("cache/objects").join(object.sha256.as_str()),
            bytes,
        )
        .unwrap();
        object
    };
    let text = cached(b"Synthetic source text");
    let ass = cached(b"[Script Info]\nScriptType: v4.00+\n");
    let receipt = json!({"schema":"reel.editable-layer-compile-receipt.v1","scene_id":"scene-1","language":"es",
        "template_id":"poem-template","template_definition_sha256":"a".repeat(64),
        "ass_sha256":ass.sha256,"source_text_sha256":text.sha256,"sample_rate":48000,"duration_samples":48000});
    let receipt = cached(&serde_json::to_vec(&receipt).unwrap());
    let definition = Object::from_bytes(&fs::read(root.join("definitions/media.json")).unwrap());
    let mut parent = load("records/scene.tig");
    for (key, id, object) in [
        ("source-text", "presentation/text", text),
        ("ass", "presentation/ass", ass),
        ("receipt", "presentation/receipt", receipt.clone()),
    ] {
        write(
            root,
            &format!("records/presentation-{key}.tig"),
            &json!({"schema":"tig.instance.design-1",
            "id":id,"definition":{"id":"types/media","sha256":definition.sha256,"bytes":definition.bytes},
            "authored":{},"selected":object,"streams":[],"status":{"selection_state":"selected-private-production"},"metadata":{}}),
        );
        parent["authored"]["binding_ids"][key] = json!({"id":id});
    }
    let scene_path = format!(
        "cache/objects/{}",
        parent["selected"]["sha256"].as_str().unwrap()
    );
    let mut scene = load(&scene_path);
    scene["presentation"] = json!({"role":"internal-poem","template_id":"poem-template","content":{
        "source_text_bindings":{"es":"source-text"},"ass_layer_bindings":{"es":"ass"},"template_receipt_bindings":{"es":"receipt"}}});
    parent["selected"] =
        serde_json::to_value(cached(&serde_json::to_vec(&scene).unwrap())).unwrap();
    let catalog = json!({"schema":"reel.scene-template-catalog.v1","templates":[{"template_id":"poem-template","kind":"internal-poem",
        "definition_sha256":"a".repeat(64),"required_content_keys":["source_text_bindings","ass_layer_bindings","template_receipt_bindings"]}]});
    let pin = cached(&serde_json::to_vec(&catalog).unwrap());
    for stream in parent["streams"].as_array_mut().unwrap() {
        if stream["slot"] == "catalog.json" {
            stream["object"] = serde_json::to_value(&pin).unwrap();
        }
    }
    if alias {
        parent["streams"].as_array_mut().unwrap().push(json!({"slot":"retained-receipt","role":"native-control","group":"control","object":receipt}));
        request.controls.template_receipt = Some("retained-receipt".into());
    }
    write(root, "records/scene.tig", &parent);
    git(root, &["add", "records"]);
    git(
        root,
        &["commit", "-m", "native editable presentation fixture"],
    );
    f.profile["commit"] = json!(git(root, &["rev-parse", "HEAD"]));
    (f, request)
}
fn replace_receipt(f: &mut Fixture, update: impl FnOnce(&mut Value)) -> Object {
    let path = f.root().join("records/presentation-receipt.tig");
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut value: Value = serde_json::from_slice(
        &fs::read(
            f.root()
                .join("cache/objects")
                .join(record["selected"]["sha256"].as_str().unwrap()),
        )
        .unwrap(),
    )
    .unwrap();
    update(&mut value);
    let bytes = serde_json::to_vec(&value).unwrap();
    let object = Object::from_bytes(&bytes);
    fs::write(
        f.root().join("cache/objects").join(object.sha256.as_str()),
        bytes,
    )
    .unwrap();
    record["selected"] = serde_json::to_value(&object).unwrap();
    write(f.root(), "records/presentation-receipt.tig", &record);
    git(f.root(), &["add", "records/presentation-receipt.tig"]);
    git(f.root(), &["commit", "-m", "revise only the receipt asset"]);
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
    object
}
fn contract(dir: &Path) -> Value {
    serde_json::from_slice(&fs::read(dir.join("delivery/contract.json")).unwrap()).unwrap()
}
#[test]
fn current_receipt_leaf_changes_presentation_without_parent_edit_and_prior_view_replays() {
    let (mut f, request) = presentation_fixture(false);
    let parent = fs::read(f.root().join("records/scene.tig")).unwrap();
    let mut old = f.source();
    let first = f.root().join("presentation-first");
    reel::tig_scene::compile(&mut old, &request, &first).unwrap();
    assert!(contract(&first).get("tail_duration_samples").is_none());
    let changed = replace_receipt(&mut f, |v| v["duration_samples"] = json!(57600));
    assert_eq!(
        fs::read(f.root().join("records/scene.tig")).unwrap(),
        parent
    );
    let next = f.root().join("presentation-next");
    let mut source = f.source();
    reel::tig_scene::compile(&mut source, &request, &next).unwrap();
    assert_eq!(contract(&next)["tail_duration_samples"], 9600);
    assert_eq!(
        Object::from_bytes(&fs::read(next.join("template-receipt.json")).unwrap()),
        changed
    );
    let info: Value =
        serde_json::from_slice(&fs::read(next.join("native-source.json")).unwrap()).unwrap();
    let primary: Vec<Object> =
        serde_json::from_value(info["language_inputs"]["primary_objects"].clone()).unwrap();
    assert!(primary.contains(&changed));
    let before: Value =
        serde_json::from_slice(&fs::read(first.join("native-source.json")).unwrap()).unwrap();
    let before: std::collections::BTreeSet<Object> =
        serde_json::from_value(before["language_inputs"]["primary_objects"].clone()).unwrap();
    let after: std::collections::BTreeSet<Object> = primary.into_iter().collect();
    assert_eq!(before.difference(&after).count(), 1);
    assert_eq!(
        after.difference(&before).cloned().collect::<Vec<_>>(),
        vec![changed]
    );
    let previous = f.root().join("presentation-prior");
    reel::tig_scene::compile(&mut old, &request, &previous).unwrap();
    assert_eq!(contract(&previous), contract(&first));
    assert_eq!(old.active_readers(), 0);
    assert_eq!(source.active_readers(), 0);
}
#[test]
fn explicit_stale_receipt_alias_refuses_before_output_and_auto_uses_current_leaf() {
    let (mut f, mut request) = presentation_fixture(true);
    let matched = f.root().join("presentation-alias-matched");
    reel::tig_scene::compile(&mut f.source(), &request, &matched).unwrap();
    replace_receipt(&mut f, |v| v["duration_samples"] = json!(57600));
    let refused = f.root().join("presentation-alias-refused");
    let error = reel::tig_scene::compile(&mut f.source(), &request, &refused).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("explicit template receipt differs")
    );
    assert!(!refused.exists());
    request.controls.template_receipt = None;
    let current = f.root().join("presentation-auto-current");
    reel::tig_scene::compile(&mut f.source(), &request, &current).unwrap();
    assert_eq!(contract(&current)["tail_duration_samples"], 9600);
}
#[test]
fn wrong_language_and_corrupt_current_receipts_refuse_without_successful_job() {
    let (mut f, request) = presentation_fixture(false);
    let object = replace_receipt(&mut f, |v| v["language"] = json!("en"));
    let output = f.root().join("presentation-wrong-language");
    let error = reel::tig_scene::compile(&mut f.source(), &request, &output).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("presentation clock receipt differs")
    );
    assert!(!output.join("delivery/job.json").exists());
    fs::write(
        f.root().join("cache/objects").join(object.sha256.as_str()),
        b"corrupt",
    )
    .unwrap();
    let corrupt = f.root().join("presentation-corrupt");
    assert!(reel::tig_scene::compile(&mut f.source(), &request, &corrupt).is_err());
    assert!(!corrupt.exists());
    fs::remove_file(f.root().join("cache/objects").join(object.sha256.as_str())).unwrap();
    let missing = f.root().join("presentation-missing");
    assert!(reel::tig_scene::compile(&mut f.source(), &request, &missing).is_err());
    assert!(!missing.exists());
}

#[test]
fn archived_render_context_replays_current_receipt_without_source_git_or_parent_alias() {
    use tig_core::tree::TreeLimits;
    use tig_store::metadata::{Limits, MetadataReader, MetadataStore};
    let (f, request) = presentation_fixture(false);
    let metadata = f.root().join("presentation-metadata");
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
    let context = reel::tig_scene::freeze_render_context(
        &mut source,
        &request,
        &f.root().join("presentation-freeze"),
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
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
    let output = f.root().join("presentation-replayed");
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
    assert_eq!(restored.byte_requests(), 7);
    assert_eq!(restored.active_readers(), 0);
    assert!(output.join("template-receipt.json").is_file());
    assert_eq!(
        contract(&output),
        contract(&f.root().join("presentation-freeze"))
    );
}
