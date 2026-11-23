#![allow(dead_code)]
use reel_workflow::{film_inspect::*, *};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

pub struct Fixture {
    pub root: tempfile::TempDir,
    pub request: Request,
    pub documents: BTreeMap<String, Vec<u8>>,
}
pub fn git(root: &Path, args: &[&str]) -> String {
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
    pub fn new() -> Self {
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
    pub fn commit(&mut self) {
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
    pub fn change(&mut self, path: &str, edit: impl FnOnce(&mut Value)) {
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
    pub fn inspect(&self) -> Result<Sidecar> {
        inspect(
            &self.request,
            &mut GitReader {
                repository: self.root.path(),
            },
        )
    }
}
