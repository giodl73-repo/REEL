mod common;
use common::{command, git};
use git_asset::{
    pointer::Pointer,
    source::{Error, GitId, IssueKind, Limits, Repository, Scope},
};
use git_asset_core::{
    LogicalPath, Object,
    tree::{MemoryNodes, TreeLimits},
};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

fn executable() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join(format!("git{}", std::env::consts::EXE_SUFFIX)))
        .find(|p| p.is_file())
        .unwrap()
}
fn scope() -> Scope {
    Scope::new([LogicalPath::parse("assets").unwrap()]).unwrap()
}
fn repo() -> TempDir {
    let t = tempfile::Builder::new()
        .prefix("git-asset git source ")
        .tempdir()
        .unwrap();
    git(t.path(), &["init", "-b", "main"]);
    git(t.path(), &["config", "user.name", "Fixture"]);
    git(
        t.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    fs::create_dir(t.path().join("assets")).unwrap();
    t
}
fn stage(repo: &Path, name: &str, bytes: &[u8]) -> Object {
    let object = Object::from_bytes(bytes);
    fs::write(
        repo.join("assets").join(name),
        Pointer::new(object.clone()).to_bytes(),
    )
    .unwrap();
    git(repo, &["add", "."]);
    object
}
fn commit(repo: &Path) -> GitId {
    git(repo, &["commit", "-m", "fixture"]);
    GitId::parse(
        String::from_utf8(git(repo, &["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim(),
    )
    .unwrap()
}
fn reader(repo: &Path) -> Repository {
    Repository::new(executable(), repo.to_owned(), Limits::default()).unwrap()
}

#[test]
fn staged_snapshot_ignores_unstaged_bytes_and_later_index_changes() {
    let t = repo();
    let a = stage(t.path(), "a.bin", b"A");
    let r = reader(t.path());
    let captured = r.index().unwrap();
    fs::write(t.path().join("assets/a.bin"), b"untracked working bytes").unwrap();
    let first = r.extract(&captured, scope()).unwrap();
    assert!(first.complete());
    assert_eq!(first.bindings()[0].object, a);
    stage(t.path(), "a.bin", b"B");
    let next = r.index().unwrap();
    assert_ne!(captured.checkpoint(), next.checkpoint());
    assert_eq!(
        r.extract(&captured, scope()).unwrap().bindings()[0].object,
        a
    );
    let mut nodes = MemoryNodes::default();
    let tree = first.write_tree(&mut nodes, TreeLimits::default()).unwrap();
    assert_eq!(
        tree.get(
            &LogicalPath::parse("assets/a.bin").unwrap(),
            &mut nodes,
            TreeLimits::default()
        )
        .unwrap()
        .0,
        Some(a)
    );
}

#[test]
fn exact_commits_restore_prior_bindings_with_detached_head() {
    let t = repo();
    let a = stage(t.path(), "a.bin", b"A");
    let c1 = commit(t.path());
    let b = stage(t.path(), "a.bin", b"B");
    let c2 = commit(t.path());
    git(t.path(), &["checkout", "--detach", c1.as_str()]);
    let r = reader(t.path());
    for (id, object) in [(c1, a), (c2, b)] {
        let source = r.commit(id).unwrap();
        let out = r.extract(&source, scope()).unwrap();
        assert!(out.complete());
        assert_eq!(out.bindings()[0].object, object);
    }
    assert!(GitId::parse("main").is_err());
    assert!(GitId::parse(&"0".repeat(40)).is_err());
    assert!(r.commit(GitId::parse(&"f".repeat(40)).unwrap()).is_err());
}

#[test]
fn unresolved_merge_keeps_all_known_alternatives_and_refuses_a_tree() {
    let t = repo();
    stage(t.path(), "a.bin", b"base");
    commit(t.path());
    git(t.path(), &["checkout", "-b", "side"]);
    stage(t.path(), "a.bin", b"side");
    commit(t.path());
    git(t.path(), &["checkout", "main"]);
    stage(t.path(), "a.bin", b"main");
    commit(t.path());
    assert!(
        !command(t.path(), &["merge", "side"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let before = fs::read(t.path().join("assets/a.bin")).unwrap();
    let r = reader(t.path());
    let snapshot = r.index().unwrap();
    let out = r.extract(&snapshot, scope()).unwrap();
    assert!(!out.complete());
    assert_eq!(out.bindings().len(), 3);
    assert_eq!(
        out.bindings().iter().map(|b| b.stage).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(
        out.issues()
            .iter()
            .filter(|i| i.kind == IssueKind::Unmerged)
            .count(),
        3
    );
    assert!(matches!(
        out.write_tree(&mut MemoryNodes::default(), TreeLimits::default()),
        Err(Error::Incomplete)
    ));
    assert_eq!(fs::read(t.path().join("assets/a.bin")).unwrap(), before);
}

#[test]
fn malformed_extensions_and_conflicting_sizes_block_complete_coverage() {
    let t = repo();
    let object = stage(t.path(), "a.bin", b"A");
    let mut wrong = object;
    wrong.bytes += 1;
    fs::write(
        t.path().join("assets/b.bin"),
        Pointer::new(wrong).to_bytes(),
    )
    .unwrap();
    fs::write(t.path().join("assets/c.bin"), b"not a pointer").unwrap();
    fs::write(
        t.path().join("outside.txt"),
        b"ordinary file outside enrolled scope",
    )
    .unwrap();
    git(t.path(), &["add", "."]);
    let r = reader(t.path());
    let out = r.extract(&r.index().unwrap(), scope()).unwrap();
    assert!(!out.complete());
    assert_eq!(out.bindings().len(), 2);
    assert!(
        out.issues()
            .iter()
            .any(|i| i.kind == IssueKind::ConflictingSize)
    );
    assert!(out.issues().iter().any(|i| i.kind == IssueKind::NotPointer));
    assert!(!out.issues().iter().any(|i| i.path == "outside.txt"));
}

#[test]
fn large_nonpointer_blob_is_classified_without_loading_its_content() {
    let t = repo();
    fs::write(t.path().join("assets/big.bin"), vec![b'x'; 2 * 1024 * 1024]).unwrap();
    git(t.path(), &["add", "."]);
    let r = reader(t.path());
    let out = r.extract(&r.index().unwrap(), scope()).unwrap();
    assert!(!out.complete());
    assert!(out.bindings().is_empty());
    assert_eq!(out.issues()[0].kind, IssueKind::NotPointer);
}

#[test]
fn linked_worktrees_have_independent_index_views_and_shared_history() {
    let t = repo();
    let a = stage(t.path(), "a.bin", b"A");
    let c1 = commit(t.path());
    let second = t.path().join("linked");
    git(
        t.path(),
        &["worktree", "add", "-b", "other", second.to_str().unwrap()],
    );
    let b = stage(&second, "a.bin", b"B");
    let first = reader(t.path());
    let next = reader(&second);
    let original = first.index().unwrap();
    let changed = next.index().unwrap();
    assert_eq!(
        first.extract(&original, scope()).unwrap().bindings()[0].object,
        a
    );
    assert_eq!(
        next.extract(&changed, scope()).unwrap().bindings()[0].object,
        b
    );
    assert_eq!(
        next.extract(&next.commit(c1).unwrap(), scope())
            .unwrap()
            .bindings()[0]
            .object,
        a
    );
    assert!(matches!(
        next.extract(&original, scope()),
        Err(Error::Invalid)
    ));
}

#[test]
fn listing_entry_and_unique_blob_budgets_fail_closed() {
    let t = repo();
    stage(t.path(), "a.bin", b"A");
    stage(t.path(), "b.bin", b"B");
    for limits in [
        Limits {
            listing_bytes: 8,
            ..Limits::default()
        },
        Limits {
            entries: 1,
            ..Limits::default()
        },
    ] {
        let r = Repository::new(executable(), t.path().to_owned(), limits).unwrap();
        assert!(matches!(r.index(), Err(Error::Budget)));
    }
    let r = Repository::new(
        executable(),
        t.path().to_owned(),
        Limits {
            pointer_blobs: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    assert!(matches!(
        r.extract(&r.index().unwrap(), scope()),
        Err(Error::Budget)
    ));
    assert!(
        Repository::new(
            executable(),
            t.path().to_owned(),
            Limits {
                command_millis: 0,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(Repository::new(executable(), t.path().join("assets"), Limits::default()).is_err());
}

#[test]
fn empty_selection_deletions_and_duplicate_pointer_blobs_are_exact() {
    let t = repo();
    stage(t.path(), "a.bin", b"A");
    stage(t.path(), "b.bin", b"A");
    let r = Repository::new(
        executable(),
        t.path().to_owned(),
        Limits {
            pointer_blobs: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    let out = r.extract(&r.index().unwrap(), scope()).unwrap();
    assert!(out.complete());
    assert_eq!(out.bindings().len(), 2);
    commit(t.path());
    git(t.path(), &["rm", "assets/a.bin", "assets/b.bin"]);
    let empty = r.extract(&r.index().unwrap(), scope()).unwrap();
    assert!(empty.complete());
    assert!(empty.bindings().is_empty());
    assert!(Scope::new([]).is_err());
}

#[test]
fn git_sha256_repository_identity_is_not_payload_identity() {
    let t = tempfile::tempdir().unwrap();
    git(t.path(), &["init", "--object-format=sha256", "-b", "main"]);
    git(t.path(), &["config", "user.name", "Fixture"]);
    git(
        t.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    fs::create_dir(t.path().join("assets")).unwrap();
    let object = stage(t.path(), "a.bin", b"A");
    let id = commit(t.path());
    assert_eq!(id.as_str().len(), 64);
    let r = reader(t.path());
    let out = r.extract(&r.commit(id).unwrap(), scope()).unwrap();
    assert!(out.complete());
    assert_eq!(out.bindings()[0].object, object);
}

#[test]
fn replacement_objects_do_not_rewrite_captured_pointer_meaning() {
    let t = repo();
    let a = stage(t.path(), "a.bin", b"A");
    commit(t.path());
    let a_blob =
        String::from_utf8(git(t.path(), &["rev-parse", "HEAD:assets/a.bin"]).stdout).unwrap();
    stage(t.path(), "a.bin", b"B");
    commit(t.path());
    let b_blob =
        String::from_utf8(git(t.path(), &["rev-parse", "HEAD:assets/a.bin"]).stdout).unwrap();
    git(t.path(), &["replace", b_blob.trim(), a_blob.trim()]);
    let r = reader(t.path());
    let out = r.extract(&r.index().unwrap(), scope()).unwrap();
    assert!(out.complete());
    assert_eq!(out.bindings()[0].object, Object::from_bytes(b"B"));
    assert_ne!(out.bindings()[0].object, a);
}

fn blob_id(repo: &Path) -> String {
    String::from_utf8(git(repo, &["rev-parse", ":assets/a.bin"]).stdout)
        .unwrap()
        .trim()
        .to_owned()
}
fn loose(repo: &Path, oid: &str) -> PathBuf {
    repo.join(".git/objects").join(&oid[..2]).join(&oid[2..])
}

#[test]
fn missing_git_blob_is_unknown_and_does_not_become_an_empty_complete_scope() {
    let t = repo();
    stage(t.path(), "a.bin", b"A");
    let oid = blob_id(t.path());
    fs::remove_file(loose(t.path(), &oid)).unwrap();
    let r = reader(t.path());
    let out = r.extract(&r.index().unwrap(), scope()).unwrap();
    assert!(!out.complete());
    assert!(out.bindings().is_empty());
    assert_eq!(out.issues()[0].kind, IssueKind::MissingBlob);
}

#[test]
fn promisor_configuration_cannot_trigger_hydration_of_missing_objects() {
    let t = repo();
    stage(t.path(), "a.bin", b"A");
    commit(t.path());
    let oid = blob_id(t.path());
    let remote = t.path().join("remote.git");
    git(
        t.path(),
        &[
            "clone",
            "--bare",
            "--no-hardlinks",
            t.path().to_str().unwrap(),
            remote.to_str().unwrap(),
        ],
    );
    for (key, value) in [
        ("extensions.partialClone", "origin"),
        ("remote.origin.promisor", "true"),
        ("remote.origin.url", remote.to_str().unwrap()),
    ] {
        git(t.path(), &["config", key, value]);
    }
    fs::remove_file(loose(t.path(), &oid)).unwrap();
    let r = reader(t.path());
    match r.extract(&r.index().unwrap(), scope()) {
        Err(Error::Git) => (),
        Ok(out) => {
            assert!(!out.complete());
            assert!(
                out.issues()
                    .iter()
                    .any(|i| i.kind == IssueKind::MissingBlob)
            );
        }
        other => panic!("unexpected result: {other:?}"),
    }
    assert!(!loose(t.path(), &oid).exists());
}

#[test]
fn valid_compressed_bytes_under_the_wrong_git_id_are_rejected() {
    let t = repo();
    stage(t.path(), "a.bin", b"A");
    let a = blob_id(t.path());
    stage(t.path(), "a.bin", b"B");
    let b = blob_id(t.path());
    fs::remove_file(loose(t.path(), &b)).unwrap();
    fs::copy(loose(t.path(), &a), loose(t.path(), &b)).unwrap();
    let r = reader(t.path());
    assert!(matches!(
        r.extract(&r.index().unwrap(), scope()),
        Err(Error::Integrity)
    ));
}

#[test]
fn symlink_and_submodule_modes_are_never_followed_as_pointer_files() {
    let t = repo();
    stage(t.path(), "a.bin", b"A");
    let id = commit(t.path());
    let blob = blob_id(t.path());
    git(
        t.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("120000,{blob},assets/link"),
        ],
    );
    git(
        t.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{},assets/module", id.as_str()),
        ],
    );
    let r = reader(t.path());
    for snapshot in [r.index().unwrap(), {
        let next = commit(t.path());
        r.commit(next).unwrap()
    }] {
        let out = r.extract(&snapshot, scope()).unwrap();
        assert!(!out.complete());
        assert_eq!(out.bindings().len(), 1);
        assert_eq!(
            out.issues()
                .iter()
                .filter(|i| i.kind == IssueKind::UnsupportedMode)
                .count(),
            2
        );
    }
}

#[test]
fn unsupported_pointer_extensions_and_prefix_neighbors_are_not_silently_accepted() {
    let t = repo();
    let object = stage(t.path(), "a.bin", b"A");
    let mut extended = Pointer::new(object).to_bytes();
    extended.extend_from_slice(b"ext-0-custom data\n");
    fs::write(t.path().join("assets/a.bin"), extended).unwrap();
    fs::create_dir(t.path().join("assets-neighbor")).unwrap();
    fs::write(t.path().join("assets-neighbor/ordinary"), b"outside scope").unwrap();
    git(t.path(), &["add", "."]);
    let r = reader(t.path());
    let out = r.extract(&r.index().unwrap(), scope()).unwrap();
    assert!(!out.complete());
    assert_eq!(out.issues().len(), 1);
    assert_eq!(out.issues()[0].kind, IssueKind::NotPointer);
}

#[test]
fn bare_commit_reads_are_supported_without_inventing_an_empty_index() {
    let t = repo();
    let object = stage(t.path(), "a.bin", b"A");
    let id = commit(t.path());
    let bare = t.path().join("remote.git");
    git(
        t.path(),
        &[
            "clone",
            "--bare",
            t.path().to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    let r = reader(&bare);
    assert!(matches!(r.index(), Err(Error::Invalid)));
    let out = r.extract(&r.commit(id).unwrap(), scope()).unwrap();
    assert!(out.complete());
    assert_eq!(out.bindings()[0].object, object);
}

#[test]
fn exact_git_roots_survive_journal_reopen_and_another_branch_keeps_old_inputs() {
    use git_asset::cache::ReadNodes;
    use git_asset_store::{
        Key,
        source_claims::{Command, Role, Selection, SourceJournal},
    };
    let key = |s: &str| Key::parse(s).unwrap();
    let t = repo();
    let runtime = tempfile::tempdir().unwrap();
    let nodes_dir = runtime.path().join("nodes");
    fs::create_dir(&nodes_dir).unwrap();
    let unchanged = stage(t.path(), "cel-a.bin", b"cel A");
    let old = stage(t.path(), "cel-b.bin", b"old cel B");
    let cue = stage(t.path(), "cue.bin", b"shared cue");
    let c1 = commit(t.path());
    git(t.path(), &["branch", "side", c1.as_str()]);
    let r = reader(t.path());
    let capture = |id: GitId| {
        let selected = r.extract(&r.commit(id).unwrap(), scope()).unwrap();
        let mut nodes = MemoryNodes::default();
        let tree = selected
            .write_authority_tree(&mut nodes, TreeLimits::default())
            .unwrap();
        for (id, bytes) in nodes.objects() {
            fs::write(nodes_dir.join(id.as_str()), bytes).unwrap();
        }
        Selection {
            root: tree.root().clone(),
            context: selected.selection_digest().unwrap(),
        }
    };
    let first = capture(c1);
    let root = runtime.path().join("claims");
    let mut j = SourceJournal::create(&root, key("project"), key("epoch")).unwrap();
    let prepare = |source: &str, rev, selection: Selection| Command::Prepare {
        source: key(source),
        owner: key("owner"),
        expected_revision: rev,
        selection,
    };
    let accept = |source: &str, rev, prepared| Command::Adopt {
        source: key(source),
        owner: key("owner"),
        expected_revision: rev,
        prepared,
    };
    for source in ["main", "side"] {
        let p = j
            .apply(
                key(&format!("prepare-{source}")),
                prepare(source, 0, first.clone()),
            )
            .unwrap();
        j.apply(
            key(&format!("accept-{source}")),
            accept(source, 1, p.reference),
        )
        .unwrap();
    }
    let new = stage(t.path(), "cel-b.bin", b"new cel B");
    let c2 = commit(t.path());
    let second = capture(c2);
    assert_ne!(first.context, second.context);
    let request = prepare("main", 2, second.clone());
    let p = j
        .apply(key("prepare-replacement"), request.clone())
        .unwrap();
    let witness = j.checkpoint();
    drop(j);
    let mut j = SourceJournal::open(&root, &witness).unwrap();
    assert_eq!(j.obligations(10).unwrap().len(), 3);
    j.apply(key("accept-replacement"), accept("main", 3, p.reference))
        .unwrap();
    j.apply(
        key("retire-main-predecessor"),
        Command::Retire {
            source: key("main"),
            owner: key("owner"),
            expected_revision: 4,
            predecessor_revision: 2,
        },
    )
    .unwrap();
    let roots = j.obligations(10).unwrap();
    assert_eq!(roots.len(), 2);
    assert!(
        roots
            .iter()
            .any(|v| v.source == key("side") && v.role == Role::Current && v.selection == first)
    );
    let mut nodes = ReadNodes::new(nodes_dir);
    let lookup = |root: &Object, object: &Object, nodes: &mut ReadNodes| {
        git_asset_core::tree::Tree::from_root(root.clone())
            .get(
                &LogicalPath::parse(object.sha256.as_str()).unwrap(),
                nodes,
                TreeLimits::default(),
            )
            .unwrap()
            .0
    };
    assert_eq!(lookup(&first.root, &old, &mut nodes), Some(old.clone()));
    assert_eq!(lookup(&second.root, &old, &mut nodes), None);
    assert_eq!(lookup(&second.root, &new, &mut nodes), Some(new));
    for input in [unchanged, cue] {
        assert_eq!(lookup(&first.root, &input, &mut nodes), Some(input.clone()));
        assert_eq!(lookup(&second.root, &input, &mut nodes), Some(input));
    }
    assert!(
        j.apply(key("prepare-replacement"), request)
            .unwrap()
            .replayed
    );
    assert_eq!(
        j.source(&key("main"))
            .unwrap()
            .unwrap()
            .current
            .unwrap()
            .selection,
        second
    );
    let tracked = String::from_utf8(git(t.path(), &["ls-files"]).stdout).unwrap();
    assert!(!tracked.contains("sqlite"));
    assert!(!tracked.contains("claims.json"));
}

#[test]
fn selection_context_is_deterministic_and_binds_view_scope_and_conflict_state() {
    let t = repo();
    stage(t.path(), "a.bin", b"A");
    let id = commit(t.path());
    let first = reader(t.path());
    let second = reader(t.path());
    let a = first
        .extract(&first.index().unwrap(), scope())
        .unwrap()
        .selection_digest()
        .unwrap();
    let b = second
        .extract(&second.index().unwrap(), scope())
        .unwrap()
        .selection_digest()
        .unwrap();
    assert_eq!(a, b);
    let by_commit = first
        .extract(&first.commit(id).unwrap(), scope())
        .unwrap()
        .selection_digest()
        .unwrap();
    assert_ne!(a, by_commit);
    let narrower = first
        .extract(
            &first.index().unwrap(),
            Scope::new([LogicalPath::parse("assets/a.bin").unwrap()]).unwrap(),
        )
        .unwrap()
        .selection_digest()
        .unwrap();
    assert_ne!(a, narrower);
    fs::write(t.path().join("assets/a.bin"), b"not a pointer").unwrap();
    git(t.path(), &["add", "."]);
    assert!(matches!(
        first
            .extract(&first.index().unwrap(), scope())
            .unwrap()
            .selection_digest(),
        Err(Error::Incomplete)
    ));
}
