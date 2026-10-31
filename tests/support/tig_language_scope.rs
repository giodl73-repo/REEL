use super::*;
#[path = "tig_authored_controls.rs"]
mod authored_controls;
#[path = "tig_binding_variants.rs"]
mod binding_variants;
#[path = "tig_control_references.rs"]
mod control_references;
#[cfg(all(windows, feature = "tig-publication"))]
#[path = "tig_projected_jobs.rs"]
mod projected_jobs;
#[path = "tig_render_context.rs"]
mod render_context;
#[path = "tig_render_recipe.rs"]
mod render_recipe;
#[path = "tig_scoped_context.rs"]
mod scoped_context;
use std::collections::BTreeSet;

fn cached(f: &Fixture, raw: &[u8]) -> Object {
    let object = Object::from_bytes(raw);
    fs::write(
        f.root().join("cache/objects").join(object.sha256.as_str()),
        raw,
    )
    .unwrap();
    object
}
fn load(f: &Fixture, path: &str) -> Value {
    serde_json::from_slice(&fs::read(f.root().join(path)).unwrap()).unwrap()
}
fn save_commit(f: &mut Fixture) {
    git(f.root(), &["add", "records"]);
    git(
        f.root(),
        &["commit", "-m", "synthetic language dependency update"],
    );
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
}
fn bilingual() -> (Fixture, reel::tig_scene::Request) {
    let (mut f, mut request) = native_scene_fixture();
    let mut voice = load(&f, "records/cue-main.tig");
    voice["id"] = "cue/english".into();
    let mut bytes = wav();
    *bytes.last_mut().unwrap() = 17;
    let take = cached(&f, &bytes);
    voice["selected"] = json!(take);
    write(f.root(), "records/cue-english.tig", &voice);
    let mut alignment = load(&f, "records/alignment.tig");
    alignment["id"] = "alignment/english".into();
    let bytes = serde_json::to_vec(&json!({"schema":"reel.scene-native-alignment.v1","language":"en","cue_id":"en-1","selected_take_sha256":take.sha256,"sample_rate":48000,"cue_end_sample":48000,"semantic_markers":{"first-line":0,"second-line":24000}})).unwrap();
    alignment["streams"][0]["object"] = json!(cached(&f, &bytes));
    write(f.root(), "records/alignment-english.tig", &alignment);
    let mut parent = load(&f, "records/scene.tig");
    let scene_pin: Object = serde_json::from_value(parent["selected"].clone()).unwrap();
    let mut scene: Value = serde_json::from_slice(
        &fs::read(
            f.root()
                .join("cache/objects")
                .join(scene_pin.sha256.as_str()),
        )
        .unwrap(),
    )
    .unwrap();
    let mut lane = scene["languages"]["es"].clone();
    lane["cues"][0]["cue_id"] = "en-1".into();
    lane["cues"][0]["take_binding"] = "take/en~selected".into();
    lane["cues"][0]["phrase_alignment_binding"] = "alignment/en~current".into();
    for event in lane["events"].as_array_mut().unwrap() {
        event["cue_id"] = "en-1".into();
    }
    scene["languages"]["en"] = lane;
    parent["selected"] = json!(cached(&f, &serde_json::to_vec(&scene).unwrap()));
    parent["authored"]["binding_ids"]["take/en~selected"] = json!({"id":"cue/english"});
    parent["authored"]["binding_ids"]["alignment/en~current"] = json!({"id":"alignment/english"});
    write(f.root(), "records/scene.tig", &parent);
    save_commit(&mut f);
    request.language = "en".into();
    (f, request)
}
#[cfg(all(windows, feature = "tig-publication"))]
#[test]
fn unused_input_definitions_cannot_be_output_templates_before_job_adoption() {
    use tig_store::{metadata::MetadataReader, source_claims::SourceJournal};
    let (mut f, request) = control_references::fixture();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    for (name, id) in [
        ("cue-main", "cue/unrelated-output-trap"),
        ("alignment", "alignment/unrelated-output-trap"),
        ("profile", "controls/unrelated-output-trap"),
    ] {
        let mut record = load(&f, &format!("records/{name}.tig"));
        record["id"] = json!(id);
        write(f.root(), &format!("records/unused-{name}.tig"), &record);
    }
    save_commit(&mut f);
    let capture = crate::capture::profile(&f, &request);
    let captured = crate::capture::run(&f, &capture, "role-context.json");
    assert!(
        captured.status.success(),
        "{}",
        String::from_utf8_lossy(&captured.stderr)
    );
    let fragment = load(&f, "role-context.json");
    let stores = load(&f, "capture-stores.json");
    let metadata = crate::capture::metadata_root(f.root());
    let minimum = serde_json::from_value(fragment["metadata_minimum"].clone()).unwrap();
    let mut reader = MetadataReader::open(&metadata, &minimum).unwrap();
    let before_metadata = reader.checkpoint().clone();
    let (_, context) = reel::tig_scene::restore_render_context(
        serde_json::from_value(fragment["source"].clone()).unwrap(),
        &request,
        &serde_json::from_value(fragment["context"].clone()).unwrap(),
        &f.root().join("role-preflight"),
        &mut reader,
        tig_core::tree::TreeLimits::default(),
    )
    .unwrap();
    drop(reader);
    let journal_root = Path::new(stores["journal_root"].as_str().unwrap());
    let journal_minimum = serde_json::from_value(stores["journal_minimum"].clone()).unwrap();
    let before_journal = SourceJournal::open(journal_root, &journal_minimum)
        .unwrap()
        .checkpoint();
    let executable = Path::new(env!("CARGO_BIN_EXE_reel-tig-job"))
        .canonicalize()
        .unwrap();
    let pin = Object::from_bytes(&fs::read(&executable).unwrap());
    let file = json!({"path":executable,"sha256":pin.sha256,"bytes":pin.bytes});
    fs::create_dir_all(f.root().join("capture-capacity/objects/sha256")).unwrap();
    for (id, name) in [
        ("cue/unrelated-output-trap", "unused-cue-main"),
        ("alignment/unrelated-output-trap", "unused-alignment"),
        ("controls/unrelated-output-trap", "unused-profile"),
    ] {
        let template = tig_assets::graph::Reference {
            id: id.into(),
            object: Object::from_bytes(
                &fs::read(f.root().join(format!("records/{name}.tig"))).unwrap(),
            ),
        };
        assert!(
            !context.requirements().metadata().contains(&template.object),
            "the target must exercise type admission outside the current input-identity guard"
        );
        let mut worker = fragment.clone();
        worker["runtime"] = json!({"host":pin,"ffmpeg":file,"ffprobe":file,"dependencies":{}});
        worker["output_budget"] = json!({"default_max_bytes":4*1024*1024_u64});
        worker["record_outputs"] =
            json!([{"output":"master.mkv","template":template,"selector":{"kind":"selected"}}]);
        let profile = json!({"schema":"reel.tig-native-job-profile.experimental-1",
            "worker":worker,"cache":capture["cache"],"preparation_cache":capture["preparation_cache"],
            "journal_root":journal_root,"journal_minimum":journal_minimum,
            "source_id":"role-guard","owner":"fixture-owner","expected_revision":0,
            "start_operation":"role-start","retain_operation":"role-retain",
            "budget_bytes":1073741824_u64,"scratch_bytes":536870912_u64,"timeout_seconds":30});
        let path = f.root().join("role-profile.json");
        fs::write(&path, serde_json::to_vec(&profile).unwrap()).unwrap();
        let refused = Command::new(&executable)
            .arg("plan-outputs")
            .arg(&path)
            .output()
            .unwrap();
        assert!(!refused.status.success());
        assert!(
            String::from_utf8_lossy(&refused.stderr)
                .contains("output template uses a production-input definition"),
            "{}",
            String::from_utf8_lossy(&refused.stderr)
        );
        assert_eq!(
            SourceJournal::open(journal_root, &journal_minimum)
                .unwrap()
                .checkpoint(),
            before_journal
        );
        assert_eq!(
            MetadataReader::open(&metadata, &minimum)
                .unwrap()
                .checkpoint(),
            &before_metadata
        );
        assert!(!f.root().join("capture-capacity/staging/caminos").exists());
    }
}

