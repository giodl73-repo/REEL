use super::*;
use tig_core::tree::{MemoryNodes, TreeLimits};

fn fixture() -> (Fixture, reel::tig_scene::Request) {
    let (mut f, request) = bilingual();
    let mut definition = load(&f, "definitions/scene.json");
    definition["authoring"]["properties"]
        .as_array_mut()
        .unwrap()
        .push(json!("language_binding_ids"));
    let pin = write(f.root(), "definitions/scene.json", &definition);
    let mut parent = load(&f, "records/scene.tig");
    parent["definition"]["sha256"] = json!(pin.sha256);
    parent["definition"]["bytes"] = json!(pin.bytes);
    parent["authored"]["language_binding_ids"] = json!({"en":{"a":{"id":"cel/english-a"}}});
    write(f.root(), "records/scene.tig", &parent);
    let mut leaf = load(&f, "records/cel-a.tig");
    leaf["id"] = json!("cel/english-a");
    leaf["selected"] = json!(Object::from_bytes(
        b"English-only picture v1; metadata compilation does not read it"
    ));
    write(f.root(), "records/variant.tig", &leaf);
    for admitted in f.profile["admitted"].as_array_mut().unwrap() {
        if admitted["id"] == "types/scene" {
            admitted["object"] = json!(pin);
        }
    }
    git(f.root(), &["add", "definitions"]);
    save_commit(&mut f);
    (f, request)
}

#[test]
fn explicit_variant_selects_language_and_only_its_recipe_changes_after_leaf_edit() {
    let (mut f, mut request) = fixture();
    let mut nodes = MemoryNodes::default();
    let producer = Object::from_bytes(b"explicit external producer, no execution claim");
    let parent = fs::read(f.root().join("records/scene.tig")).unwrap();
    let old_profile = f.profile.clone();
    let en = compile(&f, &request, "variant-en-before");
    request.language = "es".into();
    let es = compile(&f, &request, "variant-es-before");
    let selected: Object =
        serde_json::from_value(load(&f, "records/variant.tig")["selected"].clone()).unwrap();
    assert!(primary(&en, "language_inputs").contains(&selected));
    assert!(!primary(&es, "language_inputs").contains(&selected));
    assert!(primary(&es, "declared_inputs").contains(&selected));
    let (_, es_before, _) = render_recipe::plan(
        &f,
        &request,
        &producer,
        "variant-es-plan-before",
        &mut nodes,
    );
    request.language = "en".into();
    let (_, en_before, _) = render_recipe::plan(
        &f,
        &request,
        &producer,
        "variant-en-plan-before",
        &mut nodes,
    );
    let mut leaf = load(&f, "records/variant.tig");
    leaf["selected"] = json!(Object::from_bytes(b"English-only picture v2"));
    write(f.root(), "records/variant.tig", &leaf);
    save_commit(&mut f);
    let (_, en_after, _) =
        render_recipe::plan(&f, &request, &producer, "variant-en-plan-after", &mut nodes);
    request.language = "es".into();
    let (_, es_after, _) =
        render_recipe::plan(&f, &request, &producer, "variant-es-plan-after", &mut nodes);
    assert_ne!(en_before.fingerprint(), en_after.fingerprint());
    assert_eq!(es_before.fingerprint(), es_after.fingerprint());
    assert_eq!(
        parent,
        fs::read(f.root().join("records/scene.tig")).unwrap()
    );
    let mut original = Source::open(serde_json::from_value(old_profile).unwrap()).unwrap();
    request.language = "en".into();
    let out = f.root().join("variant-old-view");
    reel::tig_scene::compile(&mut original, &request, &out).unwrap();
    let old: Value =
        serde_json::from_slice(&fs::read(out.join("native-source.json")).unwrap()).unwrap();
    assert_eq!(
        primary(&en, "language_inputs"),
        primary(&old, "language_inputs")
    );
}

#[test]
fn malformed_inactive_variants_cannot_hide_invalid_references_or_types() {
    for value in [
        json!(null),
        json!({}),
        json!([]),
        json!({"en":null}),
        json!({"en":{}}),
        json!({"bad/tag":{"a":{"id":"cel/a"}}}),
        json!({"en":{"typo":{"id":"cel/a"}}}),
        json!({"en":{"a":{"id":"cel/a","extra":true}}}),
        json!({"en":{"a":{"id":"missing"}}}),
        json!({"en":{"a":{"id":"scene/native"}}}),
    ] {
        let (mut f, mut request) = fixture();
        request.language = "es".into();
        let mut parent = load(&f, "records/scene.tig");
        parent["authored"]["language_binding_ids"] = value;
        write(f.root(), "records/scene.tig", &parent);
        save_commit(&mut f);
        let mut source = f.source();
        let out = f.root().join("invalid-variant");
        assert!(reel::tig_scene::compile(&mut source, &request, &out).is_err());
        assert_eq!(source.byte_requests(), 0);
        assert!(!out.exists());
    }
    let (mut f, mut request) = fixture();
    request.language = "es".into();
    let mut parent = load(&f, "records/scene.tig");
    parent["authored"]["language_binding_ids"] = json!({"fr":{"a":{"id":"cel/english-a"}}});
    write(f.root(), "records/scene.tig", &parent);
    save_commit(&mut f);
    let mut source = f.source();
    assert!(
        reel::tig_scene::compile(&mut source, &request, &f.root().join("unknown-language"))
            .is_err()
    );
    assert_eq!(source.byte_requests(), 1); // selected scene only, before controls/alignment/media
}

#[test]
fn inactive_draft_variant_is_not_consumed_and_frozen_selected_variant_replays_offline() {
    let (mut f, mut request) = fixture();
    let mut leaf = load(&f, "records/variant.tig");
    leaf["status"]["selection_state"] = json!("private-unselected-candidate");
    write(f.root(), "records/variant.tig", &leaf);
    save_commit(&mut f);
    request.language = "es".into();
    compile(&f, &request, "inactive-draft-variant");
    request.language = "en".into();
    let out = f.root().join("active-draft-variant");
    assert!(reel::tig_scene::compile(&mut f.source(), &request, &out).is_err());
    assert!(!out.exists());
    leaf["status"]["selection_state"] = json!("selected-private-production");
    write(f.root(), "records/variant.tig", &leaf);
    save_commit(&mut f);
    let mut nodes = MemoryNodes::default();
    let mut source = f.source();
    let context = reel::tig_scene::freeze_language_context(
        &mut source,
        &request,
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
    let (mut restored, _) = reel::tig_scene::restore_language_context(
        profile,
        &request,
        context.selection(),
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let out = f.root().join("offline-variant");
    reel::tig_scene::compile_language_context(&mut restored, &request, &out).unwrap();
    let result: Value =
        serde_json::from_slice(&fs::read(out.join("native-source.json")).unwrap()).unwrap();
    let selected: Object = serde_json::from_value(leaf["selected"].clone()).unwrap();
    assert!(primary(&result, "language_inputs").contains(&selected));
}
