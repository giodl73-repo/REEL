mod common;
use common::{git, git_executable};
use git_asset::source::{
    Error, GitId, Limits, Repository,
    proposal::{Identity, Replacement},
};
use git_asset_core::{LogicalPath, Object};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

fn text(root: &Path, args: &[&str]) -> String {
    String::from_utf8(git(root, args).stdout)
        .unwrap()
        .trim_end()
        .into()
}
fn head(root: &Path) -> GitId {
    GitId::parse(&text(root, &["rev-parse", "HEAD"])).unwrap()
}
fn reader(root: &Path) -> Repository {
    Repository::new(git_executable(), root.into(), Limits::default()).unwrap()
}
fn identity() -> Identity {
    Identity {
        name: "Synthetic Fixture".into(),
        email: "fixture@example.invalid".into(),
        unix_seconds: 1_700_000_000,
    }
}
fn objects(root: &Path) -> u64 {
    text(root, &["count-objects", "-v"])
        .lines()
        .find_map(|line| line.strip_prefix("count: "))
        .unwrap()
        .parse()
        .unwrap()
}
fn replacement(path: &str, old: &[u8], new: &[u8]) -> Replacement {
    Replacement {
        path: LogicalPath::parse(path).unwrap(),
        expected: Object::from_bytes(old),
        bytes: new.to_vec(),
    }
}
fn fixture(format: &str) -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    git(
        root,
        &["init", "-b", "main", &format!("--object-format={format}")],
    );
    git(root, &["config", "user.name", "Fixture"]);
    git(root, &["config", "user.email", "fixture@example.invalid"]);
    fs::create_dir_all(root.join("records/scene/01")).unwrap();
    fs::write(root.join(".gitattributes"), "* -text\n").unwrap();
    fs::write(root.join("records/scene/01/cel.asset"), b"cel v1\n").unwrap();
    fs::write(root.join("records/scene/01/cue.asset"), b"cue v1\n").unwrap();
    fs::write(root.join("other.txt"), b"unrelated\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);
    temp
}
fn cel() -> Replacement {
    replacement("records/scene/01/cel.asset", b"cel v1\n", b"cel v2\n")
}
#[test]
fn proposal_preserves_dirty_index_worktree_and_head_and_retries_identically() {
    let temp = fixture("sha1");
    let root = temp.path();
    let base = head(root);
    fs::write(root.join("other.txt"), b"staged unrelated\n").unwrap();
    git(root, &["add", "other.txt"]);
    fs::write(root.join("records/scene/01/cel.asset"), b"private edits\n").unwrap();
    let index = fs::read(root.join(".git/index")).unwrap();
    let status = text(root, &["status", "--porcelain=v1"]);
    let repo = reader(root);
    let before_objects = objects(root);
    let first = repo
        .propose(base.clone(), &[cel()], &identity(), "new cel")
        .unwrap();
    let second = repo
        .propose(base.clone(), &[cel()], &identity(), "new cel")
        .unwrap();
    assert_eq!(first.commit(), second.commit());
    assert_eq!(first.reference(), second.reference());
    // One new small blob, four affected trees and one commit. Retry adds none.
    assert_eq!(objects(root) - before_objects, 6);
    assert_eq!(
        text(root, &["rev-parse", first.reference()]),
        first.commit().as_str()
    );
    assert_eq!(head(root), base);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(text(root, &["status", "--porcelain=v1"]), status);
    assert_eq!(
        fs::read(root.join("records/scene/01/cel.asset")).unwrap(),
        b"private edits\n"
    );
    assert_eq!(
        text(
            root,
            &["rev-parse", &format!("{}^", first.commit().as_str())]
        ),
        base.as_str()
    );
    assert_eq!(
        text(
            root,
            &[
                "diff-tree",
                "--no-commit-id",
                "--name-only",
                "-r",
                first.commit().as_str()
            ]
        ),
        "records/scene/01/cel.asset"
    );
    assert_eq!(
        text(
            root,
            &[
                "show",
                &format!("{}:records/scene/01/cel.asset", first.commit().as_str())
            ]
        ),
        "cel v2"
    );
}
#[test]
fn standard_git_cherry_pick_combines_disjoint_proposals_from_the_same_base() {
    let temp = fixture("sha1");
    let root = temp.path();
    let base = head(root);
    let repo = reader(root);
    let first = repo
        .propose(base.clone(), &[cel()], &identity(), "cel")
        .unwrap();
    let second = repo
        .propose(
            base,
            &[replacement(
                "records/scene/01/cue.asset",
                b"cue v1\n",
                b"cue v2\n",
            )],
            &identity(),
            "cue",
        )
        .unwrap();
    git(root, &["cherry-pick", first.commit().as_str()]);
    git(root, &["cherry-pick", second.commit().as_str()]);
    assert_eq!(
        fs::read(root.join("records/scene/01/cel.asset")).unwrap(),
        b"cel v2\n"
    );
    assert_eq!(
        fs::read(root.join("records/scene/01/cue.asset")).unwrap(),
        b"cue v2\n"
    );
    assert!(text(root, &["status", "--porcelain=v1"]).is_empty());
}
#[test]
fn standard_git_same_file_conflict_does_not_select_a_new_commit() {
    let temp = fixture("sha1");
    let root = temp.path();
    let proposal = reader(root)
        .propose(head(root), &[cel()], &identity(), "candidate")
        .unwrap();
    fs::write(root.join("records/scene/01/cel.asset"), b"different cel\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "other edit"]);
    let before = head(root);
    let failed = Command::new(git_executable())
        .arg("-C")
        .arg(root)
        .args(["cherry-pick", proposal.commit().as_str()])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert_eq!(head(root), before);
    assert!(!text(root, &["ls-files", "--unmerged"]).is_empty());
    git(root, &["cherry-pick", "--abort"]);
    assert_eq!(
        fs::read(root.join("records/scene/01/cel.asset")).unwrap(),
        b"different cel\n"
    );
}
#[test]
fn wrong_source_duplicate_missing_oversize_and_identity_injection_refuse() {
    let temp = fixture("sha1");
    let root = temp.path();
    let repo = reader(root);
    let base = head(root);
    let before_objects = objects(root);
    let mut wrong = cel();
    wrong.expected = Object::from_bytes(b"foreign source");
    assert_eq!(
        repo.propose(base.clone(), &[wrong], &identity(), "x").err(),
        Some(Error::Integrity)
    );
    assert_eq!(
        repo.propose(base.clone(), &[cel(), cel()], &identity(), "x")
            .err(),
        Some(Error::Invalid)
    );
    let missing = replacement("records/missing.asset", b"", b"new");
    assert_eq!(
        repo.propose(base.clone(), &[missing], &identity(), "x")
            .err(),
        Some(Error::Incomplete)
    );
    let mut large = cel();
    large.bytes = vec![b'x'; 65_537];
    assert_eq!(
        repo.propose(base.clone(), &[large], &identity(), "x").err(),
        Some(Error::Budget)
    );
    let mut who = identity();
    who.name = "name\nparent injected".into();
    assert_eq!(
        repo.propose(base.clone(), &[cel()], &who, "x").err(),
        Some(Error::Invalid)
    );
    assert!(text(root, &["for-each-ref", "refs/tig/proposals"]).is_empty());
    assert_eq!(head(root), base);
    assert_eq!(objects(root), before_objects);
}
#[test]
fn foreign_and_symbolic_proposal_refs_are_never_overwritten_or_followed() {
    let temp = fixture("sha1");
    let root = temp.path();
    let repo = reader(root);
    let base = head(root);
    let proposal = repo
        .propose(base.clone(), &[cel()], &identity(), "candidate")
        .unwrap();
    git(root, &["update-ref", proposal.reference(), base.as_str()]);
    assert_eq!(
        repo.propose(base.clone(), &[cel()], &identity(), "candidate")
            .err(),
        Some(Error::Integrity)
    );
    assert_eq!(
        text(root, &["rev-parse", proposal.reference()]),
        base.as_str()
    );
    git(
        root,
        &[
            "update-ref",
            "refs/heads/protected",
            proposal.commit().as_str(),
        ],
    );
    git(
        root,
        &["symbolic-ref", proposal.reference(), "refs/heads/protected"],
    );
    assert_eq!(
        repo.propose(base.clone(), &[cel()], &identity(), "candidate")
            .err(),
        Some(Error::Invalid)
    );
    assert_eq!(
        text(root, &["rev-parse", "refs/heads/protected"]),
        proposal.commit().as_str()
    );
    assert_eq!(head(root), base);
}
#[test]
fn linked_worktree_proposal_does_not_change_either_checkout() {
    let temp = fixture("sha1");
    let root = temp.path();
    let linked = tempfile::tempdir().unwrap();
    let worktree = linked.path().join("worker");
    git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            "worker",
            worktree.to_str().unwrap(),
        ],
    );
    let before = head(root);
    let proposal = reader(&worktree)
        .propose(before.clone(), &[cel()], &identity(), "worker")
        .unwrap();
    assert_eq!(head(&worktree), before);
    assert_eq!(head(root), before);
    assert_eq!(
        text(root, &["rev-parse", proposal.reference()]),
        proposal.commit().as_str()
    );
    assert_eq!(
        fs::read(worktree.join("records/scene/01/cel.asset")).unwrap(),
        b"cel v1\n"
    );
    git(root, &["worktree", "remove", worktree.to_str().unwrap()]);
}
#[test]
fn git_sha256_repository_accepts_the_same_generic_small_file_contract() {
    let temp = fixture("sha256");
    let root = temp.path();
    let proposal = reader(root)
        .propose(
            head(root),
            &[replacement(
                "other.txt",
                b"unrelated\n",
                b"invoice corrected\n",
            )],
            &identity(),
            "generic authored text",
        )
        .unwrap();
    assert_eq!(proposal.commit().as_str().len(), 64);
    git(root, &["cherry-pick", proposal.commit().as_str()]);
    assert_eq!(
        fs::read(root.join("other.txt")).unwrap(),
        b"invoice corrected\n"
    );
}
#[test]
fn unrelated_symlink_gitlink_and_executable_modes_survive_tree_rewrite() {
    let temp = fixture("sha1");
    let root = temp.path();
    let base = head(root);
    let blob = text(root, &["rev-parse", "HEAD:other.txt"]);
    git(
        root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("120000,{blob},link"),
        ],
    );
    git(
        root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{},submodule", base.as_str()),
        ],
    );
    git(root, &["update-index", "--chmod=+x", "other.txt"]);
    git(root, &["commit", "-m", "modes"]);
    let base = head(root);
    let proposal = reader(root)
        .propose(base.clone(), &[cel()], &identity(), "preserve modes")
        .unwrap();
    for path in ["link", "submodule", "other.txt"] {
        assert_eq!(
            text(root, &["ls-tree", base.as_str(), path]),
            text(root, &["ls-tree", proposal.commit().as_str(), path])
        );
    }
    assert_eq!(
        reader(root)
            .propose(
                base,
                &[replacement("other.txt", b"unrelated\n", b"new")],
                &identity(),
                "reject executable target"
            )
            .err(),
        Some(Error::Invalid)
    );
}

