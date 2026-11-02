#![cfg(feature = "tig-history")]
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tig_core::Object;

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}
fn write(root: &Path, path: &str, value: Value) -> Object {
    let raw = serde_json::to_vec(&value).unwrap();
    fs::write(root.join(path), &raw).unwrap();
    Object::from_bytes(&raw)
}
struct Fixture {
    temp: tempfile::TempDir,
    repo: PathBuf,
    cache: PathBuf,
    profile: Value,
    request: Value,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let repo = root.join("source");
        for path in [
            &repo,
            &root.join("artifact"),
            &root.join("coordination"),
            &root.join("cache/objects/sha256"),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        fs::create_dir(repo.join("records")).unwrap();
        fs::create_dir(repo.join("definitions")).unwrap();
        git(&repo, &["init", "-b", "main"]);
        let schema = write(
            &repo,
            "definitions/authored.json",
            json!({"type":"object","required":["text"]}),
        );
        let definition = write(
            &repo,
            "definitions/type.json",
            json!({"schema":"tig.definition.design-1","id":"fixture/document",
            "authored_schema":schema,"authoring":{"properties":["text"],"required":["text"],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
        );
        let mut pin = serde_json::to_value(definition).unwrap();
        pin["id"] = json!("fixture/document");
        let expected = write(
            &repo,
            "records/item.tig",
            json!({"schema":"tig.instance.design-1","id":"document/1","definition":pin,
            "authored":{"text":"synthetic private text"},"selected":null,"streams":[],"metadata":{"rights":"synthetic"},"status":{"stage":"draft"}}),
        );
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "fixture"]);
        let base = git(&repo, &["rev-parse", "HEAD"]);
        let raw=serde_json::to_vec(&json!({"schema":"tig.asset-revision.experimental-1","asset":"document/1","revision":"run-a","parents":[],
            "representations":[{"key":"main","object":Object::from_bytes(b"not copied media")}],"provenance":{"kind":"imported","source":Object::from_bytes(b"synthetic import")}})).unwrap();
        fs::write(root.join("revision.json"), &raw).unwrap();
        let exe = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join(if cfg!(windows) { "git.exe" } else { "git" }))
            .find(|p| p.is_absolute() && p.is_file())
            .unwrap();
        let cache = root.join("cache");
        let protection = if cfg!(windows) {
            "prevent-mutation"
        } else {
            "cooperative-immutable"
        };
        let profile = json!({"schema":"tig.history-command.experimental-1","git_executable":exe,"repository":repo,"base":base,
            "records":["records"],"definitions":["definitions"],"asset":"document/1","expected":expected,"dialect":"2020-12",
            "byte_source":{"places":[{"root":cache,"layout":"reel-cas"}],"required":protection},
            "publication":{"root":cache,"layout":"reel-cas","operation":"register-1","owner":"fixture-owner","required":"process_restart",
                "settings":{"schema":"reel.history-publisher.experimental-1","artifact_root":root.join("artifact"),"namespace":"fixture-history",
                    "storage":{"path_policy":"configured-local-roots","cache_roots":[cache],"coordination_root":root.join("coordination"),
                        "capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0,"max_object_bytes":1048576}},"protection":protection}},
            "proposal":{"path":"records/item.tig","identity":{"name":"Fixture","email":"fixture@example.invalid","unix_seconds":1700000000},"message":"register history"}});
        let request = json!({"operation":"register-revision","document":{"path":"revision.json","expected":Object::from_bytes(&raw)},"coverage":"complete"});
        Self {
            temp,
            repo,
            cache,
            profile,
            request,
        }
    }
    fn run(&self, action: &str) -> std::process::Output {
        fs::write(
            self.temp.path().join("profile.json"),
            serde_json::to_vec(&self.profile).unwrap(),
        )
        .unwrap();
        fs::write(
            self.temp.path().join("request.json"),
            serde_json::to_vec(&self.request).unwrap(),
        )
        .unwrap();
        Command::new(env!("CARGO_BIN_EXE_reel-tig-history"))
            .arg(action)
            .arg(self.temp.path().join("profile.json"))
            .arg(self.temp.path().join("request.json"))
            .output()
            .unwrap()
    }
    fn ok(&self, action: &str) -> Value {
        let out = self.run(action);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8(out.stdout).unwrap();
        for s in ["synthetic private text", "fixture-owner", "document/1"] {
            assert!(!text.contains(s));
        }
        assert!(!text.contains(&self.cache.to_string_lossy().to_string()));
        serde_json::from_str(&text).unwrap()
    }
}

#[test]
fn existing_reel_cache_host_checks_publishes_reopens_and_proposes_without_new_cache_or_media() {
    let mut f = Fixture::new();
    let head = git(&f.repo, &["rev-parse", "HEAD"]);
    let original = fs::read(f.repo.join("records/item.tig")).unwrap();
    let check = f.ok("check");
    assert_eq!(check["published"], false);
    assert!(
        fs::read_dir(f.cache.join("objects/sha256"))
            .unwrap()
            .next()
            .is_none()
    );
    assert!(
        fs::read_dir(f.temp.path().join("coordination"))
            .unwrap()
            .next()
            .is_none()
    );
    let proposed = f.ok("propose");
    let retry = f.ok("propose");
    assert_eq!(proposed["proposal"], retry["proposal"]);
    assert_eq!(proposed["new_metadata_readback_verified"], true);
    assert_eq!(proposed["old_protection_released"], false);
    for object in proposed["metadata"]
        .as_array()
        .unwrap()
        .iter()
        .chain([&proposed["record"]])
    {
        let object: Object = serde_json::from_value(object.clone()).unwrap();
        let path = reel_assets::object(&f.cache, object.sha256.as_str());
        assert_eq!(Object::from_bytes(&fs::read(path).unwrap()), object);
    }
    assert!(
        !reel_assets::object(
            &f.cache,
            Object::from_bytes(b"not copied media").sha256.as_str()
        )
        .exists()
    );
    assert!(!f.cache.join("store.json").exists());
    assert_eq!(git(&f.repo, &["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read(f.repo.join("records/item.tig")).unwrap(), original);
    git(
        &f.repo,
        &[
            "cherry-pick",
            proposed["proposal"]["commit"].as_str().unwrap(),
        ],
    );
    f.profile["base"] = json!(git(&f.repo, &["rev-parse", "HEAD"]));
    f.profile["expected"] = proposed["record"].clone();
    let noop = f.ok("propose");
    assert_eq!(noop["noop"], true);
    assert!(noop["proposal"].is_null());
    let changed: Value =
        serde_json::from_slice(&fs::read(f.repo.join("records/item.tig")).unwrap()).unwrap();
    let old: Value = serde_json::from_slice(&original).unwrap();
    for field in ["selected", "authored", "status", "metadata"] {
        if field == "metadata" {
            assert_eq!(changed[field]["rights"], old[field]["rights"]);
        } else {
            assert_eq!(changed[field], old[field]);
        }
    }
}

#[test]
fn check_does_not_enroll_and_bad_host_settings_refuse_without_leaking_or_creating_roots() {
    let mut f = Fixture::new();
    let missing = f.temp.path().join("not-created");
    f.profile["publication"]["settings"]["artifact_root"] = json!(missing);
    f.ok("check");
    assert!(!missing.exists());
    let out = f.run("propose");
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let error = String::from_utf8(out.stderr).unwrap();
    assert!(error.contains("Invalid"));
    assert!(!error.contains(&missing.to_string_lossy().to_string()));
    assert!(!missing.exists());
    assert!(
        fs::read_dir(f.cache.join("objects/sha256"))
            .unwrap()
            .next()
            .is_none()
    );
    f.profile["publication"]["settings"]["artifact_root"] = json!(f.temp.path().join("artifact"));
    f.profile["publication"]["settings"]["unknown"] = json!(true);
    let out = f.run("publish");
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(
        fs::read_dir(f.cache.join("objects/sha256"))
            .unwrap()
            .next()
            .is_none()
    );
}
