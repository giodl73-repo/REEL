#[path = "../../reel-workflow/tests/support/film_fixture.rs"]
mod fixture;
use fixture::{Fixture, git};
use reel_workflow::{Artifact, ErrorKind, digest, film_inspect};
use reel_workflow_ui::{
    film::{self, AssetBinding, Request},
    *,
};
use serde_json::json;
use std::{collections::BTreeMap, fs, process::Command};

fn request(f: &Fixture) -> Request {
    let sidecar = f.inspect().unwrap();
    let mut assets = BTreeMap::new();
    for lane in sidecar.lanes.values() {
        for picture in lane.events.iter().map(|e| &e.selected_picture) {
            assets.insert(
                (picture.logical_id.clone(), picture.sha256.clone()),
                AssetBinding {
                    asset: Artifact {
                        logical_id: picture.logical_id.clone(),
                        sha256: picture.sha256.clone(),
                        bytes: picture.bytes,
                        media_type: "image/svg+xml".into(),
                    },
                    preview: None,
                },
            );
        }
        for take in lane.cues.iter().map(|c| &c.selected_take) {
            assets.insert(
                (take.logical_id.clone(), take.sha256.clone()),
                AssetBinding {
                    asset: Artifact {
                        logical_id: take.logical_id.clone(),
                        sha256: take.sha256.clone(),
                        bytes: take.bytes,
                        media_type: "audio/wav".into(),
                    },
                    preview: None,
                },
            );
        }
    }
    Request {
        schema: "reel.workflow-film-board.v1".into(),
        inspection: f.request.clone(),
        title: "Synthetic bilingual film".into(),
        scene_title: "Across the water".into(),
        language_labels: BTreeMap::from([
            ("es".into(), "Español".into()),
            ("en".into(), "English".into()),
        ]),
        language_order: vec!["es".into(), "en".into()],
        assets: assets.into_values().collect(),
        media: BTreeMap::new(),
        tasks: BTreeMap::new(),
    }
}
fn project(f: &Fixture, r: &Request) -> reel_workflow::Result<Board> {
    film::project(
        r,
        &mut film_inspect::GitReader {
            repository: f.root.path(),
        },
    )
}
#[test]
fn projects_committed_text_clocks_and_subjects_without_reading_payloads_or_changing_source() {
    let mut f = Fixture::new();
    let evidence = serde_json::to_vec(
        &json!({"scope":"synthetic-only","decision":"existing-second-reference"}),
    )
    .unwrap();
    f.documents
        .insert("approval-2.json".into(), evidence.clone());
    f.request.approvals.push(film_inspect::Approval {
        subject: f.request.approvals[0].subject.clone(),
        evidence: film_inspect::Control {
            path: "approval-2.json".into(),
            artifact: Artifact {
                logical_id: "approval/second".into(),
                sha256: digest(&evidence),
                bytes: evidence.len() as u64,
                media_type: "application/json".into(),
            },
        },
    });
    f.commit();
    let mut r = request(&f);
    // The subject is SVG, but only an explicitly declared passive PNG derivative
    // may be a browser image. Neither file exists: projection reads no payloads.
    let picture = r
        .assets
        .iter_mut()
        .find(|a| a.asset.media_type == "image/svg+xml")
        .unwrap();
    picture.preview = Some("thumb".into());
    r.media.insert(
        "thumb".into(),
        Media {
            artifact: Artifact {
                logical_id: "preview/picture".into(),
                sha256: "d".repeat(64),
                bytes: 128,
                media_type: "image/png".into(),
            },
            path: f.root.path().join("never-open-this.png"),
            subject: Some(picture.asset.clone()),
        },
    );
    fs::write(
        f.root.path().join("es.txt"),
        "An uncommitted rewrite must not become the script.",
    )
    .unwrap();
    let before = git(f.root.path(), &["status", "--porcelain"]);
    let board = project(&f, &r).unwrap();
    assert_eq!(board.languages[0].id, "es");
    assert_eq!(
        board
            .units
            .iter()
            .map(|u| u.display_id.as_str())
            .collect::<Vec<_>>(),
        ["C100", "C101"]
    );
    let second = &board.units[1];
    assert_eq!(second.id, "second-line");
    assert_eq!(
        second
            .spans
            .iter()
            .find(|s| s.language == "es")
            .unwrap()
            .start_sample,
        400
    );
    assert_eq!(
        second
            .spans
            .iter()
            .find(|s| s.language == "en")
            .unwrap()
            .start_sample,
        1000
    );
    let es = board.cues.iter().find(|c| c.language == "es").unwrap();
    let en = board.cues.iter().find(|c| c.language == "en").unwrap();
    assert_eq!(
        (&es.text, es.sample_rate, es.samples),
        (&"Hola olas".to_string(), 8000, 8000)
    );
    assert_eq!(
        (&en.text, en.sample_rate, en.samples),
        (&"Hello waves".to_string(), 16000, 24000)
    );
    assert_eq!(es.markers[1].word, "olas");
    assert_eq!(en.markers[1].word, "waves");
    assert_eq!(board.approvals.len(), 2);
    for (actual, original) in board.approvals.iter().zip(&f.request.approvals) {
        assert_eq!(actual.subject, original.subject);
        assert_eq!(actual.evidence, original.evidence.artifact);
    }
    assert!(!board.synthetic);
    assert_eq!(git(f.root.path(), &["status", "--porcelain"]), before);
}
#[test]
fn new_drawing_keeps_unit_identity_and_does_not_transfer_historical_approvals() {
    let mut f = Fixture::new();
    let old = project(&f, &request(&f)).unwrap();
    f.change("scene-bindings.json", |v| {
        v["assets"]["picture.first-line"]["sha256"] = json!("8".repeat(64));
        v["assets"]["picture.first-line"]["cache_uri"] =
            json!(format!("cache://sha256/{}", "8".repeat(64)));
    });
    let new = project(&f, &request(&f)).unwrap();
    assert_eq!(
        old.units
            .iter()
            .map(|u| (&u.id, &u.story_id, &u.display_id))
            .collect::<Vec<_>>(),
        new.units
            .iter()
            .map(|u| (&u.id, &u.story_id, &u.display_id))
            .collect::<Vec<_>>()
    );
    assert_eq!(new.units[0].versions[0].asset.sha256, "8".repeat(64));
    assert_ne!(old.units[0].versions[0].id, new.units[0].versions[0].id);
    assert_eq!(new.approvals[0].subject.sha256, "7".repeat(64));
    assert!(
        new.units
            .iter()
            .flat_map(|u| &u.versions)
            .all(|v| v.approval_reference.is_none())
    );
}
#[test]
fn wrong_logical_binding_borrowed_take_preview_and_missing_history_refuse() {
    let f = Fixture::new();
    let r = request(&f);
    let mut bad = r.clone();
    bad.assets[0].asset.logical_id = "wrong/record-with-same-bytes".into();
    assert_eq!(
        project(&f, &bad).unwrap_err().code,
        "film-board-exact-subject"
    );
    let mut bad = r.clone();
    let takes: Vec<_> = bad
        .assets
        .iter()
        .enumerate()
        .filter(|(_, a)| a.asset.media_type == "audio/wav")
        .map(|(i, a)| (i, a.asset.clone()))
        .collect();
    bad.assets[takes[0].0].preview = Some("borrowed".into());
    bad.media.insert(
        "borrowed".into(),
        Media {
            artifact: takes[1].1.clone(),
            path: f.root.path().join("different-language.wav"),
            subject: None,
        },
    );
    assert_eq!(
        project(&f, &bad).unwrap_err().code,
        "board-cue-take-preview"
    );
    let mut bad = r.clone();
    bad.inspection.source_commit = "b".repeat(40);
    assert!(project(&f, &bad).is_err());
    let mut bad = r.clone();
    bad.assets.push(bad.assets[0].clone());
    assert_eq!(
        project(&f, &bad).unwrap_err().code,
        "film-board-duplicate-subject"
    );
    let mut bad = r.clone();
    bad.language_order = vec!["es".into(), "es".into()];
    assert_eq!(
        project(&f, &bad).unwrap_err().code,
        "film-board-language-order"
    );
}
#[test]
fn cli_pins_request_exports_once_and_never_overwrites_or_emits_private_source() {
    let f = Fixture::new();
    let r = request(&f);
    let raw = serde_json::to_vec(&r).unwrap();
    let path = f.root.path().join("private-request.json");
    fs::write(&path, &raw).unwrap();
    let pin = digest(&raw);
    let output = f.root.path().join("board.json");
    let call = |sha: &str, out: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_reel-workflow-ui"))
            .arg("project-film")
            .arg(&path)
            .arg(sha)
            .arg(f.root.path())
            .arg(out)
            .output()
            .unwrap()
    };
    let result = call(&pin, &output);
    assert!(result.status.success(), "{result:?}");
    let status: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(status["media_verified"], false);
    assert_eq!(status["production_adopted"], false);
    assert!(!String::from_utf8(result.stdout).unwrap().contains("Hola"));
    let original = fs::read(&output).unwrap();
    assert_eq!(status["board"]["sha256"], digest(&original));
    let board: Board = serde_json::from_slice(&original).unwrap();
    board.check().unwrap();
    assert_eq!(board.projection_request.unwrap().sha256, pin);
    assert!(!call(&pin, &output).status.success());
    assert_eq!(fs::read(&output).unwrap(), original);
    let refused = f.root.path().join("must-not-exist.json");
    let result = call(&"f".repeat(64), &refused);
    assert!(!result.status.success());
    assert!(!refused.exists());
    let error: serde_json::Value = serde_json::from_slice(&result.stderr).unwrap();
    assert_eq!(
        error["error"]["kind"],
        serde_json::to_value(ErrorKind::Integrity).unwrap()
    );
    #[cfg(windows)]
    {
        let missing = f.root.path().join("missing-native-control.json");
        let result = Command::new(env!("CARGO_BIN_EXE_reel-workflow-ui"))
            .arg("project-native-film")
            .arg(&path)
            .arg("f".repeat(64))
            .arg(&missing)
            .arg(&missing)
            .arg(&refused)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(!refused.exists());
        let error: serde_json::Value = serde_json::from_slice(&result.stderr).unwrap();
        assert_eq!(error["error"]["code"], "film-board-request-pin");
    }
}
