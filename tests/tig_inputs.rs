#![cfg(feature = "tig")]
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
        "7e43e272eb6e96f6b6b464b9b2ee267dd5d3cf9c"
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
    assert_eq!(listed, actual);
    assert_eq!(listed.len(), 45);
    assert_eq!(total, 481289);
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
fn native_scene_rejects_draft_leaf_and_wrong_primary_type_before_byte_io() {
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
    assert_eq!(source.byte_requests(), 0);
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
#[ignore = "requires native FFmpeg; standard Tig CLI render/version/restore"]
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
}
