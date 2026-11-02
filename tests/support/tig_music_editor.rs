use super::*;
use reel::tig_music_editor::Editor;

fn underlay_fixture() -> Fixture {
    let mut f = fixture();
    let lyric = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("manifests/fixtures/music-model-corrected/lyrics.txt"),
    )
    .unwrap();
    f.add("music/lyric", &lyric);
    let schema = Object::from_bytes(&fs::read(f.root().join("definitions/authored.json")).unwrap());
    let text_definition = write(
        f.root(),
        "definitions/lyric.json",
        &json!({
        "schema":"tig.definition.design-1","id":"types/lyric","authored_schema":schema,
        "authoring":{"properties":["language","lyric_kind"],"required":[],"defaults":{}},
        "stream_rules":{},"semantic_rules":{"profile":"reel.music-lyric-source.v1"}}),
    );
    let mut lyric_record: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/music-lyric.tig")).unwrap())
            .unwrap();
    lyric_record["definition"] =
        json!({"id":"types/lyric","sha256":text_definition.sha256,"bytes":text_definition.bytes});
    lyric_record["authored"] = json!({"language":"und","lyric_kind":"canonical"});
    write(f.root(), "records/music-lyric.tig", &lyric_record);
    f.profile["admitted"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"types/lyric","object":text_definition,"payload":{"kind":"selected"}}));
    let mut definition: Value =
        serde_json::from_slice(&fs::read(f.root().join("definitions/music-model.json")).unwrap())
            .unwrap();
    definition["semantic_rules"]["binding_profile"] = json!("reel.music-model-input-bindings.v1");
    definition["semantic_rules"]["underlay_profile"] = json!("reel.music-underlay-authoring.v1");
    definition["authoring"]["properties"] = json!(["binding_ids"]);
    let pin = write(f.root(), "definitions/music-model.json", &definition);
    let mut record: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/music-model.tig")).unwrap())
            .unwrap();
    record["definition"] = json!({"id":"types/music-model","sha256":pin.sha256,"bytes":pin.bytes});
    record["authored"] = json!({"binding_ids":{"lyric:canonical-synthetic":{"id":"music/lyric"}}});
    write(f.root(), "records/music-model.tig", &record);
    for admitted in f.profile["admitted"].as_array_mut().unwrap() {
        if admitted["id"] == "types/music-model" {
            admitted["object"] = json!(pin);
        }
    }
    git(f.root(), &["add", "definitions"]);
    f.commit();
    f
}

