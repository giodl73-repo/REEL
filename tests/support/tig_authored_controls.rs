use super::*;
use tig_core::tree::{MemoryNodes, TreeLimits};

fn fixture() -> (Fixture, reel::tig_scene::Request) {
    let (mut f, mut request) = control_references::fixture();
    let mut control = load(&f, "records/profile.tig");
    let pin: Object = serde_json::from_value(control["selected"].clone()).unwrap();
    let cached_profile = f.root().join("cache/objects").join(pin.sha256.as_str());
    control["authored"]["parameters"] =
        serde_json::from_slice(&fs::read(&cached_profile).unwrap()).unwrap();
    control["selected"] = Value::Null;
    let mut definition = load(&f, "definitions/control.json");
    definition["authoring"]["properties"] = json!(["parameters"]);
    definition["authoring"]["required"] = json!(["parameters"]);
    let definition_pin = write(f.root(), "definitions/control.json", &definition);
    control["definition"]["sha256"] = json!(definition_pin.sha256);
    control["definition"]["bytes"] = json!(definition_pin.bytes);
    write(f.root(), "records/profile.tig", &control);
    for admitted in f.profile["admitted"].as_array_mut().unwrap() {
        if admitted["id"] == "types/control" {
            admitted["object"] = json!(definition_pin);
        }
    }
    request
        .authored_control_fields
        .insert("types/control".into(), "parameters".into());
    git(f.root(), &["add", "definitions"]);
    save_commit(&mut f);
    fs::remove_file(cached_profile).unwrap(); // Inline control must need no cache copy.
    (f, request)
}

