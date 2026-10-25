use reel_assets::cel_source::project::{FILES, Project};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("native");
    let config = temp.path().join("config");
    fs::create_dir_all(data.join("controls")).unwrap();
    fs::create_dir(&config).unwrap();
    let mut files = BTreeMap::new();
    for (index, role) in FILES.iter().take(6).enumerate() {
        let path = format!("controls/input-{index}.json");
        fs::write(
            data.join(&path),
            serde_json::to_vec(&json!({"fixture-role":role})).unwrap(),
        )
        .unwrap();
        files.insert(role.to_string(), path);
    }
    let profile = config.join("project.json");
    put(
        &profile,
        &json!({"schema":"reel.cel-sorter-project.v1","root":"../native","files":files}),
    );
    (temp, profile, data)
}
fn put(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn get(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn relative_root_and_six_explicit_controls_load_without_optional_context_or_media() {
    let (_temp, profile, _data) = fixture();
    let project = Project::open(&profile).unwrap();
    let snapshot = project.load().unwrap();
    assert_eq!(snapshot.controls.len(), 6);
    assert_eq!(snapshot.hashes.as_object().unwrap().len(), 6);
    assert_eq!(
        snapshot.controls["scene.json"]["fixture-role"],
        "scene.json"
    );
    assert!(snapshot.bytes.values().all(|bytes| *bytes > 0));
    assert!(project.accepts_edit(
        &json!({"schema":"reel.cel-sorter-edit.v2","expected_project":project.descriptor_sha256()})
    ));
    assert!(!project.accepts_edit(&json!({"schema":"reel.cel-sorter-edit.v1"})));
}
#[test]
fn unsupported_duplicate_missing_and_oversized_descriptors_reject() {
    let (_temp, profile, _data) = fixture();
    let original = get(&profile);
    for case in 0..4 {
        let mut value = original.clone();
        match case {
            0 => value["schema"] = json!("future"),
            1 => {
                value["files"].as_object_mut().unwrap().remove("scene.json");
            }
            2 => value["files"]["unknown-role"] = json!("controls/extra.json"),
            _ => value["unknown"] = json!(true),
        }
        put(&profile, &value);
        assert!(Project::open(&profile).is_err(), "case {case}");
    }
    let original = serde_json::to_string(&original).unwrap();
    fs::write(
        &profile,
        original.replace(
            "\"scene.json\":",
            "\"scene.json\":\"duplicate.json\",\"scene.json\":",
        ),
    )
    .unwrap();
    assert!(
        Project::open(&profile)
            .err()
            .unwrap()
            .to_string()
            .contains("duplicate")
    );
    fs::write(&profile, vec![b' '; 64 * 1024 + 1]).unwrap();
    assert!(Project::open(&profile).is_err());
}
#[test]
fn escaping_aliased_device_and_reserved_control_paths_reject() {
    let (_temp, profile, _data) = fixture();
    let original = get(&profile);
    for path in [
        "../outside.json",
        "/absolute.json",
        "a\\b.json",
        "C:/outside.json",
        "a/../b.json",
        "x//y.json",
        "nul.json",
        "COM1.txt",
        "one.",
        "edit-request.json",
        "cel-sorter-project.json",
        "CEL-SORTER-PROJECT.JSON/sub.json",
        "a:stream.json",
        "a?b.json",
        "a*b.json",
        "a\"b.json",
        "<bad>.json",
        "newline\n.json",
    ] {
        let mut value = original.clone();
        value["files"]["scene.json"] = json!(path);
        put(&profile, &value);
        assert!(Project::open(&profile).is_err(), "{path}");
    }
    let mut value = original.clone();
    value["files"]["scene.json"] = value["files"]["profile.json"].clone();
    put(&profile, &value);
    assert!(Project::open(&profile).is_err());
    value = original;
    value["files"]["scene.json"] = json!("CONTROLS/INPUT-5.JSON");
    put(&profile, &value);
    assert!(Project::open(&profile).is_err());
}
#[test]
fn descriptor_bytes_are_part_of_edit_identity_even_when_controls_match() {
    let (_temp, profile, _data) = fixture();
    let project = Project::open(&profile).unwrap();
    let before = project.load().unwrap();
    let request =
        json!({"schema":"reel.cel-sorter-edit.v2","expected_project":project.descriptor_sha256()});
    let mut bytes = fs::read(&profile).unwrap();
    bytes.push(b'\n');
    fs::write(&profile, bytes).unwrap();
    let changed = Project::open(&profile).unwrap();
    assert_eq!(changed.load().unwrap().hashes, before.hashes);
    assert!(!changed.accepts_edit(&request));
}
#[test]
fn new_draft_preserves_relative_layout_and_reopens_without_source_changes() {
    let (temp, profile, _data) = fixture();
    let project = Project::open(&profile).unwrap();
    let before = project.load().unwrap();
    let request =
        json!({"schema":"reel.cel-sorter-edit.v2","expected_project":project.descriptor_sha256()});
    let out = temp.path().join("candidate");
    project
        .write_draft(&out, &before.controls, &request)
        .unwrap();
    assert!(!out.join("scene.json").exists());
    assert!(out.join("controls/input-0.json").is_file());
    let candidate = Project::open(&out.join("cel-sorter-project.json")).unwrap();
    assert_eq!(candidate.load().unwrap().controls, before.controls);
    assert_eq!(project.load().unwrap().hashes, before.hashes);
    assert!(!candidate.accepts_edit(&request));
    assert!(
        project
            .write_draft(&out, &before.controls, &request)
            .is_err()
    );
}
#[test]
fn missing_or_oversized_input_and_unreadable_draft_budget_hold() {
    let (temp, profile, data) = fixture();
    let project = Project::open(&profile).unwrap();
    let mut snapshot = project.load().unwrap();
    snapshot.controls.insert(
        "scene.json".into(),
        json!({"oversized":"x".repeat(1024*1024)}),
    );
    let out = temp.path().join("candidate");
    assert!(
        project
            .write_draft(&out, &snapshot.controls, &json!({}))
            .is_err()
    );
    assert!(!out.exists());
    fs::write(
        data.join("controls/input-0.json"),
        vec![b' '; 2 * 1024 * 1024 + 1],
    )
    .unwrap();
    assert!(project.load().is_err());
    fs::remove_file(data.join("controls/input-0.json")).unwrap();
    assert!(project.load().is_err());
}
#[cfg(unix)]
#[test]
fn symlink_control_does_not_follow_an_outside_file() {
    let (temp, profile, data) = fixture();
    let path = data.join("controls/input-0.json");
    fs::remove_file(&path).unwrap();
    let outside = temp.path().join("outside.json");
    fs::write(&outside, b"not an input").unwrap();
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    assert!(Project::open(&profile).unwrap().load().is_err());
}

fn multi_paths_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let (temp, profile, data) = fixture();
    let mut config = get(&profile);
    config["schema"] = json!("reel.cel-sorter-project.v2");
    config["files"]
        .as_object_mut()
        .unwrap()
        .remove("alignment.json");
    config["files"]["alignment-paths.json"] = json!("timing/paths.json");
    fs::create_dir(data.join("timing")).unwrap();
    put(
        &data.join("timing/paths.json"),
        &json!({"a":"one.json","b":"two.json"}),
    );
    put(&data.join("timing/one.json"), &json!({"cue":"a"}));
    put(&data.join("timing/two.json"), &json!({"cue":"b"}));
    put(&profile, &config);
    (temp, profile, data)
}
#[test]
fn native_paths_are_manifest_relative_locked_once_and_drafts_reopen() {
    let (temp, profile, data) = multi_paths_fixture();
    let project = Project::open(&profile).unwrap();
    let original = fs::read(data.join("timing/paths.json")).unwrap();
    put(
        &data.join("timing/paths.json"),
        &json!({"new":"absent.json"}),
    );
    let snapshot = project.load().unwrap();
    assert_eq!(snapshot.controls["alignment:a"]["cue"], "a");
    assert_eq!(snapshot.hashes["alignment-paths.json"], {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(&original))
    });
    assert_eq!(project.session_schema(), "reel.cel-sorter-session.v3");
    let request =
        json!({"schema":"reel.cel-sorter-edit.v3","expected_project":project.descriptor_sha256()});
    assert!(project.accepts_edit(&request));
    assert!(!project.accepts_edit(
        &json!({"schema":"reel.cel-sorter-edit.v2","expected_project":project.descriptor_sha256()})
    ));
    let out = temp.path().join("candidate");
    project
        .write_draft(&out, &snapshot.controls, &request)
        .unwrap();
    let next = Project::open(&out.join("cel-sorter-project.json")).unwrap();
    assert_eq!(next.load().unwrap().hashes, snapshot.hashes);
    // Reopening observes the new index. The captured map is no namespace lease.
    assert!(Project::open(&profile).unwrap().load().is_err());
}
#[test]
fn native_paths_refuse_duplicates_empty_excessive_and_aliased_or_escaping_routes() {
    let (_temp, profile, data) = multi_paths_fixture();
    let paths = data.join("timing/paths.json");
    for value in [
        json!({}),
        json!({" ":"one.json"}),
        json!({"a":"../outside.json"}),
        json!({"a":"/absolute.json"}),
        json!({"a":"one.json","b":"ONE.JSON"}),
        json!({"a":"../controls/input-0.json"}),
        json!({"a":"paths.json"}),
        json!({"a":"C:/file.json"}),
        json!({"a":"x\\y.json"}),
    ] {
        put(&paths, &value);
        assert!(Project::open(&profile).is_err(), "{value}");
    }
    fs::write(&paths, br#"{"a":"one.json","a":"two.json"}"#).unwrap();
    assert!(Project::open(&profile).is_err());
    let many: BTreeMap<_, _> = (0..129)
        .map(|i| (format!("cue-{i}"), format!("cue-{i}.json")))
        .collect();
    put(&paths, &json!(many));
    assert!(Project::open(&profile).is_err());
    fs::write(&paths, vec![b' '; 128 * 1024 + 1]).unwrap();
    assert!(Project::open(&profile).is_err());
}
#[test]
fn multi_schema_rejects_legacy_alignment_and_mismatched_control_sets() {
    let (_temp, profile, _data) = multi_paths_fixture();
    let mut config = get(&profile);
    config["files"]["alignment.json"] = json!("legacy.json");
    put(&profile, &config);
    assert!(Project::open(&profile).is_err());
    config["files"]
        .as_object_mut()
        .unwrap()
        .remove("alignment.json");
    config["files"]
        .as_object_mut()
        .unwrap()
        .remove("alignment-paths.json");
    put(&profile, &config);
    assert!(Project::open(&profile).is_err());
}

#[test]
fn unchanged_controls_preserve_exact_snapshot_bytes_and_tampered_raw_buffers_refuse() {
    let (temp, profile, data) = fixture();
    let config = get(&profile);
    let path = data.join(config["files"]["profile.json"].as_str().unwrap());
    fs::write(&path, b"{\n  \"fixture-role\": \"profile.json\"\n}\n").unwrap();
    let project = Project::open(&profile).unwrap();
    let mut snapshot = project.load().unwrap();
    let output = temp.path().join("raw-draft");
    project
        .write_draft_from_snapshot(&output, &snapshot.controls, &json!({}), &snapshot)
        .unwrap();
    let next = Project::open(&output.join("cel-sorter-project.json")).unwrap();
    assert_eq!(next.load().unwrap().hashes, snapshot.hashes);
    assert_eq!(
        fs::read(output.join(config["files"]["profile.json"].as_str().unwrap())).unwrap(),
        fs::read(path).unwrap()
    );
    snapshot.raw.get_mut("profile.json").unwrap().push(b' ');
    let rejected = temp.path().join("tampered");
    assert!(
        project
            .write_draft_from_snapshot(&rejected, &snapshot.controls, &json!({}), &snapshot)
            .is_err()
    );
    assert!(!rejected.exists());
}
#[test]
fn scoped_indexes_keep_identical_local_cue_ids_in_distinct_language_roles() {
    use reel_assets::cel_source::project::scoped_alignment_role;
    let (_temp, profile, data) = fixture();
    let mut config = get(&profile);
    config["schema"] = json!("reel.cel-sorter-project.v3");
    config["language"] = json!("es");
    config["files"]
        .as_object_mut()
        .unwrap()
        .remove("alignment.json");
    config["alignment_paths"] = json!({"es":"es-paths.json","en":"en-paths.json"});
    for language in ["es", "en"] {
        put(
            &data.join(format!("{language}-paths.json")),
            &json!({"same":format!("{language}-clock.json")}),
        );
        put(
            &data.join(format!("{language}-clock.json")),
            &json!({"clock":language}),
        );
    }
    put(&profile, &config);
    let project = Project::open(&profile).unwrap();
    let snapshot = project.load().unwrap();
    assert_eq!(
        snapshot.controls[&scoped_alignment_role("es", "same")]["clock"],
        "es"
    );
    assert_eq!(
        snapshot.controls[&scoped_alignment_role("en", "same")]["clock"],
        "en"
    );
    assert_ne!(
        scoped_alignment_role("a:b", "c"),
        scoped_alignment_role("a", "b:c")
    );
}
#[test]
fn scoped_index_route_aliases_escaping_and_duplicate_language_keys_refuse() {
    let (_temp, profile, data) = fixture();
    let mut config = get(&profile);
    config["schema"] = json!("reel.cel-sorter-project.v3");
    config["language"] = json!("es");
    config["files"]
        .as_object_mut()
        .unwrap()
        .remove("alignment.json");
    config["alignment_paths"] = json!({"es":"paths.json","en":"other.json"});
    put(&data.join("paths.json"), &json!({"cue":"clock.json"}));
    put(&data.join("other.json"), &json!({"cue":"another.json"}));
    for route in [
        "paths.json",
        "PATHS.JSON",
        "../outside.json",
        "C:/outside.json",
    ] {
        let mut value = config.clone();
        value["alignment_paths"]["en"] = json!(route);
        put(&profile, &value);
        assert!(Project::open(&profile).is_err());
    }
    let raw = serde_json::to_string(&config).unwrap().replace(
        "\"es\":\"paths.json\"",
        "\"es\":\"paths.json\",\"es\":\"other.json\"",
    );
    fs::write(&profile, raw).unwrap();
    assert!(
        Project::open(&profile)
            .err()
            .unwrap()
            .to_string()
            .contains("duplicate")
    );
}
