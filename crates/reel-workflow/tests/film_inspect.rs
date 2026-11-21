use reel_workflow::{film_inspect::*, *};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

struct Fixture {
    root: tempfile::TempDir,
    request: Request,
    documents: BTreeMap<String, Vec<u8>>,
}
fn git(root: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=Synthetic Test",
            "-c",
            "user.email=synthetic@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=disabled-hooks",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(result.status.success(), "{:?}", result);
    String::from_utf8(result.stdout).unwrap().trim().into()
}
fn control(documents: &mut BTreeMap<String, Vec<u8>>, path: &str, value: Value) -> Control {
    put(
        documents,
        path,
        serde_json::to_vec(&value).unwrap(),
        "application/json",
    )
}
fn put(
    documents: &mut BTreeMap<String, Vec<u8>>,
    path: &str,
    bytes: Vec<u8>,
    media: &str,
) -> Control {
    let a = Artifact {
        logical_id: format!("controls/{path}"),
        sha256: digest(&bytes),
        bytes: bytes.len() as u64,
        media_type: media.into(),
    };
    documents.insert(path.into(), bytes);
    Control {
        path: path.into(),
        artifact: a,
    }
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init"]);
        let mut documents = BTreeMap::new();
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/scene-authoring");
        let mut originals = BTreeMap::new();
        for name in [
            "episode",
            "scene",
            "season-bindings",
            "episode-bindings",
            "scene-bindings",
            "catalog",
            "policy",
        ] {
            originals.insert(
                name,
                serde_json::from_slice::<Value>(
                    &fs::read(fixtures.join(format!("{name}.json"))).unwrap(),
                )
                .unwrap(),
            );
        }
        let authored = originals.get_mut("scene").unwrap();
        authored["schema"] = json!("reel.scene-authoring.v2");
        authored["canonical_cue_ids"] = json!(["source-block-1"]);
        authored["shared_events"] = json!([{"semantic_id":"first-line", "canonical_cue_id":"source-block-1",
            "picture_binding":"picture.first-line", "score":{"disposition":"role","role":"poem.intimate"}},
            {"semantic_id":"second-line", "canonical_cue_id":"source-block-1",
            "picture_binding":"picture.first-line", "score":{"disposition":"role","role":"poem.intimate"}}]);
        let mut cues = BTreeMap::new();
        let mut identities = BTreeMap::new();
        let mut alignments = BTreeMap::new();
        for (lang, text, rate, start, end, sha) in [
            ("es", "Hola olas", 8000, 100, 8000, "3"),
            ("en", "Hello waves", 16000, 700, 24000, "5"),
        ] {
            let cue_id = format!("{lang}-1");
            authored["languages"][lang]["cues"][0]["exact_text_sha256"] =
                json!(digest(text.as_bytes()));
            authored["languages"][lang]["events"] = json!([]);
            authored["language_event_bindings"][lang] = json!([{"semantic_id":"first-line", "event_id":format!("{lang}-event-1"),
                "cue_id":cue_id, "semantic_trigger_id":"first-line", "spoken_trigger_phrase":text.split_whitespace().next().unwrap(),
                "picture_slot_id":format!("{lang}-picture-slot")},
                {"semantic_id":"second-line", "event_id":format!("{lang}-event-2"),
                "cue_id":cue_id,"semantic_trigger_id":"second-line", "spoken_trigger_phrase":text.split_whitespace().nth(1).unwrap(),
                "picture_slot_id":format!("{lang}-picture-slot-2")}]);
            let text = put(
                &mut documents,
                &format!("{lang}.txt"),
                text.as_bytes().to_vec(),
                "text/plain",
            );
            let words = control(
                &mut documents,
                &format!("{lang}-words.json"),
                json!({"schema":"reel.scene-word-timing-evidence.v1",
                "language":lang, "cue_id":cue_id, "selected_take_sha256":sha.repeat(64), "sample_rate":rate, "cue_end_sample":end,
                "words":[{"word":if lang == "es" {"Hola"} else {"Hello"}, "start_sample":start,"end_sample":start+100},
                    {"word":if lang == "es" {"olas"} else {"waves"},"start_sample":start+300,"end_sample":start+500}]}),
            );
            let mut alignment = control(
                &mut documents,
                &format!("{lang}-alignment.json"),
                json!({
                    "schema":"reel.scene-native-alignment.v1", "language":lang,"cue_id":cue_id,
                    "selected_take_sha256":sha.repeat(64),"sample_rate":rate,"cue_end_sample":end,"semantic_markers":{"first-line":0,"second-line":start+300}
                }),
            );
            alignment.artifact.logical_id = format!("{lang}-alignment-1");
            alignments.insert(lang, alignment.artifact.clone());
            cues.insert(
                lang.into(),
                BTreeMap::from([(
                    cue_id,
                    CueEvidence {
                        text,
                        words,
                        selected_alignment: Some(alignment),
                    },
                )]),
            );
            identities.insert(
                lang.into(),
                BTreeMap::from([
                    (
                        format!("{lang}-event-1"),
                        CelIdentity {
                            story_id: "story-1".into(),
                            cel_id: "C100".into(),
                        },
                    ),
                    (
                        format!("{lang}-event-2"),
                        CelIdentity {
                            story_id: "story-1".into(),
                            cel_id: "C101".into(),
                        },
                    ),
                ]),
            );
        }
        for (language, a) in alignments {
            let binding = &mut originals.get_mut("scene-bindings").unwrap()["assets"]
                [format!("{language}.alignment")];
            binding["sha256"] = json!(a.sha256);
            binding["bytes"] = json!(a.bytes);
            binding["cache_uri"] = json!(format!("cache://sha256/{}", a.sha256));
        }
        let mut named = BTreeMap::new();
        for (name, value) in originals {
            named.insert(
                name,
                control(&mut documents, &format!("{name}.json"), value),
            );
        }
        let settings = control(
            &mut documents,
            "renderer-settings.json",
            json!({"format":"html-css-svg", "width":1920,"height":1080}),
        );
        let evidence = control(
            &mut documents,
            "approval.json",
            json!({"scope":"synthetic-fixture-only", "status":"existing-record"}),
        );
        let subject = Artifact {
            logical_id: "picture-1".into(),
            sha256: "7".repeat(64),
            bytes: 700,
            media_type: "image/svg+xml".into(),
        };
        let request = Request {
            schema: REQUEST_SCHEMA.into(),
            project: "synthetic-film".into(),
            source_commit: "a".repeat(40),
            episode: named["episode"].clone(),
            scene: named["scene"].clone(),
            scopes: [
                named["season-bindings"].clone(),
                named["episode-bindings"].clone(),
                named["scene-bindings"].clone(),
            ],
            catalog: named["catalog"].clone(),
            policy: named["policy"].clone(),
            cues,
            identities,
            direction: Direction {
                desired_seconds_per_cel: [1, 3],
                visual_school: "realistic".into(),
                performance_style: "theatrical".into(),
                animation_technique: "held-cels".into(),
                renderer: Artifact {
                    logical_id: "tools/renderer".into(),
                    sha256: "9".repeat(64),
                    bytes: 1000,
                    media_type: "application/octet-stream".into(),
                },
                settings,
            },
            approvals: vec![Approval { subject, evidence }],
        };
        let mut f = Self {
            root,
            request,
            documents,
        };
        f.commit();
        f
    }
    fn commit(&mut self) {
        for (path, bytes) in &self.documents {
            fs::write(self.root.path().join(path), bytes).unwrap();
        }
        git(self.root.path(), &["add", "."]);
        git(
            self.root.path(),
            &["commit", "--allow-empty", "-m", "Synthetic controls"],
        );
        self.request.source_commit = git(self.root.path(), &["rev-parse", "HEAD"]);
    }
    fn change(&mut self, path: &str, edit: impl FnOnce(&mut Value)) {
        let mut value: Value = serde_json::from_slice(&self.documents[path]).unwrap();
        edit(&mut value);
        let bytes = serde_json::to_vec(&value).unwrap();
        self.documents.insert(path.into(), bytes.clone());
        fn update(value: &mut Value, path: &str, bytes: &[u8]) {
            if value.get("path").and_then(Value::as_str) == Some(path) {
                value["artifact"]["sha256"] = json!(digest(bytes));
                value["artifact"]["bytes"] = json!(bytes.len());
            }
            match value {
                Value::Object(map) => {
                    for child in map.values_mut() {
                        update(child, path, bytes);
                    }
                }
                Value::Array(array) => {
                    for child in array {
                        update(child, path, bytes);
                    }
                }
                _ => (),
            }
        }
        let mut request = serde_json::to_value(&self.request).unwrap();
        update(&mut request, path, &bytes);
        self.request = serde_json::from_value(request).unwrap();
        self.commit();
    }
    fn inspect(&self) -> Result<Sidecar> {
        inspect(
            &self.request,
            &mut GitReader {
                repository: self.root.path(),
            },
        )
    }
}

