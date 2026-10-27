use super::*;
use std::collections::BTreeSet;

fn cached(f: &Fixture, raw: &[u8]) -> Object {
    let object = Object::from_bytes(raw);
    fs::write(
        f.root().join("cache/objects").join(object.sha256.as_str()),
        raw,
    )
    .unwrap();
    object
}
fn load(f: &Fixture, path: &str) -> Value {
    serde_json::from_slice(&fs::read(f.root().join(path)).unwrap()).unwrap()
}
fn save_commit(f: &mut Fixture) {
    git(f.root(), &["add", "records"]);
    git(
        f.root(),
        &["commit", "-m", "synthetic language dependency update"],
    );
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
}
fn bilingual() -> (Fixture, reel::tig_scene::Request) {
    let (mut f, mut request) = native_scene_fixture();
    let mut voice = load(&f, "records/cue-main.tig");
    voice["id"] = "cue/english".into();
    let mut bytes = wav();
    *bytes.last_mut().unwrap() = 17;
    let take = cached(&f, &bytes);
    voice["selected"] = json!(take);
    write(f.root(), "records/cue-english.tig", &voice);
    let mut alignment = load(&f, "records/alignment.tig");
    alignment["id"] = "alignment/english".into();
    let bytes = serde_json::to_vec(&json!({"schema":"reel.scene-native-alignment.v1","language":"en","cue_id":"en-1","selected_take_sha256":take.sha256,"sample_rate":48000,"cue_end_sample":48000,"semantic_markers":{"first-line":0,"second-line":24000}})).unwrap();
    alignment["streams"][0]["object"] = json!(cached(&f, &bytes));
    write(f.root(), "records/alignment-english.tig", &alignment);
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
    let mut lane = scene["languages"]["es"].clone();
    lane["cues"][0]["cue_id"] = "en-1".into();
    lane["cues"][0]["take_binding"] = "take/en~selected".into();
    lane["cues"][0]["phrase_alignment_binding"] = "alignment/en~current".into();
    for event in lane["events"].as_array_mut().unwrap() {
        event["cue_id"] = "en-1".into();
    }
    scene["languages"]["en"] = lane;
    parent["selected"] = json!(cached(&f, &serde_json::to_vec(&scene).unwrap()));
    parent["authored"]["binding_ids"]["take/en~selected"] = json!({"id":"cue/english"});
    parent["authored"]["binding_ids"]["alignment/en~current"] = json!({"id":"alignment/english"});
    write(f.root(), "records/scene.tig", &parent);
    save_commit(&mut f);
    request.language = "en".into();
    (f, request)
}
fn compile(f: &Fixture, request: &reel::tig_scene::Request, name: &str) -> Value {
    let output = f.root().join(name);
    reel::tig_scene::compile(&mut f.source(), request, &output).unwrap();
    serde_json::from_slice(&fs::read(output.join("native-source.json")).unwrap()).unwrap()
}
fn primary(report: &Value, scope: &str) -> BTreeSet<Object> {
    serde_json::from_value(report[scope]["primary_objects"].clone()).unwrap()
}

