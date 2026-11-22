#![cfg(windows)]
//! Real local Git transactions over synthetic text only. No asset/payload,
//! approval, native-store retention or production adoption is asserted here.
use reel_workflow::{Artifact, ErrorKind, digest};
use reel_workflow_native::{
    Tool,
    selection::{self, Change, Identity, Request},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tig_core::Object;

fn git_path() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|folder| folder.join("git.exe"))
        .find(|path| path.is_file())
        .unwrap()
}
fn git(root: &Path, args: &[&str]) -> String {
    let result = Command::new(git_path())
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap().trim().into()
}
fn fixture() -> (tempfile::TempDir, Request) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-b", "main"]);
    fs::write(root.path().join("cel.txt"), b"old cel").unwrap();
    git(root.path(), &["add", "cel.txt"]);
    git(
        root.path(),
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-m",
            "base",
        ],
    );
    let path = git_path();
    let raw = fs::read(&path).unwrap();
    let request = Request {
        schema: "reel.workflow-owned-selection.v1".into(),
        git: Tool {
            path,
            sha256: digest(&raw),
            bytes: raw.len() as u64,
        },
        repository: root.path().canonicalize().unwrap(),
        operation: "fix-c101".into(),
        selection: "refs/reel-workflow/selections/fixture".into(),
        base: git(root.path(), &["rev-parse", "HEAD"]),
        expected: None,
        decision: Artifact {
            logical_id: "synthetic/decision".into(),
            sha256: digest(b"fixture decision"),
            bytes: 16,
            media_type: "application/json".into(),
        },
        identity: Identity {
            name: "Fixture".into(),
            email: "fixture@example.invalid".into(),
            unix_seconds: 1_700_000_000,
        },
        changes: vec![Change {
            path: "cel.txt".into(),
            expected: Object::from_bytes(b"old cel"),
            bytes: b"new cel".to_vec(),
        }],
    };
    (root, request)
}

#[test]
fn preview_is_read_only_and_apply_replays_without_changing_head_or_worktree() {
    let (root, request) = fixture();
    let index = fs::read(root.path().join(".git/index")).unwrap();
    let before = git(root.path(), &["show-ref"]);
    assert!(!selection::run(&request, false).unwrap().applied);
    assert_eq!(git(root.path(), &["show-ref"]), before);
    // A configured reference hook must not run during proposal or CAS.
    let hooks = root.path().join("hooks");
    fs::create_dir(&hooks).unwrap();
    fs::write(hooks.join("reference-transaction"), "#!/bin/sh\nexit 1\n").unwrap();
    git(
        root.path(),
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
    );
    let applied = selection::run(&request, true).unwrap();
    assert!(applied.applied && applied.currently_selected);
    // Discarding a response needs no new operation: completion readback recovers it.
    let replay = selection::run(&request, true).unwrap();
    assert_eq!(replay.proposal, applied.proposal);
    assert_eq!(
        git(
            root.path(),
            &["show", &format!("{}:cel.txt", replay.proposal)]
        ),
        "new cel"
    );
    assert_eq!(git(root.path(), &["rev-parse", "HEAD"]), request.base);
    assert_eq!(fs::read(root.path().join("cel.txt")).unwrap(), b"old cel");
    assert_eq!(fs::read(root.path().join(".git/index")).unwrap(), index);
}

#[test]
fn changed_operation_conflicts_and_historical_replay_never_reselects() {
    let (root, mut request) = fixture();
    let first = selection::run(&request, true).unwrap();
    request.changes[0].bytes = b"different cel".to_vec();
    assert_eq!(
        selection::run(&request, true).unwrap_err().kind,
        ErrorKind::Conflict
    );
    request.changes[0].bytes = b"new cel".to_vec();
    // A subsequent explicit operation can choose a different snapshot. The old
    // operation's marker remains, but recovery must never roll the lane back.
    git(
        root.path(),
        &[
            "update-ref",
            &request.selection,
            &request.base,
            &first.proposal,
        ],
    );
    let replay = selection::run(&request, true).unwrap();
    assert!(replay.applied && !replay.currently_selected);
    assert_eq!(
        git(root.path(), &["rev-parse", &request.selection]),
        request.base
    );
}

