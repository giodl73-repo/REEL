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