#[test]
fn native_language_scope_retains_alignments_and_controls_without_other_language_bytes() {
    let (f, request) = bilingual();
    let report = compile(&f, &request, "english-scope");
    let full = primary(&report, "declared_inputs");
    let english = primary(&report, "language_inputs");
    assert_eq!(full.len(), 6);
    assert_eq!(english.len(), 4);
    let english_alignment: Object = serde_json::from_value(
        load(&f, "records/alignment-english.tig")["streams"][0]["object"].clone(),
    )
    .unwrap();
    assert!(english.contains(&english_alignment));
    let spanish_alignment: Object =
        serde_json::from_value(load(&f, "records/alignment.tig")["streams"][0]["object"].clone())
            .unwrap();
    assert!(!english.contains(&spanish_alignment));
    assert_eq!(
        report["language_inputs"]["control_objects"],
        report["declared_inputs"]["control_objects"]
    );
    assert_eq!(
        report["language_inputs"]["control_objects"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(report["language_inputs"]["authorizes_eviction"], false);
    assert_eq!(
        report["language_inputs"]["complete_semantic_coverage"],
        false
    );
}

#[test]
fn spanish_take_edit_preserves_english_scope_and_native_semantics() {
    let (mut f, request) = bilingual();
    let before = compile(&f, &request, "before-spanish-edit");
    let parent = fs::read(f.root().join("records/scene.tig")).unwrap();
    let mut voice = load(&f, "records/cue-main.tig");
    // This other-language media is deliberately absent; English does not read it.
    voice["selected"] = json!(Object::from_bytes(b"new Spanish take"));
    write(f.root(), "records/cue-main.tig", &voice);
    save_commit(&mut f);
    let after = compile(&f, &request, "after-spanish-edit");
    assert_eq!(
        primary(&before, "language_inputs"),
        primary(&after, "language_inputs")
    );
    assert_ne!(
        primary(&before, "declared_inputs"),
        primary(&after, "declared_inputs")
    );
    assert_eq!(
        before["language_inputs"]["binding_sha256"],
        after["language_inputs"]["binding_sha256"]
    );
    assert_eq!(
        before["language_inputs"]["native_semantic_sha256"],
        after["language_inputs"]["native_semantic_sha256"]
    );
    assert_ne!(before["source_checkpoint"], after["source_checkpoint"]);
    assert_eq!(
        parent,
        fs::read(f.root().join("records/scene.tig")).unwrap()
    );
}

#[test]
fn shared_cel_replacement_changes_language_scope_and_old_git_view_keeps_old_inputs() {
    let (mut f, request) = bilingual();
    let mut old = f.source();
    let before = compile(&f, &request, "before-cel-replacement");
    let parent = fs::read(f.root().join("records/scene.tig")).unwrap();
    let mut cel = load(&f, "records/cel-b.tig");
    let replacement = cached(&f, &picture([0, 255, 0]));
    cel["selected"] = json!(replacement);
    write(f.root(), "records/cel-b.tig", &cel);
    save_commit(&mut f);
    let after = compile(&f, &request, "after-cel-replacement");
    let a = primary(&before, "language_inputs");
    let b = primary(&after, "language_inputs");
    assert_eq!(a.difference(&b).count(), 1);
    assert_eq!(
        b.difference(&a).cloned().collect::<Vec<_>>(),
        vec![replacement]
    );
    assert_ne!(
        before["language_inputs"]["native_semantic_sha256"],
        after["language_inputs"]["native_semantic_sha256"]
    );
    assert_ne!(
        before["language_inputs"]["binding_sha256"],
        after["language_inputs"]["binding_sha256"]
    );
    assert_eq!(
        parent,
        fs::read(f.root().join("records/scene.tig")).unwrap()
    );
    let path = f.root().join("old-view-replay");
    reel::tig_scene::compile(&mut old, &request, &path).unwrap();
    let replay: Value =
        serde_json::from_slice(&fs::read(path.join("native-source.json")).unwrap()).unwrap();
    assert_eq!(before["language_inputs"], replay["language_inputs"]);
}

#[test]
fn native_scope_rejects_missing_sonic_offset_evidence_before_creating_output() {
    let (mut f, request) = bilingual();
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
    scene["languages"]["en"]["events"][0]["sonic_bindings"] = json!(["effect"]);
    parent["selected"] = json!(cached(&f, &serde_json::to_vec(&scene).unwrap()));
    parent["authored"]["binding_ids"]["effect"] = json!({"id":"cue/english"});
    let profile_slot = parent["streams"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["slot"] == "profile.json")
        .unwrap();
    let pin: Object = serde_json::from_value(profile_slot["object"].clone()).unwrap();
    let mut profile: Value = serde_json::from_slice(
        &fs::read(f.root().join("cache/objects").join(pin.sha256.as_str())).unwrap(),
    )
    .unwrap();
    profile["sonic_placement_offsets_by_binding"] = json!({"effect":{"samples":0,"evidence_binding":"missing-evidence","reason":"synthetic placement"}});
    profile_slot["object"] = json!(cached(&f, &serde_json::to_vec(&profile).unwrap()));
    write(f.root(), "records/scene.tig", &parent);
    save_commit(&mut f);
    let output = f.root().join("missing-evidence-output");
    assert!(reel::tig_scene::compile(&mut f.source(), &request, &output).is_err());
    assert!(!output.exists());
    #[cfg(windows)]
    {
        f.profile["byte_source"]["required"] = "prevent-mutation".into();
        let evidence = cached(&f, b"synthetic Sonic placement evidence");
        let mut record = load(&f, "records/cue-main.tig");
        record["id"] = "evidence/placement".into();
        record["selected"] = json!(evidence);
        write(f.root(), "records/placement-evidence.tig", &record);
        parent["authored"]["binding_ids"]["missing-evidence"] = json!({"id":"evidence/placement"});
        write(f.root(), "records/scene.tig", &parent);
        save_commit(&mut f);
        let report = compile(&f, &request, "retained-placement-evidence");
        assert!(primary(&report, "language_inputs").contains(&evidence));
    }
}

#[test]
fn native_language_scope_supports_more_than_128_selected_bindings_without_rule_expansion() {
    let (mut f, request) = bilingual();
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
    let mut events = Vec::new();
    let mut markers = serde_json::Map::new();
    for i in 0..129 {
        let id = format!("cel/scaled-{i}");
        let key = format!("picture/{i}~selected");
        let mut cel = load(&f, "records/cel-a.tig");
        cel["id"] = id.clone().into();
        write(f.root(), &format!("records/scaled-{i}.tig"), &cel);
        parent["authored"]["binding_ids"][&key] = json!({"id":id});
        let marker = format!("marker-{i}");
        markers.insert(marker.clone(), json!(i * 2000));
        events.push(json!({"event_id":format!("event-{i}"),"cue_id":"en-1","semantic_trigger_id":marker,"picture_slot_id":format!("picture-{i}"),"picture_binding":key,"score":{"disposition":"silence"}}));
    }
    scene["languages"]["en"]["events"] = json!(events);
    parent["selected"] = json!(cached(&f, &serde_json::to_vec(&scene).unwrap()));
    let mut alignment = load(&f, "records/alignment-english.tig");
    let take = load(&f, "records/cue-english.tig");
    let raw=serde_json::to_vec(&json!({"schema":"reel.scene-native-alignment.v1","language":"en","cue_id":"en-1","selected_take_sha256":take["selected"]["sha256"],"sample_rate":48000,"cue_end_sample":258000,"semantic_markers":markers})).unwrap();
    alignment["streams"][0]["object"] = json!(cached(&f, &raw));
    write(f.root(), "records/alignment-english.tig", &alignment);
    let slot = parent["streams"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["slot"] == "profile.json")
        .unwrap();
    let pin: Object = serde_json::from_value(slot["object"].clone()).unwrap();
    let mut profile: Value = serde_json::from_slice(
        &fs::read(f.root().join("cache/objects").join(pin.sha256.as_str())).unwrap(),
    )
    .unwrap();
    profile["max_composition_samples"] = json!(288000);
    slot["object"] = json!(cached(&f, &serde_json::to_vec(&profile).unwrap()));
    write(f.root(), "records/scene.tig", &parent);
    save_commit(&mut f);
    let report = compile(&f, &request, "scaled-scope");
    assert_eq!(primary(&report, "language_inputs").len(), 3);
    assert!(
        report["language_inputs"]["interpretation_objects"]
            .as_array()
            .unwrap()
            .len()
            > 128
    );
    assert_eq!(primary(&report, "declared_inputs").len(), 6);
}
