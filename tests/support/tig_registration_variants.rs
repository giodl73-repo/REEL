use super::*;

fn descriptor(object: &Object) -> Value {
    json!({"sha256":object.sha256,"bytes":object.bytes,"cache_uri":format!("cache://sha256/{}",object.sha256.as_str())})
}
fn object_json(f: &Fixture, hash: &str) -> Value {
    serde_json::from_slice(&fs::read(f.root.path().join("objects").join(hash)).unwrap()).unwrap()
}
fn fixture(edit: impl FnOnce(&mut Value)) -> Fixture {
    let mut f = Fixture::standard();
    let mut package = object_json(&f, f.profile["package"]["sha256"].as_str().unwrap());
    let mut scene = object_json(
        &f,
        package["assets"]["source/scene"]["sha256"]
            .as_str()
            .unwrap(),
    );
    scene["languages"] = json!({"es":{"cues":[]},"en":{"cues":[]}});
    package["assets"]["source/scene"] =
        descriptor(&put(f.root.path(), &serde_json::to_vec(&scene).unwrap()));
    let mut bindings = object_json(
        &f,
        package["assets"]["source/bindings"]["sha256"]
            .as_str()
            .unwrap(),
    );
    bindings["assets"]["cel-a"]["logical_id"] = json!("cel:English");
    bindings["assets"]["cel-a"]["sha256"] = json!("c".repeat(64));
    bindings["assets"]["cel-a"]["bytes"] = json!(11111);
    bindings["assets"]["cel-a"]["cache_uri"] = json!(format!("cache://sha256/{}", "c".repeat(64)));
    edit(&mut bindings);
    package["assets"]["source/english-bindings"] =
        descriptor(&put(f.root.path(), &serde_json::to_vec(&bindings).unwrap()));
    f.profile["package"] = json!(put(f.root.path(), &serde_json::to_vec(&package).unwrap()));
    f.profile["language_bindings_assets"] = json!({"en":"source/english-bindings"});
    let path = f.profile["scene_definition"]["definition"]["path"]
        .as_str()
        .unwrap();
    let mut definition: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    definition["authoring"]["properties"]
        .as_array_mut()
        .unwrap()
        .push(json!("language_binding_ids"));
    f.profile["scene_definition"]["definition"] =
        file(Path::new(path), &serde_json::to_vec(&definition).unwrap());
    f
}

#[test]
fn explicit_binding_sources_deduplicate_shared_assets_and_preserve_variant_provenance() {
    let f = fixture(|_| {});
    let profile = f.parsed();
    let mut cache = ReadCache::new(profile.byte_source.clone()).unwrap();
    let prepared = tig_native_registration::prepare(&profile, &mut cache).unwrap();
    assert_eq!(cache.requests(), 4); // package, scene, base bindings, explicit language bindings
    let output = f.root.path().join("variants");
    let report = prepared.write(&output).unwrap();
    assert_eq!(report["records"], 4); // parent, two shared/base leaves, one variant
    let parent = Instance::parse(&fs::read(output.join("scene.tig")).unwrap()).unwrap();
    assert_eq!(
        parent.value().authored["binding_ids"]["cel-a"]["id"],
        "cel:A"
    );
    assert_eq!(
        parent.value().authored["language_binding_ids"],
        json!({"en":{"cel-a":{"id":"cel:English"}}})
    );
    assert!(parent.value().metadata["native_registration"]["language_bindings"]["en"].is_object());
    let leaf = Instance::parse(&fs::read(output.join("inputs/cel-a.en.tig")).unwrap()).unwrap();
    assert_eq!(leaf.value().id, "cel:English");
    assert_eq!(leaf.value().selected.as_ref().unwrap().bytes, 11111);
    assert_eq!(
        leaf.value().status["selection_state"],
        "private-unselected-candidate"
    );
    assert_eq!(
        leaf.value().metadata["native_registration"]["language"],
        "en"
    );
    assert_eq!(cache.active_readers(), 0);
    assert!(!f.root.path().join("objects").join("c".repeat(64)).exists());
    let cli = f.run(&f.root.path().join("cli-variants"));
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    assert!(!f.run(&output).status.success());
}

#[test]
fn conflicting_shared_ids_scopes_keys_and_sizes_refuse_before_publication() {
    for case in 0..8 {
        let f = fixture(|bindings| match case {
            0 => bindings["scope_id"] = json!("other-scene"),
            1 => bindings["assets"]["cel-a"]["logical_id"] = json!("scene:01"),
            2 => bindings["assets"]["shared-cue"]["selection_state"] = json!("different-state"),
            3 => bindings["assets"]["shared-cue"]["bytes"] = json!(1),
            4 => bindings["assets"]["shared-cue"]["logical_id"] = json!("cel:A"),
            5 => bindings["assets"]["new-unknown-alias"] = bindings["assets"]["cel-a"].clone(),
            6 => bindings["assets"]["cel-a"]["cache_uri"] = json!("cache://invalid"),
            7 => bindings["assets"] = json!({}),
            _ => unreachable!(),
        });
        let out = f.root.path().join("invalid-variants");
        assert!(!f.run(&out).status.success(), "case {case}");
        assert!(!out.exists());
    }
}

