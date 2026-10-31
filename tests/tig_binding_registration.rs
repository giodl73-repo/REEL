#![cfg(feature = "tig")]
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tig_core::Object;
fn file(root: &Path, name: &str, bytes: &[u8]) -> Value {
    let path = root.join(name);
    fs::write(&path, bytes).unwrap();
    let pin = Object::from_bytes(bytes);
    json!({"path":path,"sha256":pin.sha256,"bytes":pin.bytes})
}
struct Fixture {
    root: tempfile::TempDir,
    profile: Value,
    source: Value,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("objects")).unwrap();
        let schema = b"true";
        let def=serde_json::to_vec(&json!({"schema":"tig.definition.design-1","id":"types/delivery","authored_schema":Object::from_bytes(schema),"authoring":{"properties":["language"],"required":["language"],"defaults":{}},"stream_rules":{},"semantic_rules":{}})).unwrap();
        let definition = json!({"definition":file(root.path(),"definition.json",&def),"authored_schema":file(root.path(),"schema.json",schema)});
        let asset = |bytes: &[u8], id: &str, state: &str| {
            let pin = Object::from_bytes(bytes);
            json!({"logical_id":id,"sha256":pin.sha256,"bytes":pin.bytes,"cache_uri":format!("cache://sha256/{}",pin.sha256.as_str()),"selection_state":state})
        };
        // Only the metadata manifest is present. Delivery and authority bytes
        // must never be fetched merely to register their exact pointers.
        let source = json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"video/example","assets":{
            "delivery/movie":asset(b"current movie unavailable","video/example/movie","selected-private-production"),
            "history/first/delivery/movie":asset(b"old movie unavailable","video/example/movie","retained-imported-evidence"),
            "authority/owner":asset(b"original private decision","authority/owner","original-scoped-decision")}});
        let profile = json!({"schema":reel::tig_binding_registration::SCHEMA,"source":Object::from_bytes(b"pending"),"scope_id":"video/example",
            "byte_source":{"places":[{"root":root.path(),"layout":"flat"}],"required":if cfg!(windows){"prevent-mutation"}else{"cooperative-immutable"}},
            "entries":{"delivery/movie":{"id":"video/example/movie","filename":"movie.tig","definition":definition,"authored":{"language":"es"},"history":{"first":"history/first/delivery/movie"},"authorities":["authority/owner"]}},"max_output_bytes":1048576});
        Self {
            root,
            profile,
            source,
        }
    }
    fn invoke(&mut self) -> std::process::Output {
        let raw = serde_json::to_vec(&self.source).unwrap();
        let pin = Object::from_bytes(&raw);
        fs::write(
            self.root.path().join("objects").join(pin.sha256.as_str()),
            raw,
        )
        .unwrap();
        self.profile["source"] = json!(pin);
        let path = self.root.path().join("profile.json");
        fs::write(&path, serde_json::to_vec(&self.profile).unwrap()).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_reel-tig-register"))
            .arg("--binding-assets")
            .arg(&path)
            .arg(self.root.path().join("candidates"))
            .env("PATH", "")
            .output()
            .unwrap();
        let logs = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        for private in [
            "video/example",
            "sha256",
            "original private decision",
            self.root.path().to_str().unwrap(),
        ] {
            assert!(!logs.contains(private));
        }
        result
    }
}
#[test]
fn cli_registers_current_history_and_original_authorities_without_delivery_reads() {
    let mut f = Fixture::new();
    let result = f.invoke();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let path = f.root.path().join("candidates/movie.tig");
    let before = fs::read(&path).unwrap();
    let record: Value = serde_json::from_slice(&before).unwrap();
    let selected = &f.source["assets"]["delivery/movie"];
    assert_eq!(record["selected"]["sha256"], selected["sha256"]);
    assert_eq!(
        record["streams"][1]["object"]["sha256"],
        f.source["assets"]["history/first/delivery/movie"]["sha256"]
    );
    assert_eq!(
        record["streams"][1]["provenance"]["source_binding"],
        "history/first/delivery/movie"
    );
    assert_eq!(
        record["metadata"]["source_authorities"]["authority/owner"]["object"]["sha256"],
        f.source["assets"]["authority/owner"]["sha256"]
    );
    assert_eq!(record["status"]["subject"], record["selected"]);
    assert_eq!(record["status"]["human_approval"], "not-reasserted");
    assert_eq!(
        record["metadata"]["binding_assets_registration"]["native_build"],
        "not-established"
    );
    assert!(!f.invoke().status.success());
    assert_eq!(fs::read(path).unwrap(), before);
}
#[test]
fn invalid_identity_scope_history_authorities_bounds_and_paths_refuse_without_candidates() {
    for case in [
        "scope",
        "rename",
        "wrong-history-id",
        "history-current",
        "missing-history",
        "missing-authority",
        "uri",
        "length-conflict",
        "traversal",
        "duplicate-file",
        "output-budget",
        "unknown-profile",
    ] {
        let mut f = Fixture::new();
        match case {
            "scope" => f.profile["scope_id"] = json!("foreign"),
            "rename" => f.profile["entries"]["delivery/movie"]["id"] = json!("renamed"),
            "wrong-history-id" => {
                f.source["assets"]["history/first/delivery/movie"]["logical_id"] =
                    json!("other/movie")
            }
            "history-current" => {
                f.profile["entries"]["delivery/movie"]["history"] =
                    json!({"current":"history/first/delivery/movie"})
            }
            "missing-history" => {
                f.profile["entries"]["delivery/movie"]["history"] = json!({"missing":"missing"})
            }
            "missing-authority" => {
                f.profile["entries"]["delivery/movie"]["authorities"] = json!(["missing"])
            }
            "uri" => f.source["assets"]["delivery/movie"]["cache_uri"] = json!("file:///private"),
            "length-conflict" => {
                f.source["assets"]["history/first/delivery/movie"]["sha256"] =
                    f.source["assets"]["delivery/movie"]["sha256"].clone();
                f.source["assets"]["history/first/delivery/movie"]["cache_uri"] =
                    f.source["assets"]["delivery/movie"]["cache_uri"].clone();
            }
            "traversal" => {
                f.profile["entries"]["delivery/movie"]["filename"] = json!("../movie.tig")
            }
            "duplicate-file" => {
                f.profile["entries"]["other"] = f.profile["entries"]["delivery/movie"].clone()
            }
            "output-budget" => f.profile["max_output_bytes"] = json!(1),
            _ => f.profile["future"] = json!(true),
        }
        let result = f.invoke();
        assert!(!result.status.success(), "{case}");
        assert!(!f.root.path().join("candidates").exists(), "{case}");
        assert_eq!(
            String::from_utf8_lossy(&result.stderr).trim(),
            "binding asset registration refused; verify source, scope, mappings, definitions, bounds and new destination"
        );
    }
}

