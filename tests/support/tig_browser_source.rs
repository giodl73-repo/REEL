use super::*;
use reel::tig_browser::{self as adapter, Bounds, Request};
use std::collections::BTreeSet;

fn edit(f: &Fixture, name: &str, action: impl FnOnce(&mut Value)) {
    let path = format!("records/{name}.tig");
    let mut value = serde_json::from_slice(&fs::read(f.root().join(&path)).unwrap()).unwrap();
    action(&mut value);
    write(f.root(), &path, &value);
}
fn commit(f: &mut Fixture) {
    git(f.root(), &["add", "records", "definitions"]);
    git(f.root(), &["commit", "-m", "synthetic browser revision"]);
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
}
fn fixture() -> (Fixture, Request) {
    let mut f = Fixture::new();
    for name in ["cel-a", "cel-b", "cue-main"] {
        edit(&f, name, |v| {
            v["status"]["selection_state"] = json!("selected-private-production")
        });
    }
    let mut svg: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/cel-a.tig")).unwrap()).unwrap();
    svg["id"] = json!("svg/main");
    svg["selected"] = serde_json::to_value(Object::from_bytes(b"<svg />")).unwrap();
    write(f.root(), "records/svg.tig", &svg);
    let mut definition: Value =
        serde_json::from_slice(&fs::read(f.root().join("definitions/media.json")).unwrap())
            .unwrap();
    definition["id"] = json!("types/browser");
    definition["authoring"]["properties"] = json!(["document"]);
    let pin = write(f.root(), "definitions/browser.json", &definition);
    f.profile["admitted"].as_array_mut().unwrap().push(json!({
        "id":"types/browser","object":pin,"payload":{"kind":"selected"}
    }));
    write(
        f.root(),
        "records/browser.tig",
        &json!({
            "schema":"tig.instance.design-1","id":"browser/source/es",
            "definition":{"id":"types/browser","sha256":pin.sha256,"bytes":pin.bytes},
            "authored":{"document":{"source":"opening.svg","audio":"audio/main.wav",
                "inputs":{"opening.svg":{"id":"svg/main"},"pictures/a.ppm":{"id":"cel/a"},
                    "pictures/b.ppm":{"id":"cel/b"},"audio/main.wav":{"id":"cue/main"}},
                "width":8,"height":8,"fps":24,"frame_count":24}},
            "selected":null,"streams":[],"status":{"selection_state":"selected-private-production"},"metadata":{}
        }),
    );
    commit(&mut f);
    (
        f,
        Request {
            schema: adapter::SCHEMA.into(),
            root_id: "browser/source/es".into(),
            root_definition: "types/browser".into(),
            primary_definitions: BTreeSet::from(["types/media".into()]),
            bounds: Bounds {
                max_inputs: 8,
                max_input_bytes: 1024 * 1024,
                max_frames: 48,
                max_canvas_pixels: 128,
            },
            private_review: None,
        },
    )
}

#[test]
fn native_browser_lowers_exact_primary_ids_without_media_or_output_creation() {
    let (f, request) = fixture();
    let mut source = f.source();
    let prepared = adapter::lower(&mut source, &request).unwrap();
    assert_eq!(prepared.manifest.source.path, "opening.svg");
    assert_eq!(prepared.manifest.assets.len(), 2);
    assert_eq!(prepared.inputs.len(), 4);
    assert_eq!(prepared.inputs["pictures/a.ppm"].path, Path::new("cel/a"));
    assert_eq!(prepared.unique_objects, 4);
    assert_eq!(prepared.alias_bytes, prepared.unique_bytes);
    assert_eq!(prepared.manifest.frame_count, 24);
    assert_eq!(source.byte_requests(), 0);
    assert!(!f.root().join("exports").exists());
}

#[test]
fn changing_one_leaf_changes_lowered_manifest_without_parent_churn() {
    let (mut f, request) = fixture();
    let parent = fs::read(f.root().join("records/browser.tig")).unwrap();
    let before = adapter::lower(&mut f.source(), &request).unwrap();
    edit(&f, "cel-a", |v| {
        v["selected"] = serde_json::to_value(Object::from_bytes(b"replacement picture")).unwrap()
    });
    commit(&mut f);
    let after = adapter::lower(&mut f.source(), &request).unwrap();
    assert_ne!(before.manifest_bytes, after.manifest_bytes);
    assert_eq!(
        before.inputs["pictures/b.ppm"],
        after.inputs["pictures/b.ppm"]
    );
    assert_eq!(before.source_record, after.source_record);
    assert_ne!(before.source_checkpoint, after.source_checkpoint);
    assert_eq!(
        fs::read(f.root().join("records/browser.tig")).unwrap(),
        parent
    );
}