#[test]
fn language_sources_filenames_and_budgets_are_explicit_and_empty_extension_is_compatible() {
    for case in 0..7 {
        let mut f = fixture(|_| {});
        match case {
            0 => f.profile["language_bindings_assets"] = json!({"fr":"source/english-bindings"}),
            1 => f.profile["language_bindings_assets"] = json!({"en":"missing-package-role"}),
            2 => f.profile["language_filenames"] = json!({"en":{"cel-a":"CEL-A.tig"}}),
            3 => f.profile["language_filenames"] = json!({"en":{"shared-cue":"../bad.tig"}}),
            4 => f.profile["language_filenames"] = json!({"es":{"cel-a":"unused.tig"}}),
            5 => f.profile["language_filenames"] = json!({"en":{"typo":"unused.tig"}}),
            6 => f.profile["max_output_bytes"] = json!(1),
            _ => unreachable!(),
        }
        let out = f.root.path().join("invalid-settings");
        assert!(!f.run(&out).status.success(), "case {case}");
        assert!(!out.exists());
    }
    let mut f = Fixture::standard();
    assert!(f.run(&f.root.path().join("legacy")).status.success());
    f.profile["language_bindings_assets"] = json!({});
    f.profile["language_filenames"] = json!({});
    assert!(
        f.run(&f.root.path().join("empty-extension"))
            .status
            .success()
    );
    for name in ["scene.tig", "inputs/cel-a.tig", "inputs/shared-cue.tig"] {
        assert_eq!(
            fs::read(f.root.path().join("legacy").join(name)).unwrap(),
            fs::read(f.root.path().join("empty-extension").join(name)).unwrap()
        );
    }
}

fn inherited_fixture() -> Fixture {
    let mut f = fixture(|_| {});
    let mut package = object_json(&f, f.profile["package"]["sha256"].as_str().unwrap());
    for (lang, hash) in [("es", "d"), ("en", "e")] {
        let binding = json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"episode:01","assets":{
            "score":{"logical_id":format!("score:{lang}"),"sha256":hash.repeat(64),"bytes":300,
                "cache_uri":format!("cache://sha256/{}",hash.repeat(64)),"selection_state":"selected-private-production"},
            "cel-a":{"logical_id":"shadowed:ancestor","sha256":"f".repeat(64),"bytes":400,
                "cache_uri":format!("cache://sha256/{}","f".repeat(64)),"selection_state":"private-unselected-candidate"}}});
        package["assets"][format!("source/episode-{lang}")] =
            descriptor(&put(f.root.path(), &serde_json::to_vec(&binding).unwrap()));
    }
    f.profile["package"] = json!(put(f.root.path(), &serde_json::to_vec(&package).unwrap()));
    f.profile["binding_ancestors"] = json!([{"asset":"source/episode-es","scope_id":"episode:01"}]);
    f.profile["language_binding_ancestors"] =
        json!({"en":[{"asset":"source/episode-en","scope_id":"episode:01"}]});
    f
}

#[test]
fn explicit_inherited_scopes_preserve_language_scores_and_local_precedence() {
    let f = inherited_fixture();
    let profile = f.parsed();
    let mut cache = ReadCache::new(profile.byte_source.clone()).unwrap();
    let prepared = tig_native_registration::prepare(&profile, &mut cache).unwrap();
    assert_eq!(cache.requests(), 6);
    let out = f.root.path().join("inherited");
    assert_eq!(prepared.write(&out).unwrap()["records"], 6);
    let parent = Instance::parse(&fs::read(out.join("scene.tig")).unwrap()).unwrap();
    assert_eq!(
        parent.value().authored["binding_ids"]["score"]["id"],
        "score:es"
    );
    assert_eq!(
        parent.value().authored["binding_ids"]["cel-a"]["id"],
        "cel:A"
    );
    assert_eq!(
        parent.value().authored["language_binding_ids"]["en"]["score"]["id"],
        "score:en"
    );
    assert_eq!(
        parent.value().metadata["native_registration"]["binding_ancestors"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        parent.value().metadata["native_registration"]["language_binding_ancestors"]["en"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let score = Instance::parse(&fs::read(out.join("inputs/score.en.tig")).unwrap()).unwrap();
    assert_eq!(
        score.value().metadata["native_registration"]["binding_ancestors"][0]["scope_id"],
        "episode:01"
    );
    assert!(!out.join("inputs/shadowed.tig").exists());
}

#[test]
fn inherited_scope_identity_role_count_and_language_mismatches_refuse() {
    for case in 0..5 {
        let mut f = inherited_fixture();
        match case {
            0 => f.profile["binding_ancestors"][0]["scope_id"] = json!("wrong-episode"),
            1 => f.profile["binding_ancestors"][0]["asset"] = json!("missing-role"),
            2 => {
                f.profile["binding_ancestors"] = json!(
                    (0..9)
                        .map(|_| json!({"asset":"source/episode-es","scope_id":"episode:01"}))
                        .collect::<Vec<_>>()
                )
            }
            3 => f.profile["language_binding_ancestors"] = json!({"es":[]}),
            4 => {
                f.profile["language_binding_ancestors"]["en"][0]["scope_id"] =
                    json!("wrong-episode")
            }
            _ => unreachable!(),
        }
        let out = f.root.path().join("bad-ancestor");
        assert!(!f.run(&out).status.success(), "case {case}");
        assert!(!out.exists());
    }
}