#[test]
fn canonical_ids_require_complete_exact_source_aliases_and_preserve_each_old_name() {
    for case in ["complete", "partial", "wrong-source"] {
        let mut f = Fixture::new();
        f.source["assets"]["delivery/movie"]["logical_id"] = json!("video/example/v2/movie");
        f.source["assets"]["history/first/delivery/movie"]["logical_id"] =
            json!("video/example/v1/movie");
        f.profile["entries"]["delivery/movie"]["source_ids"] = json!({"delivery/movie":"video/example/v2/movie","history/first/delivery/movie":"video/example/v1/movie"});
        if case == "partial" {
            f.profile["entries"]["delivery/movie"]["source_ids"]
                .as_object_mut()
                .unwrap()
                .remove("history/first/delivery/movie");
        }
        if case == "wrong-source" {
            f.profile["entries"]["delivery/movie"]["source_ids"]["delivery/movie"] =
                json!("not-the-source-id");
        }
        let result = f.invoke();
        assert_eq!(result.status.success(), case == "complete", "{case}");
        if case == "complete" {
            let record: Value = serde_json::from_slice(
                &fs::read(f.root.path().join("candidates/movie.tig")).unwrap(),
            )
            .unwrap();
            assert_eq!(record["id"], "video/example/movie");
            assert_eq!(
                record["streams"][0]["provenance"]["source_logical_id"],
                "video/example/v2/movie"
            );
            assert_eq!(
                record["streams"][1]["provenance"]["source_logical_id"],
                "video/example/v1/movie"
            );
            assert_eq!(record["streams"][2]["object"], f.profile["source"]);
        }
    }
}

#[test]
fn large_source_manifest_remains_an_asset_while_native_records_stay_small() {
    let mut f = Fixture::new();
    let template = f.source["assets"]["delivery/movie"].clone();
    for n in 0..400 {
        f.source["assets"][format!("unused/{n:04}")] = template.clone();
    }
    assert!(serde_json::to_vec(&f.source).unwrap().len() > 65536);
    let result = f.invoke();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = fs::read(f.root.path().join("candidates/movie.tig")).unwrap();
    assert!(bytes.len() < 65536);
    let record: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        record["streams"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["slot"] == "source-bindings"
                && s["object"]["bytes"].as_u64().unwrap() > 65536)
    );
}
