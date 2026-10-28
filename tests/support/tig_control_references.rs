use super::*;
use tig_core::tree::{MemoryNodes, TreeLimits};

pub(super) fn fixture() -> (Fixture, reel::tig_scene::Request) {
    let (mut f, mut request) = bilingual();
    let mut definition = load(&f, "definitions/scene.json");
    definition["authoring"]["properties"]
        .as_array_mut()
        .unwrap()
        .push(json!("control_ids"));
    let pin = write(f.root(), "definitions/scene.json", &definition);
    let mut parent = load(&f, "records/scene.tig");
    parent["definition"]["sha256"] = json!(pin.sha256);
    parent["definition"]["bytes"] = json!(pin.bytes);
    let profile = parent["streams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["slot"] == "profile.json")
        .unwrap()["object"]
        .clone();
    parent["streams"]
        .as_array_mut()
        .unwrap()
        .retain(|s| s["slot"] != "profile.json");
    parent["authored"]["control_ids"] = json!({"profile.json":{"id":"controls/profile"}});
    let schema = Object::from_bytes(&fs::read(f.root().join("definitions/authored.json")).unwrap());
    let control = write(
        f.root(),
        "definitions/control.json",
        &json!({"schema":"tig.definition.design-1","id":"types/control","authored_schema":schema,"authoring":{"properties":[],"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
    );
    write(
        f.root(),
        "records/profile.tig",
        &json!({"schema":"tig.instance.design-1","id":"controls/profile","definition":{"id":"types/control","sha256":control.sha256,"bytes":control.bytes},"authored":{},"selected":profile,"streams":[],"status":{"selection_state":"selected-private-production"},"metadata":{}}),
    );
    write(f.root(), "records/scene.tig", &parent);
    for admitted in f.profile["admitted"].as_array_mut().unwrap() {
        if admitted["id"] == "types/scene" {
            admitted["object"] = json!(pin);
        }
    }
    f.profile["admitted"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"types/control","object":control,"payload":{"kind":"selected"}}));
    request.control_definitions.push("types/control".into());
    git(f.root(), &["add", "definitions"]);
    save_commit(&mut f);
    (f, request)
}

#[test]
fn profile_leaf_edit_changes_job_and_recipe_without_parent_edit_and_old_view_replays() {
    let (mut f, request) = fixture();
    let parent = fs::read(f.root().join("records/scene.tig")).unwrap();
    let original_profile = f.profile.clone();
    let mut nodes = MemoryNodes::default();
    let producer = Object::from_bytes(b"external admitted producer; no execution claim");
    let (_, before, _) = render_recipe::plan(&f, &request, &producer, "control-before", &mut nodes);
    let mut control = load(&f, "records/profile.tig");
    let pin: Object = serde_json::from_value(control["selected"].clone()).unwrap();
    let mut value: Value = serde_json::from_slice(
        &fs::read(f.root().join("cache/objects").join(pin.sha256.as_str())).unwrap(),
    )
    .unwrap();
    value["width"] = json!(128);
    control["selected"] = json!(cached(&f, &serde_json::to_vec(&value).unwrap()));
    write(f.root(), "records/profile.tig", &control);
    save_commit(&mut f);
    let (_, after, _) = render_recipe::plan(&f, &request, &producer, "control-after", &mut nodes);
    assert_ne!(before.fingerprint(), after.fingerprint());
    assert_eq!(
        parent,
        fs::read(f.root().join("records/scene.tig")).unwrap()
    );
    let mut old = Source::open(serde_json::from_value(original_profile).unwrap()).unwrap();
    let out = f.root().join("control-old-view");
    reel::tig_scene::compile(&mut old, &request, &out).unwrap();
    let job: Value =
        serde_json::from_slice(&fs::read(out.join("delivery/job.json")).unwrap()).unwrap();
    assert_eq!(job["width"], 64);
    let job: Value = serde_json::from_slice(
        &fs::read(f.root().join("control-after-context/delivery/job.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(job["width"], 128);
    let mut source = f.source();
    let required = reel::tig_scene::requirements(&mut source, &request).unwrap();
    let selected: Object = serde_json::from_value(control["selected"].clone()).unwrap();
    assert!(required.controls().contains(&selected));
    assert!(!required.primary().contains(&selected));
    // A control rule alone must not accidentally require language_binding_ids.
    assert!(reel::tig_scene::recipe(&mut source, &request, &producer).is_ok());
}

#[test]
fn named_control_payloads_and_unused_drafts_keep_explicit_scopes() {
    let (mut f, request) = fixture();
    let mut control = load(&f, "records/profile.tig");
    let selected = control.as_object_mut().unwrap().remove("selected").unwrap();
    control["selected"] = Value::Null;
    control["streams"] =
        json!([{"slot":"parameters","role":"native-control","group":"control","object":selected}]);
    write(f.root(), "records/profile.tig", &control);
    for admitted in f.profile["admitted"].as_array_mut().unwrap() {
        if admitted["id"] == "types/control" {
            admitted["payload"] = json!({"kind":"stream","slot":"parameters"});
        }
    }
    control["id"] = json!("controls/unused-draft");
    control["status"]["selection_state"] = json!("draft");
    let unavailable = Object::from_bytes(b"not materialized inactive control");
    control["streams"][0]["object"] = json!(unavailable);
    write(f.root(), "records/unused-control.tig", &control);
    let mut parent = load(&f, "records/scene.tig");
    parent["authored"]["control_ids"]["profile.fr.json"] = json!({"id":"controls/unused-draft"});
    write(f.root(), "records/scene.tig", &parent);
    save_commit(&mut f);
    let result = compile(&f, &request, "named-control");
    let declared: BTreeSet<Object> =
        serde_json::from_value(result["declared_inputs"]["control_objects"].clone()).unwrap();
    let scoped: BTreeSet<Object> =
        serde_json::from_value(result["language_inputs"]["control_objects"].clone()).unwrap();
    assert!(declared.contains(&unavailable));
    assert!(!scoped.contains(&unavailable));
    let pin: Object = serde_json::from_value(selected).unwrap();
    assert!(scoped.contains(&pin));
}

#[test]
fn control_references_survive_frozen_offline_replay() {
    let (f, request) = fixture();
    let mut nodes = MemoryNodes::default();
    let mut source = f.source();
    let context = reel::tig_scene::freeze_render_context(
        &mut source,
        &request,
        &f.root().join("control-frozen"),
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
    let out = f.root().join("control-restored");
    let (_, restored) = reel::tig_scene::restore_render_context(
        profile,
        &request,
        context.selection(),
        &out,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(context.requirements(), restored.requirements());
    assert_eq!(
        fs::read(f.root().join("control-frozen/delivery/job.json")).unwrap(),
        fs::read(out.join("delivery/job.json")).unwrap()
    );
}

#[test]
fn malformed_inactive_unadmitted_and_conflicting_controls_refuse_before_bytes() {
    for value in [
        json!(null),
        json!({}),
        json!([]),
        json!({"profile.json":{"id":"controls/profile","extra":true}}),
        json!({"unused":{"id":"missing"}}),
        json!({"unused":{"id":"cel/a"}}),
        json!({"unused":{"id":"scene/native"}}),
    ] {
        let (mut f, request) = fixture();
        let mut parent = load(&f, "records/scene.tig");
        parent["authored"]["control_ids"] = value;
        write(f.root(), "records/scene.tig", &parent);
        save_commit(&mut f);
        let mut source = f.source();
        let out = f.root().join("bad-control");
        assert!(reel::tig_scene::compile(&mut source, &request, &out).is_err());
        assert_eq!(source.byte_requests(), 0);
        assert!(!out.exists());
    }
    let (mut f, mut request) = fixture();
    request.control_definitions.push("types/media".into());
    assert!(reel::tig_scene::requirements(&mut f.source(), &request).is_err());
    request.control_definitions = vec!["types/control".into()];
    let mut parent = load(&f, "records/scene.tig");
    parent["streams"].as_array_mut().unwrap().push(json!({"slot":"profile.json","role":"native-control","group":"control","object":Object::from_bytes(b"stale duplicate")}));
    write(f.root(), "records/scene.tig", &parent);
    save_commit(&mut f);
    let mut source = f.source();
    assert!(
        reel::tig_scene::compile(&mut source, &request, &f.root().join("stale-control")).is_err()
    );
    assert_eq!(source.byte_requests(), 0);
}

#[test]
fn matching_legacy_alias_is_allowed_but_used_draft_or_missing_bytes_never_fall_back() {
    let (mut f, request) = fixture();
    let mut parent = load(&f, "records/scene.tig");
    let mut control = load(&f, "records/profile.tig");
    parent["streams"].as_array_mut().unwrap().push(json!({"slot":"profile.json","role":"native-control","group":"control","object":control["selected"]}));
    write(f.root(), "records/scene.tig", &parent);
    save_commit(&mut f);
    compile(&f, &request, "matched-control");
    control["status"]["selection_state"] = json!("draft");
    write(f.root(), "records/profile.tig", &control);
    save_commit(&mut f);
    let out = f.root().join("draft-control");
    assert!(reel::tig_scene::compile(&mut f.source(), &request, &out).is_err());
    assert!(!out.exists());
    control["status"]["selection_state"] = json!("selected-private-production");
    write(f.root(), "records/profile.tig", &control);
    save_commit(&mut f);
    fs::remove_file(
        f.root()
            .join("cache/objects")
            .join(control["selected"]["sha256"].as_str().unwrap()),
    )
    .unwrap();
    assert!(
        reel::tig_scene::compile(&mut f.source(), &request, &f.root().join("missing-control"))
            .is_err()
    );
}
