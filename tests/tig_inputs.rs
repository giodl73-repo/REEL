#![cfg(feature = "tig")]
#[cfg(all(windows, feature = "tig-publication"))]
#[path = "support/tig_candidate_publication.rs"]
mod candidate_publication;
#[cfg(all(windows, feature = "tig-publication"))]
#[path = "support/tig_capture.rs"]
mod capture;
#[cfg(all(windows, feature = "tig-publication"))]
#[path = "support/tig_episode_lifecycle.rs"]
mod fake_episode_lifecycle;
#[path = "support/tig_language_scope.rs"]
mod language_scope;
#[cfg(all(windows, feature = "tig-publication"))]
#[path = "support/tig_native_cli.rs"]
mod native_cli;
#[cfg(all(windows, feature = "tig-publication"))]
#[path = "support/tig_native_job.rs"]
mod native_job;
#[path = "support/tig_presentation.rs"]
mod presentation;
#[cfg(all(windows, feature = "tig-publication"))]
#[path = "support/tig_producer_reuse.rs"]
mod producer_reuse;
#[cfg(all(windows, feature = "tig-publication"))]
#[path = "support/tig_retained_comparison.rs"]
mod retained_comparison;
#[cfg(all(windows, feature = "tig-publication"))]
#[path = "support/tig_store_setup.rs"]
mod store_setup;
#[cfg(all(windows, feature = "tig-publication"))]
#[path = "support/tig_runner.rs"]
mod tig_runner;
#[cfg(windows)]
use reel::scene_delivery::{FileRef, InputFiles};
use reel::{
    scene_delivery::Job,
    tig_inputs::{Profile, Source},
};
use reel_assembly::scene_authoring::AssetRef;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
#[cfg(windows)]
use std::process::Output;
use std::{
    collections::BTreeMap,
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
    let output = Command::new(git_executable())
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
fn write(root: &Path, name: &str, value: &Value) -> Object {
    let bytes = serde_json::to_vec(value).unwrap();
    fs::write(root.join(name), &bytes).unwrap();
    Object::from_bytes(&bytes)
}
fn reference(id: &str, object: &Object) -> Value {
    json!({"path":id,"sha256":object.sha256,"bytes":object.bytes})
}
fn picture(rgb: [u8; 3]) -> Vec<u8> {
    [b"P6\n8 8\n255\n".as_slice(), &rgb.repeat(64)].concat()
}
fn wav() -> Vec<u8> {
    let size = 48000u32 * 6;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((36 + size).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(48000u32.to_le_bytes());
    b.extend(288000u32.to_le_bytes());
    b.extend(6u16.to_le_bytes());
    b.extend(24u16.to_le_bytes());
    b.extend(b"data");
    b.extend(size.to_le_bytes());
    b.extend([0x10, 0, 0, 0x10, 0, 0].repeat(48000));
    b
}
struct Fixture {
    temp: tempfile::TempDir,
    profile: Value,
    #[cfg(windows)]
    media: Object,
    alignment: Vec<u8>,
    job: Value,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for dir in ["records", "definitions", "cache", "cache/objects"] {
            fs::create_dir(root.join(dir)).unwrap();
        }
        git(root, &["init", "-b", "main"]);
        git(root, &["config", "user.name", "Synthetic fixture"]);
        git(root, &["config", "user.email", "fixture@example.invalid"]);
        fs::write(root.join(".gitattributes"), "* text eol=lf\n").unwrap();
        fs::write(root.join(".gitignore"), "cache/\nexports/\n").unwrap();
        let schema = write(root, "definitions/authored.json", &json!({"type":"object"}));
        let mut definitions = Vec::new();
        for id in ["media", "alignment"] {
            definitions.push(write(root,&format!("definitions/{id}.json"),&json!({"schema":"tig.definition.design-1","id":format!("types/{id}"),"authored_schema":schema,"authoring":{"properties":[],"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}})));
        }
        let mut objects = BTreeMap::new();
        for (id, bytes) in [
            ("cel/a", picture([255, 0, 0])),
            ("cel/b", picture([0, 0, 255])),
            ("cue/main", wav()),
        ] {
            let object = Object::from_bytes(&bytes);
            fs::write(
                root.join("cache/objects").join(object.sha256.as_str()),
                bytes,
            )
            .unwrap();
            write(
                root,
                &format!("records/{}.tig", id.replace('/', "-")),
                &json!({"schema":"tig.instance.design-1","id":id,"definition":{"id":"types/media","sha256":definitions[0].sha256,"bytes":definitions[0].bytes},"authored":{},"streams":[],"selected":object,"status":{},"metadata":{}}),
            );
            objects.insert(id.to_owned(), object);
        }
        let mut streams = Vec::new();
        let mut alignment = Vec::new();
        for language in ["es", "en"] {
            let bytes=serde_json::to_vec(&json!({"schema":"reel.scene-native-alignment.v1","language":language,"cue_id":format!("{language}-1"),"selected_take_sha256":objects["cue/main"].sha256,"sample_rate":48000,"cue_end_sample":48000,"semantic_markers":{"first-line":0}})).unwrap();
            let object = Object::from_bytes(&bytes);
            fs::write(
                root.join("cache/objects").join(object.sha256.as_str()),
                &bytes,
            )
            .unwrap();
            streams.push(
                json!({"slot":language,"role":"alignment","group":"language","object":object}),
            );
            if language == "es" {
                alignment = bytes;
            }
        }
        write(
            root,
            "records/alignment.tig",
            &json!({"schema":"tig.instance.design-1","id":"alignment/main","definition":{"id":"types/alignment","sha256":definitions[1].sha256,"bytes":definitions[1].bytes},"authored":{},"streams":streams,"selected":null,"status":{},"metadata":{}}),
        );
        let production = write(
            root,
            "production.json",
            &json!({"manifest_version":"reel.manifest.v0.2","profile":"animatic","timing_status":"conformed","work":"synthetic","title":"Synthetic","scenes":[{"id":"scene"}],"shots":[{"id":"shot","scene_id":"scene"}],"narration_cues":[{"id":"main","speaker_id":"narrator"}],"audio_events":[{"id":"main","role":"narration","source":"cue/main","start_seconds":0}]}),
        );
        let contract = write(
            root,
            "contract.json",
            &json!({"schema":"reel.cue-relative-assembly.v0.1","id":"scene","production_manifest":"production.json","sample_rate":48000,"frame_rate":{"numerator":24,"denominator":1},"cues":[{"cue_id":"main","duration_samples":48000}],"attachments":[
                {"id":"p-a","target":{"kind":"cel","shot_id":"shot","cel_id":"a"},"start":{"kind":"cue-start","cue_id":"main"},"end":{"kind":"cue-start","cue_id":"main","offset_samples":24000}},
                {"id":"p-b","target":{"kind":"cel","shot_id":"shot","cel_id":"b"},"start":{"kind":"cue-start","cue_id":"main","offset_samples":24000},"end":{"kind":"cue-end","cue_id":"main"}},
                {"id":"d","target":{"kind":"audio","audio_event_id":"main"},"start":{"kind":"cue-start","cue_id":"main"},"end":{"kind":"cue-end","cue_id":"main"}}
            ]}),
        );
        let job = json!({"schema":"reel.scene-delivery.v0.1","id":"scene","contract":reference("contract.json",&contract),"production_manifest_sha256":production.sha256,"width":64,"height":64,"max_composition_samples":480000,"pictures":[{"attachment_id":"p-a","source":reference("cel/a",&objects["cel/a"]),"kind":"still","attention":"First"},{"attachment_id":"p-b","source":reference("cel/b",&objects["cel/b"]),"kind":"still","attention":"Second"}],"audio":[{"attachment_id":"d","source":reference("cue/main",&objects["cue/main"]),"bus":"D","cue_id":"main"}],"buses":{"D":{"state":"present","reason":"Synthetic cue"},"M":{"state":"intentional-silence","reason":"No music"},"E":{"state":"intentional-silence","reason":"No effects"}}});
        write(root, "job.json", &job);
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "synthetic native assets"]);
        let profile = json!({"schema":"reel.tig-input-profile.experimental-1","git_executable":git_executable(),"repository":root,"commit":git(root,&["rev-parse","HEAD"]),"records":["records"],"definitions":["definitions"],"admitted":[{"id":"types/media","object":definitions[0],"payload":{"kind":"selected"}},{"id":"types/alignment","object":definitions[1],"payload":{"kind":"stream","slot":"es"}}],"byte_source":{"places":[{"root":root.join("cache"),"layout":"flat"}],"required":"cooperative-immutable"}});
        Self {
            temp,
            profile,
            #[cfg(windows)]
            media: definitions[0].clone(),
            alignment,
            job,
        }
    }
    fn root(&self) -> &Path {
        self.temp.path()
    }
    fn source(&self) -> Source {
        Source::open(serde_json::from_value(self.profile.clone()).unwrap()).unwrap()
    }
    fn asset(&self) -> AssetRef {
        let object = Object::from_bytes(&self.alignment);
        AssetRef {
            logical_id: "alignment/main".into(),
            sha256: object.sha256.as_str().into(),
            bytes: object.bytes,
            cache_uri: format!("cache://sha256/{}", object.sha256.as_str()),
            selection_state: "selected-private-production".into(),
        }
    }
    #[cfg(windows)]
    fn add(&self, id: &str, bytes: &[u8]) -> Object {
        let object = Object::from_bytes(bytes);
        fs::write(
            self.root()
                .join("cache/objects")
                .join(object.sha256.as_str()),
            bytes,
        )
        .unwrap();
        write(
            self.root(),
            &format!("records/{}.tig", id.replace(['/', ':'], "-")),
            &json!({"schema":"tig.instance.design-1","id":id,"definition":{"id":"types/media","sha256":self.media.sha256,"bytes":self.media.bytes},"authored":{},"streams":[],"selected":object,"status":{},"metadata":{}}),
        );
        object
    }
    #[cfg(windows)]
    fn commit(&mut self) {
        git(self.root(), &["add", "records"]);
        git(self.root(), &["commit", "-m", "synthetic revision"]);
        self.profile["commit"] = json!(git(self.root(), &["rev-parse", "HEAD"]));
    }
}

#[test]
fn vendored_runtime_source_matches_exact_upstream_pin() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/tig");
    let manifest: Value =
        serde_json::from_slice(&fs::read(root.join("source-pin.json")).unwrap()).unwrap();
    assert_eq!(
        manifest["commit"],
        "9252a8a1822e99155e5d2bc9bdd4a96476cfe22f"
    );
    let mut listed = std::collections::BTreeSet::new();
    let mut total = 0;
    for item in manifest["files"].as_array().unwrap() {
        let path = item["path"].as_str().unwrap();
        assert!(!path.contains(".."));
        assert!(!Path::new(path).is_absolute());
        let bytes = fs::read(root.join(path)).unwrap();
        assert_eq!(bytes.len() as u64, item["bytes"].as_u64().unwrap());
        let sha = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(sha, item["sha256"].as_str().unwrap());
        assert!(listed.insert(path.to_owned()));
        total += bytes.len();
    }
    fn walk(root: &Path, path: &Path, found: &mut std::collections::BTreeSet<String>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            assert!(
                !fs::symlink_metadata(&path)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            if path.is_dir() {
                walk(root, &path, found);
            } else if path.extension().is_some_and(|x| x == "rs")
                || path.file_name().is_some_and(|x| x == "Cargo.toml")
            {
                found.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut actual = std::collections::BTreeSet::new();
    walk(&root, &root, &mut actual);
    actual.insert("LICENSE".into());
    actual.insert("Cargo.lock".into());
    assert_eq!(listed, actual);
    assert_eq!(listed.len(), 65);
    assert_eq!(total, 694972);
}

#[test]
fn payload_selectors_reject_extra_selected_or_stream_fields_in_git_and_archive_profiles() {
    let f = Fixture::new();
    for selector in [
        json!({"kind":"selected","slot":"en"}),
        json!({"kind":"selected","role":"audio"}),
        json!({"kind":"selected","unexpected":null}),
        json!({"kind":"stream","slot":"en","unexpected":true}),
    ] {
        let mut value = f.profile.clone();
        value["admitted"][0]["payload"] = selector.clone();
        let path = f.root().join("malformed-profile.json");
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            Profile::read(&path).is_err(),
            "malformed payload selector was accepted"
        );
        assert!(serde_json::from_value::<Profile>(value).is_err());
        let archive = json!({"schema":reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA,"archive":Object::from_bytes(b"archive"),"checkpoint":tig_core::Digest::blob(b"checkpoint"),"admitted":f.profile["admitted"],"byte_source":f.profile["byte_source"]});
        let mut archive = archive;
        archive["admitted"][0]["payload"] = selector;
        assert!(serde_json::from_value::<reel::tig_inputs::ArchiveProfile>(archive).is_err());
    }
    let selected: Profile = serde_json::from_value(f.profile.clone()).unwrap();
    assert_eq!(Source::open(selected).unwrap().byte_requests(), 0);
    let mut stream = f.profile.clone();
    stream["admitted"][0]["payload"] = json!({"kind":"stream","slot":"en"});
    let stream: Profile = serde_json::from_value(stream).unwrap();
    assert_eq!(Source::open(stream).unwrap().byte_requests(), 0);
}

fn native_scene_fixture() -> (Fixture, reel::tig_scene::Request) {
    let mut f = Fixture::new();
    let root = f.root();
    let cached = |bytes: &[u8]| {
        let object = Object::from_bytes(bytes);
        fs::write(
            root.join("cache/objects").join(object.sha256.as_str()),
            bytes,
        )
        .unwrap();
        object
    };
    let load = |name: &str| -> Value {
        serde_json::from_slice(&fs::read(root.join(name)).unwrap()).unwrap()
    };
    for name in ["cel-a", "cel-b", "cue-main", "alignment"] {
        let path = format!("records/{name}.tig");
        let mut record = load(&path);
        record["status"] = json!({"selection_state":"selected-private-production"});
        if name == "alignment" {
            let bytes = serde_json::to_vec(&json!({"schema":"reel.scene-native-alignment.v1","language":"es","cue_id":"es-1","selected_take_sha256":load("records/cue-main.tig")["selected"]["sha256"],"sample_rate":48000,"cue_end_sample":48000,"semantic_markers":{"first-line":0,"second-line":24000}})).unwrap();
            record["streams"][0]["object"] = serde_json::to_value(cached(&bytes)).unwrap();
        }
        write(root, &path, &record);
    }
    let schema = Object::from_bytes(&fs::read(root.join("definitions/authored.json")).unwrap());
    let definition = write(
        root,
        "definitions/scene.json",
        &json!({"schema":"tig.definition.design-1","id":"types/scene","authored_schema":schema,"authoring":{"properties":["scene_id","binding_ids"],"required":["scene_id","binding_ids"],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
    );
    let scene = json!({"schema":"reel.scene-authoring.v1","scene_id":"scene-1","episode_id":"episode-1","authoring_state":"ready-for-private-build","source_scope_ids":["source-block"],"source_authority_id":"synthetic-source","holds":["Private review remains external"],
    "languages":{"es":{"cues":[{"cue_id":"es-1","source_id":"source-block","exact_text_sha256":"a".repeat(64),"narration_slot_id":"narration","take_binding":"take","phrase_alignment_binding":"alignment"}],"events":[
        {"event_id":"first","cue_id":"es-1","semantic_trigger_id":"first-line","picture_slot_id":"p-a","picture_binding":"a","score":{"disposition":"silence"}},
        {"event_id":"second","cue_id":"es-1","semantic_trigger_id":"second-line","picture_slot_id":"p-b","picture_binding":"b","score":{"disposition":"silence"}}
    ]}}});
    let selected = cached(&serde_json::to_vec(&scene).unwrap());
    let mut streams = Vec::new();
    for (slot, value) in [
        (
            "catalog.json",
            json!({"schema":"reel.scene-template-catalog.v1","templates":[]}),
        ),
        (
            "episode.json",
            json!({"schema":"reel.episode-authoring.v1","episode_id":"episode-1","season_id":"season-1","authoring_state":"scene-build-context","master_template_id":"master","scene_policy_id":"policy","presentation":[],"score_palette":[],"scene_ids":["scene-1"]}),
        ),
        (
            "policy.json",
            json!({"schema":"reel.scene-policy.v1","policy_id":"policy","target_composition_seconds_min":0.5,"target_composition_seconds_max":2.0,"hard_unchanged_composition_seconds_max":2.0,"semantic_cuts_required":true}),
        ),
        (
            "profile.json",
            json!({"schema":"reel.scene-delivery-profile.v1","width":64,"height":64,"sample_rate":48000,"frame_rate_numerator":24,"frame_rate_denominator":1,"max_composition_samples":96000,"score_gain_db":-18,"score_fade_in_samples":0,"score_fade_out_samples":0}),
        ),
    ] {
        streams.push(json!({"slot":slot,"role":"native-control","group":"control","object":cached(&serde_json::to_vec(&value).unwrap())}));
    }
    // This retained old binding object is deliberately unavailable and unusable.
    streams.push(json!({"slot":"bindings.json","role":"old-evidence","group":"evidence","object":Object::from_bytes(b"unavailable old bindings")}));
    write(
        root,
        "records/scene.tig",
        &json!({"schema":"tig.instance.design-1","id":"scene/native","definition":{"id":"types/scene","sha256":definition.sha256,"bytes":definition.bytes},"authored":{"scene_id":"scene-1","binding_ids":{"a":{"id":"cel/a"},"b":{"id":"cel/b"},"take":{"id":"cue/main"},"alignment":{"id":"alignment/main"}}},"selected":selected,"streams":streams,"status":{"new_production_approval":false},"metadata":{}}),
    );
    git(root, &["add", "records", "definitions"]);
    git(root, &["commit", "-m", "synthetic native scene controls"]);
    f.profile["commit"] = json!(git(root, &["rev-parse", "HEAD"]));
    f.profile["admitted"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"types/scene","object":definition,"payload":{"kind":"selected"}}));
    let request = serde_json::from_value(json!({"schema":"reel.tig-native-scene.experimental-1","scene_id":"scene/native","scene_definition":"types/scene","primary_definitions":["types/media","types/alignment"],"language":"es","controls":{"catalog":"catalog.json","episode":"episode.json","policy":"policy.json","profile":"profile.json"},"delivery_id":"native","delivery_title":"Synthetic native scene"})).unwrap();
    (f, request)
}

#[test]
fn native_scene_compiles_fresh_leaf_versions_without_changing_parent_or_old_view() {
    let (mut f, request) = native_scene_fixture();
    let parent = fs::read(f.root().join("records/scene.tig")).unwrap();
    let mut old_view = f.source();
    let first = f.root().join("native-first");
    let result = reel::tig_scene::compile(&mut old_view, &request, &first).unwrap();
    assert_eq!(result["semantic_event_count"], 2);
    assert_eq!(old_view.byte_requests(), 6);
    let job = |dir: &Path| -> Value {
        serde_json::from_slice(&fs::read(dir.join("delivery/job.json")).unwrap()).unwrap()
    };
    let old_job = job(&first);
    let cli_profile = f.root().join("native-profile.json");
    fs::write(&cli_profile, serde_json::to_vec(&f.profile).unwrap()).unwrap();
    let cli_request = f.root().join("native-request.json");
    fs::write(&cli_request, serde_json::to_vec(&request).unwrap()).unwrap();
    let cli_dir = f.root().join("native-cli");
    let cli = Command::new(env!("CARGO_BIN_EXE_reel-tig-scene"))
        .arg("--profile")
        .arg(&cli_profile)
        .arg("compile-native")
        .arg(&cli_request)
        .arg(&cli_dir)
        .output()
        .unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let cli_receipt: Value = serde_json::from_slice(&cli.stdout).unwrap();
    assert_eq!(cli_receipt["complete_semantic_coverage"], false);
    assert_eq!(job(&cli_dir), old_job);
    assert_eq!(old_job["pictures"][1]["source"]["path"], "cel/b");
    assert_eq!(old_job["audio"][0]["source"]["path"], "cue/main");
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(first.join("scene.json")).unwrap()).unwrap()["holds"],
        json!(["Private review remains external"])
    );
    let replacement = picture([0, 255, 0]);
    let pin = Object::from_bytes(&replacement);
    fs::write(
        f.root().join("cache/objects").join(pin.sha256.as_str()),
        replacement,
    )
    .unwrap();
    let mut leaf: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/cel-b.tig")).unwrap()).unwrap();
    leaf["selected"] = serde_json::to_value(&pin).unwrap();
    write(f.root(), "records/cel-b.tig", &leaf);
    git(f.root(), &["add", "records"]);
    git(f.root(), &["commit", "-m", "change only cel B"]);
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
    let next = f.root().join("native-next");
    reel::tig_scene::compile(&mut f.source(), &request, &next).unwrap();
    let next_job = job(&next);
    let context = |dir: &Path| -> Value {
        serde_json::from_slice(&fs::read(dir.join("native-source.json")).unwrap()).unwrap()
    };
    let before_context = context(&first);
    let next_context = context(&next);
    assert_eq!(before_context["scene_record"], next_context["scene_record"]);
    assert_eq!(
        before_context["declared_inputs"]["control_objects"],
        next_context["declared_inputs"]["control_objects"]
    );
    assert_ne!(
        before_context["declared_inputs"]["binding_sha256"],
        next_context["declared_inputs"]["binding_sha256"]
    );
    let objects = |value: &Value| -> std::collections::BTreeSet<Object> {
        serde_json::from_value(value["declared_inputs"]["primary_objects"].clone()).unwrap()
    };
    let before = objects(&before_context);
    let after = objects(&next_context);
    assert_eq!(before.difference(&after).count(), 1);
    assert_eq!(
        after.difference(&before).cloned().collect::<Vec<_>>(),
        vec![pin.clone()]
    );
    assert_eq!(next_job["pictures"][0], old_job["pictures"][0]);
    assert_eq!(next_job["audio"], old_job["audio"]);
    assert_eq!(
        next_job["pictures"][1]["source"]["sha256"],
        pin.sha256.as_str()
    );
    assert_ne!(
        next_job["pictures"][1]["source"],
        old_job["pictures"][1]["source"]
    );
    assert_eq!(
        fs::read(f.root().join("records/scene.tig")).unwrap(),
        parent
    );
    let replay = f.root().join("native-replay");
    reel::tig_scene::compile(&mut old_view, &request, &replay).unwrap();
    assert_eq!(job(&replay), old_job);
    assert_eq!(old_view.active_readers(), 0);
    assert!(reel::tig_scene::compile(&mut f.source(), &request, &next).is_err());
}

#[test]
fn native_requirements_are_metadata_only_and_recorded_in_existing_private_source_context() {
    let (f, request) = native_scene_fixture();
    let mut source = f.source();
    let required = reel::tig_scene::requirements(&mut source, &request).unwrap();
    assert_eq!(source.byte_requests(), 0);
    assert_eq!(required.primary().len(), 4);
    assert_eq!(required.controls().len(), 5);
    assert_eq!(required.metadata().len(), 9);
    assert_eq!(required.all_objects().len(), 18);
    assert!(
        !required
            .all_objects()
            .contains(&Object::from_bytes(b"unavailable old bindings"))
    );
    let output = f.root().join("requirements-build");
    reel::tig_scene::compile(&mut source, &request, &output).unwrap();
    let context: Value =
        serde_json::from_slice(&fs::read(output.join("native-source.json")).unwrap()).unwrap();
    assert_eq!(
        context["declared_inputs"]["primary_objects"],
        serde_json::to_value(required.primary()).unwrap()
    );
    assert_eq!(
        context["declared_inputs"]["control_objects"],
        serde_json::to_value(required.controls()).unwrap()
    );
    assert_eq!(
        context["declared_inputs"]["interpretation_objects"],
        serde_json::to_value(required.metadata()).unwrap()
    );
    assert_eq!(
        context["declared_inputs"]["source_checkpoint"],
        serde_json::to_value(source.checkpoint()).unwrap()
    );
    assert_eq!(context["declared_inputs"]["authorizes_eviction"], false);
    assert_eq!(
        context["declared_inputs"]["complete_semantic_coverage"],
        false
    );
    assert_eq!(context["new_production_approval"], false);
    assert_eq!(
        context["tig_source_pin_sha256"],
        serde_json::to_value(
            Object::from_bytes(include_bytes!("../vendor/tig/source-pin.json")).sha256
        )
        .unwrap()
    );
}

#[test]
fn native_recipe_binds_explicit_producer_and_request_without_payload_reads() {
    let (f, mut request) = native_scene_fixture();
    let producer = Object::from_bytes(b"externally admitted synthetic producer package");
    let mut source = f.source();
    let original = reel::tig_scene::recipe(&mut source, &request, &producer).unwrap();
    assert_eq!(original.nodes().len(), 5);
    assert!(!original.complete_semantic_coverage());
    let upgraded = reel::tig_scene::recipe(
        &mut source,
        &request,
        &Object::from_bytes(b"different producer package"),
    )
    .unwrap();
    assert_ne!(upgraded.fingerprint(), original.fingerprint());
    assert_eq!(upgraded.nodes(), original.nodes());
    request.language = "en".into();
    let english = reel::tig_scene::recipe(&mut source, &request, &producer).unwrap();
    assert_ne!(english.fingerprint(), original.fingerprint());
    assert_ne!(
        english.operation().parameters,
        original.operation().parameters
    );
    assert_eq!(english.nodes(), original.nodes());
    request.language = "es".into();
    request.delivery_title = "Another title".into();
    assert_ne!(
        reel::tig_scene::recipe(&mut source, &request, &producer)
            .unwrap()
            .fingerprint(),
        original.fingerprint()
    );
    request.controls.template_receipt = Some("missing-control".into());
    assert!(reel::tig_scene::recipe(&mut source, &request, &producer).is_err());
    request.controls.template_receipt = None;
    request.delivery_title = "x".repeat(65_536);
    assert!(reel::tig_scene::recipe(&mut source, &request, &producer).is_err());
    request.delivery_title.clear();
    let unsafe_producer = Object {
        sha256: producer.sha256.clone(),
        bytes: u64::MAX,
    };
    assert!(reel::tig_scene::recipe(&mut source, &request, &unsafe_producer).is_err());
    request.primary_definitions.clear();
    assert!(reel::tig_scene::recipe(&mut source, &request, &producer).is_err());
    assert_eq!(source.byte_requests(), 0);
    assert_eq!(source.active_readers(), 0);
}

#[test]
fn native_recipe_distinguishes_production_changes_from_annotation_and_status() {
    for change in [
        "annotation",
        "status",
        "cel",
        "cue",
        "control",
        "scene",
        "bindings",
        "scene-id",
    ] {
        let (mut f, request) = native_scene_fixture();
        let producer = Object::from_bytes(b"synthetic producer");
        let mut old = f.source();
        let before = reel::tig_scene::recipe(&mut old, &request, &producer).unwrap();
        let file = match change {
            "annotation" | "status" | "cel" => "records/cel-b.tig",
            "cue" => "records/cue-main.tig",
            _ => "records/scene.tig",
        };
        let mut record: Value =
            serde_json::from_slice(&fs::read(f.root().join(file)).unwrap()).unwrap();
        match change {
            "annotation" => record["metadata"]["review_note"] = json!("new private note"),
            "status" => record["status"]["selection_state"] = json!("held"),
            "cel" | "cue" | "scene" => {
                record["selected"] =
                    serde_json::to_value(Object::from_bytes(change.as_bytes())).unwrap();
            }
            "control" => {
                let control = record["streams"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|stream| stream["slot"] == "profile.json")
                    .unwrap();
                control["object"] =
                    serde_json::to_value(Object::from_bytes(b"changed profile bytes")).unwrap();
            }
            "bindings" => {
                let a = record["authored"]["binding_ids"]["a"].clone();
                record["authored"]["binding_ids"]["a"] =
                    record["authored"]["binding_ids"]["b"].clone();
                record["authored"]["binding_ids"]["b"] = a;
            }
            "scene-id" => record["authored"]["scene_id"] = json!("another-scene"),
            _ => unreachable!(),
        }
        write(f.root(), file, &record);
        git(f.root(), &["add", "records"]);
        git(
            f.root(),
            &["commit", "-m", "change one production or annotation field"],
        );
        f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
        let mut current = f.source();
        let after = reel::tig_scene::recipe(&mut current, &request, &producer).unwrap();
        assert_ne!(before.checkpoint(), after.checkpoint(), "{change}");
        assert_ne!(before.binding(), after.binding(), "{change}");
        assert_eq!(
            before.fingerprint() == after.fingerprint(),
            matches!(change, "annotation" | "status"),
            "{change}"
        );
        assert_eq!(
            reel::tig_scene::recipe(&mut old, &request, &producer)
                .unwrap()
                .fingerprint(),
            before.fingerprint(),
            "older captured source must remain independent: {change}"
        );
        if change == "status" {
            // Recipe equality cannot turn an ineligible leaf into a usable input.
            let output = f.root().join("must-not-compile");
            assert!(reel::tig_scene::compile(&mut current, &request, &output).is_err());
            assert!(!output.exists());
        }
        assert_eq!(old.byte_requests(), 0, "{change}");
        assert_eq!(
            current.byte_requests(),
            if change == "status" { 5 } else { 0 },
            "{change}"
        );
    }
}

#[test]
fn native_render_candidate_refuses_foreign_context_before_payload_or_output_io() {
    use tig_core::tree::{MemoryNodes, TreeLimits};
    let (mut f, mut request) = native_scene_fixture();
    let producer = Object::from_bytes(b"synthetic admitted producer");
    let mut source = f.source();
    let context = reel::tig_scene::freeze_context(
        &mut source,
        &request,
        &mut MemoryNodes::default(),
        TreeLimits::default(),
    )
    .unwrap();
    let job = f.root().join("must-not-create-job");
    let output = f.root().join("must-not-render");
    request.language = "en".into();
    assert!(
        reel::tig_scene_production::render(
            &mut source,
            &request,
            &context,
            &producer,
            &job,
            &output
        )
        .is_err()
    );
    assert_eq!(source.byte_requests(), 0);
    request.language = "es".into();
    let file = "records/cel-b.tig";
    let mut record: Value =
        serde_json::from_slice(&fs::read(f.root().join(file)).unwrap()).unwrap();
    record["metadata"]["note"] = json!("same recipe, different source context");
    write(f.root(), file, &record);
    git(f.root(), &["add", "records"]);
    git(f.root(), &["commit", "-m", "advance source context only"]);
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
    let mut advanced = f.source();
    assert_eq!(
        reel::tig_scene::recipe(&mut source, &request, &producer)
            .unwrap()
            .fingerprint(),
        reel::tig_scene::recipe(&mut advanced, &request, &producer)
            .unwrap()
            .fingerprint()
    );
    assert!(
        reel::tig_scene_production::render(
            &mut advanced,
            &request,
            &context,
            &producer,
            &job,
            &output
        )
        .is_err()
    );
    assert_eq!(advanced.byte_requests(), 0);
    assert!(!job.exists());
    assert!(!output.exists());
}

#[cfg(windows)]
#[test]
#[ignore = "requires native FFmpeg; native render-to-candidate-claim handoff"]
fn native_render_candidate_claims_only_independently_checked_matching_bytes() {
    use reel::tig_scene_production::{self, OutputBinding};
    use tig_assets::{
        graph::Reference,
        graph::recipe::produced::{BoundSubject, Claim, Selector},
        native::Instance,
    };
    use tig_core::tree::TreeLimits;
    use tig_store::{
        Key,
        metadata::{MetadataReader, MetadataStore},
        source_claims::{Command as OwnerCommand, SourceJournal},
    };
    let (mut f, request) = native_scene_fixture();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    #[cfg(feature = "tig-publication")]
    native_cli::seed(&mut f);
    let native_file = |path: std::path::PathBuf| {
        let path = path.canonicalize().unwrap();
        let identity = reel::media_runtime::ByteIdentity::observe_file(&path).unwrap();
        reel::scene_delivery::FileRef {
            sha256: identity.sha256,
            bytes: identity.bytes,
            path,
        }
    };
    let locate = |name: &str| {
        std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|dir| dir.join(format!("{name}.exe")))
            .find(|path| path.is_file())
            .unwrap()
    };
    let host = native_file(std::env::current_exe().unwrap());
    let programs = reel::media_runtime::BoundPrograms::bind(&reel::media_runtime::Bindings {
        host: reel::media_runtime::ByteIdentity {
            sha256: host.sha256,
            bytes: host.bytes,
        },
        ffmpeg: native_file(locate("ffmpeg")),
        ffprobe: native_file(locate("ffprobe")),
        dependencies: Default::default(),
    })
    .unwrap();
    let producer = Object::from_bytes(programs.manifest_bytes());
    let mut source = f.source();
    let metadata = f.root().join("native-job-metadata");
    let mut store = MetadataStore::create(
        &metadata,
        tig_store::metadata::Limits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let context =
        reel::tig_scene::freeze_context(&mut source, &request, &mut store, TreeLimits::default())
            .unwrap();
    let profile = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    let checkpoint = store.finish().unwrap();
    drop(store);
    drop(source);
    let mut reader = MetadataReader::open(&metadata, &checkpoint).unwrap();
    let (mut source, context) = reel::tig_scene::restore_context(
        profile.clone(),
        &request,
        context.selection(),
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    let key = |s: &str| Key::parse(s).unwrap();
    let mut owners =
        SourceJournal::create(&f.root().join("job-owner"), key("fixture"), key("epoch")).unwrap();
    let prepared = owners
        .apply(
            key("prepare"),
            OwnerCommand::Prepare {
                source: key("job"),
                owner: key("test"),
                expected_revision: 0,
                selection: context.selection().clone(),
            },
        )
        .unwrap();
    owners
        .apply(
            key("adopt"),
            OwnerCommand::Adopt {
                source: key("job"),
                owner: key("test"),
                expected_revision: 1,
                prepared: prepared.reference,
            },
        )
        .unwrap();
    let job = f.root().join("rendered-native-job");
    let output = f.root().join("rendered-native-output");
    let candidate = tig_scene_production::render_with_programs(
        &mut source,
        &request,
        &context,
        &programs,
        &job,
        &output,
    )
    .unwrap();
    assert_eq!(&candidate.recipe().operation().producer, &producer);
    assert_eq!(source.active_readers(), 0);
    assert_eq!(
        candidate.recipe().operation().parameters,
        Object::from_bytes(context.descriptor())
    );
    assert_eq!(
        candidate.receipt_object(),
        &Object::from_bytes(&fs::read(output.join("receipt.json")).unwrap())
    );
    assert!(candidate.claim(&[]).is_err());
    let schema = fs::read(f.root().join("definitions/authored.json")).unwrap();
    let definition = serde_json::to_vec(&json!({"schema":"tig.definition.design-1","id":"types/output","authored_schema":Object::from_bytes(&schema),"authoring":{"properties":[],"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}})).unwrap();
    let admission = Reference {
        id: "types/output".into(),
        object: Object::from_bytes(&definition),
    };
    let bind_as = |id: &str, pin: &Object| {
        let record = serde_json::to_vec(&json!({"schema":"tig.instance.design-1","id":id,"definition":{"id":admission.id,"sha256":admission.object.sha256,"bytes":admission.object.bytes},"authored":{},"selected":pin,"streams":[],"status":{},"metadata":{}})).unwrap();
        let bound = Instance::parse(&record)
            .unwrap()
            .bind(&definition, &schema)
            .unwrap();
        BoundSubject::bind(&bound, &admission, Selector::Selected {})
            .unwrap()
            .unwrap()
    };
    let bind = |pin: &Object| bind_as("delivery/scene", pin);
    #[cfg(feature = "tig-publication")]
    {
        let plan = reel::tig_scene_production::jobs::JobPlan::from_context(
            &mut source,
            &request,
            &context,
            &programs,
        )
        .unwrap();
        native_job::exercise(
            &plan,
            &mut source,
            &mut owners,
            f.root(),
            (&metadata, &checkpoint),
            bind_as,
        );
    }
    let original = fs::read(output.join("master.mkv")).unwrap();
    let subject = bind(&Object::from_bytes(&original));
    let mapping = || OutputBinding {
        output_name: "master.mkv",
        subject: &subject,
    };
    let claim = candidate.claim(&[mapping()]).unwrap();
    let claim = Claim::parse(claim.object(), claim.bytes()).unwrap();
    claim.check_plan(candidate.recipe()).unwrap();
    assert_eq!(claim.context().root, context.selection().root);
    assert_eq!(claim.outputs(), &[subject.subject().clone()]);
    #[cfg(feature = "tig-publication")]
    let claim = candidate_publication::exercise(&candidate, &subject, f.root(), bind_as);
    #[cfg(feature = "tig-publication")]
    native_cli::exercise(
        f.root(),
        &request,
        context.selection(),
        (&metadata, &checkpoint),
        json!({"schema":reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA,"archive":context.archive(),"checkpoint":source.checkpoint(),"admitted":f.profile["admitted"],"byte_source":f.profile["byte_source"]}),
        candidate.receipt(),
    );
    drop(reader);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut reader = MetadataReader::open(&metadata, &checkpoint).unwrap();
    let (restored, replay) = reel::tig_scene::restore_claim(
        profile,
        &request,
        &producer,
        &claim,
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(restored.byte_requests(), 0);
    assert_eq!(replay.context().selection(), context.selection());
    assert_eq!(
        tig_assets::graph::recipe::produced::compare(
            candidate.recipe(),
            Some(&subject),
            Some(replay.checked()),
        )
        .unwrap(),
        tig_assets::graph::recipe::produced::Correspondence::ClaimedRecipeMatch
    );
    assert!(candidate.claim(&[mapping(), mapping()]).is_err());
    assert!(
        candidate
            .claim(&[OutputBinding {
                output_name: "unknown",
                subject: &subject
            }])
            .is_err()
    );
    let wrong = bind(&Object::from_bytes(b"foreign candidate"));
    assert!(
        candidate
            .claim(&[OutputBinding {
                output_name: "master.mkv",
                subject: &wrong
            }])
            .is_err()
    );
    let oversized: Vec<_> = std::iter::repeat_with(mapping).take(257).collect();
    assert!(candidate.claim(&oversized).is_err());
    let mut same_size_damage = original.clone();
    same_size_damage[0] ^= 1;
    fs::write(output.join("master.mkv"), same_size_damage).unwrap();
    assert!(candidate.claim(&[mapping()]).is_err());
    fs::write(output.join("master.mkv"), b"damaged after render").unwrap();
    assert!(candidate.claim(&[mapping()]).is_err());
    fs::write(output.join("master.mkv"), original).unwrap();
    assert!(candidate.claim(&[mapping()]).is_ok());
    let receipt = fs::read(output.join("receipt.json")).unwrap();
    fs::write(output.join("receipt.json"), b"changed receipt").unwrap();
    assert!(candidate.claim(&[mapping()]).is_err());
    fs::write(output.join("receipt.json"), receipt).unwrap();
    assert!(
        tig_scene_production::render(&mut source, &request, &context, &producer, &job, &output)
            .is_err()
    );
    assert!(!owners.obligations(10).unwrap().complete_claim_coverage());
}

fn metadata_claim_fixture(
    f: &Fixture,
    request: &reel::tig_scene::Request,
    producer: &Object,
    store: &mut impl tig_core::tree::NodeStore,
) -> (
    tig_assets::graph::recipe::produced::Claim,
    reel::tig_inputs::ArchiveProfile,
    tig_assets::graph::recipe::produced::BoundSubject,
) {
    use tig_assets::{
        graph::{
            Reference,
            recipe::produced::{BoundSubject, Claim, ContextPin, Selector},
        },
        native::Instance,
    };
    let mut source = f.source();
    let plan = reel::tig_scene::recipe(&mut source, request, producer).unwrap();
    let context = reel::tig_scene::freeze_context(
        &mut source,
        request,
        store,
        tig_core::tree::TreeLimits::default(),
    )
    .unwrap();
    let schema = fs::read(f.root().join("definitions/authored.json")).unwrap();
    let definition = serde_json::to_vec(&json!({"schema":"tig.definition.design-1","id":"types/output","authored_schema":Object::from_bytes(&schema),"authoring":{"properties":[],"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}})).unwrap();
    let admission = Reference {
        id: "types/output".into(),
        object: Object::from_bytes(&definition),
    };
    // Unsigned metadata fixture only: never evidence of actual producer execution.
    let selected = Object::from_bytes(b"unit-test assertion, not actual output proof");
    let record = serde_json::to_vec(&json!({"schema":"tig.instance.design-1","id":"delivery/test","definition":{"id":admission.id,"sha256":admission.object.sha256,"bytes":admission.object.bytes},"authored":{},"selected":selected,"streams":[],"status":{},"metadata":{}})).unwrap();
    let bound = Instance::parse(&record)
        .unwrap()
        .bind(&definition, &schema)
        .unwrap();
    let subject = BoundSubject::bind(&bound, &admission, Selector::Selected {})
        .unwrap()
        .unwrap();
    let claim = Claim::create(
        &plan,
        ContextPin {
            root: context.selection().root.clone(),
            identity: context.selection().context.clone(),
        },
        vec![subject.subject().clone()],
    )
    .unwrap();
    assert_eq!(source.byte_requests(), 0);
    (
        claim,
        archived_profile(f, context.archive().clone(), source.checkpoint().clone()),
        subject,
    )
}

struct ReadOnlyMetadata<'a, T> {
    inner: &'a T,
    reads: std::cell::Cell<usize>,
}
impl<T: tig_core::tree::NodeStore> tig_core::tree::NodeStore for ReadOnlyMetadata<'_, T> {
    fn get(&self, object: &Object) -> tig_core::tree::TreeResult<Option<Vec<u8>>> {
        self.reads.set(self.reads.get() + 1);
        self.inner.get(object)
    }
    fn put(&mut self, _: &[u8]) -> tig_core::tree::TreeResult<Object> {
        panic!("native claim replay must not write metadata");
    }
}

#[test]
fn native_claim_replay_reopens_original_context_after_main_changes_and_git_is_offline() {
    use tig_assets::graph::recipe::produced::{self, Correspondence};
    use tig_core::tree::TreeLimits;
    use tig_store::metadata::{MetadataReader, MetadataStore};
    let (mut f, request) = native_scene_fixture();
    let producer = Object::from_bytes(b"synthetic admitted recipe producer");
    let metadata = f.root().join("claim-metadata");
    let mut store = MetadataStore::create(
        &metadata,
        tig_store::metadata::Limits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let (claim, profile, subject) = metadata_claim_fixture(&f, &request, &producer, &mut store);
    let file = "records/cel-b.tig";
    let mut record: Value =
        serde_json::from_slice(&fs::read(f.root().join(file)).unwrap()).unwrap();
    record["metadata"]["note"] = json!("review note only");
    write(f.root(), file, &record);
    git(f.root(), &["add", "records"]);
    git(f.root(), &["commit", "-m", "annotations only"]);
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
    let mut annotation_source = f.source();
    let annotation_plan =
        reel::tig_scene::recipe(&mut annotation_source, &request, &producer).unwrap();
    record["selected"] =
        serde_json::to_value(Object::from_bytes(b"changed primary input")).unwrap();
    write(f.root(), file, &record);
    git(f.root(), &["add", "records"]);
    git(f.root(), &["commit", "-m", "replace one primary input"]);
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
    let mut changed_source = f.source();
    let changed_plan = reel::tig_scene::recipe(&mut changed_source, &request, &producer).unwrap();
    assert_eq!(annotation_source.byte_requests(), 0);
    assert_eq!(changed_source.byte_requests(), 0);
    let witness = store.finish().unwrap();
    drop(store);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let reader = MetadataReader::open(&metadata, &witness).unwrap();
    let mut readonly = ReadOnlyMetadata {
        inner: &reader,
        reads: 0.into(),
    };
    let (mut original, replay) = reel::tig_scene::restore_claim(
        profile,
        &request,
        &producer,
        &claim,
        &mut readonly,
        TreeLimits::default(),
    )
    .unwrap();
    assert!(readonly.reads.get() > 0);
    assert_eq!(original.checkpoint(), claim.origin().checkpoint());
    assert_eq!(replay.checked().claim().object(), claim.object());
    assert_eq!(
        produced::compare(&annotation_plan, Some(&subject), Some(replay.checked())).unwrap(),
        Correspondence::ClaimedRecipeMatch
    );
    assert_eq!(
        produced::compare(&changed_plan, Some(&subject), Some(replay.checked())).unwrap(),
        Correspondence::StaleRecipe
    );
    let replayed_plan = reel::tig_scene::recipe(&mut original, &request, &producer).unwrap();
    assert_eq!(replayed_plan.fingerprint(), claim.recipe());
    assert!(!replayed_plan.complete_semantic_coverage());
    let mut changed = request;
    changed.language = "en".into();
    assert!(reel::tig_scene::recipe(&mut original, &changed, &producer).is_err());
    assert_eq!(original.byte_requests(), 0);
}

#[test]
fn native_claim_replay_refuses_operation_before_store_io_and_corrupt_provenance() {
    use tig_assets::graph::recipe::produced::Claim;
    use tig_core::{
        Digest,
        tree::{MemoryNodes, TreeLimits},
    };
    let (f, mut request) = native_scene_fixture();
    let producer = Object::from_bytes(b"synthetic producer");
    let mut nodes = MemoryNodes::default();
    let (claim, profile, _) = metadata_claim_fixture(&f, &request, &producer, &mut nodes);
    let mut readonly = ReadOnlyMetadata {
        inner: &nodes,
        reads: 0.into(),
    };
    let wrong_producer = Object::from_bytes(b"wrong producer");
    assert!(
        reel::tig_scene::restore_claim(
            profile.clone(),
            &request,
            &wrong_producer,
            &claim,
            &mut readonly,
            TreeLimits::default()
        )
        .is_err()
    );
    request.language = "en".into();
    assert!(
        reel::tig_scene::restore_claim(
            profile.clone(),
            &request,
            &producer,
            &claim,
            &mut readonly,
            TreeLimits::default()
        )
        .is_err()
    );
    assert_eq!(readonly.reads.get(), 0);
    request.language = "es".into();
    let mut wrong = profile.clone();
    wrong.checkpoint = Digest::blob(b"wrong source checkpoint");
    assert!(
        reel::tig_scene::restore_claim(
            wrong,
            &request,
            &producer,
            &claim,
            &mut readonly,
            TreeLimits::default()
        )
        .is_err()
    );
    let mut wrong = profile.clone();
    wrong.archive = Object::from_bytes(b"missing archive");
    assert!(
        reel::tig_scene::restore_claim(
            wrong,
            &request,
            &producer,
            &claim,
            &mut readonly,
            TreeLimits::default()
        )
        .is_err()
    );
    let mut wrong = profile.clone();
    wrong.admitted[0].object = Object::from_bytes(b"wrong definition");
    assert!(
        reel::tig_scene::restore_claim(
            wrong,
            &request,
            &producer,
            &claim,
            &mut readonly,
            TreeLimits::default()
        )
        .is_err()
    );
    let mut wrong = profile.clone();
    wrong.admitted[1].payload = reel::tig_inputs::Payload::Stream { slot: "en".into() };
    assert!(
        reel::tig_scene::restore_claim(
            wrong,
            &request,
            &producer,
            &claim,
            &mut readonly,
            TreeLimits::default()
        )
        .is_err()
    );
    let empty = MemoryNodes::default();
    let mut unavailable = ReadOnlyMetadata {
        inner: &empty,
        reads: 0.into(),
    };
    assert!(
        reel::tig_scene::restore_claim(
            profile.clone(),
            &request,
            &producer,
            &claim,
            &mut unavailable,
            TreeLimits::default()
        )
        .is_err()
    );
    for field in ["recipe", "binding", "header", "root", "size"] {
        let mut wire: Value = serde_json::from_slice(claim.bytes()).unwrap();
        match field {
            "recipe" => wire["recipe"] = json!("0".repeat(64)),
            "binding" => wire["origin"]["binding"] = json!("0".repeat(64)),
            "header" => wire["context"]["identity"] = json!("0".repeat(64)),
            "root" => {
                wire["context"]["root"] =
                    serde_json::to_value(Object::from_bytes(b"absent context root")).unwrap()
            }
            "size" => {
                let record: Value =
                    serde_json::from_slice(&fs::read(f.root().join("records/cel-b.tig")).unwrap())
                        .unwrap();
                wire["outputs"][0]["object"] = record["selected"].clone();
                wire["outputs"][0]["object"]["bytes"] =
                    json!(record["selected"]["bytes"].as_u64().unwrap() + 1);
            }
            _ => unreachable!(),
        }
        let bytes = serde_json::to_vec(&wire).unwrap();
        let bad = Claim::parse(&Object::from_bytes(&bytes), &bytes).unwrap();
        assert!(
            reel::tig_scene::restore_claim(
                profile.clone(),
                &request,
                &producer,
                &bad,
                &mut readonly,
                TreeLimits::default()
            )
            .is_err(),
            "{field}"
        );
    }
    let tiny = TreeLimits {
        max_reads: 1,
        ..TreeLimits::default()
    };
    assert!(
        reel::tig_scene::restore_claim(profile, &request, &producer, &claim, &mut readonly, tiny)
            .is_err()
    );
}

fn archived_profile(
    f: &Fixture,
    archive: Object,
    checkpoint: tig_core::Digest,
) -> reel::tig_inputs::ArchiveProfile {
    serde_json::from_value(json!({"schema":reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA,
        "archive":archive,"checkpoint":checkpoint,"admitted":f.profile["admitted"],
        "byte_source":f.profile["byte_source"]}))
    .unwrap()
}

#[test]
fn archived_source_compiles_exact_native_scene_with_git_and_working_metadata_unavailable() {
    use tig_core::tree::{MemoryNodes, TreeLimits};
    let (f, request) = native_scene_fixture();
    let mut source = f.source();
    let required = reel::tig_scene::requirements(&mut source, &request).unwrap();
    let first = f.root().join("before-archive");
    reel::tig_scene::compile(&mut source, &request, &first).unwrap();
    let mut nodes = MemoryNodes::default();
    let root = source
        .archive_metadata(&mut nodes, TreeLimits::default())
        .unwrap();
    let profile = archived_profile(&f, root, source.checkpoint().clone());
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    assert!(Source::open(serde_json::from_value(f.profile.clone()).unwrap()).is_err());
    let mut restored = Source::from_archive(profile, &mut nodes, TreeLimits::default()).unwrap();
    assert_eq!(restored.byte_requests(), 0);
    assert_eq!(
        reel::tig_scene::requirements(&mut restored, &request).unwrap(),
        required
    );
    let next = f.root().join("from-archive");
    reel::tig_scene::compile(&mut restored, &request, &next).unwrap();
    assert_eq!(restored.byte_requests(), 6);
    for file in [
        "delivery/job.json",
        "native-source.json",
        "scene.json",
        "alignment-0000.json",
    ] {
        assert_eq!(
            fs::read(first.join(file)).unwrap(),
            fs::read(next.join(file)).unwrap(),
            "{file}"
        );
    }
}

#[test]
fn archive_and_requirement_admission_refuse_stale_missing_and_unknown_inputs() {
    use tig_core::{
        Digest,
        tree::{MemoryNodes, TreeLimits},
    };
    let (f, mut request) = native_scene_fixture();
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let root = source
        .archive_metadata(&mut nodes, TreeLimits::default())
        .unwrap();
    let mut profile = archived_profile(&f, root.clone(), Digest::blob(b"wrong checkpoint"));
    assert!(Source::from_archive(profile.clone(), &mut nodes, TreeLimits::default()).is_err());
    profile.checkpoint = source.checkpoint().clone();
    profile.archive = Object::from_bytes(b"missing archive");
    assert!(Source::from_archive(profile.clone(), &mut nodes, TreeLimits::default()).is_err());
    profile.archive = root;
    profile.schema = "unknown".into();
    assert!(Source::from_archive(profile, &mut nodes, TreeLimits::default()).is_err());
    request.controls.template_receipt = Some("absent-template".into());
    assert!(reel::tig_scene::requirements(&mut source, &request).is_err());
    request.controls.template_receipt = None;
    request
        .primary_definitions
        .push(request.primary_definitions[0].clone());
    assert!(reel::tig_scene::requirements(&mut source, &request).is_err());
    assert_eq!(source.byte_requests(), 0);
}

#[test]
fn frozen_native_job_survives_main_advancement_provider_reopen_and_git_unavailability() {
    use tig_core::tree::TreeLimits;
    use tig_store::{
        Key,
        metadata::{MetadataReader, MetadataStore},
        source_claims::{Command as Claim, SourceJournal},
    };
    let (mut f, request) = native_scene_fixture();
    let mut old = f.source();
    let producer = Object::from_bytes(b"synthetic admitted native producer package");
    let old_recipe = reel::tig_scene::recipe(&mut old, &request, &producer).unwrap();
    let first = f.root().join("context-before");
    reel::tig_scene::compile(&mut old, &request, &first).unwrap();
    let runtime = tempfile::tempdir().unwrap();
    let metadata = runtime.path().join("metadata");
    let mut store = MetadataStore::create(
        &metadata,
        tig_store::metadata::Limits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let before = old.byte_requests();
    let old_context =
        reel::tig_scene::freeze_context(&mut old, &request, &mut store, TreeLimits::default())
            .unwrap();
    assert_eq!(
        old_recipe.operation().parameters,
        Object::from_bytes(old_context.descriptor())
    );
    assert_eq!(old.byte_requests(), before);
    let old_profile = archived_profile(&f, old_context.archive().clone(), old.checkpoint().clone());
    let bytes = picture([0, 255, 0]);
    let updated = Object::from_bytes(&bytes);
    fs::write(
        f.root().join("cache/objects").join(updated.sha256.as_str()),
        bytes,
    )
    .unwrap();
    let mut leaf: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/cel-b.tig")).unwrap()).unwrap();
    leaf["selected"] = serde_json::to_value(&updated).unwrap();
    write(f.root(), "records/cel-b.tig", &leaf);
    git(f.root(), &["add", "records"]);
    git(f.root(), &["commit", "-m", "replace only cel B"]);
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
    let mut new = f.source();
    let new_recipe = reel::tig_scene::recipe(&mut new, &request, &producer).unwrap();
    assert_ne!(old_recipe.fingerprint(), new_recipe.fingerprint());
    let new_context =
        reel::tig_scene::freeze_context(&mut new, &request, &mut store, TreeLimits::default())
            .unwrap();
    assert_eq!(new.byte_requests(), 0);
    assert!(new_context.requirements().primary().contains(&updated));
    assert!(!old_context.requirements().primary().contains(&updated));
    assert_eq!(
        new_context.requirements().controls(),
        old_context.requirements().controls()
    );
    let new_profile = archived_profile(&f, new_context.archive().clone(), new.checkpoint().clone());
    let checkpoint = store.finish().unwrap();
    drop(store);
    drop(old);
    drop(new);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut nodes = MetadataReader::open(&metadata, &checkpoint).unwrap();
    for (profile, context, expected_recipe) in [
        (&old_profile, &old_context, &old_recipe),
        (&new_profile, &new_context, &new_recipe),
    ] {
        let (mut source, replay) = reel::tig_scene::restore_context(
            profile.clone(),
            &request,
            context.selection(),
            &mut nodes,
            TreeLimits::default(),
        )
        .unwrap();
        assert_eq!(source.byte_requests(), 0);
        assert_eq!(replay.requirements(), context.requirements());
        assert_eq!(replay.descriptor(), context.descriptor());
        let recipe = reel::tig_scene::recipe(&mut source, &request, &producer).unwrap();
        assert_eq!(recipe.fingerprint(), expected_recipe.fingerprint());
        assert_eq!(recipe.checkpoint(), expected_recipe.checkpoint());
        assert_eq!(recipe.binding(), expected_recipe.binding());
        assert_eq!(source.byte_requests(), 0);
    }
    // Explicit readback precedes owner adoption. This journal is fixture-only.
    let key = |value: &str| Key::parse(value).unwrap();
    let journal_path = runtime.path().join("journal");
    let mut journal = SourceJournal::create(&journal_path, key("fixture"), key("epoch")).unwrap();
    for name in ["main", "render-job"] {
        let prepared = journal
            .apply(
                key(&format!("prepare-{name}")),
                Claim::Prepare {
                    source: key(name),
                    owner: key("owner"),
                    expected_revision: 0,
                    selection: old_context.selection().clone(),
                },
            )
            .unwrap();
        journal
            .apply(
                key(&format!("adopt-{name}")),
                Claim::Adopt {
                    source: key(name),
                    owner: key("owner"),
                    expected_revision: 1,
                    prepared: prepared.reference,
                },
            )
            .unwrap();
    }
    let prepared = journal
        .apply(
            key("prepare-new-main"),
            Claim::Prepare {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 2,
                selection: new_context.selection().clone(),
            },
        )
        .unwrap();
    let witness = journal.checkpoint();
    drop(journal);
    let mut journal = SourceJournal::open(&journal_path, &witness).unwrap();
    assert_eq!(journal.obligations(10).unwrap().len(), 3);
    journal
        .apply(
            key("adopt-new-main"),
            Claim::Adopt {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 3,
                prepared: prepared.reference,
            },
        )
        .unwrap();
    journal
        .apply(
            key("retire-old-main"),
            Claim::Retire {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 4,
                predecessor_revision: 2,
            },
        )
        .unwrap();
    let witness = journal.checkpoint();
    drop(journal);
    let mut journal = SourceJournal::open(&journal_path, &witness).unwrap();
    let obligations = journal.obligations(10).unwrap();
    assert_eq!(obligations.len(), 2);
    assert!(!obligations.complete_claim_coverage());
    assert!(
        obligations
            .iter()
            .any(|o| o.source == key("render-job") && &o.selection == old_context.selection())
    );
    assert!(
        obligations
            .iter()
            .any(|o| o.source == key("main") && &o.selection == new_context.selection())
    );
    drop(nodes);
    let mut nodes = MetadataReader::open(&metadata, &checkpoint).unwrap();
    for (profile, context, name) in [
        (old_profile, old_context, "context-old-job"),
        (new_profile, new_context, "context-new-main"),
    ] {
        let (mut source, _) = reel::tig_scene::restore_context(
            profile,
            &request,
            context.selection(),
            &mut nodes,
            TreeLimits::default(),
        )
        .unwrap();
        let next = f.root().join(name);
        let changed = reel::tig_scene::Request {
            language: "en".into(),
            ..serde_json::from_value(serde_json::to_value(&request).unwrap()).unwrap()
        };
        let refused = f.root().join(format!("{name}-wrong-request"));
        assert!(reel::tig_scene::recipe(&mut source, &changed, &producer).is_err());
        assert!(reel::tig_scene::compile(&mut source, &changed, &refused).is_err());
        assert_eq!(source.byte_requests(), 0);
        assert!(!refused.exists());
        reel::tig_scene::compile(&mut source, &request, &next).unwrap();
        let bytes = fs::read(next.join("delivery/job.json")).unwrap();
        let original = fs::read(first.join("delivery/job.json")).unwrap();
        if name == "context-old-job" {
            assert_eq!(bytes, original);
            assert_eq!(
                fs::read(next.join("native-source.json")).unwrap(),
                fs::read(first.join("native-source.json")).unwrap()
            );
        } else {
            assert_ne!(bytes, original);
        }
    }
}

#[test]
fn frozen_context_rejects_request_policy_source_and_budget_mismatches_without_byte_io() {
    use tig_core::{
        Digest,
        tree::{MemoryNodes, TreeLimits},
    };
    let (f, mut request) = native_scene_fixture();
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let context =
        reel::tig_scene::freeze_context(&mut source, &request, &mut nodes, TreeLimits::default())
            .unwrap();
    let profile = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    let restore = |profile, request: &reel::tig_scene::Request, nodes: &mut MemoryNodes| {
        reel::tig_scene::restore_context(
            profile,
            request,
            context.selection(),
            nodes,
            TreeLimits::default(),
        )
    };
    request.language = "en".into();
    assert!(restore(profile.clone(), &request, &mut nodes).is_err());
    request.language = "es".into();
    request.delivery_title.push_str(" changed");
    assert!(restore(profile.clone(), &request, &mut nodes).is_err());
    request
        .delivery_title
        .truncate(request.delivery_title.len() - 8);
    let mut wrong = profile.clone();
    wrong.checkpoint = Digest::blob(b"wrong source");
    assert!(restore(wrong, &request, &mut nodes).is_err());
    let mut wrong = profile.clone();
    wrong.archive = Object::from_bytes(b"missing archive");
    assert!(restore(wrong, &request, &mut nodes).is_err());
    let mut wrong = profile.clone();
    wrong.admitted[0].object = Object::from_bytes(b"wrong definition");
    assert!(restore(wrong, &request, &mut nodes).is_err());
    let mut wrong = profile.clone();
    wrong.admitted[0].payload = reel::tig_inputs::Payload::Stream {
        slot: "not-admitted".into(),
    };
    assert!(restore(wrong, &request, &mut nodes).is_err());
    assert!(restore(profile.clone(), &request, &mut MemoryNodes::default()).is_err());
    let count = nodes.objects().count();
    request.delivery_title = "x".repeat(65_536);
    assert!(
        reel::tig_scene::freeze_context(&mut source, &request, &mut nodes, TreeLimits::default())
            .is_err()
    );
    assert_eq!(nodes.objects().count(), count);
    request.delivery_title.clear();
    let tiny = TreeLimits {
        max_entries: 1,
        ..TreeLimits::default()
    };
    assert!(reel::tig_scene::freeze_context(&mut source, &request, &mut nodes, tiny).is_err());
    assert_eq!(nodes.objects().count(), count);
    assert_eq!(source.byte_requests(), 0);
}

#[test]
fn valid_generic_context_with_changed_adapter_descriptor_is_not_a_reel_job_context() {
    use tig_core::{
        LogicalPath,
        tree::{MemoryNodes, NodeStore, Tree, TreeLimits},
    };
    use tig_store::source_claims::Selection;
    let (f, request) = native_scene_fixture();
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let context =
        reel::tig_scene::freeze_context(&mut source, &request, &mut nodes, TreeLimits::default())
            .unwrap();
    let mut bindings = Tree::from_root(context.selection().root.clone())
        .entries(&mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    let key = |s: &str| LogicalPath::parse(s).unwrap();
    let mut descriptor: Value = serde_json::from_slice(context.descriptor()).unwrap();
    descriptor["scene_adapter_sha256"] = json!("0".repeat(64));
    let descriptor = nodes
        .put(&serde_json::to_vec(&descriptor).unwrap())
        .unwrap();
    let header = bindings[&key("context/identity")].clone();
    let mut header: Value = serde_json::from_slice(&nodes.get(&header).unwrap().unwrap()).unwrap();
    header["descriptor"] = serde_json::to_value(&descriptor).unwrap();
    let header = nodes
        .put(&tig_core::identity::canonical_json(&header).unwrap())
        .unwrap();
    bindings.insert(key("context/identity"), header.clone());
    bindings.insert(key("operation/descriptor"), descriptor);
    let tree = Tree::build(&bindings, &mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    let selection = Selection {
        root: tree.root().clone(),
        context: header.sha256,
    };
    // Identity-valid opaque descriptors are allowed by generic Tig. The producer
    // still has to admit its exact request/adapter, before any byte reads.
    assert!(
        tig_git::source::native::context::read_descriptor(
            &selection,
            &mut nodes,
            TreeLimits::default()
        )
        .is_ok()
    );
    let profile = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    assert!(
        reel::tig_scene::restore_context(
            profile,
            &request,
            &selection,
            &mut nodes,
            TreeLimits::default()
        )
        .is_err()
    );
    assert_eq!(source.byte_requests(), 0);
}

#[test]
fn native_scene_rejects_wrong_type_before_bytes_and_used_draft_after_controls_only() {
    let (mut f, mut request) = native_scene_fixture();
    request.primary_definitions = vec!["types/alignment".into()];
    let mut source = f.source();
    let output = f.root().join("refused");
    assert!(reel::tig_scene::compile(&mut source, &request, &output).is_err());
    assert_eq!(source.byte_requests(), 0);
    assert!(!output.exists());
    request.primary_definitions.push("types/media".into());
    let mut leaf: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/cel-b.tig")).unwrap()).unwrap();
    leaf["status"]["selection_state"] = json!("draft");
    write(f.root(), "records/cel-b.tig", &leaf);
    git(f.root(), &["add", "records"]);
    git(
        f.root(),
        &["commit", "-m", "draft state with unchanged media"],
    );
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
    let mut source = f.source();
    assert!(reel::tig_scene::compile(&mut source, &request, &output).is_err());
    // Consumption is derived from the selected scene and four named controls.
    // The selected draft fails before any alignment/media reads or output writes.
    assert_eq!(source.byte_requests(), 5);
    assert!(!output.exists());
}

#[test]
fn real_git_alignment_stream_is_bounded_pinned_and_drives_native_reader() {
    let f = Fixture::new();
    let mut source = f.source();
    let expected = f.asset();
    assert_eq!(
        source.read_alignment(&expected, f.alignment.len()).unwrap(),
        f.alignment
    );
    assert_eq!(source.active_readers(), 0);
    let before = source.byte_requests();
    let mut stale = expected.clone();
    stale.sha256 = "0".repeat(64);
    assert!(source.read_alignment(&stale, 4096).is_err());
    assert!(source.read_alignment(&expected, 1).is_err());
    assert_eq!(source.byte_requests(), before);
    let scene = serde_json::from_str(include_str!("fixtures/scene-authoring/scene.json")).unwrap();
    let bindings = reel_assembly::scene_authoring::ScopedBindings {
        schema: "reel.scene-asset-bindings.v1".into(),
        scope_id: "scene".into(),
        assets: BTreeMap::from([("es.alignment".into(), expected)]),
    };
    let alignments = reel::scene_authoring_inputs::read_verified_alignments_from_assets(
        &scene,
        "es",
        &[&bindings],
        |a, n| source.read_alignment(a, n),
    )
    .unwrap();
    assert_eq!(alignments["es-1"].cue_end_sample, 48000);
}

#[test]
fn definition_and_stream_changes_do_not_silently_fall_back() {
    let f = Fixture::new();
    for (field, value) in [
        (
            "object",
            json!({"sha256":"0".repeat(64),"bytes":f.profile["admitted"][1]["object"]["bytes"]}),
        ),
        ("payload", json!({"kind":"stream","slot":"missing"})),
        ("payload", json!({"kind":"selected"})),
    ] {
        let mut profile = f.profile.clone();
        profile["admitted"][1][field] = value;
        let mut source = Source::open(serde_json::from_value(profile).unwrap()).unwrap();
        assert!(source.read_alignment(&f.asset(), 4096).is_err());
        assert_eq!(source.byte_requests(), 0);
    }
    let mut duplicate = f.profile.clone();
    let entry = duplicate["admitted"][0].clone();
    duplicate["admitted"].as_array_mut().unwrap().push(entry);
    assert!(Source::open(serde_json::from_value(duplicate).unwrap()).is_err());
}

#[test]
fn english_stream_requires_an_explicit_selector_and_matching_pin() {
    let mut f = Fixture::new();
    let mut english: Value = serde_json::from_slice(&f.alignment).unwrap();
    english["language"] = json!("en");
    english["cue_id"] = json!("en-1");
    let bytes = serde_json::to_vec(&english).unwrap();
    let object = Object::from_bytes(&bytes);
    let mut expected = f.asset();
    expected.sha256 = object.sha256.as_str().to_owned();
    expected.bytes = object.bytes;
    expected.cache_uri = format!("cache://sha256/{}", object.sha256.as_str());
    let mut spanish = f.source();
    assert!(spanish.read_alignment(&expected, 4096).is_err());
    assert_eq!(spanish.byte_requests(), 0);
    f.profile["admitted"][1]["payload"]["slot"] = json!("en");
    assert_eq!(f.source().read_alignment(&expected, 4096).unwrap(), bytes);
}

#[test]
fn profile_reader_refuses_duplicate_unknown_and_oversized_fields() {
    let f = Fixture::new();
    let path = f.root().join("profile.json");
    write(f.root(), "profile.json", &f.profile);
    assert!(Profile::read(&path).is_ok());
    for bytes in [
        br#"{"schema":"x","schema":"y"}"#.to_vec(),
        vec![b' '; 65537],
    ] {
        fs::write(&path, bytes).unwrap();
        assert!(Profile::read(&path).is_err());
    }
    let mut unknown = f.profile.clone();
    unknown["extra"] = json!(true);
    write(f.root(), "profile.json", &unknown);
    assert!(Profile::read(&path).is_err());
}

#[test]
fn present_corruption_refuses_even_with_a_valid_later_tier() {
    let mut f = Fixture::new();
    let expected = f.asset();
    let original = f.root().join("cache/objects").join(&expected.sha256);
    let cold = f.root().join("cold");
    fs::create_dir(&cold).unwrap();
    fs::create_dir(cold.join("objects")).unwrap();
    fs::write(cold.join("objects").join(&expected.sha256), &f.alignment).unwrap();
    fs::write(original, vec![b'x'; f.alignment.len()]).unwrap();
    f.profile["byte_source"]["places"]
        .as_array_mut()
        .unwrap()
        .push(json!({"root":cold,"layout":"flat"}));
    assert!(f.source().read_alignment(&expected, 4096).is_err());
}

#[test]
fn cooperative_mode_never_becomes_a_path_provider() {
    let f = Fixture::new();
    let mut source = f.source();
    let job: Job = serde_json::from_value(f.job.clone()).unwrap();
    assert!(source.prepare(&job).is_err());
    assert_eq!(source.byte_requests(), 0);
}

#[cfg(not(windows))]
#[test]
fn unsupported_mandatory_paths_refuse_without_byte_io() {
    let mut f = Fixture::new();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    let mut source = f.source();
    assert!(
        source
            .prepare(&serde_json::from_value(f.job.clone()).unwrap())
            .is_err()
    );
    assert_eq!(source.byte_requests(), 0);
}

#[cfg(windows)]
#[test]
fn prepared_inputs_deduplicate_guard_bytes_and_survive_source_drop() {
    let mut f = Fixture::new();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    let same = f.add("cel/same", &picture([255, 0, 0]));
    f.commit();
    f.job["pictures"][1]["source"] = reference("cel/same", &same);
    let mut source = f.source();
    let job: Job = serde_json::from_value(f.job.clone()).unwrap();
    let inputs = source.prepare(&job).unwrap();
    assert_eq!(inputs.names(), 3);
    assert_eq!(inputs.objects(), 2);
    assert_eq!(source.active_readers(), 2);
    assert_eq!(source.byte_requests(), 2);
    let path = inputs.verified_path(&job.pictures[0].source).unwrap();
    drop(source);
    assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
    assert!(fs::remove_file(&path).is_err());
    fs::write(
        f.root().join("cache/objects/unrelated"),
        b"new independent bytes",
    )
    .unwrap();
    let mut wrong = job.pictures[0].source.clone();
    wrong.bytes += 1;
    assert!(inputs.verified_path(&wrong).is_err());
    drop(inputs);
    assert!(fs::OpenOptions::new().write(true).open(path).is_ok());
}

#[cfg(windows)]
#[test]
fn opaque_asset_and_definition_ids_are_not_local_filesystem_paths() {
    let mut f = Fixture::new();
    let id = "scene:picture.p01";
    let object = f.add(id, &picture([255, 0, 0]));
    let mut definition: Value =
        serde_json::from_slice(&fs::read(f.root().join("definitions/media.json")).unwrap())
            .unwrap();
    definition["id"] = json!("types:media");
    let pin = write(f.root(), "definitions/media.json", &definition);
    for entry in fs::read_dir(f.root().join("records")).unwrap() {
        let path = entry.unwrap().path();
        let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        if record["definition"]["id"] == "types/media" {
            record["definition"] =
                json!({"id":"types:media","sha256":pin.sha256,"bytes":pin.bytes});
            fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
        }
    }
    f.profile["admitted"][0]["id"] = json!("types:media");
    f.profile["admitted"][0]["object"] = serde_json::to_value(pin).unwrap();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    git(f.root(), &["add", "definitions/media.json"]);
    f.commit();
    f.job["pictures"][0]["source"] = reference(id, &object);
    let job: Job = serde_json::from_value(f.job.clone()).unwrap();
    let inputs = f.source().prepare(&job).unwrap();
    let physical = inputs.verified_path(&job.pictures[0].source).unwrap();
    assert_eq!(fs::read(physical).unwrap(), picture([255, 0, 0]));
    assert!(!f.root().join("scene:picture.p01").exists());
}

#[cfg(windows)]
#[test]
fn partial_guard_budget_failure_releases_readers_and_stale_preflight_does_no_io() {
    let mut f = Fixture::new();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    f.profile["byte_source"]["limits"] = json!({"live_readers":2});
    let mut source = f.source();
    let mut job: Job = serde_json::from_value(f.job.clone()).unwrap();
    job.audio[0].source.sha256 = "0".repeat(64);
    assert!(source.prepare(&job).is_err());
    assert_eq!(source.byte_requests(), 0);
    let job: Job = serde_json::from_value(f.job.clone()).unwrap();
    assert!(source.prepare(&job).is_err());
    assert_eq!(source.active_readers(), 0);
}

#[cfg(windows)]
#[test]
fn derivation_components_fonts_and_camera_evidence_are_prepared_from_the_job() {
    let mut f = Fixture::new();
    let evidence = f.add("evidence/selected", b"selected");
    let font = f.add("fonts/test", b"font");
    let output = f.add("effect/render", b"render");
    let component = f.add("effect/component", b"component");
    let selected = reference("evidence/selected", &evidence);
    let rendered = reference("effect/render", &output);
    let body=serde_json::to_vec(&json!({"schema":"reel.timed-overlay-derivation.v1","selected_evidence":selected,"output":rendered,"inputs":[reference("effect/component",&component)],"recipe":{}})).unwrap();
    let receipt = f.add("effect/receipt", &body);
    f.job["external_layers"] = json!([{"attachment_id":"effect","reason":"test","evidence":selected,"font":reference("fonts/test",&font),"render_source":rendered,"derivation_receipt":reference("effect/receipt",&receipt)}]);
    f.job["post_compose_camera"] =
        json!({"evidence":selected,"zoom_step":0.01,"zoom_max":1.1,"windows":[]});
    f.commit();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    let mut source = f.source();
    let job: Job = serde_json::from_value(f.job.clone()).unwrap();
    let inputs = source.prepare(&job).unwrap();
    assert_eq!(inputs.names(), 8);
    assert_eq!(inputs.objects(), 8);
    let extra: FileRef = serde_json::from_value(reference("effect/component", &component)).unwrap();
    assert!(inputs.verified_path(&extra).is_ok());
    drop(inputs);
    assert_eq!(source.active_readers(), 0);
}

#[cfg(windows)]
#[test]
fn committed_revisions_keep_old_views_and_reject_new_selection_with_old_job() {
    let mut f = Fixture::new();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    let old_profile = f.profile.clone();
    let updated = f.add("cel/b", &picture([0, 255, 0]));
    f.commit();
    let mut old = Source::open(serde_json::from_value(old_profile).unwrap()).unwrap();
    let job: Job = serde_json::from_value(f.job.clone()).unwrap();
    let before = old.prepare(&job).unwrap();
    let mut new = f.source();
    assert!(new.prepare(&job).is_err());
    assert_eq!(new.byte_requests(), 0);
    f.job["pictures"][1]["source"] = reference("cel/b", &updated);
    let after = new
        .prepare(&serde_json::from_value(f.job).unwrap())
        .unwrap();
    assert_ne!(before.checkpoint(), after.checkpoint());
}

#[cfg(windows)]
#[test]
fn oversized_derivation_receipt_refuses_before_any_payload_read() {
    let mut f = Fixture::new();
    let mut receipt = f.add("effect/oversize", b"{}");
    receipt.bytes = reel::tig_inputs::JOB_BYTES_MAX as u64 + 1;
    let path = f.root().join("records/effect-oversize.tig");
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["selected"] = serde_json::to_value(&receipt).unwrap();
    write(f.root(), "records/effect-oversize.tig", &record);
    f.commit();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    f.job["external_layers"] = json!([{
        "attachment_id":"effect", "reason":"test metadata budget",
        "evidence":f.job["pictures"][0]["source"],
        "render_source":f.job["pictures"][1]["source"],
        "derivation_receipt":reference("effect/oversize",&receipt)
    }]);
    let mut source = f.source();
    let error = source
        .prepare(&serde_json::from_value(f.job.clone()).unwrap())
        .err()
        .unwrap();
    assert!(error.to_string().contains("metadata exceeds budget"));
    assert_eq!(source.byte_requests(), 0);
    assert_eq!(source.active_readers(), 0);
}

#[cfg(windows)]
fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_reel-tig-scene"))
        .current_dir(root)
        .arg("--profile")
        .arg(root.join("profile.json"))
        .args(args)
        .output()
        .unwrap()
}

#[cfg(windows)]
#[test]
#[ignore = "requires native FFmpeg; standard Tig CLI render/version/archive restore"]
fn native_tig_cli_renders_two_cels_shared_cue_then_replaces_only_cel_b() {
    let mut f = Fixture::new();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    write(f.root(), "profile.json", &f.profile);
    let original_job = fs::read(f.root().join("job.json")).unwrap();
    let original_profile = f.profile.clone();
    let plan = cli(f.root(), &["plan", "job.json"]);
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let first = cli(f.root(), &["render", "job.json", "exports/first"]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let result: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(result["verified_objects"], 3);
    assert_eq!(result["complete_semantic_coverage"], false);
    assert!(!String::from_utf8_lossy(&first.stdout).contains(f.root().to_str().unwrap()));
    assert!(
        cli(f.root(), &["check", "job.json", "exports/first"])
            .status
            .success()
    );
    let baseline = f.add(
        "delivery/reference",
        &fs::read(f.root().join("exports/first/master.mkv")).unwrap(),
    );
    f.commit();
    write(f.root(), "profile.json", &f.profile);
    write(
        f.root(),
        "reference.json",
        &reference("delivery/reference", &baseline),
    );
    let comparison = cli(
        f.root(),
        &["compare", "job.json", "exports/first", "reference.json"],
    );
    assert!(
        comparison.status.success(),
        "{}",
        String::from_utf8_lossy(&comparison.stderr)
    );
    let compared: Value = serde_json::from_slice(&comparison.stdout).unwrap();
    assert_eq!(compared["result"]["equal"], true);
    assert_eq!(compared["result"]["approval_transferred"], false);
    let updated = f.add("cel/b", &picture([0, 255, 0]));
    f.commit();
    write(f.root(), "profile.json", &f.profile);
    assert!(
        !cli(f.root(), &["render", "job.json", "exports/stale"])
            .status
            .success()
    );
    assert!(!f.root().join("exports/stale").exists());
    f.job["pictures"][1]["source"] = reference("cel/b", &updated);
    write(f.root(), "job.json", &f.job);
    let next = cli(f.root(), &["render", "job.json", "exports/next"]);
    assert!(
        next.status.success(),
        "{}",
        String::from_utf8_lossy(&next.stderr)
    );
    let comparison = cli(
        f.root(),
        &["compare", "job.json", "exports/next", "reference.json"],
    );
    assert!(!comparison.status.success());
    let compared: Value = serde_json::from_slice(&comparison.stdout).unwrap();
    assert_eq!(compared["result"]["picture_planes_equal"], false);
    assert_eq!(compared["result"]["pcm_equal"], true);
    let decode = |folder: &str, name: &str, audio: bool| {
        let mut command = Command::new("ffmpeg");
        command
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(f.root().join(folder).join(name));
        if audio {
            command.args(["-map", "0:a:0", "-acodec", "pcm_s24le", "-f", "s24le", "-"]);
        } else {
            command.args([
                "-map",
                "0:v:0",
                "-fps_mode",
                "passthrough",
                "-pix_fmt",
                "rgb24",
                "-f",
                "rawvideo",
                "-",
            ]);
        }
        let output = command.output().unwrap();
        assert!(output.status.success());
        output.stdout
    };
    let before = decode("exports/first", "picture.mkv", false);
    let after = decode("exports/next", "picture.mkv", false);
    let half = 12 * 64 * 64 * 3;
    assert_eq!(before.len(), 24 * 64 * 64 * 3);
    assert_eq!(before[..half], after[..half]);
    assert_ne!(before[half..], after[half..]);
    assert_eq!(
        decode("exports/first", "mix.wav", true),
        decode("exports/next", "mix.wav", true)
    );
    let audio = cli(f.root(), &["render-audio", "job.json", "exports/audio"]);
    assert!(
        audio.status.success(),
        "{}",
        String::from_utf8_lossy(&audio.stderr)
    );
    assert!(
        cli(f.root(), &["check-audio", "job.json", "exports/audio"])
            .status
            .success()
    );
    fs::write(f.root().join("job.json"), original_job).unwrap();
    write(f.root(), "profile.json", &original_profile);
    assert!(
        cli(f.root(), &["check", "job.json", "exports/first"])
            .status
            .success()
    );
    // Freeze the original captured view, then remove every Git/native-metadata
    // path before preparing guarded inputs and rendering from the archive alone.
    let mut nodes = tig_core::tree::MemoryNodes::default();
    let source = Source::open(serde_json::from_value(original_profile).unwrap()).unwrap();
    let root = source
        .archive_metadata(&mut nodes, tig_core::tree::TreeLimits::default())
        .unwrap();
    let profile = archived_profile(&f, root, source.checkpoint().clone());
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut restored =
        Source::from_archive(profile, &mut nodes, tig_core::tree::TreeLimits::default()).unwrap();
    let job_path = f.root().join("job.json");
    let job = reel::tig_inputs::read_job(&job_path).unwrap();
    let inputs = restored.prepare(&job).unwrap();
    drop(restored);
    let output = f.root().join("exports/archive");
    reel::scene_delivery::render_with_inputs(&job_path, &inputs, &output).unwrap();
    reel::scene_delivery::check_with_inputs(&job_path, &inputs, &output).unwrap();
    assert_eq!(decode("exports/archive", "picture.mkv", false), before);
    assert_eq!(
        decode("exports/archive", "mix.wav", true),
        decode("exports/first", "mix.wav", true)
    );
}