#[test]
fn parallel_identical_proposals_converge_without_an_index_or_worktree_lock() {
    let temp = fixture("sha1");
    let root = temp.path();
    let base = head(root);
    let repo = reader(root);
    let start = std::sync::Barrier::new(2);
    let run = || {
        start.wait();
        repo.propose(base.clone(), &[cel()], &identity(), "concurrent proposal")
    };
    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(run);
        let second = scope.spawn(run);
        (first.join().unwrap(), second.join().unwrap())
    });
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(first.commit(), second.commit());
    assert_eq!(first.reference(), second.reference());
    assert_eq!(head(root), base);
    assert!(text(root, &["status", "--porcelain=v1"]).is_empty());
    assert!(!root.join(".git/index.lock").exists());
}

#[test]
fn one_proposal_updates_multiple_records_together_and_preserves_the_old_commit() {
    let temp = fixture("sha1");
    let root = temp.path();
    let base = head(root);
    let changes = [
        cel(),
        replacement("records/scene/01/cue.asset", b"cue v1\n", b"cue v2\n"),
    ];
    let proposal = reader(root)
        .propose(
            base.clone(),
            &changes,
            &identity(),
            "coherent scene changes",
        )
        .unwrap();
    assert_eq!(
        text(
            root,
            &[
                "diff-tree",
                "--no-commit-id",
                "--name-only",
                "-r",
                proposal.commit().as_str()
            ]
        )
        .lines()
        .count(),
        2
    );
    assert_eq!(
        text(
            root,
            &[
                "show",
                &format!("{}:records/scene/01/cel.asset", base.as_str())
            ]
        ),
        "cel v1"
    );
    assert_eq!(
        text(
            root,
            &[
                "show",
                &format!("{}:records/scene/01/cue.asset", base.as_str())
            ]
        ),
        "cue v1"
    );
    git(root, &["cherry-pick", proposal.commit().as_str()]);
    assert_eq!(
        fs::read(root.join("records/scene/01/cel.asset")).unwrap(),
        b"cel v2\n"
    );
    assert_eq!(
        fs::read(root.join("records/scene/01/cue.asset")).unwrap(),
        b"cue v2\n"
    );
}

