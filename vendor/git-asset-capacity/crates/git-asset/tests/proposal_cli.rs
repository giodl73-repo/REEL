mod common;
use common::{git, git_executable};
use git_asset_core::Object;
use git_asset_store::existing::Layout;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

struct Fixture {
    temp: TempDir,
    repo: PathBuf,
    profile: Value,
    object: PathBuf,
}
fn text(root: &Path, args: &[&str]) -> String {
    String::from_utf8(git(root, args).stdout)
        .unwrap()
        .trim_end()
        .into()
}
impl Fixture {
    fn new(layout: Layout) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-b", "main"]);
        git(&repo, &["config", "user.name", "Fixture"]);
        git(&repo, &["config", "user.email", "fixture@example.invalid"]);
        fs::write(repo.join(".gitattributes"), "* -text\n").unwrap();
        fs::write(repo.join("a.asset"), b"old a\n").unwrap();
        fs::write(repo.join("b.asset"), b"old b\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "base"]);
        let cache = temp.path().join("cache");
        fs::create_dir(&cache).unwrap();
        let new = b"PRIVATE_NEW_RECORD\n";
        let object_pin = Object::from_bytes(new);
        let object = layout.object_path(&cache, &object_pin.sha256);
        fs::create_dir_all(object.parent().unwrap()).unwrap();
        fs::write(&object, new).unwrap();
        let profile = json!({
            "schema":"tig.git-proposal-profile.experimental-1",
            "git_executable":git_executable(),"repository":repo,
            "base":text(&repo,&["rev-parse","HEAD"]),
            "identity":{"name":"Fixture","email":"fixture@example.invalid","unix_seconds":1700000000},
            "message":"propose authored records",
            "byte_source":{"places":[{"root":cache,"layout":layout}],"required":if cfg!(windows) {"prevent-mutation"} else {"cooperative-immutable"}},
            "changes":[{"path":"a.asset","expected":Object::from_bytes(b"old a\n"),"object":object_pin}]
        });
        Self {
            temp,
            repo,
            profile,
            object,
        }
    }
    fn run(&self, action: &str) -> Output {
        let path = self.temp.path().join("profile.json");
        fs::write(&path, serde_json::to_vec(&self.profile).unwrap()).unwrap();
        Command::new(env!("CARGO_BIN_EXE_git-asset-propose"))
            .arg(action)
            .arg(path)
            .output()
            .unwrap()
    }
    fn successful(&self, action: &str) -> Value {
        let output = self.run(action);
        assert!(output.status.success(), "{output:?}");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE_NEW_RECORD"));
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
#[test]
fn actual_cli_checks_without_writes_then_creates_reviewable_proposals_from_all_layouts() {
    for layout in [Layout::Flat, Layout::ReelCas, Layout::GitLfs] {
        let f = Fixture::new(layout);
        let head = text(&f.repo, &["rev-parse", "HEAD"]);
        let index = fs::read(f.repo.join(".git/index")).unwrap();
        let objects = text(&f.repo, &["count-objects", "-v"]);
        let checked = f.successful("check");
        assert_eq!(checked["action"], "check");
        assert!(checked["proposal"].is_null());
        assert_eq!(checked["changes"], f.profile["changes"]);
        assert_eq!(text(&f.repo, &["count-objects", "-v"]), objects);
        assert!(text(&f.repo, &["for-each-ref", "refs/tig/proposals"]).is_empty());
        let created = f.successful("create");
        assert_eq!(created["action"], "create");
        assert_eq!(created["current_selection"], false);
        assert_eq!(created["payload_custody"], false);
        assert_eq!(created["complete_semantic_coverage"], false);
        assert_eq!(f.successful("create")["proposal"], created["proposal"]);
        assert_eq!(text(&f.repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(fs::read(f.repo.join(".git/index")).unwrap(), index);
        assert!(text(&f.repo, &["status", "--porcelain=v1"]).is_empty());
        git(
            &f.repo,
            &[
                "cherry-pick",
                created["proposal"]["commit"].as_str().unwrap(),
            ],
        );
        assert_eq!(
            fs::read(f.repo.join("a.asset")).unwrap(),
            b"PRIVATE_NEW_RECORD\n"
        );
    }
}
#[test]
fn actual_cli_deduplicates_shared_objects_under_one_reader_and_request_budget() {
    let mut f = Fixture::new(Layout::Flat);
    let mut second = f.profile["changes"][0].clone();
    second["path"] = json!("b.asset");
    second["expected"] = json!(Object::from_bytes(b"old b\n"));
    f.profile["changes"].as_array_mut().unwrap().push(second);
    f.profile["byte_source"]["limits"] = json!({"requests":1,"live_readers":1});
    let checked = f.successful("check");
    assert_eq!(checked["verified_objects"], 1);
    assert_eq!(checked["changes"].as_array().unwrap().len(), 2);
    let created = f.successful("create");
    git(
        &f.repo,
        &[
            "cherry-pick",
            created["proposal"]["commit"].as_str().unwrap(),
        ],
    );
    assert_eq!(
        fs::read(f.repo.join("a.asset")).unwrap(),
        fs::read(f.repo.join("b.asset")).unwrap()
    );
}
#[test]
fn actual_cli_rejects_corrupt_missing_and_stale_inputs_before_git_writes() {
    for kind in ["corrupt", "missing", "stale", "duplicate", "oversize"] {
        let mut f = Fixture::new(Layout::Flat);
        match kind {
            "corrupt" => fs::write(&f.object, b"WRONG_NEW_RECORD!!\n").unwrap(),
            "missing" => fs::remove_file(&f.object).unwrap(),
            "stale" => f.profile["changes"][0]["expected"] = json!(Object::from_bytes(b"foreign")),
            "duplicate" => {
                let copy = f.profile["changes"][0].clone();
                f.profile["changes"].as_array_mut().unwrap().push(copy);
            }
            "oversize" => f.profile["changes"][0]["object"]["bytes"] = json!(65537),
            _ => unreachable!(),
        }
        let before = text(&f.repo, &["count-objects", "-v"]);
        for action in ["check", "create"] {
            let output = f.run(action);
            assert_eq!(output.status.code(), Some(2), "{kind}: {output:?}");
            assert!(output.stdout.is_empty());
            assert!(
                !String::from_utf8_lossy(&output.stderr).contains(f.temp.path().to_str().unwrap())
            );
        }
        assert_eq!(text(&f.repo, &["count-objects", "-v"]), before);
        assert!(text(&f.repo, &["for-each-ref", "refs/tig/proposals"]).is_empty());
        assert!(text(&f.repo, &["status", "--porcelain=v1"]).is_empty());
    }
}
#[test]
fn actual_cli_uses_ordered_existing_roots_without_initializing_missing_roots() {
    let mut f = Fixture::new(Layout::Flat);
    let missing = f.temp.path().join("not-initialized");
    f.profile["byte_source"]["places"]
        .as_array_mut()
        .unwrap()
        .insert(0, json!({"root":missing,"layout":"flat"}));
    f.successful("check");
    assert!(!missing.exists());
    let corrupt_root = f.temp.path().join("corrupt");
    let object: Object = serde_json::from_value(f.profile["changes"][0]["object"].clone()).unwrap();
    let corrupt = Layout::Flat.object_path(&corrupt_root, &object.sha256);
    fs::create_dir_all(corrupt.parent().unwrap()).unwrap();
    fs::write(corrupt, b"wrong").unwrap();
    f.profile["byte_source"]["places"][0]["root"] = json!(corrupt_root);
    assert_eq!(f.run("check").status.code(), Some(2));
    assert!(!missing.exists());
}
#[test]
fn actual_cli_requires_explicit_action_and_bounded_strict_profile() {
    let f = Fixture::new(Layout::Flat);
    for args in [
        vec![],
        vec!["profile.json"],
        vec!["run", "profile.json"],
        vec!["check", "a", "extra"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_git-asset-propose"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).starts_with("usage:"));
    }
    let path = f.temp.path().join("invalid.json");
    for bytes in [b"{\"schema\":1,\"schema\":2}".to_vec(), vec![b' '; 65537]] {
        fs::write(&path, bytes).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_git-asset-propose"))
            .arg("create")
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    assert!(text(&f.repo, &["for-each-ref", "refs/tig/proposals"]).is_empty());
}
