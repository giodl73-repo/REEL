use super::*;
#[path = "tig_delivery_template_context.rs"]
mod context;
use reel::tig_delivery_template::{self as adapter, Request};

fn record(f: &Fixture, name: &str, edit: impl FnOnce(&mut Value)) {
    let path = format!("records/{name}.tig");
    let mut value = serde_json::from_slice(&fs::read(f.root().join(&path)).unwrap()).unwrap();
    edit(&mut value);
    write(f.root(), &path, &value);
}
fn commit(f: &mut Fixture) {
    git(f.root(), &["add", "records", "definitions"]);
    git(f.root(), &["commit", "-m", "explicit template fixture"]);
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
}
fn fixture() -> (Fixture, Request) {
    let mut f = Fixture::new();
    for name in ["cel-a", "cel-b", "cue-main"] {
        record(&f, name, |r| {
            r["status"]["selection_state"] = json!("selected-private-production")
        });
    }
    let mut definition: Value =
        serde_json::from_slice(&fs::read(f.root().join("definitions/media.json")).unwrap())
            .unwrap();
    let mut pins = BTreeMap::new();
    for kind in ["root", "control"] {
        definition["id"] = json!(format!("types/{kind}"));
        definition["authoring"]["properties"] = if kind == "root" {
            json!(["binding_ids", "control_ids", "language_binding_ids"])
        } else {
            json!(["document"])
        };
        let pin = write(f.root(), &format!("definitions/{kind}.json"), &definition);
        f.profile["admitted"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":format!("types/{kind}"),"object":pin,"payload":{"kind":"selected"}}));
        pins.insert(kind, pin);
    }
    let mut job = f.job.clone();
    job["schema"] = json!(adapter::SCHEMA);
    job.as_object_mut().unwrap().remove("contract");
    job.as_object_mut()
        .unwrap()
        .remove("production_manifest_sha256");
    for (index, alias) in ["a", "b"].into_iter().enumerate() {
        job["pictures"][index]["source"] = json!({"binding":alias});
    }
    job["audio"][0]["source"] = json!({"binding":"recording"});
    job["audio"][0]["bus"] = json!("M");
    job["audio"][0].as_object_mut().unwrap().remove("cue_id");
    job["buses"]["D"] = json!({"state":"intentional-silence","reason":"No speech"});
    job["buses"]["M"] = json!({"state":"present","reason":"Recording"});
    let mut contract: Value =
        serde_json::from_slice(&fs::read(f.root().join("contract.json")).unwrap()).unwrap();
    contract["schema"] = json!(reel::cue_relative::AUDIO_EVENT_CONTRACT_SCHEMA);
    let production = json!({"manifest_version":"reel.manifest.v0.2","work":"template","title":"Template",
        "shots":[{"id":"shot","scene_id":"scene"}],"audio_events":[{"id":"main","role":"music","source":"cue/main","start_seconds":0}]});
    for (name, document) in [
        ("job", job),
        ("contract", contract),
        ("production", production),
    ] {
        write(
            f.root(),
            &format!("records/control-{name}.tig"),
            &json!({"schema":"tig.instance.design-1","id":format!("controls/{name}"),
            "definition":{"id":"types/control","sha256":pins["control"].sha256,"bytes":pins["control"].bytes},
            "authored":{"document":document},"selected":null,"streams":[],"status":{"selection_state":"selected-private-production"},"metadata":{}}),
        );
    }
    write(
        f.root(),
        "records/root.tig",
        &json!({"schema":"tig.instance.design-1","id":"delivery/source",
        "definition":{"id":"types/root","sha256":pins["root"].sha256,"bytes":pins["root"].bytes},
        "authored":{"binding_ids":{"a":{"id":"cel/a"},"b":{"id":"cel/b"},"recording":{"id":"cue/main"}},
            "control_ids":{"job":{"id":"controls/job"},"contract":{"id":"controls/contract"},"production":{"id":"controls/production"}}},
        "selected":null,"streams":[],"status":{"selection_state":"selected-private-production"},"metadata":{}}),
    );
    commit(&mut f);
    (
        f,
        Request {
            schema: adapter::SCHEMA.into(),
            root_id: "delivery/source".into(),
            root_definition: "types/root".into(),
            primary_definitions: BTreeSet::from(["types/media".into()]),
            control_definitions: BTreeSet::from(["types/control".into()]),
            authored_control_fields: BTreeMap::from([("types/control".into(), "document".into())]),
        },
    )
}
use std::collections::BTreeSet;

#[test]
fn sparse_leaf_edit_lowers_current_pins_without_parent_edits_or_payload_reads() {
    let (mut f, request) = fixture();
    let parent = fs::read(f.root().join("records/root.tig")).unwrap();
    let mut source = f.source();
    let before = adapter::lower(&mut source, &request).unwrap();
    assert_eq!(before.consumed_bindings.len(), 3);
    assert_eq!(source.byte_requests(), 0);
    let new = Object::from_bytes(b"unavailable new candidate bytes");
    record(&f, "cel-b", |r| r["selected"] = json!(new));
    commit(&mut f);
    let mut next = f.source();
    let after = adapter::lower(&mut next, &request).unwrap();
    assert_eq!(after.job.pictures[1].source.sha256, new.sha256.as_str());
    assert_ne!(before.job.pictures[1].source, after.job.pictures[1].source);
    assert_eq!(before.job.pictures[0].source, after.job.pictures[0].source);
    assert_eq!(before.job.audio[0].source, after.job.audio[0].source);
    assert_eq!(before.contract_bytes, after.contract_bytes);
    assert_eq!(parent, fs::read(f.root().join("records/root.tig")).unwrap());
    assert_eq!(next.byte_requests(), 0);
}

#[test]
fn authored_controls_replay_from_archive_with_git_and_cache_unavailable() {
    let (f, request) = fixture();
    let mut source = f.source();
    let expected = adapter::lower(&mut source, &request).unwrap();
    let mut nodes = tig_core::tree::MemoryNodes::default();
    let archive = source
        .archive_metadata(&mut nodes, Default::default())
        .unwrap();
    let profile = archived_profile(&f, archive, source.checkpoint().clone());
    drop(source);
    for name in [".git", "records", "definitions", "cache"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut restored = Source::from_archive(profile, &mut nodes, Default::default()).unwrap();
    let actual = adapter::lower(&mut restored, &request).unwrap();
    assert_eq!(
        serde_json::to_vec(&expected.job).unwrap(),
        serde_json::to_vec(&actual.job).unwrap()
    );
    assert_eq!(expected.contract_bytes, actual.contract_bytes);
    assert_eq!(expected.production_bytes, actual.production_bytes);
    assert_eq!(restored.byte_requests(), 0);
}

#[test]
fn wrong_primary_type_and_used_draft_refuse_without_byte_reads() {
    let (mut f, request) = fixture();
    record(&f, "root", |r| {
        r["authored"]["binding_ids"]["b"]["id"] = json!("controls/job")
    });
    commit(&mut f);
    let mut source = f.source();
    assert!(
        adapter::lower(&mut source, &request)
            .err()
            .unwrap()
            .to_string()
            .contains("primary definitions")
    );
    assert_eq!(source.byte_requests(), 0);
    record(&f, "root", |r| {
        r["authored"]["binding_ids"]["b"]["id"] = json!("cel/b")
    });
    record(&f, "cel-b", |r| {
        r["status"]["selection_state"] = json!("draft")
    });
    commit(&mut f);
    let mut source = f.source();
    assert!(
        adapter::lower(&mut source, &request)
            .err()
            .unwrap()
            .to_string()
            .contains("not selected")
    );
    assert_eq!(source.byte_requests(), 0);
}

#[test]
fn raw_file_pins_unknown_bindings_and_unresolved_variants_refuse() {
    let (mut f, request) = fixture();
    let original = fs::read(f.root().join("records/control-job.tig")).unwrap();
    for replacement in [
        json!({"path":"cel/a","sha256":"0".repeat(64),"bytes":1}),
        json!({"binding":"missing"}),
        json!({"binding":"a","bytes":1}),
    ] {
        record(&f, "control-job", |r| {
            r["authored"]["document"]["pictures"][0]["source"] = replacement
        });
        commit(&mut f);
        let mut source = f.source();
        assert!(adapter::lower(&mut source, &request).is_err());
        assert_eq!(source.byte_requests(), 0);
    }
    fs::write(f.root().join("records/control-job.tig"), original).unwrap();
    record(&f, "root", |r| {
        r["authored"]["language_binding_ids"] = json!({"en":{"a":{"id":"cel/b"}}})
    });
    commit(&mut f);
    let mut source = f.source();
    assert!(adapter::lower(&mut source, &request).is_err());
    assert_eq!(source.byte_requests(), 0);
}

#[test]
fn unused_draft_alternative_does_not_block_but_consuming_it_refuses() {
    let (mut f, request) = fixture();
    let mut alternative: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/cel-a.tig")).unwrap()).unwrap();
    alternative["id"] = json!("cel/alternative");
    alternative["selected"] = Value::Null;
    alternative["status"]["selection_state"] = json!("draft");
    write(f.root(), "records/alternative.tig", &alternative);
    record(&f, "root", |r| {
        r["authored"]["binding_ids"]["alternative"] = json!({"id":"cel/alternative"})
    });
    commit(&mut f);
    let mut source = f.source();
    assert_eq!(
        adapter::lower(&mut source, &request)
            .unwrap()
            .consumed_bindings
            .len(),
        3
    );
    record(&f, "control-job", |r| {
        r["authored"]["document"]["pictures"][0]["source"] = json!({"binding":"alternative"})
    });
    commit(&mut f);
    let mut source = f.source();
    assert!(adapter::lower(&mut source, &request).is_err());
    assert_eq!(source.byte_requests(), 0);
}

#[test]
fn derived_layer_receipt_cannot_hide_unlisted_or_stale_primary_inputs() {
    let (mut f, request) = fixture();
    let a = f.job["pictures"][0]["source"].clone();
    let b = f.job["pictures"][1]["source"].clone();
    let mut receipt = json!({"schema":"reel.timed-overlay-derivation.v1",
        "selected_evidence":a,"output":b,"inputs":[a],"recipe":{}});
    let mut leaf: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/cel-a.tig")).unwrap()).unwrap();
    leaf["id"] = json!("layer/receipt");
    record(&f, "root", |r| {
        r["authored"]["binding_ids"]["receipt"] = json!({"id":"layer/receipt"})
    });
    record(&f, "control-job", |r| {
        r["authored"]["document"]["external_layers"] = json!([
        {"attachment_id":"overlay","reason":"Metadata closure fixture","evidence":{"binding":"a"},
            "render_mode":"timed-video-overlay","render_source":{"binding":"b"},"derivation_receipt":{"binding":"receipt"}}])
    });
    for mode in 0..3 {
        if mode == 1 {
            receipt["inputs"][0]["path"] = json!("unlisted/input");
        }
        if mode == 2 {
            receipt["inputs"][0]["path"] = json!("cel/b");
        }
        let raw = serde_json::to_vec(&receipt).unwrap();
        let pin = Object::from_bytes(&raw);
        fs::write(
            f.root().join("cache/objects").join(pin.sha256.as_str()),
            raw,
        )
        .unwrap();
        leaf["selected"] = json!(pin);
        write(f.root(), "records/receipt.tig", &leaf);
        commit(&mut f);
        let mut source = f.source();
        let result = adapter::lower(&mut source, &request);
        assert_eq!(result.is_ok(), mode == 0);
        assert_eq!(
            source.byte_requests(),
            1,
            "only selected receipt metadata may be read"
        );
        assert_eq!(source.active_readers(), 0);
    }
}

#[test]
fn selected_large_controls_reuse_existing_selectors_and_keep_fractional_media_values() {
    let (mut f, mut request) = fixture();
    request.authored_control_fields.clear();
    for name in ["job", "contract", "production"] {
        let path = format!("records/control-{name}.tig");
        let mut control: Value =
            serde_json::from_slice(&fs::read(f.root().join(&path)).unwrap()).unwrap();
        let mut doc = control["authored"]["document"].take();
        if name == "job" {
            doc["pictures"][0]["attention"] = json!("x".repeat(70000));
            doc["audio"][0]["gain_db"] = json!(-3.25);
        }
        let bytes = serde_json::to_vec(&doc).unwrap();
        let pin = Object::from_bytes(&bytes);
        fs::write(
            f.root().join("cache/objects").join(pin.sha256.as_str()),
            bytes,
        )
        .unwrap();
        control["selected"] = json!(pin);
        control["authored"] = json!({});
        write(f.root(), &path, &control);
    }
    commit(&mut f);
    let mut source = f.source();
    let prepared = adapter::lower(&mut source, &request).unwrap();
    assert_eq!(prepared.job.audio[0].gain_db, -3.25);
    assert_eq!(prepared.job.pictures[0].attention.len(), 70000);
    assert_eq!(source.byte_requests(), 3);
    assert_eq!(source.active_readers(), 0);
    drop(source);
    let duplicate = br#"{"schema":"first","schema":"second"}"#;
    let pin = Object::from_bytes(duplicate);
    fs::write(
        f.root().join("cache/objects").join(pin.sha256.as_str()),
        duplicate,
    )
    .unwrap();
    record(&f, "control-job", |r| r["selected"] = json!(pin));
    commit(&mut f);
    let mut source = f.source();
    assert!(
        adapter::lower(&mut source, &request)
            .err()
            .unwrap()
            .to_string()
            .contains("duplicate")
    );
}

#[cfg(windows)]
#[test]
#[ignore = "requires native FFmpeg; all controls and bindings replay from Tig"]
fn lowered_archived_controls_render_without_loose_json_or_git() {
    let (mut f, request) = fixture();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    for (name, rgb) in [("cel-a", [0, 0, 0]), ("cel-b", [255, 255, 255])] {
        let bytes = picture(rgb);
        let pin = Object::from_bytes(&bytes);
        fs::write(
            f.root().join("cache/objects").join(pin.sha256.as_str()),
            bytes,
        )
        .unwrap();
        record(&f, name, |r| r["selected"] = json!(pin));
    }
    commit(&mut f);
    let source = f.source();
    let mut nodes = tig_core::tree::MemoryNodes::default();
    let archive = source
        .archive_metadata(&mut nodes, Default::default())
        .unwrap();
    let profile = archived_profile(&f, archive, source.checkpoint().clone());
    drop(source);
    for name in [
        ".git",
        "records",
        "definitions",
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
    let mut source = Source::from_archive(profile, &mut nodes, Default::default()).unwrap();
    let prepared = adapter::lower(&mut source, &request).unwrap();
    let inputs = source.prepare(&prepared.job).unwrap();
    drop(source);
    let controls = f.root().join("lowered");
    fs::create_dir(&controls).unwrap();
    fs::write(controls.join("contract.json"), prepared.contract_bytes).unwrap();
    fs::write(controls.join("production.json"), prepared.production_bytes).unwrap();
    fs::write(
        controls.join("job.json"),
        serde_json::to_vec(&prepared.job).unwrap(),
    )
    .unwrap();
    let output = f.root().join("render");
    reel::scene_delivery::render_with_inputs(&controls.join("job.json"), &inputs, &output).unwrap();
    reel::scene_delivery::check_with_inputs(&controls.join("job.json"), &inputs, &output).unwrap();
    let pcm = Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(output.join("M.wav"))
        .args(["-acodec", "pcm_s24le", "-f", "s24le", "-"])
        .output()
        .unwrap();
    assert!(pcm.status.success());
    assert!(pcm.stdout == wav()[44..], "music PCM differs");
}