#[test]
fn committed_bilingual_inspection_preserves_ids_clocks_approvals_and_worktree() {
    let f = Fixture::new();
    fs::write(f.root.path().join("scene.json"), "uncommitted broken edit").unwrap();
    let before = git(f.root.path(), &["status", "--porcelain"]);
    let sidecar = f.inspect().unwrap();
    assert_eq!(sidecar.lanes["es"].events[0].identity.cel_id, "C100");
    assert_eq!(sidecar.lanes["en"].events[0].identity.cel_id, "C100");
    // The established cue-opening rule covers the leading recorded breath.
    assert_eq!(sidecar.lanes["es"].events[0].start_sample, 0);
    assert_eq!(sidecar.lanes["en"].events[0].start_sample, 0);
    assert_eq!(sidecar.lanes["es"].events[1].start_sample, 400);
    assert_eq!(sidecar.lanes["en"].events[1].start_sample, 1000);
    assert_eq!(sidecar.lanes["es"].cues[0].alignment.cue_end_sample, 8000);
    assert_eq!(sidecar.lanes["en"].cues[0].alignment.sample_rate, 16000);
    assert_eq!(sidecar.request.approvals[0].subject.sha256, "7".repeat(64));
    assert!(!sidecar.media_verified && !sidecar.approvals_verified && !sidecar.production_adopted);
    assert_eq!(git(f.root.path(), &["status", "--porcelain"]), before);
    let output = f.root.path().join("sidecar.json");
    write_new(&sidecar, &output).unwrap();
    let original = fs::read(&output).unwrap();
    assert!(write_new(&sidecar, &output).is_err());
    assert_eq!(fs::read(&output).unwrap(), original);
}