fn plan(
    f: &Fixture,
    request: &reel::tig_scene::Request,
    name: &str,
    nodes: &mut MemoryNodes,
) -> (
    tig_assets::graph::recipe::Plan,
    tig_assets::graph::recipe::Plan,
) {
    let mut source = f.source();
    let producer = Object::from_bytes(b"explicit test producer; no execution claim");
    let whole = reel::tig_scene::recipe(&mut source, request, &producer).unwrap();
    assert_eq!(source.byte_requests(), 0);
    let context = reel::tig_scene::freeze_render_context(
        &mut source,
        request,
        &f.root().join(format!("{name}-context")),
        nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let render = reel::tig_scene::render_recipe(
        &mut source,
        request,
        &context,
        &producer,
        &f.root().join(format!("{name}-recipe")),
    )
    .unwrap();
    assert_eq!(source.byte_requests(), 10); // No profile cache read, on either compilation.
    (whole, render)
}

#[test]
fn inline_profile_edits_invalidate_both_recipes_and_preserve_parent_and_old_view() {
    let (mut f, request) = fixture();
    let parent = fs::read(f.root().join("records/scene.tig")).unwrap();
    let original = f.profile.clone();
    let mut nodes = MemoryNodes::default();
    let (whole, render) = plan(&f, &request, "inline-before", &mut nodes);
    let report = load(&f, "inline-before-context/native-source.json");
    assert!(report["control_objects"].get("profile.json").is_none());
    let origin = &report["authored_controls"]["profile.json"];
    assert_eq!(origin["field"], "parameters");
    assert_eq!(origin["stored_byte_object"], false);
    let synthetic: Object = serde_json::from_value(origin["canonical_json"].clone()).unwrap();
    let mut source = f.source();
    let required = reel::tig_scene::requirements(&mut source, &request).unwrap();
    assert!(!required.controls().contains(&synthetic));
    assert!(required.metadata().contains(&Object::from_bytes(
        &fs::read(f.root().join("records/profile.tig")).unwrap()
    )));
    let mut control = load(&f, "records/profile.tig");
    control["authored"]["parameters"]["width"] = json!(128);
    write(f.root(), "records/profile.tig", &control);
    save_commit(&mut f);
    let (whole_after, render_after) = plan(&f, &request, "inline-after", &mut nodes);
    assert_ne!(whole.fingerprint(), whole_after.fingerprint());
    assert_ne!(render.fingerprint(), render_after.fingerprint());
    assert_eq!(
        parent,
        fs::read(f.root().join("records/scene.tig")).unwrap()
    );
    let mut old = Source::open(serde_json::from_value(original).unwrap()).unwrap();
    reel::tig_scene::compile(&mut old, &request, &f.root().join("inline-old")).unwrap();
    assert_eq!(load(&f, "inline-old/delivery/job.json")["width"], 64);
    assert_eq!(
        load(&f, "inline-after-context/delivery/job.json")["width"],
        128
    );
}

#[test]
fn frozen_inline_parameters_restore_with_git_and_records_unavailable() {
    let (f, request) = fixture();
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let context = reel::tig_scene::freeze_render_context(
        &mut source,
        &request,
        &f.root().join("inline-frozen"),
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let profile = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let (_, restored) = reel::tig_scene::restore_render_context(
        profile,
        &request,
        context.selection(),
        &f.root().join("inline-restored"),
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(context.requirements(), restored.requirements());
    assert_eq!(
        fs::read(f.root().join("inline-frozen/delivery/job.json")).unwrap(),
        fs::read(f.root().join("inline-restored/delivery/job.json")).unwrap()
    );
}

#[test]
fn inline_control_admission_and_field_shape_refuse_before_byte_io() {
    let (mut f, mut request) = fixture();
    request.authored_control_fields.clear();
    let mut source = f.source();
    assert!(reel::tig_scene::requirements(&mut source, &request).is_err());
    assert_eq!(source.byte_requests(), 0); // Never infer inline content from a field name.
    for field in ["", "missing", "bad\nfield"] {
        request
            .authored_control_fields
            .insert("types/control".into(), field.into());
        let mut source = f.source();
        assert!(
            reel::tig_scene::compile(
                &mut source,
                &request,
                &f.root().join("invalid-inline-field")
            )
            .is_err()
        );
        assert_eq!(source.byte_requests(), 0);
    }
    request.authored_control_fields.clear();
    request
        .authored_control_fields
        .insert("types/media".into(), "parameters".into());
    assert!(reel::tig_scene::requirements(&mut f.source(), &request).is_err());
    request.authored_control_fields.clear();
    request
        .authored_control_fields
        .insert("types/control".into(), "parameters".into());
    for value in [json!(null), json!([]), json!("not an object")] {
        let mut control = load(&f, "records/profile.tig");
        control["authored"]["parameters"] = value;
        write(f.root(), "records/profile.tig", &control);
        save_commit(&mut f);
        let mut source = f.source();
        assert!(
            reel::tig_scene::compile(
                &mut source,
                &request,
                &f.root().join("invalid-inline-shape")
            )
            .is_err()
        );
        assert_eq!(source.byte_requests(), 0);
    }
}

#[test]
fn score_starts_use_exact_selected_source_and_refuse_stale_or_unknown_bindings() {
    let (mut f, request) = fixture();
    let mut parent = load(&f, "records/scene.tig");
    let scene_pin: Object = serde_json::from_value(parent["selected"].clone()).unwrap();
    let mut scene: Value = serde_json::from_slice(
        &fs::read(
            f.root()
                .join("cache/objects")
                .join(scene_pin.sha256.as_str()),
        )
        .unwrap(),
    )
    .unwrap();
    for lane in scene["languages"].as_object_mut().unwrap().values_mut() {
        for event in lane["events"].as_array_mut().unwrap() {
            event["score"] = json!({"disposition":"role","role":"theme"});
        }
    }
    parent["selected"] = json!(cached(&f, &serde_json::to_vec(&scene).unwrap()));
    let music = load(&f, "records/cue-main.tig");
    parent["authored"]["binding_ids"]["score.theme"] = json!({"id":"cue/main"});
    let episode_slot = parent["streams"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["slot"] == "episode.json")
        .unwrap();
    let mut episode: Value = serde_json::from_slice(
        &fs::read(
            f.root()
                .join("cache/objects")
                .join(episode_slot["object"]["sha256"].as_str().unwrap()),
        )
        .unwrap(),
    )
    .unwrap();
    episode["score_palette"] = json!([{"role":"theme","theme_id":"theme","source_poem_id":"poem","arrangement_id":"arrangement","asset_binding":"score.theme"}]);
    episode_slot["object"] = json!(cached(&f, &serde_json::to_vec(&episode).unwrap()));
    write(f.root(), "records/scene.tig", &parent);
    let mut control = load(&f, "records/profile.tig");
    control["authored"]["parameters"]["score_source_starts_by_binding"] =
        json!({"score.theme":{"source_sha256":music["selected"]["sha256"],"start_sample":12000}});
    write(f.root(), "records/profile.tig", &control);
    save_commit(&mut f);
    compile(&f, &request, "score-offset");
    let job = load(&f, "score-offset/delivery/job.json");
    let score = job["audio"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["bus"] == "M")
        .unwrap();
    assert_eq!(score["source_start_sample"], 12000);
    for parameters in [
        json!({"typo":{"source_sha256":music["selected"]["sha256"],"start_sample":1}}),
        json!({"score.theme":{"source_sha256":"f".repeat(64),"start_sample":1}}),
    ] {
        control["authored"]["parameters"]["score_source_starts_by_binding"] = parameters;
        write(f.root(), "records/profile.tig", &control);
        save_commit(&mut f);
        assert!(
            reel::tig_scene::compile(
                &mut f.source(),
                &request,
                &f.root().join("bad-score-offset")
            )
            .is_err()
        );
        assert!(!f.root().join("bad-score-offset/delivery/job.json").exists());
    }
}