#[test]
fn browser_aliases_preserve_paths_and_separate_unique_from_declared_bytes() {
    let (mut f, mut request) = fixture();
    edit(&f, "browser", |v| {
        v["authored"]["document"]["inputs"]["pictures/a-copy.ppm"] = json!({"id":"cel/a"})
    });
    commit(&mut f);
    let prepared = adapter::lower(&mut f.source(), &request).unwrap();
    assert_eq!(prepared.inputs.len(), 5);
    assert_eq!(prepared.unique_objects, 4);
    assert!(prepared.alias_bytes > prepared.unique_bytes);
    request.bounds.max_input_bytes = prepared.unique_bytes;
    assert!(adapter::lower(&mut f.source(), &request).is_err());
}

#[test]
fn browser_refuses_case_prefix_and_escaping_resource_paths() {
    for path in ["../outside", "/outside", "pictures/A.ppm", "pictures"] {
        let (mut f, request) = fixture();
        edit(&f, "browser", |v| {
            v["authored"]["document"]["inputs"][path] = json!({"id":"cel/a"})
        });
        commit(&mut f);
        let mut source = f.source();
        assert!(adapter::lower(&mut source, &request).is_err(), "{path}");
        assert_eq!(source.byte_requests(), 0);
    }
}

#[test]
fn browser_bounds_and_undeclared_or_pinned_authoring_references_refuse() {
    for (key, value) in [
        ("width", json!(0)),
        ("width", json!(7)),
        ("height", json!(64)),
        ("fps", json!(0)),
        ("frame_count", json!(49)),
        ("source", json!("undeclared.svg")),
    ] {
        let (mut f, request) = fixture();
        edit(&f, "browser", |v| v["authored"]["document"][key] = value);
        commit(&mut f);
        assert!(adapter::lower(&mut f.source(), &request).is_err());
    }
    let (mut f, request) = fixture();
    edit(&f, "browser", |v| {
        v["authored"]["document"]["inputs"]["opening.svg"]["sha256"] =
            json!("a competing parent hash")
    });
    commit(&mut f);
    assert!(adapter::lower(&mut f.source(), &request).is_err());
}

#[test]
fn browser_cannot_consume_an_unadmitted_definition_or_an_unbuilt_primary() {
    let (mut f, request) = fixture();
    edit(&f, "browser", |v| {
        v["authored"]["document"]["inputs"]["opening.svg"] = json!({"id":"alignment/main"})
    });
    commit(&mut f);
    assert!(adapter::lower(&mut f.source(), &request).is_err());
    let (mut f, request) = fixture();
    edit(&f, "svg", |v| v["selected"] = Value::Null);
    commit(&mut f);
    assert!(adapter::lower(&mut f.source(), &request).is_err());
}

#[test]
fn browser_private_review_is_exact_scoped_and_does_not_promote_selection() {
    let (mut f, mut request) = fixture();
    edit(&f, "cel-a", |v| {
        v["status"]["selection_state"] = json!("draft")
    });
    commit(&mut f);
    let raw = fs::read(f.root().join("records/cel-a.tig")).unwrap();
    let mut source = f.source();
    assert!(adapter::lower(&mut source, &request).is_err());
    request.private_review = Some(reel::tig_delivery_template::PrivateReview {
        checkpoint: source.checkpoint().clone(),
        records: BTreeMap::from([("cel/a".into(), Object::from_bytes(&raw))]),
    });
    adapter::lower(&mut source, &request).unwrap();
    assert_eq!(source.byte_requests(), 0);
    assert_eq!(fs::read(f.root().join("records/cel-a.tig")).unwrap(), raw);
    request.private_review.as_mut().unwrap().records.insert(
        "cel/b".into(),
        Object::from_bytes(&fs::read(f.root().join("records/cel-b.tig")).unwrap()),
    );
    assert!(adapter::lower(&mut source, &request).is_err());
}

#[test]
fn browser_recipe_tracks_leaf_bytes_but_not_annotations_or_resource_limits() {
    use tig_core::tree::MemoryNodes;
    let (mut f, mut request) = fixture();
    let producer = Object::from_bytes(b"synthetic admitted browser producer");
    let mut nodes = MemoryNodes::default();
    let mut source = f.source();
    let (_, original) =
        adapter::freeze_context(&mut source, &request, &mut nodes, Default::default()).unwrap();
    let first = adapter::recipe(&mut source, &request, &original, &producer).unwrap();
    assert_eq!(original.requirements().primary().len(), 4);
    edit(&f, "cel-a", |v| {
        v["metadata"]["note"] = json!("annotation only")
    });
    commit(&mut f);
    let mut current = f.source();
    let (_, annotated) =
        adapter::freeze_context(&mut current, &request, &mut nodes, Default::default()).unwrap();
    let annotation_recipe = adapter::recipe(&mut current, &request, &annotated, &producer).unwrap();
    assert_ne!(original.selection(), annotated.selection());
    assert_eq!(first.fingerprint(), annotation_recipe.fingerprint());
    request.bounds.max_frames = 72;
    let mut current = f.source();
    let (_, bounded) =
        adapter::freeze_context(&mut current, &request, &mut nodes, Default::default()).unwrap();
    let bounded_recipe = adapter::recipe(&mut current, &request, &bounded, &producer).unwrap();
    assert_ne!(annotated.descriptor(), bounded.descriptor());
    assert_eq!(first.fingerprint(), bounded_recipe.fingerprint());
    edit(&f, "cel-a", |v| {
        v["selected"] =
            serde_json::to_value(Object::from_bytes(b"changed primary picture")).unwrap()
    });
    commit(&mut f);
    let mut current = f.source();
    let (_, changed) =
        adapter::freeze_context(&mut current, &request, &mut nodes, Default::default()).unwrap();
    let changed_recipe = adapter::recipe(&mut current, &request, &changed, &producer).unwrap();
    assert_ne!(first.fingerprint(), changed_recipe.fingerprint());
    assert_eq!(source.byte_requests() + current.byte_requests(), 0);
}

