#[path = "support/film_fixture.rs"]
mod fixture;
use fixture::{Fixture, git};
use reel_workflow::{film_inspect::*, *};
use serde_json::{Value, json};
use std::{fs, process::Command};

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