fn compile(f: &Fixture, request: &reel::tig_scene::Request, name: &str) -> Value {
    let output = f.root().join(name);
    reel::tig_scene::compile(&mut f.source(), request, &output).unwrap();
    serde_json::from_slice(&fs::read(output.join("native-source.json")).unwrap()).unwrap()
}
fn primary(report: &Value, scope: &str) -> BTreeSet<Object> {
    serde_json::from_value(report[scope]["primary_objects"].clone()).unwrap()
}

#[test]
fn unused_drafts_need_no_payload_reads_and_leave_actual_language_contracts_unchanged() {
    let (mut f, request) = bilingual();
    let before = compile(&f, &request, "before-unused-drafts");
    let mut parent = load(&f, "records/scene.tig");
    let mut unused = BTreeSet::new();
    for (name, status) in [
        ("candidate", "candidate-unselected"),
        ("held", "held-for-listening"),
    ] {
        let mut record = load(&f, "records/cue-main.tig");
        let object = Object::from_bytes(format!("never materialized {name}").as_bytes());
        record["id"] = json!(format!("cue/unused-{name}"));
        record["selected"] = json!(object);
        record["status"]["selection_state"] = json!(status);
        write(f.root(), &format!("records/unused-{name}.tig"), &record);
        parent["authored"]["binding_ids"][format!("unused-{name}")] =
            json!({"id":format!("cue/unused-{name}")});
        assert!(
            !f.root()
                .join("cache/objects")
                .join(object.sha256.as_str())
                .exists()
        );
        unused.insert(object);
    }
    write(f.root(), "records/scene.tig", &parent);
    save_commit(&mut f);
    let after = compile(&f, &request, "with-unused-drafts");
    assert_eq!(
        primary(&before, "language_inputs"),
        primary(&after, "language_inputs")
    );
    assert_eq!(
        before["language_inputs"]["native_semantic_sha256"],
        after["language_inputs"]["native_semantic_sha256"]
    );
    let declared = primary(&after, "declared_inputs");
    assert!(unused.iter().all(|object| declared.contains(object)));
    assert!(unused.is_disjoint(&primary(&after, "language_inputs")));
    for name in ["production.json", "contract.json", "job.json"] {
        assert_eq!(
            fs::read(f.root().join("before-unused-drafts/delivery").join(name)).unwrap(),
            fs::read(f.root().join("with-unused-drafts/delivery").join(name)).unwrap()
        );
    }
    // The exact old snapshot continues to resolve after the authoring catalog edit.
    let mut old = f.profile.clone();
    old["commit"] = json!(git(f.root(), &["rev-parse", "HEAD^"]));
    let mut old_source = Source::open(serde_json::from_value(old).unwrap()).unwrap();
    reel::tig_scene::compile(&mut old_source, &request, &f.root().join("old-drafts-view")).unwrap();
    // Unused does not bypass type admission: a scene/output is never a primary.
    parent["authored"]["binding_ids"]["unused-wrong-definition"] = json!({"id":request.scene_id});
    write(f.root(), "records/scene.tig", &parent);
    save_commit(&mut f);
    let mut source = f.source();
    let refused = f.root().join("unused-wrong-definition");
    assert!(reel::tig_scene::compile(&mut source, &request, &refused).is_err());
    assert_eq!(source.byte_requests(), 0);
    assert!(!refused.exists());
}