#[test]
fn tig_underlay_pairs_exact_text_with_model_and_preserves_all_source_subjects() {
    let mut f = underlay_fixture();
    let model_before = fs::read(f.root().join("records/music-model.tig")).unwrap();
    let lyric_before = fs::read(f.root().join("records/music-lyric.tig")).unwrap();
    let session = f.root().join("underlay-session");
    assert_eq!(
        editor(&f).export_underlay("music/model", &session).unwrap(),
        4
    );
    let value: Value =
        serde_json::from_slice(&fs::read(session.join("session.json")).unwrap()).unwrap();
    let mut request = value["edit_request"].clone();
    request["underlay"][0]["note_ids"] = json!(["note-1", "note-2"]);
    request["underlay"][0]["text_end_byte"] = json!(5);
    request["underlay"].as_array_mut().unwrap().remove(1);
    request["rationale"] = json!("Source phrase across two retained notes");
    let edit = f.root().join("underlay-edit.json");
    fs::write(&edit, serde_json::to_vec(&request).unwrap()).unwrap();
    f.add("music/unrelated", b"unrelated branch advance");
    f.commit();
    let output = f.root().join("underlay-proposal");
    assert_eq!(
        editor(&f)
            .propose_underlay(&session.join("source-lock.json"), &edit, &output)
            .unwrap(),
        3
    );
    let candidate: Value =
        serde_json::from_slice(&fs::read(output.join("record.tig")).unwrap()).unwrap();
    let before: Value = serde_json::from_slice(&model_before).unwrap();
    for field in ["id", "definition", "authored", "streams"] {
        assert_eq!(candidate[field], before[field]);
    }
    assert_eq!(candidate["status"]["human_approval"], false);
    assert_eq!(
        candidate["metadata"]["reel_music_edit"]["verified_lyric"]["record"]["id"],
        "music/lyric"
    );
    let after: reel_music::model::MusicModel =
        serde_json::from_slice(&fs::read(output.join("model.json")).unwrap()).unwrap();
    assert_eq!(after.review.status, "not-reviewed");
    assert!(
        after
            .review
            .required_roles
            .iter()
            .any(|r| r == "lyrics-vocal-adaptation-editor")
    );
    assert_eq!(
        fs::read(f.root().join("records/music-model.tig")).unwrap(),
        model_before
    );
    assert_eq!(
        fs::read(f.root().join("records/music-lyric.tig")).unwrap(),
        lyric_before
    );
    // A text record annotation change is stale even if bytes remain identical.
    let mut lyric: Value = serde_json::from_slice(&lyric_before).unwrap();
    lyric["metadata"]["annotation"] = json!("new branch decision");
    write(f.root(), "records/music-lyric.tig", &lyric);
    f.commit();
    let refused = f.root().join("stale-underlay");
    assert!(
        editor(&f)
            .propose_underlay(&session.join("source-lock.json"), &edit, &refused)
            .is_err()
    );
    assert!(!refused.exists());
}

#[test]
fn tig_underlay_refuses_missing_bindings_stale_text_and_wrong_language_before_output() {
    for mutation in ["binding", "bytes", "language", "kind", "definition"] {
        let mut f = underlay_fixture();
        let path = f.root().join(if mutation == "binding" {
            "records/music-model.tig"
        } else {
            "records/music-lyric.tig"
        });
        let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        match mutation {
            "binding" => record["authored"]["binding_ids"] = json!({}),
            "bytes" => {
                let object = Object::from_bytes(b"changed text");
                fs::write(
                    f.root().join("cache/objects").join(object.sha256.as_str()),
                    b"changed text",
                )
                .unwrap();
                record["selected"] = json!(object);
            }
            "language" => record["authored"]["language"] = json!("other"),
            "kind" => record["authored"]["lyric_kind"] = json!("as-sung"),
            "definition" => record["definition"]["sha256"] = json!("0".repeat(64)),
            _ => unreachable!(),
        }
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        f.commit();
        let output = f.root().join("refused-underlay");
        let result = Editor::open(serde_json::from_value(f.profile.clone()).unwrap())
            .and_then(|mut editor| editor.export_underlay("music/model", &output));
        assert!(result.is_err(), "accepted {mutation}");
        assert!(!output.exists());
    }
}

#[test]
fn tig_underlay_cli_exports_only_counts_and_never_lyric_text() {
    let f = underlay_fixture();
    let profile = f.root().join("profile.json");
    fs::write(&profile, serde_json::to_vec(&f.profile).unwrap()).unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_reel-tig-music-editor"))
        .arg("--profile")
        .arg(profile)
        .arg("export-underlay")
        .arg("music/model")
        .arg(f.root().join("cli-underlay"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let summary: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary["underlay_spans"], 4);
    assert_eq!(summary["media_resolved"], false);
    assert_eq!(summary["human_approval"], false);
    assert_eq!(summary.as_object().unwrap().len(), 7);
}

fn fixture() -> Fixture {
    fixture_with_permission(true)
}
fn fixture_with_permission(editable: bool) -> Fixture {
    let mut f = Fixture::new();
    f.profile["byte_source"]["required"] = serde_json::to_value(if cfg!(windows) {
        tig_store::existing::RequiredProtection::PreventMutation
    } else {
        tig_store::existing::RequiredProtection::CooperativeImmutable
    })
    .unwrap();
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
