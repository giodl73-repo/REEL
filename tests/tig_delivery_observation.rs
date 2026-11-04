#![cfg(feature = "tig-history")]
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tig_core::Object;

fn git_executable() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join(if cfg!(windows) { "git.exe" } else { "git" }))
        .find(|p| p.is_file())
        .unwrap()
        .canonicalize()
        .unwrap()
}
fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new(git_executable())
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
fn write(root: &Path, name: &str, value: &Value) -> Object {
    let raw = serde_json::to_vec(value).unwrap();
    fs::write(root.join(name), &raw).unwrap();
    Object::from_bytes(&raw)
}
fn local(root: &Path, name: &str, pin: &Object) -> Value {
    json!({"path":root.join(name),"sha256":pin.sha256,"bytes":pin.bytes})
}
#[test]
fn committed_receipts_reconcile_without_payloads_and_never_imply_remote_verification() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for path in ["assets", "definitions", "cache/objects"] {
        fs::create_dir_all(root.join(path)).unwrap();
    }
    let payload = Object::from_bytes(b"NO MEDIA!");
    let receipt_raw =
        serde_json::to_vec(&json!({"files":[{"path":"Shelf/film.mp4","asset":payload}]})).unwrap();
    let receipt = Object::from_bytes(&receipt_raw);
    fs::write(
        root.join("cache/objects").join(receipt.sha256.as_str()),
        &receipt_raw,
    )
    .unwrap();
    // Deliberately no payload object in cache. Only the named metadata is readable.
    let schema = write(
        root,
        "definitions/output.authored.json",
        &json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":{"target":{"type":"object"}},"required":["target"],"additionalProperties":false}),
    );
    let definition = write(
        root,
        "definitions/output.json",
        &json!({"schema":"tig.definition.design-1","id":"types/output","authored_schema":schema,"authoring":{"properties":["target"],"required":["target"],"defaults":{}},"stream_rules":{"profile":"fixture.output.v1"},"semantic_rules":{"profile":"fixture.output.v1"}}),
    );
    write(
        root,
        "assets/film.tig",
        &json!({"schema":"tig.instance.design-1","id":"film","definition":{"id":"types/output","sha256":definition.sha256,"bytes":definition.bytes},"authored":{"target":{"provider":"files","scope":"review","receipt_slot":"receipt","receipt_selector":"/files/0"}},"selected":payload,"streams":[{"slot":"retained","role":"delivery/file","group":"retained","object":payload},{"slot":"receipt","role":"evidence/delivery-receipt","group":"source","object":receipt}],"status":{},"metadata":{}}),
    );
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "Fixture"]);
    git(root, &["config", "user.email", "fixture@example.invalid"]);
    git(root, &["config", "core.autocrlf", "false"]);
    git(root, &["add", "assets", "definitions"]);
    git(root, &["commit", "-qm", "frozen targets"]);
    let commit = git(root, &["rev-parse", "HEAD"]);
    // A dirty checkout cannot replace the frozen committed target.
    fs::write(root.join("assets/film.tig"), b"not the committed record").unwrap();
    let policy = write(
        root,
        "policy.json",
        &json!({"schema":"tig.publication-evidence-policy.experimental-1","target_pointer":"/target","subject_slot":"retained","subject_role":"delivery/file","subject_selection":"declared-stream","receipt_role":"evidence/delivery-receipt","limits":{"bytes":65536,"nodes":8192,"depth":32},"rules":[{"kind":"array-rows","root":"/files","fields":{"sha256":"/asset/sha256","bytes":"/asset/bytes","target":"/path"}}]}),
    );
    let rows = b"{\"relative_path\":\"Shelf/film.mp4\",\"bytes\":9}\n{\"relative_path\":\"Other/film.mp4\",\"bytes\":9}\n{\"relative_path\":\"desktop.ini\",\"bytes\":1}\n";
    fs::write(root.join("inventory.jsonl"), rows).unwrap();
    let inventory = Object::from_bytes(rows);
    let mut profile = json!({"schema":"reel.tig-delivery-observation.experimental-1","source":{"schema":"reel.tig-input-profile.experimental-1","git_executable":git_executable(),"repository":root,"commit":commit,"records":["assets"],"definitions":["definitions"],"admitted":[],"byte_source":{"places":[{"root":root.join("cache"),"layout":"flat"}],"required":"cooperative-immutable"}},"inventory":local(root,"inventory.jsonl",&inventory),"policies":{"receipt":local(root,"policy.json",&policy)},"mappings":[{"provider":"files","scope":"review","definition_id":"types/output","policies":["receipt"],"observation_prefix":"","receipt_prefix":""}],"client_metadata_basenames":["desktop.ini"],"max_report_bytes":65536});
    write(root, "profile.json", &profile);
    let output = root.join("report.json");
    let summary = reel::tig_delivery_observation::run(&root.join("profile.json"), &output).unwrap();
    assert_eq!(summary["verified_retained_bindings"], 1);
    assert_eq!(summary["counts"]["retained-target-length-matches"], 1);
    assert_eq!(summary["counts"]["unmatched"], 1);
    assert_eq!(summary["counts"]["client-metadata"], 1);
    assert_eq!(summary["payload_verified"], false);
    assert_eq!(summary["current_remote_content_verified"], false);
    assert_eq!(summary["authorizes_eviction"], false);
    let saved = fs::read(&output).unwrap();
    assert!(reel::tig_delivery_observation::run(&root.join("profile.json"), &output).is_err());
    assert_eq!(fs::read(&output).unwrap(), saved);

    // One pinned external table can carry many exact associations without
    // expanding the bounded profile or inferring identity from filenames.
    let prefix_profile = profile.clone();
    let mut table_rows =
        vec![json!({"observed":"C:\\Local\\Shelf\\film.mp4","target":"Shelf/film.mp4"})];
    for i in 0..146 {
        table_rows.push(json!({"observed":format!("C:\\Local\\Shelf\\extra-{i}.mp4"),"target":format!("Shelf/extra-{i}.mp4")}));
    }
    let table = json!({"rows":table_rows,"uninterpreted":"x".repeat(70_000)});
    let table_pin = write(root, "correspondence.json", &table);
    assert!(table_pin.bytes > 65_536);
    profile["mappings"][0]["correspondence"] = json!({
        "table":local(root,"correspondence.json",&table_pin),
        "rows_pointer":"/rows","observed_path_pointer":"/observed",
        "receipt_target_pointer":"/target","observed_path_prefix":"C:\\Local\\",
        "backslash_separator":true
    });
    write(root, "profile.json", &profile);
    let mapped = reel::tig_delivery_observation::run(
        &root.join("profile.json"),
        &root.join("table-report.json"),
    )
    .unwrap();
    assert_eq!(mapped["verified_retained_bindings"], 1);
    assert_eq!(mapped["counts"]["retained-target-length-matches"], 1);
    assert_eq!(mapped["counts"]["unmatched"], 1);
    assert_eq!(mapped["payload_verified"], false);
    assert!(
        !root
            .join("cache/objects")
            .join(payload.sha256.as_str())
            .exists()
    );

    let refuse = |profile: &Value, name: &str| {
        write(root, "profile.json", profile);
        let report = root.join(name);
        assert!(reel::tig_delivery_observation::run(&root.join("profile.json"), &report).is_err());
        assert!(!report.exists());
    };
    let mut wrong_pin = profile.clone();
    wrong_pin["mappings"][0]["correspondence"]["table"]["sha256"] =
        json!(Object::from_bytes(b"wrong").sha256);
    refuse(&wrong_pin, "wrong-table-pin.json");
    let mut conflicting = profile.clone();
    conflicting["mappings"][0]["receipt_prefix"] = json!("not-ignored/");
    refuse(&conflicting, "conflicting-table-mode.json");
    let mut bad_pointer = profile.clone();
    bad_pointer["mappings"][0]["correspondence"]["rows_pointer"] = json!("/rows~2");
    refuse(&bad_pointer, "invalid-table-pointer.json");
    for (case, mut changed) in [
        ("duplicate-observation", table.clone()),
        ("duplicate-target", table.clone()),
        ("outside-prefix", table.clone()),
        ("traversal", table.clone()),
        ("missing-target", table.clone()),
    ] {
        match case {
            "duplicate-observation" => {
                changed["rows"][1]["observed"] = changed["rows"][0]["observed"].clone()
            }
            "duplicate-target" => {
                changed["rows"][1]["target"] = changed["rows"][0]["target"].clone()
            }
            "outside-prefix" => {
                changed["rows"][0]["observed"] = json!("D:\\Local\\Shelf\\film.mp4")
            }
            "traversal" => changed["rows"][0]["observed"] = json!("C:\\Local\\../film.mp4"),
            "missing-target" => {
                changed["rows"][0].as_object_mut().unwrap().remove("target");
            }
            _ => unreachable!(),
        }
        let pin = write(root, "changed-table.json", &changed);
        let mut candidate = profile.clone();
        candidate["mappings"][0]["correspondence"]["table"] =
            local(root, "changed-table.json", &pin);
        refuse(&candidate, &format!("refused-{case}.json"));
    }
    let duplicate_keys = br#"{"rows":[],"rows":[]}"#;
    fs::write(root.join("duplicate-keys.json"), duplicate_keys).unwrap();
    let mut candidate = profile.clone();
    candidate["mappings"][0]["correspondence"]["table"] = local(
        root,
        "duplicate-keys.json",
        &Object::from_bytes(duplicate_keys),
    );
    refuse(&candidate, "refused-duplicate-keys.json");
    profile = prefix_profile;
    write(root, "profile.json", &profile);
    fs::write(
        root.join("cache/objects").join(receipt.sha256.as_str()),
        b"corrupt receipt",
    )
    .unwrap();
    let degraded = reel::tig_delivery_observation::run(
        &root.join("profile.json"),
        &root.join("corrupt-report.json"),
    )
    .unwrap();
    assert_eq!(degraded["verified_retained_bindings"], 0);
    assert_eq!(degraded["binding_gaps"], 1);
    fs::write(root.join("inventory.jsonl"), b"changed").unwrap();
    assert!(
        reel::tig_delivery_observation::run(
            &root.join("profile.json"),
            &root.join("bad-inventory.json")
        )
        .is_err()
    );
    assert!(!root.join("bad-inventory.json").exists());
    // Exact inventory duplicate rows must refuse rather than double-count delivery.
    let duplicate = [rows.as_slice(), rows.as_slice()].concat();
    fs::write(root.join("inventory.jsonl"), &duplicate).unwrap();
    profile["inventory"] = local(root, "inventory.jsonl", &Object::from_bytes(&duplicate));
    write(root, "profile.json", &profile);
    assert!(
        reel::tig_delivery_observation::run(
            &root.join("profile.json"),
            &root.join("duplicate-report.json")
        )
        .is_err()
    );
}
