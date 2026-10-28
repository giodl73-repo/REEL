#![cfg(feature = "tig")]
use reel::{
    tig_episode::{self, Request},
    tig_inputs::Source,
};
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
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn write(root: &Path, name: &str, value: &Value) -> Object {
    let raw = serde_json::to_vec(value).unwrap();
    fs::write(root.join(name), &raw).unwrap();
    Object::from_bytes(&raw)
}
struct Fixture {
    root: tempfile::TempDir,
    profile: Value,
    request: Value,
    conform: Value,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let p = root.path();
        for name in ["records", "definitions", "cache", "cache/objects"] {
            fs::create_dir(p.join(name)).unwrap();
        }
        git(p, &["init", "-b", "main"]);
        git(p, &["config", "user.name", "Test"]);
        git(p, &["config", "user.email", "test@example.invalid"]);
        git(p, &["config", "core.autocrlf", "false"]);
        let schema = write(p, "definitions/schema.json", &json!(true));
        let mut admissions = Vec::new();
        for (name, fields) in [
            (
                "assembly",
                vec!["episode", "language", "conform_slot", "components"],
            ),
            ("episode", vec!["episode_id", "scene_ids"]),
            ("scene", vec!["episode_id", "scene_id"]),
            (
                "output",
                vec![
                    "language",
                    "source_segment_id",
                    "component_kind",
                    "scene",
                    "episode",
                ],
            ),
        ] {
            let pin = write(
                p,
                &format!("definitions/{name}.json"),
                &json!({"schema":"tig.definition.design-1","id":name,
                "authored_schema":schema,"authoring":{"properties":fields,"required":[],"defaults":{}},"semantic_rules":{},"stream_rules":{}}),
            );
            admissions.push(json!({"id":name,"object":pin,"payload":{"kind":"selected"}}));
        }
        let ep = json!({"schema":"reel.episode-authoring.v1","episode_id":"native-episode","season_id":"season","authoring_state":"draft",
            "master_template_id":"template","scene_policy_id":"policy","presentation":[{"role":"z-opening","template_id":"opening","content":{}}],"score_palette":[],"scene_ids":["native-scene"]});
        let episode = Object::from_bytes(&serde_json::to_vec(&ep).unwrap());
        fs::write(
            p.join("cache/objects").join(episode.sha256.as_str()),
            serde_json::to_vec(&ep).unwrap(),
        )
        .unwrap();
        let master = Object::from_bytes(b"movie NOT PRESENT");
        let control = json!({"path":"private/original/path","sha256":"a".repeat(64),"bytes":7});
        let conform = json!({"schema":"reel.episode-conform.v1","episode_id":"native-episode","language":"es","output_sample_rate":48000,
            "catalog":control,"master_template_definition":control,"episode":control,"season_bindings":control,"episode_bindings":control,
            "segments":[{"id":"z-opening","kind":"episode-presentation","master":{"path":"old/master","sha256":master.sha256,"bytes":master.bytes},"source_receipt":control},
            {"id":"native-scene","kind":"scene","master":{"path":"old/master","sha256":master.sha256,"bytes":master.bytes},"source_receipt":control}]});
        let conform_pin = Object::from_bytes(&serde_json::to_vec(&conform).unwrap());
        fs::write(
            p.join("cache/objects").join(conform_pin.sha256.as_str()),
            serde_json::to_vec(&conform).unwrap(),
        )
        .unwrap();
        let definition = |name: &str| {
            let bytes = fs::read(p.join(format!("definitions/{name}.json"))).unwrap();
            let pin = Object::from_bytes(&bytes);
            json!({"id":name,"sha256":pin.sha256,"bytes":pin.bytes})
        };
        for (name, id, kind, authored, selected, streams) in [
            (
                "assembly",
                "assemble/es",
                "assembly",
                json!({"episode":{"id":"logical/episode"},"language":"es","conform_slot":"retained-es",
                "components":{"native-scene":{"id":"output/scene"},"z-opening":{"id":"output/opening"}}}),
                Value::Null,
                json!([]),
            ),
            (
                "episode",
                "logical/episode",
                "episode",
                json!({"episode_id":"native-episode","scene_ids":[{"id":"logical/scene"}]}),
                json!(episode),
                json!([{"slot":"retained-es","role":"evidence/conform","group":"source","object":conform_pin}]),
            ),
            (
                "scene",
                "logical/scene",
                "scene",
                json!({"episode_id":"native-episode","scene_id":"native-scene"}),
                json!(Object::from_bytes(b"native scene source NOT PRESENT")),
                json!([]),
            ),
            (
                "opening",
                "output/opening",
                "output",
                json!({"language":"es","source_segment_id":"z-opening","component_kind":"episode-presentation","episode":{"id":"logical/episode"}}),
                json!(master),
                json!([]),
            ),
            (
                "master",
                "output/scene",
                "output",
                json!({"language":"es","source_segment_id":"native-scene","component_kind":"scene","scene":{"id":"logical/scene"}}),
                json!(master),
                json!([]),
            ),
        ] {
            let record = json!({"schema":"tig.instance.design-1","id":id,"definition":definition(kind),"authored":authored,"streams":streams,"selected":selected,"status":{"production_state":"current"},"metadata":{}});
            write(p, &format!("records/{name}.tig"), &record);
        }
        git(p, &["add", "records", "definitions"]);
        git(p, &["commit", "-m", "fixture"]);
        let profile = json!({"schema":"reel.tig-input-profile.experimental-1","git_executable":git_executable(),"repository":p,"commit":git(p,&["rev-parse","HEAD"]),
            "records":["records"],"definitions":["definitions"],"admitted":admissions,"byte_source":{"places":[{"root":p.join("cache"),"layout":"flat"}],"required":"cooperative-immutable",
            "limits":{"object_bytes":65536,"total_bytes":131072,"requests":2,"live_readers":1}}});
        let request = json!({"schema":tig_episode::SCHEMA,"assembly_id":"assemble/es","assembly_definition":"assembly","episode_definition":"episode","scene_definitions":["scene"],"component_definitions":["output"]});
        Self {
            root,
            profile,
            request,
            conform,
        }
    }
    fn source(&self) -> Source {
        Source::open(serde_json::from_value(self.profile.clone()).unwrap()).unwrap()
    }
    fn request(&self) -> Request {
        serde_json::from_value(self.request.clone()).unwrap()
    }
    fn edit(&mut self, name: &str, change: impl FnOnce(&mut Value)) {
        let path = format!("records/{name}.tig");
        let mut value: Value =
            serde_json::from_slice(&fs::read(self.root.path().join(&path)).unwrap()).unwrap();
        change(&mut value);
        write(self.root.path(), &path, &value);
        self.commit();
    }
    fn commit(&mut self) {
        git(self.root.path(), &["add", "records"]);
        git(self.root.path(), &["commit", "-m", "edit"]);
        self.profile["commit"] = json!(git(self.root.path(), &["rev-parse", "HEAD"]));
    }
    fn conform_edit(&mut self, change: impl FnOnce(&mut Value)) {
        change(&mut self.conform);
        let raw = serde_json::to_vec(&self.conform).unwrap();
        let pin = Object::from_bytes(&raw);
        fs::write(
            self.root
                .path()
                .join("cache/objects")
                .join(pin.sha256.as_str()),
            &raw,
        )
        .unwrap();
        self.edit("episode", |v| v["streams"][0]["object"] = json!(pin));
    }
}
#[test]
fn order_comes_from_native_conform_and_outputs_are_not_opened_or_promoted() {
    let f = Fixture::new();
    let mut source = f.source();
    let plan = tig_episode::plan(&mut source, &f.request()).unwrap();
    assert_eq!(plan.components.len(), 2);
    assert_eq!(plan.components[0].segment_id, "z-opening");
    assert_eq!(plan.components[1].segment_id, "native-scene");
    assert_eq!(
        plan.components[1].source_scene.as_ref().unwrap().id,
        "logical/scene"
    );
    assert_eq!(plan.unique_output_objects, 1);
    assert_eq!(plan.unique_output_bytes, b"movie NOT PRESENT".len() as u64);
    assert_eq!(plan.changed_components, 0);
    assert!(!plan.executable);
    assert!(!plan.production_currency_verified);
    assert!(!plan.payloads_verified);
    assert!(!plan.authorizes_eviction);
    assert_eq!(source.byte_requests(), 2);
    assert_eq!(source.active_readers(), 0);
}
#[test]
fn independent_output_and_source_edits_resolve_without_editing_the_assembly_and_old_commits_restore()
 {
    let mut f = Fixture::new();
    let original_profile = f.profile.clone();
    let assembly = fs::read(f.root.path().join("records/assembly.tig")).unwrap();
    let old = tig_episode::plan(&mut f.source(), &f.request()).unwrap();
    f.edit("master", |v| {
        v["selected"] = json!(Object::from_bytes(b"new movie also NOT PRESENT"))
    });
    f.edit("scene", |v| {
        v["selected"] = json!(Object::from_bytes(b"new authored source NOT PRESENT"))
    });
    let new = tig_episode::plan(&mut f.source(), &f.request()).unwrap();
    assert_eq!(
        fs::read(f.root.path().join("records/assembly.tig")).unwrap(),
        assembly
    );
    assert_eq!(new.changed_components, 1);
    assert_ne!(new.components[1].selected, old.components[1].selected);
    assert_ne!(
        new.components[1].source_scene_selected,
        old.components[1].source_scene_selected
    );
    assert!(!new.production_currency_verified);
    f.profile = original_profile;
    let restored = tig_episode::plan(&mut f.source(), &f.request()).unwrap();
    assert_eq!(restored.components[1].selected, old.components[1].selected);
}
#[test]
fn wrong_scopes_mappings_definitions_and_order_refuse_without_publishing_a_plan() {
    for case in 0..12 {
        let mut f = Fixture::new();
        match case {
            0 => f.edit("assembly", |v| v["authored"]["language"] = json!("en")),
            1 => f.edit("master", |v| {
                v["authored"]["scene"]["id"] = json!("logical/elsewhere")
            }),
            2 => f.edit("opening", |v| {
                v["authored"]["episode"]["id"] = json!("other-episode")
            }),
            3 => f.edit("master", |v| {
                v["authored"]["component_kind"] = json!("episode-presentation")
            }),
            4 => f.request["component_definitions"] = json!(["unadmitted"]),
            5 => f.request["component_definitions"] = json!(["scene"]),
            6 => f.edit("assembly", |v| {
                v["authored"]["components"]["native-scene"]["id"] = json!("missing")
            }),
            7 => f.edit("assembly", |v| {
                v["authored"]["components"]["extra"] = json!({"id":"output/scene"})
            }),
            8 => f.conform_edit(|v| v["segments"][1]["id"] = json!("wrong-native-scene")),
            9 => f.edit("scene", |v| v["authored"]["episode_id"] = json!("other")),
            10 => f.edit("assembly", |v| {
                v["authored"]["conform_slot"] = json!("missing")
            }),
            _ => f.profile["admitted"][3]["payload"] = json!({"kind":"stream","slot":"other"}),
        }
        let output = f.root.path().join("plan.json");
        assert!(
            tig_episode::write_plan(&mut f.source(), &f.request(), &output).is_err(),
            "case{case}"
        );
        assert!(!output.exists());
    }
}
#[test]
fn cli_emits_counts_only_and_never_overwrites_a_plan() {
    let f = Fixture::new();
    let p = f.root.path();
    write(p, "profile.json", &f.profile);
    write(p, "request.json", &f.request);
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_reel-tig-scene"))
            .arg("--profile")
            .arg(p.join("profile.json"))
            .arg("plan-episode")
            .arg(p.join("request.json"))
            .arg(p.join("plan.json"))
            .output()
            .unwrap()
    };
    let result = run();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let logs = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    for private in [
        "native-scene",
        "logical/scene",
        "private/original",
        &p.display().to_string(),
    ] {
        assert!(!logs.contains(private));
    }
    let plan = fs::read(p.join("plan.json")).unwrap();
    assert!(!run().status.success());
    assert_eq!(fs::read(p.join("plan.json")).unwrap(), plan);
}
#[test]
fn missing_or_corrupt_metadata_and_contradictory_master_sizes_refuse() {
    for case in 0..3 {
        let mut f = Fixture::new();
        match case {
            0 => {
                let pin = Object::from_bytes(&serde_json::to_vec(&f.conform).unwrap());
                fs::write(
                    f.root
                        .path()
                        .join("cache/objects")
                        .join(pin.sha256.as_str()),
                    b"corrupt",
                )
                .unwrap();
            }
            1 => f.conform_edit(|v| v["segments"][1]["master"]["bytes"] = json!(999)),
            _ => {
                let pin = Object::from_bytes(&serde_json::to_vec(&f.conform).unwrap());
                fs::remove_file(
                    f.root
                        .path()
                        .join("cache/objects")
                        .join(pin.sha256.as_str()),
                )
                .unwrap();
            }
        }
        assert!(tig_episode::plan(&mut f.source(), &f.request()).is_err());
    }
}

