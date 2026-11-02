use super::*;
use reel::tig_music_editor::Editor;

fn fixture() -> Fixture {
    fixture_with_permission(true)
}
fn fixture_with_permission(editable: bool) -> Fixture {
    let mut f = Fixture::new();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    let raw = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("manifests/fixtures/music-model-corrected/model.yaml"),
    )
    .unwrap();
    f.add("music/model", &raw);
    let path = f.root().join("records/music-model.tig");
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let schema = Object::from_bytes(&fs::read(f.root().join("definitions/authored.json")).unwrap());
    let definition = write(
        f.root(),
        "definitions/music-model.json",
        &json!({
            "schema":"tig.definition.design-1","id":"types/music-model","authored_schema":schema,
            "authoring":{"properties":[],"required":[],"defaults":{}},"stream_rules":{},
            "semantic_rules":{"profile":if editable {reel::tig_music_editor::AUTHORING_PROFILE} else {"retained-package-member"}}
        }),
    );
    record["definition"] =
        json!({"id":"types/music-model","sha256":definition.sha256,"bytes":definition.bytes});
    f.profile["admitted"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"types/music-model",
        "object":definition,"payload":{"kind":"selected"}}));
    record["status"] = json!({"review":"approved-for-old-subject"});
    record["metadata"] = json!({"project_note":"preserve", "rights":"private"});
    record["streams"] = json!([{"slot":"retained","role":"history/model","group":"history",
        "object":record["selected"],"provenance":{"review":"old exact subject"}}]);
    write(f.root(), "records/music-model.tig", &record);
    git(f.root(), &["add", "definitions"]);
    f.commit();
    f
}
fn editor(f: &Fixture) -> Editor {
    Editor::open(serde_json::from_value(f.profile.clone()).unwrap()).unwrap()
}
fn edit(f: &Fixture, session: &Path) -> PathBuf {
    let value: Value =
        serde_json::from_slice(&fs::read(session.join("session.json")).unwrap()).unwrap();
    let mut request = value["edit_request"].clone();
    request["rows"][0]["midi_note"] = json!(61);
    request["rows"][0]["rationale"] = json!("Private synthetic pitch proposal");
    let path = f.root().join("edit.json");
    fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
    path
}

#[test]
fn tig_music_editor_uses_locked_assets_and_preserves_history_without_mutating_cache() {
    let mut f = fixture();
    let original = fs::read(f.root().join("records/music-model.tig")).unwrap();
    let before: Value = serde_json::from_slice(&original).unwrap();
    // Unclassified authoring is allowed; the production resolver still refuses
    // this record's missing project selection state.
    let source = Source::open(serde_json::from_value(f.profile.clone()).unwrap()).unwrap();
    assert!(source.resolve_asset("music/model").is_err());
    let session = f.root().join("exports");
    assert_eq!(editor(&f).export("music/model", &session).unwrap(), 4);
    let request = edit(&f, &session);
    let lock = session.join("source-lock.json");
    // An unrelated asset advance does not make this model edit stale.
    f.add("music/unrelated", b"unconsumed model alternative");
    f.commit();
    let output = f.root().join("proposal");
    assert_eq!(editor(&f).propose(&lock, &request, &output).unwrap(), 1);
    let candidate: Value =
        serde_json::from_slice(&fs::read(output.join("record.tig")).unwrap()).unwrap();
    let raw = fs::read(output.join("model.json")).unwrap();
    assert_eq!(
        candidate["selected"]["sha256"],
        json!(Object::from_bytes(&raw).sha256)
    );
    assert_eq!(candidate["id"], before["id"]);
    assert_eq!(candidate["definition"], before["definition"]);
    assert_eq!(candidate["authored"], before["authored"]);
    assert_eq!(candidate["streams"], before["streams"]);
    assert_eq!(candidate["metadata"]["project_note"], "preserve");
    assert_eq!(candidate["status"]["selection_state"], "draft-unreviewed");
    assert_eq!(
        candidate["metadata"]["reel_music_edit"]["predecessor_status"],
        before["status"]
    );
    let model: reel_music::model::MusicModel = serde_json::from_slice(&raw).unwrap();
    assert_eq!(model.parts[0].notes[0].midi_note, 61);
    assert_eq!(model.review.status, "not-reviewed");
    assert!(model.review.decision_refs.is_empty());
    assert_eq!(
        fs::read(f.root().join("records/music-model.tig")).unwrap(),
        original
    );
    let old = before["selected"]["sha256"].as_str().unwrap();
    assert_eq!(
        Object::from_bytes(&fs::read(f.root().join("cache/objects").join(old)).unwrap())
            .sha256
            .as_str(),
        old
    );
    assert!(editor(&f).propose(&lock, &request, &output).is_err());

    // Even an annotation change on the edited record invalidates its exact lock.
    let mut changed = before;
    changed["metadata"]["project_note"] = json!("changed revision");
    write(f.root(), "records/music-model.tig", &changed);
    f.commit();
    let stale = f.root().join("stale");
    assert!(editor(&f).propose(&lock, &request, &stale).is_err());
    assert!(!stale.exists());
}

