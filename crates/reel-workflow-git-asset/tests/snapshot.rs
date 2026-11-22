//! Synthetic metadata only. No native cache enrollment, hydration or removal.
use git_asset::source::{Limits, Repository};
use git_asset_core::Object;
use reel_workflow::{Artifact, ErrorKind};
use reel_workflow_git_asset::{Preview, Snapshot};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
struct Fixture {
    root: tempfile::TempDir,
    exe: PathBuf,
    definition: Value,
}
impl Fixture {
    fn git(&self, args: &[&str]) -> String {
        let out = Command::new(&self.exe)
            .current_dir(self.root.path())
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .unwrap();
        assert!(out.status.success(), "synthetic git command failed");
        String::from_utf8(out.stdout).unwrap().trim().into()
    }
    fn new() -> Self {
        let exe = std::env::var_os("GIT_ASSET_TEST_GIT_EXE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if cfg!(windows) {
                    PathBuf::from("C:/Program Files/Git/cmd/git.exe")
                } else {
                    PathBuf::from("/usr/bin/git")
                }
            });
        let mut f = Self {
            root: tempfile::tempdir().unwrap(),
            exe,
            definition: Value::Null,
        };
        f.git(&["init", "-b", "main"]);
        f.git(&["config", "user.name", "Fixture"]);
        f.git(&["config", "user.email", "fixture@example.invalid"]);
        f.git(&["config", "commit.gpgsign", "false"]);
        f.git(&["config", "core.hooksPath", "no-hooks"]);
        for d in ["records", "definitions", "unrelated"] {
            fs::create_dir(f.root.path().join(d)).unwrap();
        }
        let authored = write(
            f.root.path(),
            "definitions/authored.json",
            &json!({"type":"object"}),
        );
        let definition = write(
            f.root.path(),
            "definitions/type.json",
            &json!({
                "schema":"tig.definition.design-1","id":"types/image","authored_schema":authored,
                "authoring":{"properties":[],"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}
            }),
        );
        f.definition = serde_json::to_value(definition).unwrap();
        f.definition["id"] = json!("types/image");
        // Invalid unrelated catalogue bytes must never enter the declared scope.
        fs::write(
            f.root.path().join("unrelated/broken.asset"),
            b"invalid unrelated",
        )
        .unwrap();
        f
    }
    fn record(&self, selected: Option<&[u8]>) {
        write(
            self.root.path(),
            "records/item.asset",
            &json!({
                "schema":"tig.instance.design-1","id":"image/item","definition":self.definition,"authored":{},
                "streams":[{"slot":"preview","role":"review","group":"draft","object":Object::from_bytes(b"preview")}],
                "selected":selected.map(Object::from_bytes),"status":{},"metadata":{}
            }),
        );
    }
    fn commit(&self) -> String {
        self.git(&["add", "."]);
        self.git(&["commit", "-m", "synthetic"]);
        self.git(&["rev-parse", "HEAD"])
    }
    fn capture(&self, commit: &str) -> reel_workflow::Result<Snapshot> {
        let r =
            Repository::new(self.exe.clone(), self.root.path().into(), Limits::default()).unwrap();
        Snapshot::capture(
            &r,
            commit,
            &["records/item.asset".into()],
            &["definitions".into()],
        )
    }
}
fn write(root: &Path, path: &str, value: &Value) -> Object {
    let bytes = serde_json::to_vec(value).unwrap();
    fs::write(root.join(path), &bytes).unwrap();
    Object::from_bytes(&bytes)
}
fn artifact(bytes: &[u8]) -> Artifact {
    let o = Object::from_bytes(bytes);
    Artifact {
        logical_id: "image/item".into(),
        sha256: o.sha256.as_str().into(),
        bytes: o.bytes,
        media_type: "image/png".into(),
    }
}
fn selectors(stream: Option<&str>) -> BTreeMap<String, Option<String>> {
    BTreeMap::from([("image/item".into(), stream.map(str::to_owned))])
}
fn preview(s: &Snapshot, bytes: &[u8]) -> Preview {
    s.preview(&[artifact(bytes)], &selectors(None), 1024)
        .unwrap()
}
#[test]
fn historical_commit_ignores_current_and_uncommitted_selected_bytes() {
    let f = Fixture::new();
    f.record(Some(b"old"));
    let old = f.commit();
    f.record(Some(b"new"));
    let new = f.commit();
    f.record(Some(b"uncommitted"));
    let old_snapshot = f.capture(&old).unwrap();
    let old_preview = preview(&old_snapshot, b"old");
    assert_eq!(old_preview.commit, old);
    assert_eq!(old_preview.logical_bytes, 3);
    let new_snapshot = f.capture(&new).unwrap();
    assert_eq!(preview(&new_snapshot, b"new").commit, new);
    assert_eq!(
        old_snapshot
            .preview(&[artifact(b"new")], &selectors(None), 1024)
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
    assert_eq!(
        new_snapshot
            .verify_preview(&old_preview, 1024)
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
    assert!(f.capture("HEAD").is_err());
    assert!(f.capture("main").is_err());
    assert!(f.capture(&"f".repeat(40)).is_err());
}
#[test]
fn missing_selection_or_stream_never_falls_back() {
    let f = Fixture::new();
    f.record(None);
    let commit = f.commit();
    let s = f.capture(&commit).unwrap();
    assert_eq!(
        s.preview(&[artifact(b"preview")], &selectors(None), 1024)
            .unwrap_err()
            .kind,
        ErrorKind::Missing
    );
    assert_eq!(
        s.preview(&[artifact(b"preview")], &selectors(Some("absent")), 1024)
            .unwrap_err()
            .kind,
        ErrorKind::Missing
    );
    assert_eq!(
        s.preview(&[artifact(b"preview")], &selectors(Some("preview")), 1024)
            .unwrap()
            .inputs[0]
            .object,
        Object::from_bytes(b"preview")
    );
}
#[test]
fn decoded_preview_requires_exact_source_rebinding() {
    let f = Fixture::new();
    f.record(Some(b"old"));
    let c = f.commit();
    let s = f.capture(&c).unwrap();
    let p = preview(&s, b"old");
    s.verify_preview(
        &serde_json::from_slice::<Preview>(&serde_json::to_vec(&p).unwrap()).unwrap(),
        1024,
    )
    .unwrap();
    let mut altered = p.clone();
    altered.checkpoint = Object::from_bytes(b"forged").sha256;
    assert_eq!(
        s.verify_preview(&altered, 1024).unwrap_err().kind,
        ErrorKind::Stale
    );
    let mut altered = p.clone();
    altered.inputs[0].record = Object::from_bytes(b"forged record");
    assert_eq!(
        s.verify_preview(&altered, 1024).unwrap_err().kind,
        ErrorKind::Stale
    );
    let mut altered = p.clone();
    altered.logical_bytes += 1;
    assert_eq!(
        s.verify_preview(&altered, 1024).unwrap_err().kind,
        ErrorKind::Stale
    );
    let mut altered = p;
    altered.inputs[0].artifact.media_type = "invalid".into();
    assert_eq!(
        s.verify_preview(&altered, 1024).unwrap_err().kind,
        ErrorKind::Invalid
    );
}
#[test]
fn identity_and_budget_refusals_do_not_read_payloads() {
    let f = Fixture::new();
    f.record(Some(b"old"));
    let c = f.commit();
    let s = f.capture(&c).unwrap();
    let a = artifact(b"old");
    assert_eq!(
        s.preview(std::slice::from_ref(&a), &selectors(None), 2)
            .unwrap_err()
            .kind,
        ErrorKind::Budget
    );
    assert_eq!(
        s.preview(&[a.clone(), a.clone()], &selectors(None), 1024)
            .unwrap_err()
            .kind,
        ErrorKind::Invalid
    );
    assert_eq!(
        s.preview(std::slice::from_ref(&a), &BTreeMap::new(), 1024)
            .unwrap_err()
            .kind,
        ErrorKind::Invalid
    );
    let mut bad = a.clone();
    bad.bytes += 1;
    assert_eq!(
        s.preview(&[bad], &selectors(None), 1024).unwrap_err().kind,
        ErrorKind::Conflict
    );
    let mut bad = a;
    bad.logical_id = "../escape".into();
    assert_eq!(
        s.preview(&[bad], &selectors(None), 1024).unwrap_err().kind,
        ErrorKind::Invalid
    );
    let r = Repository::new(f.exe.clone(), f.root.path().into(), Limits::default()).unwrap();
    assert!(Snapshot::capture(&r, &c, &["../escape".into()], &["definitions".into()]).is_err());
}

#[test]
fn bounded_decode_is_not_a_custody_or_source_acknowledgement() {
    assert_eq!(
        Preview::decode(&vec![b' '; 128 * 1024 + 1])
            .unwrap_err()
            .kind,
        ErrorKind::Budget
    );
    assert_eq!(Preview::decode(b"{}").unwrap_err().kind, ErrorKind::Invalid);
    let f = Fixture::new();
    f.record(Some(b"old"));
    let c = f.commit();
    let s = f.capture(&c).unwrap();
    let p = preview(&s, b"old");
    let mut v = serde_json::to_value(p).unwrap();
    v["extra"] = json!(true);
    assert_eq!(
        Preview::decode(&serde_json::to_vec(&v).unwrap())
            .unwrap_err()
            .kind,
        ErrorKind::Invalid
    );
}
