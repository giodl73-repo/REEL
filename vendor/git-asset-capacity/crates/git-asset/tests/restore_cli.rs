#![cfg(feature = "assets")]
mod common;
use common::{git, git_executable};
use git_asset_core::Object;
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

struct Fixture {
    temp: TempDir,
    profile: std::path::PathBuf,
    object: Object,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        git(root, &["init", "-b", "main"]);
        git(root, &["config", "user.name", "Fixture"]);
        git(root, &["config", "user.email", "fixture@example.invalid"]);
        fs::create_dir(root.join("records")).unwrap();
        fs::create_dir(root.join("definitions")).unwrap();
        fs::create_dir_all(root.join("cache/objects")).unwrap();
        let write = |path: &str, value: Value| {
            let bytes = serde_json::to_vec(&value).unwrap();
            fs::write(root.join(path), &bytes).unwrap();
            Object::from_bytes(&bytes)
        };
        let schema = write("definitions/authored.json", json!({"type":"object"}));
        let definition = write(
            "definitions/file.json",
            json!({"schema":"tig.definition.design-1",
            "id":"types/file", "authored_schema":schema,
            "authoring":{"properties":["path"],"required":["path"],"defaults":{}},
            "stream_rules":{},"semantic_rules":{}}),
        );
        let mut pin = serde_json::to_value(definition).unwrap();
        pin["id"] = json!("types/file");
        let object = Object::from_bytes(b"synthetic original bytes\n");
        fs::write(
            root.join("cache/objects").join(object.sha256.as_str()),
            b"synthetic original bytes\n",
        )
        .unwrap();
        write(
            "records/input.asset",
            json!({"schema":"tig.instance.design-1", "id":"input",
            "definition":pin,"authored":{"path":"data/input.txt"},
            "streams":[{"slot":"source","role":"primary","group":"input","object":object}],
            "selected":object,"status":{},"metadata":{}}),
        );
        git(root, &["add", "records", "definitions"]);
        git(root, &["commit", "-m", "fixture"]);
        let commit = String::from_utf8(git(root, &["rev-parse", "HEAD"]).stdout).unwrap();
        let profile = root.join("profile.json");
        fs::write(
            &profile,
            serde_json::to_vec(
                &json!({"schema":"tig.asset-resolver-profile.experimental-1",
            "git_executable":git_executable(),"repository":root,"commit":commit.trim(),
            "records":["records"],"definitions":["definitions"],
            "byte_source":{"places":[{"root":root.join("cache"),"layout":"flat"}],
                "required":if cfg!(windows) {"prevent-mutation"} else {"cooperative-immutable"}}}),
            )
            .unwrap(),
        )
        .unwrap();
        Self {
            temp,
            profile,
            object,
        }
    }
    fn run(&self, apply: bool, dest: &Path) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_git-asset-restore"));
        if apply {
            c.arg("--apply");
        }
        c.arg(&self.profile)
            .args(["input", "source"])
            .arg(dest)
            .output()
            .unwrap()
    }
    fn cache(&self) -> std::path::PathBuf {
        self.temp
            .path()
            .join("cache/objects")
            .join(self.object.sha256.as_str())
    }
    fn selected(&self, apply: bool, dest: &Path) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_git-asset-restore"));
        c.arg("--selected");
        if apply {
            c.arg("--apply");
        }
        c.arg(&self.profile)
            .arg("input")
            .arg(dest)
            .output()
            .unwrap()
    }
    fn commit_record(&self, record: &Value) {
        let root = self.temp.path();
        fs::write(
            root.join("records/input.asset"),
            serde_json::to_vec(record).unwrap(),
        )
        .unwrap();
        git(root, &["add", "records"]);
        git(root, &["commit", "-m", "selected fixture revision"]);
        let mut p: Value = serde_json::from_slice(&fs::read(&self.profile).unwrap()).unwrap();
        p["commit"] = json!(
            String::from_utf8(git(root, &["rev-parse", "HEAD"]).stdout)
                .unwrap()
                .trim()
        );
        fs::write(&self.profile, serde_json::to_vec(&p).unwrap()).unwrap();
    }
}
fn action(out: Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice::<Value>(&out.stdout).unwrap()["action"]
        .as_str()
        .unwrap()
        .into()
}
#[test]
fn selected_only_primary_restores_exact_commit_without_a_stream() {
    let f = Fixture::new();
    let root = f.temp.path();
    let mut record: Value =
        serde_json::from_slice(&fs::read(root.join("records/input.asset")).unwrap()).unwrap();
    let bytes = b"independently selected primary\n";
    let selected = Object::from_bytes(bytes);
    fs::write(
        root.join("cache/objects").join(selected.sha256.as_str()),
        bytes,
    )
    .unwrap();
    record["selected"] = json!(selected);
    record["streams"] = json!([]);
    f.commit_record(&record);
    let head = git(root, &["rev-parse", "HEAD"]).stdout;
    fs::write(
        root.join("records/input.asset"),
        b"uncommitted invalid metadata",
    )
    .unwrap();
    let dest = root.join("selected.txt");
    assert!(!f.run(false, &dest).status.success());
    assert_eq!(action(f.selected(false, &dest)), "would-restore");
    assert!(!dest.exists());
    assert_eq!(action(f.selected(true, &dest)), "restored");
    assert_eq!(Object::from_bytes(&fs::read(&dest).unwrap()), selected);
    assert_eq!(action(f.selected(true, &dest)), "already-present");
    assert_eq!(git(root, &["rev-parse", "HEAD"]).stdout, head);
    assert_eq!(
        fs::read(root.join("records/input.asset")).unwrap(),
        b"uncommitted invalid metadata"
    );
    // A literal stream named "selected" remains distinct from the selector.
    record["streams"] =
        json!([{"slot":"selected","role":"primary","group":"input","object":f.object}]);
    f.commit_record(&record);
    let stream_dest = root.join("literal-stream.txt");
    let out = Command::new(env!("CARGO_BIN_EXE_git-asset-restore"))
        .arg("--apply")
        .arg(&f.profile)
        .args(["input", "selected"])
        .arg(&stream_dest)
        .output()
        .unwrap();
    assert_eq!(action(out), "restored");
    assert_eq!(
        Object::from_bytes(&fs::read(stream_dest).unwrap()),
        f.object
    );
    assert_eq!(action(f.selected(true, &dest)), "already-present");
}
#[test]
fn selected_mode_never_falls_back_to_an_available_stream() {
    let f = Fixture::new();
    let root = f.temp.path();
    let mut record: Value =
        serde_json::from_slice(&fs::read(root.join("records/input.asset")).unwrap()).unwrap();
    let bytes = b"separate selected bytes\n";
    let selected = Object::from_bytes(bytes);
    let selected_path = root.join("cache/objects").join(selected.sha256.as_str());
    record["selected"] = json!(selected);
    record["streams"][0]["slot"] = json!("selected");
    f.commit_record(&record);
    let dest = root.join("selected.txt");
    fs::write(&selected_path, b"corrupt").unwrap();
    assert!(!f.selected(true, &dest).status.success());
    assert!(!dest.exists());
    fs::remove_file(&selected_path).unwrap();
    assert!(!f.selected(true, &dest).status.success());
    assert!(!dest.exists());
    record["selected"] = Value::Null;
    f.commit_record(&record);
    assert!(!f.selected(true, &dest).status.success());
    assert!(!dest.exists());
    assert_eq!(fs::read(f.cache()).unwrap(), b"synthetic original bytes\n");
}
#[test]
fn selected_mode_accepts_flag_orders_and_refuses_ambiguous_arguments() {
    let f = Fixture::new();
    let root = f.temp.path();
    let dest = root.join("selected.txt");
    for flags in [
        vec!["--selected", "--selected"],
        vec!["--apply", "--apply"],
        vec!["--unknown"],
        vec!["--selected", "--apply", "--selected"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_git-asset-restore"))
            .args(flags)
            .arg(&f.profile)
            .arg("input")
            .arg(&dest)
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(!dest.exists());
    }
    let out = Command::new(env!("CARGO_BIN_EXE_git-asset-restore"))
        .args(["--selected", "--apply"])
        .arg(&f.profile)
        .args(["input", "extra-slot"])
        .arg(&dest)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!dest.exists());
    assert_eq!(action(f.selected(true, &dest)), "restored");
    let other = root.join("other.txt");
    let out = Command::new(env!("CARGO_BIN_EXE_git-asset-restore"))
        .args(["--apply", "--selected"])
        .arg(&f.profile)
        .arg("input")
        .arg(&other)
        .output()
        .unwrap();
    assert_eq!(action(out), "restored");
    assert_eq!(fs::read(other).unwrap(), fs::read(dest).unwrap());
}
#[test]
fn preview_apply_retry_and_exact_git_selection() {
    let f = Fixture::new();
    let dest = f.temp.path().join("restored.txt");
    fs::write(
        f.temp.path().join("records/input.asset"),
        b"uncommitted invalid metadata",
    )
    .unwrap();
    assert_eq!(action(f.run(false, &dest)), "would-restore");
    assert!(!dest.exists());
    assert_eq!(action(f.run(true, &dest)), "restored");
    assert_eq!(Object::from_bytes(&fs::read(&dest).unwrap()), f.object);
    assert_eq!(action(f.run(true, &dest)), "already-present");
    assert!(!fs::read_dir(f.temp.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".partial")
    }));
}
#[test]
fn conflicting_destination_is_preserved() {
    let f = Fixture::new();
    let dest = f.temp.path().join("restored.txt");
    fs::write(&dest, b"keep these bytes").unwrap();
    assert!(!f.run(true, &dest).status.success());
    assert_eq!(fs::read(&dest).unwrap(), b"keep these bytes");
}
#[test]
fn corrupt_and_missing_source_never_publish() {
    let f = Fixture::new();
    let dest = f.temp.path().join("restored.txt");
    fs::write(f.cache(), b"corrupt").unwrap();
    assert!(!f.run(true, &dest).status.success());
    assert!(!dest.exists());
    fs::remove_file(f.cache()).unwrap();
    assert!(!f.run(true, &dest).status.success());
    assert!(!dest.exists());
}
#[test]
fn missing_parents_and_traversal_refuse_without_creating_directories() {
    let f = Fixture::new();
    assert!(
        !f.run(true, &f.temp.path().join("missing/file"))
            .status
            .success()
    );
    assert!(!f.temp.path().join("missing").exists());
    assert!(
        !f.run(true, &f.temp.path().join("../escape"))
            .status
            .success()
    );
    assert!(!f.run(true, Path::new("relative.txt")).status.success());
}
#[cfg(unix)]
#[test]
fn linked_destination_parent_refuses() {
    let f = Fixture::new();
    std::os::unix::fs::symlink(f.temp.path(), f.temp.path().join("alias")).unwrap();
    assert!(
        !f.run(true, &f.temp.path().join("alias/restored"))
            .status
            .success()
    );
}

#[cfg(windows)]
#[test]
fn junction_destination_parent_refuses() {
    let f = Fixture::new();
    let alias = f.temp.path().join("alias");
    let output = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&alias)
        .arg(f.temp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!f.run(true, &alias.join("restored")).status.success());
    fs::remove_dir(&alias).unwrap();
}