#[test]
fn tig_music_editor_refuses_cache_git_and_invalid_edits_before_output() {
    let f = fixture();
    let mut tool = editor(&f);
    for output in [f.root().join("cache/editor"), f.root().join(".git/editor")] {
        assert!(tool.export("music/model", &output).is_err());
        assert!(!output.exists());
    }
    let session = f.root().join("exports");
    tool.export("music/model", &session).unwrap();
    let request = edit(&f, &session);
    let mut bad: Value = serde_json::from_slice(&fs::read(&request).unwrap()).unwrap();
    bad["expected_model_sha256"] = json!("0".repeat(64));
    fs::write(&request, serde_json::to_vec(&bad).unwrap()).unwrap();
    let output = f.root().join("invalid");
    assert!(
        tool.propose(&session.join("source-lock.json"), &request, &output)
            .is_err()
    );
    assert!(!output.exists());
}

#[test]
fn tig_music_editor_does_not_promote_a_retained_package_member_to_editable() {
    let f = fixture_with_permission(false);
    let mut tool = editor(&f);
    let session = f.root().join("exports");
    assert_eq!(tool.export("music/model", &session).unwrap(), 4);
    let request = edit(&f, &session);
    let output = f.root().join("unauthorized-proposal");
    assert!(
        tool.propose(&session.join("source-lock.json"), &request, &output)
            .unwrap_err()
            .to_string()
            .contains("does not admit native note authoring")
    );
    assert!(!output.exists());
}

#[test]
fn tig_music_editor_cli_keeps_identity_out_of_summary() {
    let f = fixture();
    let profile = f.root().join("profile.json");
    fs::write(&profile, serde_json::to_vec(&f.profile).unwrap()).unwrap();
    let binary = env!("CARGO_BIN_EXE_reel-tig-music-editor");
    let session = f.root().join("exports");
    let result = Command::new(binary)
        .arg("--profile")
        .arg(&profile)
        .arg("export")
        .arg("music/model")
        .arg(&session)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap(),
        json!({"operation":"export","notes":4,"media_resolved":false,"native_validation":"not-run",
            "published":false,"git_changed":false,"human_approval":false})
    );
    let request = edit(&f, &session);
    let result = Command::new(binary)
        .arg("--profile")
        .arg(&profile)
        .arg("propose")
        .arg(session.join("source-lock.json"))
        .arg(request)
        .arg(f.root().join("proposal"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let summary: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary["changed_notes"], 1);
    assert_eq!(summary["human_approval"], false);
    assert!(
        !String::from_utf8(result.stdout)
            .unwrap()
            .contains("music/model")
    );
}