#[test]
fn native_language_scope_retains_alignments_and_controls_without_other_language_bytes() {
    let (f, request) = bilingual();
    let report = compile(&f, &request, "english-scope");
    let full = primary(&report, "declared_inputs");
    let english = primary(&report, "language_inputs");
    assert_eq!(full.len(), 6);
    assert_eq!(english.len(), 4);
    let english_alignment: Object = serde_json::from_value(
        load(&f, "records/alignment-english.tig")["streams"][0]["object"].clone(),
    )
    .unwrap();
    assert!(english.contains(&english_alignment));
    let spanish_alignment: Object =
        serde_json::from_value(load(&f, "records/alignment.tig")["streams"][0]["object"].clone())
            .unwrap();
    assert!(!english.contains(&spanish_alignment));
    assert_eq!(
        report["language_inputs"]["control_objects"],
        report["declared_inputs"]["control_objects"]
    );
    assert_eq!(
        report["language_inputs"]["control_objects"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(report["language_inputs"]["authorizes_eviction"], false);
    assert_eq!(
        report["language_inputs"]["complete_semantic_coverage"],
        false
    );
}

#[test]
fn spanish_take_edit_preserves_english_scope_and_native_semantics() {
    let (mut f, request) = bilingual();
    let before = compile(&f, &request, "before-spanish-edit");
    let parent = fs::read(f.root().join("records/scene.tig")).unwrap();
    let mut voice = load(&f, "records/cue-main.tig");
    // This other-language media is deliberately absent; English does not read it.
    voice["selected"] = json!(Object::from_bytes(b"new Spanish take"));
    write(f.root(), "records/cue-main.tig", &voice);
    save_commit(&mut f);
    let after = compile(&f, &request, "after-spanish-edit");
    assert_eq!(
        primary(&before, "language_inputs"),
        primary(&after, "language_inputs")
    );
    assert_ne!(
        primary(&before, "declared_inputs"),
        primary(&after, "declared_inputs")
    );
    assert_eq!(
        before["language_inputs"]["binding_sha256"],
        after["language_inputs"]["binding_sha256"]
    );
    assert_eq!(
        before["language_inputs"]["native_semantic_sha256"],
        after["language_inputs"]["native_semantic_sha256"]
    );
    assert_ne!(before["source_checkpoint"], after["source_checkpoint"]);
    assert_eq!(
        parent,
        fs::read(f.root().join("records/scene.tig")).unwrap()
    );
}

#[test]
fn other_language_drafts_do_not_block_current_language_but_consumed_drafts_refuse() {
    for name in ["cue-main", "alignment"] {
        let (mut f, request) = bilingual();
        let before = compile(&f, &request, "before-draft");
        let mut record = load(&f, &format!("records/{name}.tig"));
        record["status"]["selection_state"] = json!("draft-unreviewed");
        let missing = Object::from_bytes(b"new other-language draft not materialized");
        if name == "alignment" {
            record["streams"][0]["object"] = json!(missing);
        } else {
            record["selected"] = json!(missing);
        }
        write(f.root(), &format!("records/{name}.tig"), &record);
        save_commit(&mut f);
        let after = compile(&f, &request, "after-draft");
        assert_eq!(
            primary(&before, "language_inputs"),
            primary(&after, "language_inputs")
        );
        for field in ["binding_sha256", "native_semantic_sha256"] {
            assert_eq!(
                before["language_inputs"][field],
                after["language_inputs"][field]
            );
        }
        for file in ["production.json", "contract.json", "job.json"] {
            assert_eq!(
                fs::read(f.root().join("before-draft/delivery").join(file)).unwrap(),
                fs::read(f.root().join("after-draft/delivery").join(file)).unwrap()
            );
        }
        // Public authoring entry points must agree with the native compiler.
        // The explicit whole-scene command still checks every language.
        for (language, allowed) in [(Some("en"), true), (Some("es"), false), (None, false)] {
            let output = f
                .root()
                .join(format!("resolved-{}.json", language.unwrap_or("all")));
            let mut command = Command::new(env!("CARGO_BIN_EXE_reel-scene-authoring"));
            command.arg(if language.is_some() {
                "resolve-language"
            } else {
                "resolve"
            });
            for file in [
                "catalog.json",
                "episode.json",
                "scene.json",
                "policy.json",
                "season-bindings.json",
                "episode-bindings.json",
                "scene-bindings.json",
            ] {
                command.arg(f.root().join("after-draft").join(file));
            }
            if let Some(language) = language {
                command.arg(language);
            }
            let result = command.arg("--output").arg(&output).output().unwrap();
            assert_eq!(
                result.status.success(),
                allowed,
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            if allowed {
                let resolved: Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
                assert_eq!(
                    resolved["fingerprint_sha256"],
                    before["language_inputs"]["native_semantic_sha256"]
                );
            } else {
                assert!(!output.exists());
            }
        }
        assert!(
            !f.root()
                .join("cache/objects")
                .join(missing.sha256.as_str())
                .exists()
        );
        let mut spanish: reel::tig_scene::Request =
            serde_json::from_value(serde_json::to_value(&request).unwrap()).unwrap();
        spanish.language = "es".into();
        let refused = f.root().join("spanish-draft-refused");
        let error = reel::tig_scene::compile(&mut f.source(), &spanish, &refused).unwrap_err();
        assert!(
            error.to_string().contains("not selected for production"),
            "{error:#}"
        );
        assert!(!refused.exists());
        // The same guard applies to shared and requested-language inputs.
        let mut shared = load(&f, "records/cel-b.tig");
        shared["status"]["selection_state"] = json!("draft-unreviewed");
        write(f.root(), "records/cel-b.tig", &shared);
        save_commit(&mut f);
        let refused = f.root().join("shared-draft-refused");
        assert!(reel::tig_scene::compile(&mut f.source(), &request, &refused).is_err());
        assert!(!refused.exists());
    }
}

#[test]
fn shared_cel_replacement_changes_language_scope_and_old_git_view_keeps_old_inputs() {
    let (mut f, request) = bilingual();
    let mut old = f.source();
    let before = compile(&f, &request, "before-cel-replacement");
    let parent = fs::read(f.root().join("records/scene.tig")).unwrap();
    let mut cel = load(&f, "records/cel-b.tig");
    let replacement = cached(&f, &picture([0, 255, 0]));
    cel["selected"] = json!(replacement);
    write(f.root(), "records/cel-b.tig", &cel);
    save_commit(&mut f);
    let after = compile(&f, &request, "after-cel-replacement");
    let a = primary(&before, "language_inputs");
    let b = primary(&after, "language_inputs");
    assert_eq!(a.difference(&b).count(), 1);
    assert_eq!(
        b.difference(&a).cloned().collect::<Vec<_>>(),
        vec![replacement]
    );
    assert_ne!(
        before["language_inputs"]["native_semantic_sha256"],
        after["language_inputs"]["native_semantic_sha256"]
    );
    assert_ne!(
        before["language_inputs"]["binding_sha256"],
        after["language_inputs"]["binding_sha256"]
    );
    assert_eq!(
        parent,
        fs::read(f.root().join("records/scene.tig")).unwrap()
    );
    let path = f.root().join("old-view-replay");
    reel::tig_scene::compile(&mut old, &request, &path).unwrap();
    let replay: Value =
        serde_json::from_slice(&fs::read(path.join("native-source.json")).unwrap()).unwrap();
    assert_eq!(before["language_inputs"], replay["language_inputs"]);
}

#[test]
fn native_scope_rejects_missing_sonic_offset_evidence_before_creating_output() {
    let (mut f, request) = bilingual();
    let mut parent = load(&f, "records/scene.tig");
    let scene_pin: Object = serde_json::from_value(parent["selected"].clone()).unwrap();
    let mut scene: Value = serde_json::from_slice(
        &fs::read(
            f.root()
                .join("cache/objects")
                .join(scene_pin.sha256.as_str()),
        )
        .unwrap(),
    )
    .unwrap();
    scene["languages"]["en"]["events"][0]["sonic_bindings"] = json!(["effect"]);
    parent["selected"] = json!(cached(&f, &serde_json::to_vec(&scene).unwrap()));
    parent["authored"]["binding_ids"]["effect"] = json!({"id":"cue/english"});
    let profile_slot = parent["streams"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["slot"] == "profile.json")
        .unwrap();
    let pin: Object = serde_json::from_value(profile_slot["object"].clone()).unwrap();
    let mut profile: Value = serde_json::from_slice(
        &fs::read(f.root().join("cache/objects").join(pin.sha256.as_str())).unwrap(),
    )
    .unwrap();
    profile["sonic_placement_offsets_by_binding"] = json!({"effect":{"samples":0,"evidence_binding":"missing-evidence","reason":"synthetic placement"}});
    profile_slot["object"] = json!(cached(&f, &serde_json::to_vec(&profile).unwrap()));
    write(f.root(), "records/scene.tig", &parent);
    save_commit(&mut f);
    let output = f.root().join("missing-evidence-output");
    assert!(reel::tig_scene::compile(&mut f.source(), &request, &output).is_err());
    assert!(!output.exists());
    #[cfg(windows)]
    {
        f.profile["byte_source"]["required"] = "prevent-mutation".into();
        let evidence = cached(&f, b"synthetic Sonic placement evidence");
        let mut record = load(&f, "records/cue-main.tig");
        record["id"] = "evidence/placement".into();
        record["selected"] = json!(evidence);
        write(f.root(), "records/placement-evidence.tig", &record);
        parent["authored"]["binding_ids"]["missing-evidence"] = json!({"id":"evidence/placement"});
        write(f.root(), "records/scene.tig", &parent);
        save_commit(&mut f);
        let mut held = record.clone();
        held["status"]["selection_state"] = json!("candidate-unselected");
        write(f.root(), "records/placement-evidence.tig", &held);
        save_commit(&mut f);
        let refused = f.root().join("unapproved-placement-evidence");
        let mut source = f.source();
        assert!(reel::tig_scene::compile(&mut source, &request, &refused).is_err());
        assert_eq!(source.byte_requests(), 5);
        assert!(!refused.exists());
        write(f.root(), "records/placement-evidence.tig", &record);
        save_commit(&mut f);
        let report = compile(&f, &request, "retained-placement-evidence");
        assert!(primary(&report, "language_inputs").contains(&evidence));
    }
}

#[test]
fn native_language_scope_supports_more_than_128_selected_bindings_without_rule_expansion() {
    let (mut f, request) = bilingual();
    let mut parent = load(&f, "records/scene.tig");
    let scene_pin: Object = serde_json::from_value(parent["selected"].clone()).unwrap();
    let mut scene: Value = serde_json::from_slice(
        &fs::read(
            f.root()
                .join("cache/objects")
                .join(scene_pin.sha256.as_str()),
        )
        .unwrap(),
    )
    .unwrap();
    let mut events = Vec::new();
    let mut markers = serde_json::Map::new();
    for i in 0..129 {
        let id = format!("cel/scaled-{i}");
        let key = format!("picture/{i}~selected");
        let mut cel = load(&f, "records/cel-a.tig");
        cel["id"] = id.clone().into();
        write(f.root(), &format!("records/scaled-{i}.tig"), &cel);
        parent["authored"]["binding_ids"][&key] = json!({"id":id});
        let marker = format!("marker-{i}");
        markers.insert(marker.clone(), json!(i * 2000));
        events.push(json!({"event_id":format!("event-{i}"),"cue_id":"en-1","semantic_trigger_id":marker,"picture_slot_id":format!("picture-{i}"),"picture_binding":key,"score":{"disposition":"silence"}}));
    }
    scene["languages"]["en"]["events"] = json!(events);
    parent["selected"] = json!(cached(&f, &serde_json::to_vec(&scene).unwrap()));
    let mut alignment = load(&f, "records/alignment-english.tig");
    let take = load(&f, "records/cue-english.tig");
    let raw=serde_json::to_vec(&json!({"schema":"reel.scene-native-alignment.v1","language":"en","cue_id":"en-1","selected_take_sha256":take["selected"]["sha256"],"sample_rate":48000,"cue_end_sample":258000,"semantic_markers":markers})).unwrap();
    alignment["streams"][0]["object"] = json!(cached(&f, &raw));
    write(f.root(), "records/alignment-english.tig", &alignment);
    let slot = parent["streams"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["slot"] == "profile.json")
        .unwrap();
    let pin: Object = serde_json::from_value(slot["object"].clone()).unwrap();
    let mut profile: Value = serde_json::from_slice(
        &fs::read(f.root().join("cache/objects").join(pin.sha256.as_str())).unwrap(),
    )
    .unwrap();
    profile["max_composition_samples"] = json!(288000);
    slot["object"] = json!(cached(&f, &serde_json::to_vec(&profile).unwrap()));
    write(f.root(), "records/scene.tig", &parent);
    save_commit(&mut f);
    let report = compile(&f, &request, "scaled-scope");
    assert_eq!(primary(&report, "language_inputs").len(), 3);
    assert!(
        report["language_inputs"]["interpretation_objects"]
            .as_array()
            .unwrap()
            .len()
            > 128
    );
    assert_eq!(primary(&report, "declared_inputs").len(), 6);
}
