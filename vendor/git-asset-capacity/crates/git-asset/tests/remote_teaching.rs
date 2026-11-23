#![cfg(all(feature = "schema", feature = "upload"))]
use git_asset::catalogue::{Catalogue, Config};
use git_asset_core::Object;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
#[path = "common/catalogue_server.rs"]
mod catalogue_server;
fn save(root: &Path, value: &Value) -> Value {
    let raw = git_asset_core::identity::canonical_json(value).unwrap();
    let o = Object::from_bytes(&raw);
    fs::write(
        git_asset_store::existing::Layout::Flat
            .object_path(&root.join("lesson-evidence"), &o.sha256),
        raw,
    )
    .unwrap();
    json!(o)
}
fn render(book: &Value) {
    let mut p = Command::new("node")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/ux/recorded-lesson.cjs"
        ))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    p.stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(book).unwrap())
        .unwrap();
    let out = p.wait_with_output().unwrap();
    assert!(out.status.success(), "{out:?}");
}
fn rebind_plans(v: &mut Value) {
    for i in 0..2 {
        let p: git_asset::upload::Plan = serde_json::from_value(v["plans"][i].clone()).unwrap();
        let id = json!(p.id().unwrap());
        v["peaks"][i]["plan"] = id.clone();
        if v["cli"]["apply"][i]["schema"] == "tig.lfs-upload-receipt.design-1" {
            v["cli"]["apply"][i]["plan"] = id.clone();
        }
        if v["cli"]["status"].is_array() {
            v["cli"]["status"][i]["plan"] = id;
        }
    }
}
#[test]
fn actual_remote_publication_disabled_hold_recheck_and_strict_recorded_view() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("remote publication with spaces");
    let out = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-remote-publication")
        .arg(&root)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["completed_checkpoints"], 14);
    assert_eq!(result["complete_course"], false);
    let endpoint = &result["remote_publication_evidence"]["receiver_at_shutdown"];
    assert_eq!(endpoint["put_count"], 2);
    assert_eq!(endpoint["put_payload_bytes"], 54);
    assert_eq!(endpoint["get_payload_bytes"], 270);
    assert_eq!(endpoint["requests"], 28);
    assert_eq!(endpoint["retained_custody"], false);
    let server = catalogue_server::Server::new(&root.join("catalogue.json"));
    let (code, book) = server.json("/api/lessons");
    assert_eq!(code, 200);
    assert_eq!(book["schema"], "tig.catalogue-teaching.design-7");
    assert_eq!(book["lessons"].as_array().unwrap().len(), 14);
    for (i, phase, put, readback) in [
        (10, "upload_disabled", 0, "not_started"),
        (11, "payload_published", 54, "verified"),
        (12, "readback_held", 0, "held"),
        (13, "publication_rechecked", 0, "verified"),
    ] {
        let l = &book["lessons"][i];
        assert_eq!(l["remote_publication"]["phase"], phase);
        assert_eq!(l["remote_publication"]["put_payload_bytes_this_step"], put);
        assert_eq!(l["remote_publication"]["readback_state"], readback);
        assert_eq!(l["stream_bytes"], 0);
        assert_eq!(l["remote_publication"]["retained_custody"], false);
        assert_eq!(l["remote_publication"]["removal_authority"], false);
    }
    let encoded = serde_json::to_string(&book).unwrap();
    for private in [
        "127.0.0.1",
        "batch_url",
        "source_root",
        "witness_root",
        "ledger_root",
        "\\\\?\\",
    ] {
        assert!(!encoded.contains(private), "exported {private}");
    }
    render(&book);
    drop(server);
    for (text, roles, remote) in [
        (
            b"Shared reference grid v1\n".as_slice(),
            vec!["working", "backup"],
            true,
        ),
        (
            b"Measurement series: 2,4,8,32\n".as_slice(),
            vec!["working", "backup"],
            true,
        ),
        (
            b"Measurement series: 2,4,8,16\n".as_slice(),
            vec!["history", "backup"],
            false,
        ),
    ] {
        let o = Object::from_bytes(text);
        for role in ["working", "history", "backup", "remote-receiver"] {
            let path =
                git_asset_store::existing::Layout::Flat.object_path(&root.join(role), &o.sha256);
            if roles.contains(&role) || (role == "remote-receiver" && remote) {
                assert_eq!(fs::read(path).unwrap(), text);
            } else {
                assert!(!path.exists());
            }
        }
        let input = git_asset_store::existing::Layout::GitLfs
            .object_path(&root.join("remote-upload/source"), &o.sha256);
        if remote {
            assert_eq!(fs::read(input).unwrap(), text);
        } else {
            assert!(!input.exists());
        }
    }
    let ids = [
        "remote-upload-disabled",
        "remote-payload-published",
        "remote-readback-held",
        "remote-publication-rechecked",
    ];
    let reports = ids.map(|id| {
        serde_json::from_slice::<Value>(&fs::read(root.join(format!("reports/{id}.json"))).unwrap())
            .unwrap()
    });
    let config: Value =
        serde_json::from_slice(&fs::read(root.join("catalogue.json")).unwrap()).unwrap();
    let trace: Value = serde_json::from_slice(&fs::read(root.join("trace.json")).unwrap()).unwrap();
    for bad in [
        "source_hash",
        "source_context",
        "witness_store",
        "authority_tree",
        "native_root",
        "selected_original",
        "provider_order",
        "custody",
        "authority",
        "synthetic",
        "seed",
        "staging",
        "previous",
        "local_copy",
        "local_ledger",
        "local_plan",
        "local_status",
        "local_required_partial",
        "disabled_http",
        "disabled_success",
        "unauthorized_success",
        "receiver_custody",
        "receiver_object",
        "publish_put",
        "publish_missing",
        "publish_status",
        "receipt_io",
        "receipt_plan",
        "hold_wrong_error",
        "hold_success",
        "hold_write",
        "recheck_write",
        "recheck_put",
        "get_counter",
        "receiver_capacity",
        "receiver_toggle",
        "witness_hold",
        "witness_identity",
    ] {
        let mut v = reports.clone();
        let foreign = json!(git_asset_core::Digest::blob(b"foreign remote proof"));
        match bad {
            "source_hash" => {
                v[0]["source_checkpoint"]["hash"] = foreign.clone();
                v[0]["local_before"]["source"]["hash"] = foreign.clone();
                v[0]["local_after"]["source"]["hash"] = foreign;
            }
            "source_context" => {
                for p in v[0]["plans"].as_array_mut().unwrap() {
                    p["source"] = foreign.clone();
                    p["context"] = foreign.clone();
                }
            }
            "witness_store" => {
                for p in v[0]["plans"].as_array_mut().unwrap() {
                    p["witness"]["instance"] = foreign.clone();
                }
            }
            "authority_tree" => {
                v[0]["authority"]["sha256"] = foreign.clone();
                let a = v[0]["authority"].clone();
                for p in v[0]["plans"].as_array_mut().unwrap() {
                    p["authority"] = a.clone();
                }
            }
            "native_root" => v[0]["native_selection"]["sha256"] = foreign,
            "selected_original" => {
                v[0]["selected"][1] = json!(Object::from_bytes(b"Measurement series: 2,4,8,16\n"))
            }
            "provider_order" => v[0]["providers"].as_array_mut().unwrap().reverse(),
            "custody" => v[0]["retained_custody"] = true.into(),
            "authority" => v[0]["removal_authority"] = true.into(),
            "synthetic" => v[0]["synthetic_endpoint"] = false.into(),
            "seed" => v[0]["seed_disabled"] = false.into(),
            "staging" => v[0]["fixture_input_staging_bytes"] = 55.into(),
            "previous" => v[0]["previous_report"]["sha256"] = foreign,
            "local_copy" => {
                v[0]["local_before"]["copies"]["history"]
                    .as_object_mut()
                    .unwrap()
                    .remove("original");
                v[0]["local_after"]["copies"]["history"]
                    .as_object_mut()
                    .unwrap()
                    .remove("original");
            }
            "local_ledger" => v[0]["local_after"]["ledgers"][0]["sequence"] = 999.into(),
            "local_plan" => v[0]["local_plan"]["enrollment"] = foreign,
            "local_status" => v[0]["local_status"]["storage_converged"] = false.into(),
            "local_required_partial" => {
                let row = v[0]["local_status"]["observations"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|o| o["required"] == true)
                    .unwrap();
                row["state"] = "partial".into();
            }
            "disabled_http" => v[0]["endpoint_after"]["requests"] = 1.into(),
            "disabled_success" => v[0]["cli"]["apply"][0] = v[1]["cli"]["apply"][0].clone(),
            "unauthorized_success" => v[0]["cli"]["unauthorized_plan"]["ok"] = true.into(),
            "receiver_custody" => v[1]["endpoint_after"]["retained_custody"] = true.into(),
            "receiver_object" => v[1]["endpoint_after"]["objects"][0]["sha256"] = foreign,
            "publish_put" => v[1]["endpoint_after"]["put_payload_bytes"] = 53.into(),
            "publish_missing" => v[1]["endpoint_after"]["objects"]
                .as_array_mut()
                .unwrap()
                .clear(),
            "publish_status" => v[1]["cli"]["status"][0]["verified_present"] = false.into(),
            "receipt_io" => v[1]["cli"]["apply"][0]["payload_io_bytes"] = 0.into(),
            "receipt_plan" => v[1]["cli"]["apply"][0]["plan"] = foreign,
            "hold_wrong_error" => v[2]["cli"]["apply"][0]["code"] = "busy".into(),
            "hold_success" => v[2]["cli"]["apply"][0] = v[1]["cli"]["apply"][0].clone(),
            "hold_write" => v[2]["endpoint_after"]["checkpoint"]["sequence"] = 999.into(),
            "recheck_write" => v[3]["endpoint_after"]["checkpoint"]["sequence"] = 999.into(),
            "recheck_put" => v[3]["endpoint_after"]["put_count"] = 3.into(),
            "get_counter" => v[2]["endpoint_after"]["get_payload_bytes"] = 161.into(),
            "receiver_capacity" => v[0]["endpoint_after"]["capacity"] = foreign,
            "receiver_toggle" => v[2]["endpoint_after"]["corrupt_response_injected"] = false.into(),
            "witness_hold" => v[2]["witness_after"]["sequence"] = 999.into(),
            "witness_identity" => {
                v[2]["witness_before"]["genesis"] = foreign.clone();
                v[2]["witness_after"]["genesis"] = foreign;
            }
            _ => unreachable!(),
        }
        // Common-field mutations are propagated and SDK IDs re-bound, so a rejection
        // cannot be explained solely by an object hash or changed predecessor link.
        for field in [
            "plans",
            "authority",
            "native_selection",
            "selected",
            "providers",
            "source_checkpoint",
            "local_before",
            "fixture_input_staging_bytes",
        ] {
            let common = v[0][field].clone();
            for r in v.iter_mut().skip(1) {
                r[field] = common.clone();
            }
        }
        if matches!(bad, "source_context" | "witness_store" | "authority_tree") {
            for r in &mut v {
                rebind_plans(r);
            }
        }
        let mut changed = trace.clone();
        for (i, r) in v.iter_mut().enumerate() {
            if i > 0 {
                r["previous_report"] = changed["completed"][9 + i]["report"].clone();
            }
            changed["completed"][10 + i]["report"] = save(&root, r);
        }
        let mut c = config.clone();
        c["teaching"]["trace"] = save(&root, &changed);
        let path = root.join("negative-remote.json");
        fs::write(&path, serde_json::to_vec(&c).unwrap()).unwrap();
        let cat = Catalogue::load(Config::load(&path).unwrap()).unwrap();
        assert!(
            git_asset::catalogue::teaching::execute(cat.teaching_request().unwrap()).is_err(),
            "accepted {bad}"
        );
    }
    for missing in [true, false] {
        let mut t = trace.clone();
        if missing {
            t["completed"].as_array_mut().unwrap().pop();
        } else {
            t["completed"].as_array_mut().unwrap().swap(10, 11);
        }
        let mut c = config.clone();
        c["teaching"]["trace"] = save(&root, &t);
        let path = root.join("negative-remote-pair.json");
        fs::write(&path, serde_json::to_vec(&c).unwrap()).unwrap();
        let cat = Catalogue::load(Config::load(&path).unwrap()).unwrap();
        assert!(git_asset::catalogue::teaching::execute(cat.teaching_request().unwrap()).is_err());
    }
}