#[test]
fn frozen_browser_context_reopens_without_original_git_or_loose_controls() {
    use tig_core::tree::MemoryNodes;
    let (f, request) = fixture();
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let (prepared, context) =
        adapter::freeze_context(&mut source, &request, &mut nodes, Default::default()).unwrap();
    let profile = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let (mut restored, replay) = adapter::restore_context(
        profile,
        &request,
        context.selection(),
        &mut nodes,
        Default::default(),
    )
    .unwrap();
    assert_eq!(context.selection(), replay.selection());
    assert_eq!(
        prepared.manifest_bytes,
        adapter::lower(&mut restored, &request)
            .unwrap()
            .manifest_bytes
    );
    assert_eq!(restored.byte_requests(), 0);
    assert!(!f.root().join("exports").exists());
}

#[test]
fn thousand_browser_aliases_keep_one_primary_authority_and_no_media_reads() {
    let (mut f, mut request) = fixture();
    edit(&f, "browser", |v| {
        for index in 0..1000 {
            v["authored"]["document"]["inputs"][format!("pictures/alias-{index:04}.ppm")] =
                json!({"id":"cel/a"});
        }
    });
    commit(&mut f);
    request.bounds.max_inputs = 1024;
    let mut source = f.source();
    let prepared = adapter::lower(&mut source, &request).unwrap();
    assert_eq!(prepared.inputs.len(), 1004);
    assert_eq!(prepared.unique_objects, 4);
    assert_eq!(
        adapter::requirements(&mut source, &request)
            .unwrap()
            .primary()
            .len(),
        4
    );
    assert_eq!(source.byte_requests(), 0);
}

#[test]
fn browser_named_payload_admission_does_not_substitute_selected_bytes() {
    let (mut f, request) = fixture();
    for name in ["cel-a", "cel-b", "cue-main", "svg"] {
        edit(&f, name, |v| {
            v["streams"] = json!([{"slot":"primary","role":"source/primary","group":"input","object":v["selected"]}]);
            v["selected"] =
                serde_json::to_value(Object::from_bytes(b"unconsumed selected evidence")).unwrap();
        });
    }
    f.profile["admitted"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|v| v["id"] == "types/media")
        .unwrap()["payload"] = json!({"kind":"stream","slot":"primary"});
    commit(&mut f);
    let mut source = f.source();
    let prepared = adapter::lower(&mut source, &request).unwrap();
    assert_eq!(
        prepared.inputs["pictures/a.ppm"].sha256,
        Object::from_bytes(&picture([255, 0, 0])).sha256.as_str()
    );
    assert_eq!(source.byte_requests(), 0);
    f.profile["admitted"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|v| v["id"] == "types/media")
        .unwrap()["payload"] = json!({"kind":"stream","slot":"missing"});
    assert!(adapter::lower(&mut f.source(), &request).is_err());
}

#[test]
fn browser_plan_cli_is_metadata_only_and_redacts_ids_paths_and_errors() {
    let (f, request) = fixture();
    let profile = f.root().join("browser-profile.json");
    let request_path = f.root().join("browser-request.json");
    fs::write(&profile, serde_json::to_vec(&f.profile).unwrap()).unwrap();
    fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_reel-tig-scene"))
            .arg("--profile")
            .arg(&profile)
            .arg("plan-browser")
            .arg(&request_path)
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["input_aliases"], 4);
    assert_eq!(summary["unique_objects"], 4);
    assert_eq!(summary["primary_media_opened"], 0);
    assert_eq!(summary["production_executed"], false);
    assert_eq!(summary["authorizes_eviction"], false);
    let raw = String::from_utf8(output.stdout).unwrap();
    for secret in ["browser/source/es", "pictures/a.ppm", "cue/main"] {
        assert!(!raw.contains(secret));
    }
    let mut value = serde_json::to_value(request).unwrap();
    value["root_id"] = json!("private-secret-unavailable-record");
    fs::write(&request_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let output = run();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap().trim(),
        "browser source plan refused; verify explicit metadata and admission"
    );
    assert!(!f.root().join("exports").exists());
}