#[test]
fn newer_drawings_preserve_story_cel_and_exact_historical_approval_subjects() {
    let mut f = Fixture::new();
    let original_request = f.request.clone();
    f.change("scene-bindings.json", |v| {
        v["assets"]["picture.first-line"]["sha256"] = json!("8".repeat(64));
        v["assets"]["picture.first-line"]["cache_uri"] =
            json!(format!("cache://sha256/{}", "8".repeat(64)));
    });
    let newer = f.inspect().unwrap();
    assert_eq!(newer.lanes["es"].events[0].identity.cel_id, "C100");
    assert_eq!(
        newer.lanes["es"].events[0].selected_picture.sha256,
        "8".repeat(64)
    );
    assert_eq!(newer.request.approvals[0].subject.sha256, "7".repeat(64));
    // A local replace ref must not substitute a newer snapshot for the old one.
    git(
        f.root.path(),
        &[
            "replace",
            &original_request.source_commit,
            &f.request.source_commit,
        ],
    );
    let old = inspect(
        &original_request,
        &mut GitReader {
            repository: f.root.path(),
        },
    )
    .unwrap();
    assert_eq!(
        old.lanes["es"].events[0].selected_picture.sha256,
        "7".repeat(64)
    );
}

#[test]
fn wrong_language_take_clock_trigger_or_identity_refuses() {
    for case in [
        "language",
        "take",
        "clock",
        "trigger",
        "identity",
        "missing-cue",
    ] {
        let mut f = Fixture::new();
        match case {
            "language" => f.change("en-words.json", |v| v["language"] = json!("es")),
            "take" => f.change("es-words.json", |v| {
                v["selected_take_sha256"] = json!("5".repeat(64))
            }),
            "clock" => f.change("es-words.json", |v| v["cue_end_sample"] = json!(50)),
            "trigger" => f.change("scene.json", |v| {
                v["language_event_bindings"]["es"][0]["spoken_trigger_phrase"] = json!("absent")
            }),
            "identity" => {
                f.request
                    .identities
                    .get_mut("en")
                    .unwrap()
                    .get_mut("en-event-1")
                    .unwrap()
                    .cel_id = "C101".into()
            }
            "missing-cue" => f.request.cues.get_mut("es").unwrap().clear(),
            _ => unreachable!(),
        }
        assert!(f.inspect().is_err(), "{case}");
    }
}

