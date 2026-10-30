use super::*;
use reel::tig_presentation::{self, Request};
#[cfg(all(windows, feature = "tig-publication"))]
#[path = "tig_presentation_jobs.rs"]
mod jobs;

fn setup() -> (Fixture, Request) {
    let mut f = Fixture::new();
    let authored =
        Object::from_bytes(&fs::read(f.root().join("definitions/authored.json")).unwrap());
    let mut pins = BTreeMap::new();
    for kind in [
        "presentation",
        "template",
        "timing",
        "clock",
        "evidence",
        "output",
    ] {
        let id = format!("types/presentation-{kind}");
        let object = write(
            f.root(),
            &format!("definitions/presentation-{kind}.json"),
            &json!({"schema":"tig.definition.design-1","id":id,"authored_schema":authored,"authoring":{"properties":["document"],"required":["document"],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
        );
        f.profile["admitted"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":id,"object":object,"payload":{"kind":"selected"}}));
        pins.insert(kind, object);
    }
    let bytes = b"[Script Info]\nScriptType: v4.00+\nPlayResX: 64\nPlayResY: 64\n[V4+ Styles]\nFormat: Name,Fontname,Fontsize,PrimaryColour,SecondaryColour,OutlineColour,BackColour,Bold,Italic,Underline,StrikeOut,ScaleX,ScaleY,Spacing,Angle,BorderStyle,Outline,Shadow,Alignment,MarginL,MarginR,MarginV,Encoding\nStyle: Default,Arial,12,&H00FFFFFF,&H00000000,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,2,2,2,2,1\n[Events]\nFormat: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,Caption\n";
    let layer = Object::from_bytes(bytes);
    fs::write(
        f.root().join("cache/objects").join(layer.sha256.as_str()),
        bytes,
    )
    .unwrap();
    let media = Object::from_bytes(&fs::read(f.root().join("definitions/media.json")).unwrap());
    write(
        f.root(),
        "records/presentation-layer.tig",
        &json!({"schema":"tig.instance.design-1","id":"presentation/layer","definition":{"id":"types/media","sha256":media.sha256,"bytes":media.bytes},"authored":{},"selected":layer,"streams":[],"status":{"selection_state":"selected-private-production"},"metadata":{}}),
    );
    for name in ["cel-a", "cel-b", "cue-main"] {
        update(&f, name, |record| {
            record["status"] = json!({"selection_state":"selected-private-production"})
        });
    }
    let cue: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/cue-main.tig")).unwrap()).unwrap();
    let docs = [
        (
            "presentation",
            json!({"schema":"reel.tig-still-sequence-source.v1","episode_id":"fixture","language":"es","role":"opening","template":{"id":"presentation/template"},"pictures":[{"id":"cel/a"},{"id":"cel/b"}],"editable_layer":{"id":"presentation/layer"},"audio":{"id":"cue/main"},"picture_timing":{"id":"presentation/timing"}}),
        ),
        (
            "template",
            json!({"schema":"reel.presentation-still-template.v2","template_id":"fixture-template","width":64,"height":64,"frame_rate":24,"sample_rate":48000,"picture_count":2,"total_frames":24,"audio_required":true}),
        ),
        (
            "timing",
            json!({"schema":"reel.presentation-picture-timing.v1","clock":{"id":"presentation/clock"},"spans":[{"start_anchor":"start","end_anchor":"middle"},{"start_anchor":"middle","end_anchor":"end"}]}),
        ),
        (
            "clock",
            json!({"schema":"reel.presentation-source-clock.v1","source":{"id":"cue/main"},"source_at_measurement":cue["selected"],"sample_rate":48000,"total_samples":48000,"evidence":{"id":"presentation/evidence"},"anchors":[{"id":"start","sample":0},{"id":"middle","sample":24000},{"id":"end","sample":48000}]}),
        ),
        ("evidence", json!("synthetic measured source anchors")),
    ];
    for (kind, document) in docs {
        write(
            f.root(),
            &format!("records/presentation-{kind}.tig"),
            &json!({"schema":"tig.instance.design-1","id":if kind == "presentation" {"presentation/source".into()} else {format!("presentation/{kind}")},"definition":{"id":format!("types/presentation-{kind}"),"sha256":pins[kind].sha256,"bytes":pins[kind].bytes},"authored":{"document":document},"selected":null,"streams":[],"status":{},"metadata":{}}),
        );
    }
    commit(&mut f);
    let request = serde_json::from_value(json!({"schema":tig_presentation::SCHEMA,"presentation_id":"presentation/source","source_definition":"types/presentation-presentation","template_definition":"types/presentation-template","timing_definition":"types/presentation-timing","clock_definition":"types/presentation-clock","evidence_definition":"types/presentation-evidence","primary_definitions":["types/media"],"allowed_primary_states":["selected-private-production"]})).unwrap();
    (f, request)
}
fn update(f: &Fixture, name: &str, change: impl FnOnce(&mut Value)) {
    let path = format!("records/{name}.tig");
    let mut record = serde_json::from_slice(&fs::read(f.root().join(&path)).unwrap()).unwrap();
    change(&mut record);
    write(f.root(), &path, &record);
}
fn commit(f: &mut Fixture) {
    git(f.root(), &["add", "records", "definitions"]);
    git(
        f.root(),
        &["commit", "-m", "synthetic native presentation source"],
    );
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
}

#[test]
fn logical_presentation_compiles_with_zero_media_reads_and_sparse_cel_edits() {
    let (mut f, request) = setup();
    let source = f.source();
    let old = tig_presentation::compile(&source, &request).unwrap();
    assert_eq!(source.byte_requests(), 0);
    assert_eq!(old.primary_files().count(), 4);
    assert_eq!(old.metadata_files().count(), 5);
    assert!(reel::scene_delivery::InputFiles::verified_path(&old, old.manifest()).is_err());
    let original = old.manifest().sha256.clone();
    update(&f, "presentation-presentation", |record| {
        record["metadata"]["note"] = json!("annotation only")
    });
    commit(&mut f);
    let mut annotated = f.source();
    assert_eq!(
        tig_presentation::compile(&annotated, &request)
            .unwrap()
            .manifest()
            .sha256,
        original
    );
    assert_ne!(source.checkpoint(), annotated.checkpoint());
    assert!(old.prepare(&mut annotated).is_err());
    assert_eq!(annotated.byte_requests(), 0);
    let blue: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/cel-b.tig")).unwrap()).unwrap();
    update(&f, "cel-a", |record| {
        record["selected"] = blue["selected"].clone()
    });
    commit(&mut f);
    let edited = f.source();
    let new = tig_presentation::compile(&edited, &request).unwrap();
    assert_ne!(new.manifest().sha256, original);
    assert_eq!(old.manifest().sha256, original);
    assert_eq!(edited.byte_requests(), 0);
}

#[test]
fn changed_recording_requires_a_new_measured_clock_before_any_media_read() {
    let (mut f, request) = setup();
    update(&f, "cue-main", |record| {
        record["selected"] = json!(Object::from_bytes(b"new recording"))
    });
    commit(&mut f);
    let source = f.source();
    let error = tig_presentation::compile(&source, &request).err().unwrap();
    assert!(error.to_string().contains("clock is stale"));
    assert_eq!(source.byte_requests(), 0);
}

#[test]
fn compiled_output_cannot_masquerade_as_a_primary() {
    let (mut f, request) = setup();
    let output = Object::from_bytes(
        &fs::read(f.root().join("definitions/presentation-output.json")).unwrap(),
    );
    update(&f, "cel-a", |record| {
        record["definition"] =
            json!({"id":"types/presentation-output","sha256":output.sha256,"bytes":output.bytes});
        record["authored"] = json!({"document":{}});
    });
    commit(&mut f);
    let source = f.source();
    assert!(
        tig_presentation::compile(&source, &request)
            .err()
            .unwrap()
            .to_string()
            .contains("not an admitted primary")
    );
    assert_eq!(source.byte_requests(), 0);
}

#[cfg(windows)]
#[test]
fn native_render_reads_compiled_metadata_and_guarded_tig_primaries() {
    use reel::{presentation_still_sequence as producer, scene_delivery::FileRef};
    let (mut f, request) = setup();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    let mut source = f.source();
    let compiled = tig_presentation::compile(&source, &request).unwrap();
    let inputs = compiled.prepare(&mut source).unwrap();
    let output = f.root().join("native-render");
    producer::build_with_control_files(compiled.manifest(), &compiled, &inputs, &output).unwrap();
    producer::check_with_control_files(compiled.manifest(), &compiled, &inputs, &output).unwrap();
    let bytes = fs::read(output.join("master.mkv")).unwrap();
    let object = Object::from_bytes(&bytes);
    let master = FileRef {
        path: "master.mkv".into(),
        sha256: object.sha256.as_str().into(),
        bytes: object.bytes,
    };
    let proof = producer::compare_master_to_primaries(
        compiled.manifest(),
        &compiled,
        &inputs,
        &output,
        &master,
        f.root(),
        1024 * 1024,
    )
    .unwrap();
    assert_eq!(proof.master().sha256, master.sha256);
    assert_eq!(proof.manifest().sha256, compiled.manifest().sha256);
    assert!(source.byte_requests() > 0);
    assert!(!f.root().join("native/manifest.json").exists());
    assert!(!f.root().join("control").exists());
}

#[test]
fn archived_presentation_compiles_without_git_working_records_or_metadata_files() {
    use tig_core::tree::{MemoryNodes, TreeLimits};
    let (f, request) = setup();
    let source = f.source();
    let expected = tig_presentation::compile(&source, &request).unwrap();
    let mut nodes = MemoryNodes::default();
    let archive = source
        .archive_metadata(&mut nodes, TreeLimits::default())
        .unwrap();
    let profile = serde_json::from_value(json!({"schema":reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA,"archive":archive,"checkpoint":source.checkpoint(),"admitted":f.profile["admitted"],"byte_source":f.profile["byte_source"]})).unwrap();
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let restored = Source::from_archive(profile, &mut nodes, TreeLimits::default()).unwrap();
    let actual = tig_presentation::compile(&restored, &request).unwrap();
    assert_eq!(actual.manifest().sha256, expected.manifest().sha256);
    assert_eq!(actual.records().count(), expected.records().count());
    assert_eq!(restored.byte_requests(), 0);
}

fn frozen(f: &Fixture, request: &Request) -> (tig_core::Digest, tig_core::Digest) {
    use tig_core::tree::{MemoryNodes, TreeLimits};
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let context =
        tig_presentation::freeze_context(&mut source, request, &mut nodes, TreeLimits::default())
            .unwrap();
    let producer = Object::from_bytes(b"synthetic externally admitted producer");
    let plan = tig_presentation::recipe(&mut source, request, &context, &producer).unwrap();
    assert_eq!(source.byte_requests(), 0);
    assert_eq!(plan.nodes().len(), 9);
    (source.checkpoint().clone(), plan.fingerprint().clone())
}

#[test]
fn frozen_recipe_ignores_annotations_but_tracks_primary_template_and_timing_edits() {
    let (mut f, request) = setup();
    let original = frozen(&f, &request);
    update(&f, "cel-a", |record| {
        record["metadata"]["review_note"] = json!("private annotation")
    });
    commit(&mut f);
    let annotated = frozen(&f, &request);
    assert_ne!(original.0, annotated.0);
    assert_eq!(original.1, annotated.1);
    let blue: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/cel-b.tig")).unwrap()).unwrap();
    update(&f, "cel-a", |record| {
        record["selected"] = blue["selected"].clone()
    });
    commit(&mut f);
    let image = frozen(&f, &request);
    assert_ne!(annotated.1, image.1);
    update(&f, "presentation-template", |record| {
        record["authored"]["document"]["width"] = json!(96)
    });
    commit(&mut f);
    let template = frozen(&f, &request);
    assert_ne!(image.1, template.1);
    update(&f, "presentation-clock", |record| {
        record["authored"]["document"]["anchors"][1]["sample"] = json!(12000)
    });
    commit(&mut f);
    assert_ne!(template.1, frozen(&f, &request).1);
}

#[test]
fn frozen_context_replays_offline_and_refuses_request_or_source_substitution() {
    use tig_core::tree::{MemoryNodes, TreeLimits};
    let (f, mut request) = setup();
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let context =
        tig_presentation::freeze_context(&mut source, &request, &mut nodes, TreeLimits::default())
            .unwrap();
    let producer = Object::from_bytes(b"synthetic externally admitted producer");
    let expected = tig_presentation::recipe(&mut source, &request, &context, &producer).unwrap();
    let profile = serde_json::from_value(json!({"schema":reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA,"archive":context.archive(),"checkpoint":source.checkpoint(),"admitted":f.profile["admitted"],"byte_source":f.profile["byte_source"]})).unwrap();
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let (mut restored, replayed) = tig_presentation::restore_context(
        profile,
        &request,
        context.selection(),
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let actual = tig_presentation::recipe(&mut restored, &request, &replayed, &producer).unwrap();
    assert_eq!(actual.fingerprint(), expected.fingerprint());
    request.allowed_primary_states.insert("unreviewed".into());
    assert!(tig_presentation::recipe(&mut restored, &request, &replayed, &producer).is_err());
    assert_eq!(restored.byte_requests(), 0);
}
