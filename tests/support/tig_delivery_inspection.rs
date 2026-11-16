use super::*;
use tig_store::metadata::{Limits, MetadataReader, MetadataStore};

fn exercise(private_review: bool) {
    let (mut f, request) = if private_review {
        review_fixture()
    } else {
        fixture()
    };
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    let metadata = f.root().join("inspection-metadata");
    let mut writer = MetadataStore::create(
        &metadata,
        Limits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let mut source = f.source();
    let (prepared, context) =
        adapter::freeze_context(&mut source, &request, &mut writer, Default::default()).unwrap();
    let producer = Object::from_bytes(b"explicit renderer admission; not execution evidence");
    let recipe = adapter::recipe(&mut source, &request, &context, &producer).unwrap();
    let archive = json!({"schema":reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA,
        "archive":context.archive(),"checkpoint":source.checkpoint(),
        "admitted":f.profile["admitted"],"byte_source":f.profile["byte_source"]});
    let minimum = writer.finish().unwrap();
    drop(writer);
    drop(source);
    // Inspection must not depend on primary bytes, local source controls or Git.
    for name in [
        ".git",
        "records",
        "definitions",
        "cache",
        "job.json",
        "contract.json",
        "production.json",
    ] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let profile = json!({"schema":"reel.tig-delivery-inspection-profile.experimental-1",
        "metadata_root":metadata,"metadata_minimum":minimum,"source":archive,
        "context":context.selection(),"request":request,"producer":producer,
        "recipe":recipe.fingerprint(),"max_control_bytes":1024*1024});
    let path = f.root().join("inspect-delivery.json");
    let run = |value: &Value| {
        fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
            .arg("inspect-delivery")
            .arg(&path)
            .output()
            .unwrap()
    };
    let result = run(&profile);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let observed: Value = serde_json::from_slice(&result.stdout).unwrap();
    for (name, expected) in [
        ("job.json", serde_json::to_vec(&prepared.job).unwrap()),
        ("contract.json", prepared.contract_bytes),
        ("production.json", prepared.production_bytes),
    ] {
        let file = &observed["controls"][name];
        assert_eq!(file["utf8"].as_str().unwrap().as_bytes(), expected);
        let object = Object::from_bytes(&expected);
        assert_eq!(file["reference"]["path"], name);
        assert_eq!(file["reference"]["sha256"], object.sha256.as_str());
        assert_eq!(file["reference"]["bytes"], object.bytes);
    }
    assert_eq!(observed["primary_bindings"].as_object().unwrap().len(), 3);
    assert_eq!(observed["recipe"], profile["recipe"]);
    assert_eq!(observed["producer_admission"], profile["producer"]);
    assert_eq!(observed["preparation_only"], true);
    assert_eq!(observed["materialized_media_bytes"], 0);
    for field in [
        "execution_attested",
        "authorizes_production",
        "approval_transferred",
        "authorizes_eviction",
    ] {
        assert_eq!(observed[field], false);
    }
    assert!(observed["production_claim"].is_null());
    for case in [
        "recipe", "producer", "context", "request", "bound", "claim", "weak",
    ] {
        let mut bad = profile.clone();
        let foreign = serde_json::to_value(Object::from_bytes(b"different admission")).unwrap();
        match case {
            "recipe" => bad["recipe"] = foreign["sha256"].clone(),
            "producer" => bad["producer"] = foreign,
            "context" => bad["context"]["root"] = foreign,
            "request" => bad["request"]["root_id"] = json!("other/root"),
            "bound" => bad["max_control_bytes"] = json!(1),
            "claim" => bad["claim"] = foreign,
            "weak" => bad["source"]["byte_source"]["required"] = json!("best-effort"),
            _ => unreachable!(),
        }
        let result = run(&bad);
        assert!(!result.status.success(), "{case}");
        assert!(result.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&result.stderr).contains(f.root().to_str().unwrap()));
    }
    if private_review {
        let mut bad = profile.clone();
        bad["request"]
            .as_object_mut()
            .unwrap()
            .remove("private_review");
        assert!(!run(&bad).status.success());
    }
    assert_eq!(
        MetadataReader::open(&metadata, &minimum)
            .unwrap()
            .checkpoint(),
        &minimum
    );
    assert!(!f.root().join("cache").exists());
    assert!(!f.root().join("job.json").exists());
}

#[test]
fn inspect_frozen_delivery_controls_offline_without_primary_media() {
    exercise(false);
}

#[test]
fn inspect_frozen_draft_controls_preserves_exact_review_scope() {
    exercise(true);
}