#[test]
fn missing_history_bad_pin_and_uncommitted_source_never_fall_back_to_latest() {
    for case in [
        "missing-commit",
        "missing-path",
        "wrong-hash",
        "wrong-length",
        "traversal",
        "tree",
    ] {
        let mut f = Fixture::new();
        match case {
            "missing-commit" => f.request.source_commit = "b".repeat(40),
            "missing-path" => f.request.scene.path = "missing.json".into(),
            "wrong-hash" => f.request.scene.artifact.sha256 = "f".repeat(64),
            "wrong-length" => f.request.scene.artifact.bytes -= 1,
            "traversal" => f.request.scene.path = "../scene.json".into(),
            "tree" => f.request.source_commit = git(f.root.path(), &["rev-parse", "HEAD^{tree}"]),
            _ => unreachable!(),
        }
        assert!(f.inspect().is_err(), "{case}");
    }
}

#[test]
fn cli_emits_only_bounded_status_and_creates_no_output_on_refusal() {
    let mut f = Fixture::new();
    let request = f.root.path().join("request.json");
    let output = f.root.path().join("result.json");
    let call = || {
        Command::new(env!("CARGO_BIN_EXE_reel-workflow"))
            .arg("inspect-film")
            .arg(f.root.path())
            .arg(&request)
            .arg(&output)
            .output()
            .unwrap()
    };
    fs::write(&request, serde_json::to_vec(&f.request).unwrap()).unwrap();
    let accepted = call();
    assert!(accepted.status.success(), "{:?}", accepted);
    let status: Value = serde_json::from_slice(&accepted.stdout).unwrap();
    assert_eq!(status["media_verified"], false);
    assert!(!String::from_utf8(accepted.stdout).unwrap().contains("C100"));
    fs::remove_file(&output).unwrap();
    f.request.source_commit = "b".repeat(40);
    fs::write(&request, serde_json::to_vec(&f.request).unwrap()).unwrap();
    assert!(!call().status.success());
    assert!(!output.exists());
}

#[test]
fn selected_alignment_changes_and_partial_clone_configuration_refuse() {
    let mut f = Fixture::new();
    f.change("es-alignment.json", |v| v["sample_rate"] = json!(16000));
    assert!(f.inspect().is_err());
    let f = Fixture::new();
    git(f.root.path(), &["config", "remote.origin.promisor", "true"]);
    let before = git(f.root.path(), &["status", "--porcelain"]);
    assert_eq!(
        f.inspect().unwrap_err().code,
        "film-inspect-complete-local-repository-required"
    );
    assert_eq!(git(f.root.path(), &["status", "--porcelain"]), before);
}

#[test]
fn committed_symlink_mode_is_refused_even_when_blob_bytes_match() {
    let mut f = Fixture::new();
    let blob = git(f.root.path(), &["hash-object", "scene.json"]);
    git(
        f.root.path(),
        &[
            "update-index",
            "--cacheinfo",
            &format!("120000,{blob},scene.json"),
        ],
    );
    git(f.root.path(), &["commit", "-m", "Synthetic symlink mode"]);
    f.request.source_commit = git(f.root.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        f.inspect().unwrap_err().code,
        "film-inspect-ordinary-committed-control-required"
    );
}

#[test]
fn inspection_bounds_and_aliases_refuse_before_any_source_read() {
    struct NoReads;
    impl Reader for NoReads {
        fn read(&mut self, _: &str, _: &str, _: u64) -> Result<Vec<u8>> {
            panic!("source read before admission check");
        }
    }
    let f = Fixture::new();
    let mut oversized = f.request.clone();
    oversized.scene.artifact.bytes = MAX_CONTROL_BYTES as u64 + 1;
    assert!(inspect(&oversized, &mut NoReads).is_err());
    let mut aliased = f.request.clone();
    aliased.scene.path = aliased.episode.path.clone();
    assert_eq!(
        inspect(&aliased, &mut NoReads).unwrap_err().kind,
        ErrorKind::Conflict
    );
}