#[test]
fn native_episode_schema_and_presentation_roles_remain_authoritative() {
    for case in 0..2 {
        let mut f = Fixture::new();
        let record: Value =
            serde_json::from_slice(&fs::read(f.root.path().join("records/episode.tig")).unwrap())
                .unwrap();
        let path = f
            .root
            .path()
            .join("cache/objects")
            .join(record["selected"]["sha256"].as_str().unwrap());
        let mut native: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        if case == 0 {
            native["schema"] = json!("different");
        } else {
            native["presentation"][0]["role"] = json!("other");
        }
        let raw = serde_json::to_vec(&native).unwrap();
        let pin = Object::from_bytes(&raw);
        fs::write(
            f.root
                .path()
                .join("cache/objects")
                .join(pin.sha256.as_str()),
            raw,
        )
        .unwrap();
        f.edit("episode", |v| v["selected"] = json!(pin));
        assert!(tig_episode::plan(&mut f.source(), &f.request()).is_err());
    }
}

#[test]
fn archived_composition_resolves_with_git_and_working_records_unavailable() {
    use tig_core::tree::{MemoryNodes, TreeLimits};
    let f = Fixture::new();
    let mut source = f.source();
    let expected = tig_episode::plan(&mut source, &f.request()).unwrap();
    let mut nodes = MemoryNodes::default();
    let archive = source
        .archive_metadata(&mut nodes, TreeLimits::default())
        .unwrap();
    let profile=serde_json::from_value(json!({"schema":reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA,
        "archive":archive,"checkpoint":source.checkpoint(),"admitted":f.profile["admitted"],"byte_source":f.profile["byte_source"]})).unwrap();
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root.path().join(name),
            f.root.path().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    assert!(Source::open(serde_json::from_value(f.profile.clone()).unwrap()).is_err());
    let mut restored = Source::from_archive(profile, &mut nodes, TreeLimits::default()).unwrap();
    let actual = tig_episode::plan(&mut restored, &f.request()).unwrap();
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert_eq!(restored.byte_requests(), 2);
    assert_eq!(restored.active_readers(), 0);
}
