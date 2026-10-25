#[path = "fixtures/cel_native.rs"]
mod fixtures;
use reel_assets::cel_source::{editor, profile, project::Project};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
fn get(p: &Path) -> Value {
    serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
}
fn put(p: &Path, v: &Value) {
    fs::write(p, serde_json::to_vec(v).unwrap()).unwrap();
}
fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, p: &Path, m: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for e in fs::read_dir(p).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                visit(root, &p, m)
            } else {
                m.insert(p.strip_prefix(root).unwrap().into(), fs::read(p).unwrap());
            }
        }
    }
    let mut m = BTreeMap::new();
    visit(root, root, &mut m);
    m
}
fn source(root: &Path, layout: u8) -> PathBuf {
    let input = if layout == 0 {
        fixtures::fixture(root);
        root.to_owned()
    } else {
        let p = fixtures::multi_fixture(root);
        if layout >= 2 {
            fixtures::add_english_lane(root, &p);
        }
        if layout == 3 {
            fixtures::separate_language_graphs(root, &p);
        }
        p
    };
    let p = root.join("profile.json");
    let mut v = get(&p);
    v["schema"] = json!("reel.scene-delivery-profile.v1");
    v["score_gain_db"] = json!(-18);
    v["sonic_gain_db_by_binding"] = json!({"sonic.example":-12});
    v["untouched"] = json!({"values":[1,"keep",true,null]});
    put(&p, &v);
    input
}
fn request(input: &Path) -> Value {
    let p = Project::open(input).unwrap();
    let (d, h) = editor::load(&p).unwrap();
    let mut r = json!({"schema":"reel.cel-sorter-profile-edit.v1","scene_id":d["scene.json"]["scene_id"],"expected_inputs":h,"affected_languages":profile::context(&d).unwrap()["affected_languages"],"acknowledge_shared_profile":true,"changes":{"score_gain_db":-24,"sonic_gain_db":-30},"human_approval":false});
    if let Some(h) = p.descriptor_sha256() {
        r["expected_project"] = json!(h);
    }
    r
}
#[test]
fn profile_candidate_changes_only_gains_across_every_source_layout() {
    for layout in 0..4 {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("source");
        let input = source(&root, layout);
        let before = files(&root);
        let p = Project::open(&input).unwrap();
        let snapshot = p.load().unwrap();
        let req = t.path().join("request.json");
        put(&req, &request(&input));
        let out = t.path().join("candidate");
        profile::apply(&input, &req, &out).unwrap();
        assert_eq!(files(&root), before);
        let next = if layout == 0 {
            out.clone()
        } else {
            out.join("cel-sorter-project.json")
        };
        let p = Project::open(&next).unwrap();
        let after = p.load().unwrap();
        editor::load(&p).unwrap();
        for (role, raw) in &snapshot.raw {
            if role != "profile.json" {
                assert_eq!(&after.raw[role], raw, "{role}");
            }
        }
        let mut expected = snapshot.controls["profile.json"].clone();
        expected["score_gain_db"] = json!(-24.0);
        expected["sonic_gain_db"] = json!(-30.0);
        assert_eq!(after.controls["profile.json"], expected);
        assert!(profile::apply(&next, &req, &t.path().join("stale")).is_err());
        assert!(!t.path().join("stale").exists());
    }
}
#[test]
fn missing_shared_scope_forged_changes_and_stale_controls_refuse_before_output() {
    for mode in 0..15 {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("source");
        let input = source(&root, 3);
        let mut r = request(&input);
        match mode {
            0 => r["acknowledge_shared_profile"] = json!(false),
            1 => r["affected_languages"] = json!(["es"]),
            2 => r["affected_languages"] = json!(["es", "en", "en"]),
            3 => r["affected_languages"] = json!(["es", "en", "fr"]),
            4 => r["human_approval"] = json!(true),
            5 => r["changes"] = json!({"sample_rate":44100}),
            6 => r["changes"] = json!({"score_gain_db":61}),
            7 => r["changes"] = json!({"sonic_gain_db":-61}),
            8 => r["changes"] = json!({}),
            9 => r["changes"] = json!({"score_gain_db":null}),
            10 => r["changes"] = json!({"score_gain_db":-18.0}),
            11 => r["expected_project"] = json!("a".repeat(64)),
            12 => r["expected_inputs"]["profile.json"] = json!("a".repeat(64)),
            13 => r["scene_id"] = json!("foreign"),
            _ => r["extra"] = json!(true),
        }
        let before = files(&root);
        let req = t.path().join("request.json");
        put(&req, &r);
        let out = t.path().join("candidate");
        assert!(profile::apply(&input, &req, &out).is_err(), "mode {mode}");
        assert!(!out.exists());
        assert_eq!(files(&root), before);
    }
}
#[test]
fn duplicate_request_and_profile_keys_and_existing_output_refuse() {
    for mode in 0..5 {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("source");
        let input = source(&root, 0);
        let req = t.path().join("request.json");
        let r = request(&input);
        let mut bytes = serde_json::to_string(&r).unwrap();
        if mode == 0 {
            bytes = bytes.replacen(
                "\"score_gain_db\":-24",
                "\"score_gain_db\":-24,\"score_gain_db\":-25",
                1,
            );
        } else if mode == 1 {
            bytes = bytes.replacen(
                "\"human_approval\":false",
                "\"human_approval\":false,\"human_approval\":true",
                1,
            );
        } else if mode <= 3 {
            let p = root.join("profile.json");
            let mut b = fs::read_to_string(&p).unwrap();
            b = if mode == 2 {
                b.replacen(
                    "\"score_gain_db\":-18",
                    "\"score_gain_db\":-18,\"score_gain_db\":-19",
                    1,
                )
            } else {
                b.replacen("\"values\":[1", "\"values\":0,\"values\":[1", 1)
            };
            fs::write(&p, b).unwrap();
            bytes = serde_json::to_string(&request(&input)).unwrap();
        }
        fs::write(&req, bytes).unwrap();
        let out = t.path().join("candidate");
        if mode == 4 {
            fs::create_dir(&out).unwrap();
            fs::write(out.join("keep"), b"keep").unwrap();
        }
        let before = files(&root);
        assert!(profile::apply(&input, &req, &out).is_err(), "mode {mode}");
        assert_eq!(files(&root), before);
        if mode == 4 {
            assert_eq!(fs::read(out.join("keep")).unwrap(), b"keep");
        } else {
            assert!(!out.exists());
        }
    }
}
#[test]
fn unavailable_legacy_profile_does_not_offer_gain_controls() {
    let t = tempfile::tempdir().unwrap();
    fixtures::fixture(t.path());
    let p = Project::open(t.path()).unwrap();
    let (d, _) = editor::load(&p).unwrap();
    assert!(profile::context(&d).is_none());
}