#[test]
fn proposal_environment_child() {
    let Some(root) = std::env::var_os("GIT_ASSET_PROPOSAL_TEST_ROOT") else {
        return;
    };
    let base = GitId::parse(&std::env::var("GIT_ASSET_PROPOSAL_TEST_BASE").unwrap()).unwrap();
    reader(Path::new(&root))
        .propose(base, &[cel()], &identity(), "isolated proposal")
        .unwrap();
}

#[test]
fn inherited_git_routing_cannot_redirect_proposal_writes_to_another_repository() {
    let actual = fixture("sha1");
    let foreign = fixture("sha1");
    let actual_head = head(actual.path());
    let foreign_head = head(foreign.path());
    let foreign_index = fs::read(foreign.path().join(".git/index")).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "proposal_environment_child"])
        .env("GIT_ASSET_PROPOSAL_TEST_ROOT", actual.path())
        .env("GIT_ASSET_PROPOSAL_TEST_BASE", actual_head.as_str())
        .env("GIT_DIR", foreign.path().join(".git"))
        .env("GIT_WORK_TREE", foreign.path())
        .env("GIT_INDEX_FILE", foreign.path().join(".git/index"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(head(actual.path()), actual_head);
    assert_eq!(head(foreign.path()), foreign_head);
    assert_eq!(
        fs::read(foreign.path().join(".git/index")).unwrap(),
        foreign_index
    );
    assert!(text(foreign.path(), &["for-each-ref", "refs/tig/proposals"]).is_empty());
    assert_eq!(
        text(actual.path(), &["for-each-ref", "refs/tig/proposals"])
            .lines()
            .count(),
        1
    );
}

#[test]
fn proposal_ref_creation_does_not_execute_reference_transaction_hooks() {
    let temp = fixture("sha1");
    let root = temp.path();
    let hooks = root.join("blocking-hooks");
    fs::create_dir(&hooks).unwrap();
    let hook = hooks.join("reference-transaction");
    fs::write(&hook, b"#!/bin/sh\nexit 97\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
    git(root, &["config", "core.hooksPath", hooks.to_str().unwrap()]);
    let base = head(root);
    let blocked = Command::new(git_executable())
        .arg("-C")
        .arg(root)
        .args(["update-ref", "refs/git-asset/hook-control", base.as_str()])
        .output()
        .unwrap();
    assert!(
        !blocked.status.success(),
        "hook control must actually block"
    );
    let proposal = reader(root)
        .propose(base.clone(), &[cel()], &identity(), "no hook")
        .unwrap();
    assert_eq!(
        text(root, &["rev-parse", proposal.reference()]),
        proposal.commit().as_str()
    );
    assert_eq!(head(root), base);
}
