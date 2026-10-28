use super::*;

fn read(root: &Path, name: &str) -> Value {
    serde_json::from_slice(&fs::read(root.join(name)).unwrap()).unwrap()
}
#[test]
fn canonical_ids_preserve_source_identity_bytes_status_and_unrelated_records() {
    let mut f = variants::fixture(|_| {});
    let before = f.root.path().join("before-mapping");
    assert!(f.run(&before).status.success());
    f.profile["primary_ids"] = json!({"cel-a":"project/scene/spanish-picture"});
    f.profile["language_primary_ids"] = json!({"en":{"cel-a":"project/scene/english-picture"}});
    let after = f.root.path().join("mapped");
    assert!(f.run(&after).status.success());
    for (name, original, canonical) in [
        ("inputs/cel-a.tig", "cel:A", "project/scene/spanish-picture"),
        (
            "inputs/cel-a.en.tig",
            "cel:English",
            "project/scene/english-picture",
        ),
    ] {
        let a = read(&before, name);
        let b = read(&after, name);
        assert_eq!(b["id"], canonical);
        assert_eq!(
            b["metadata"]["native_registration"]["source_logical_id"],
            original
        );
        for field in ["selected", "status", "authored", "definition"] {
            assert_eq!(a[field], b[field]);
        }
    }
    let parent = read(&after, "scene.tig");
    assert_eq!(
        parent["authored"]["binding_ids"]["cel-a"]["id"],
        "project/scene/spanish-picture"
    );
    assert_eq!(
        parent["authored"]["language_binding_ids"]["en"]["cel-a"]["id"],
        "project/scene/english-picture"
    );
    assert_eq!(
        parent["metadata"]["native_registration"]["canonical_ids"],
        f.profile["primary_ids"]
    );
    assert_eq!(parent["selected"], read(&before, "scene.tig")["selected"]);
    assert_eq!(
        fs::read(before.join("inputs/shared-cue.tig")).unwrap(),
        fs::read(after.join("inputs/shared-cue.tig")).unwrap()
    );
}

#[test]
fn inherited_shared_identity_mapping_applies_only_to_the_same_original_asset() {
    let mut f = variants::fixture(|_| {});
    f.profile["primary_ids"] =
        json!({"shared-cue":"project/shared-cue","cel-a":"project/base-picture"});
    let out = f.root.path().join("shared-mapping");
    assert!(f.run(&out).status.success());
    assert_eq!(fs::read_dir(out.join("inputs")).unwrap().count(), 3);
    let parent = read(&out, "scene.tig");
    assert!(
        parent["authored"]["language_binding_ids"]["en"]
            .get("shared-cue")
            .is_none()
    );
    assert_eq!(
        parent["authored"]["language_binding_ids"]["en"]["cel-a"]["id"],
        "cel:English"
    );
    assert_eq!(
        parent["authored"]["binding_ids"]["shared-cue"]["id"],
        "project/shared-cue"
    );
    let mut f = variants::fixture(|_| {});
    let a = f.root.path().join("default-ids");
    assert!(f.run(&a).status.success());
    f.profile["primary_ids"] = json!({});
    f.profile["language_primary_ids"] = json!({});
    let b = f.root.path().join("empty-ids");
    assert!(f.run(&b).status.success());
    for name in [
        "scene.tig",
        "inputs/cel-a.tig",
        "inputs/shared-cue.tig",
        "inputs/cel-a.en.tig",
    ] {
        assert_eq!(
            fs::read(a.join(name)).unwrap(),
            fs::read(b.join(name)).unwrap()
        );
    }
}

#[test]
fn unknown_invalid_and_conflicting_identity_maps_cannot_publish_candidates() {
    for case in 0..10 {
        let mut f = variants::fixture(|_| {});
        match case {
            0 => f.profile["primary_ids"] = json!({"cel-a":""}),
            1 => f.profile["primary_ids"] = json!({"cel-a":"bad\nID"}),
            2 => f.profile["primary_ids"] = json!({"cel-a":"a".repeat(257)}),
            3 => f.profile["primary_ids"] = json!({"typo":"project/id"}),
            4 => f.profile["primary_ids"] = json!({"cel-a":"scene:01"}),
            5 => f.profile["primary_ids"] = json!({"cel-a":"same","shared-cue":"same"}),
            6 => f.profile["language_primary_ids"] = json!({"es":{"cel-a":"project/id"}}),
            7 => f.profile["language_primary_ids"] = json!({"en":{"typo":"project/id"}}),
            8 => f.profile["language_primary_ids"] = json!({"en":{"cel-a":"cue:shared"}}),
            9 => f.profile["primary_ids"] = json!([]),
            _ => unreachable!(),
        }
        let out = f.root.path().join("bad-map");
        assert!(!f.run(&out).status.success(), "case {case}");
        assert!(!out.exists());
    }
}

#[test]
fn two_source_aliases_can_explicitly_keep_one_canonical_scoped_asset() {
    let asset = json!({"logical_id":"scope-local","sha256":"a".repeat(64),"bytes":1,
        "cache_uri":format!("cache://sha256/{}","a".repeat(64)),"selection_state":"draft"});
    let mut f = Fixture::new(
        json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"scene:01","assets":{"one":asset,"two":asset}}),
    );
    f.profile["primary_ids"] = json!({"one":"project/scene/hold","two":"project/scene/hold"});
    let out = f.root.path().join("canonical-shared");
    assert!(f.run(&out).status.success());
    assert_eq!(fs::read_dir(out.join("inputs")).unwrap().count(), 1);
    let parent = read(&out, "scene.tig");
    assert_eq!(
        parent["authored"]["binding_ids"]["one"],
        parent["authored"]["binding_ids"]["two"]
    );
    assert_eq!(
        parent["authored"]["binding_ids"]["one"]["id"],
        "project/scene/hold"
    );
    assert_eq!(
        read(&out, "inputs/one.tig")["metadata"]["native_registration"]["source_logical_id"],
        "scope-local"
    );
}