#[test]
fn stale_baseline_or_symbolic_ref_does_not_create_operation_marker() {
    let (root, mut request) = fixture();
    git(
        root.path(),
        &["update-ref", &request.selection, &request.base],
    );
    assert_eq!(
        selection::run(&request, true).unwrap_err().kind,
        ErrorKind::Stale
    );
    assert!(
        git(
            root.path(),
            &["for-each-ref", "refs/reel-workflow/operations"]
        )
        .is_empty()
    );
    git(root.path(), &["update-ref", "-d", &request.selection]);
    git(
        root.path(),
        &["symbolic-ref", &request.selection, "refs/heads/main"],
    );
    request.expected = Some(request.base.clone());
    assert_eq!(
        selection::run(&request, true).unwrap_err().kind,
        ErrorKind::Conflict
    );
    assert_eq!(
        git(root.path(), &["symbolic-ref", &request.selection]),
        "refs/heads/main"
    );
    assert!(
        git(
            root.path(),
            &["for-each-ref", "refs/reel-workflow/operations"]
        )
        .is_empty()
    );
}

#[test]
fn concurrent_candidates_have_one_winner_and_only_one_completion() {
    let (root, first) = fixture();
    let mut second: Request = serde_json::from_slice(&serde_json::to_vec(&first).unwrap()).unwrap();
    second.operation = "other-fix".into();
    second.changes[0].bytes = b"other cel".to_vec();
    let (a, b) = std::thread::scope(|scope| {
        let a = scope.spawn(|| selection::run(&first, true));
        let b = scope.spawn(|| selection::run(&second, true));
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_ne!(a.is_ok(), b.is_ok());
    let (winner, loser) = if let Ok(winner) = a {
        (winner, b.unwrap_err())
    } else {
        (b.unwrap(), a.unwrap_err())
    };
    assert_eq!(loser.kind, ErrorKind::Stale);
    let markers = git(
        root.path(),
        &[
            "for-each-ref",
            "--format=%(objectname)",
            "refs/reel-workflow/applied",
        ],
    );
    assert_eq!(markers, winner.proposal);
    assert_eq!(
        git(root.path(), &["rev-parse", &first.selection]),
        winner.proposal
    );
}

#[test]
fn active_branch_scope_and_wrong_exact_record_bytes_are_refused() {
    let (_root, mut request) = fixture();
    request.selection = "refs/heads/main".into();
    assert_eq!(
        selection::run(&request, true).unwrap_err().kind,
        ErrorKind::Invalid
    );
    request.selection = "refs/reel-workflow/selections/fixture".into();
    request.changes[0].expected = Object::from_bytes(b"unrelated bytes");
    assert_eq!(
        selection::run(&request, true).unwrap_err().kind,
        ErrorKind::Integrity
    );
}

#[test]
fn missing_completion_reconciles_selected_commit_but_never_retries_ambiguous_intent() {
    let (root, request) = fixture();
    let first = selection::run(&request, true).unwrap();
    let completion = format!("refs/reel-workflow/applied/{}", request.operation);
    // Stop after the selection change but before confirming completion.
    git(root.path(), &["update-ref", "-d", &completion]);
    assert!(selection::run(&request, true).unwrap().applied);
    assert_eq!(
        git(root.path(), &["rev-parse", &completion]),
        first.proposal
    );
    // With an unconfirmed intent and an intervening revert, outcome is unknown.
    // Merely finding the expected old ref value again must not authorize retry.
    git(root.path(), &["update-ref", "-d", &completion]);
    git(
        root.path(),
        &[
            "update-ref",
            &request.selection,
            &request.base,
            &first.proposal,
        ],
    );
    assert_eq!(
        selection::run(&request, true).unwrap_err().kind,
        ErrorKind::NeedsRecovery
    );
    assert_eq!(
        git(root.path(), &["rev-parse", &request.selection]),
        request.base
    );
    assert!(git(root.path(), &["for-each-ref", &completion]).is_empty());
}

#[test]
fn dangling_symbolic_lane_is_not_treated_as_new_and_has_no_intent() {
    let (root, request) = fixture();
    git(
        root.path(),
        &["symbolic-ref", &request.selection, "refs/heads/absent"],
    );
    assert_eq!(
        selection::run(&request, true).unwrap_err().kind,
        ErrorKind::Conflict
    );
    assert!(
        git(
            root.path(),
            &["for-each-ref", "refs/reel-workflow/operations"]
        )
        .is_empty()
    );
}

#[test]
fn existing_owned_lane_updates_only_at_exact_expected_commit() {
    let (root, mut request) = fixture();
    git(
        root.path(),
        &["update-ref", &request.selection, &request.base],
    );
    request.expected = Some(request.base.clone());
    let applied = selection::run(&request, true).unwrap();
    assert!(applied.applied && applied.currently_selected);
    assert_eq!(
        git(
            root.path(),
            &["rev-parse", &format!("{}^", applied.proposal)]
        ),
        request.base
    );
    assert_eq!(
        git(root.path(), &["show", &format!("{}:cel.txt", request.base)]),
        "old cel"
    );
    assert_eq!(git(root.path(), &["rev-parse", "HEAD"]), request.base);
}
