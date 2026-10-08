use reel_assembly::scene_authoring::{
    Episode, ScopedBindings, TemplateCatalog, presentation_template_kind_matches,
    resolve_episode_presentation,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn reference(root: &Path, name: &str) -> Value {
    let bytes = fs::read(root.join(name)).unwrap();
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    json!({"path":name,"sha256":sha,"bytes":bytes.len()})
}

#[test]
fn occurrence_kinds_preserve_legacy_and_reject_unselected_types() {
    let legacy = json!({"role":"poem-title","template_id":"shared","content":{}});
    let use_: reel_assembly::scene_authoring::PresentationUse =
        serde_json::from_value(legacy.clone()).unwrap();
    assert_eq!(serde_json::to_value(use_).unwrap(), legacy);
    assert!(presentation_template_kind_matches(
        "poem-title",
        None,
        "poem-title"
    ));
    assert!(presentation_template_kind_matches(
        "internal-poem",
        None,
        "opening-poem"
    ));
    assert!(presentation_template_kind_matches(
        "title-one",
        Some("poem-title"),
        "poem-title"
    ));
    assert!(!presentation_template_kind_matches(
        "title-one",
        None,
        "poem-title"
    ));
    assert!(!presentation_template_kind_matches(
        "title-one",
        Some(""),
        "poem-title"
    ));
    assert!(!presentation_template_kind_matches(
        "",
        Some("poem-title"),
        "poem-title"
    ));
    assert!(!presentation_template_kind_matches(
        "title-one",
        Some("end-credits"),
        "poem-title"
    ));
}

#[test]
fn two_occurrences_adopt_distinct_masters_with_one_reusable_template() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        &root.join("template.json"),
        &json!({
            "schema":"reel.selected-presentation-master-template.v1", "template_id":"poem-title-master",
            "kind":"poem-title", "width":64,"height":64,"fps_numerator":24,"fps_denominator":1,
            "duration_frames":24,"sample_rate":44100
        }),
    );
    let template = reference(root, "template.json");
    write(
        &root.join("master-template.json"),
        &json!({
            "schema":"reel.episode-master-template.v2","template_id":"episode-master","ordered_units":[
                {"kind":"episode-presentation","id":"title-one"},
                {"kind":"episode-presentation","id":"title-two"}
            ]
        }),
    );
    let catalog = json!({"schema":"reel.scene-template-catalog.v1","templates":[
        {"template_id":"episode-master","kind":"episode-master","definition_sha256":reference(root,"master-template.json")["sha256"],"required_content_keys":[]},
        {"template_id":"poem-title-master","kind":"poem-title","definition_sha256":template["sha256"],"required_content_keys":[]}
    ]});
    write(&root.join("catalog.json"), &catalog);
    let season = json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"season","assets":{}});
    write(&root.join("season.json"), &season);
    let mut assets = serde_json::Map::new();
    for (role, color) in [("title-one", "red"), ("title-two", "blue")] {
        let source_name = format!("{role}.mkv");
        let mut ffmpeg = Command::new("ffmpeg");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            ffmpeg.creation_flags(0x0800_0000);
        }
        assert!(
            ffmpeg
                .args(["-v", "error", "-f", "lavfi", "-i"])
                .arg(format!("color=c={color}:s=64x64:r=24:d=1"))
                .args([
                    "-f",
                    "lavfi",
                    "-i",
                    "anullsrc=r=44100:cl=stereo:d=1",
                    "-shortest",
                    "-c:v",
                    "ffv1",
                    "-pix_fmt",
                    "yuv444p",
                    "-c:a",
                    "pcm_s24le"
                ])
                .arg(root.join(&source_name))
                .status()
                .unwrap()
                .success()
        );
        let source = reference(root, &source_name);
        assets.insert(role.into(), json!({"logical_id":role,"sha256":source["sha256"],"bytes":source["bytes"],
            "cache_uri":format!("cache://sha256/{}",source["sha256"].as_str().unwrap()),"selection_state":"selected-private-production"}));
    }
    let bindings =
        json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"episode","assets":assets});
    write(&root.join("bindings.json"), &bindings);
    let episode: Episode = serde_json::from_value(json!({
        "schema":"reel.episode-authoring.v1","episode_id":"episode","season_id":"season",
        "authoring_state":"ready-for-private-build","master_template_id":"episode-master",
        "scene_policy_id":"unused","score_palette":[],"scene_ids":[],"presentation":[
            {"role":"title-one","template_kind":"poem-title","template_id":"poem-title-master","content":{},"asset_binding":"title-one"},
            {"role":"title-two","template_kind":"poem-title","template_id":"poem-title-master","content":{},"asset_binding":"title-two"}
        ]
    })).unwrap();
    let catalog: TemplateCatalog = serde_json::from_value(catalog).unwrap();
    let season: ScopedBindings = serde_json::from_value(season).unwrap();
    let bindings: ScopedBindings = serde_json::from_value(bindings).unwrap();
    let resolved = resolve_episode_presentation(&catalog, &episode, &season, &bindings).unwrap();
    assert_eq!(resolved.selected_inputs.len(), 2);
    assert_eq!(resolved.template_definitions.len(), 2); // one episode template + one shared title template
    let mut wrong = episode.clone();
    wrong.presentation[1].template_kind = Some("end-credits".into());
    assert!(resolve_episode_presentation(&catalog, &wrong, &season, &bindings).is_err());
    wrong.presentation[1].template_kind = None;
    assert!(resolve_episode_presentation(&catalog, &wrong, &season, &bindings).is_err());
    let mut receipts = Vec::new();
    for role in ["title-one", "title-two"] {
        let mut manifest = json!({"schema":"reel.presentation-adopt.v1","role":role,
            "template_kind":"poem-title","language":"es","season_id":"season","episode_id":"episode",
            "template_id":"poem-title-master","catalog":reference(root,"catalog.json"),
            "template_definition":template,"season_bindings":reference(root,"season.json"),
            "episode_bindings":reference(root,"bindings.json"),"source_binding":role,
            "source":reference(root,&format!("{role}.mkv"))});
        let name = format!("{role}.json");
        write(&root.join(&name), &manifest);
        let run = |output: &str| {
            Command::new(env!("CARGO_BIN_EXE_reel-presentation-adopt"))
                .arg("build")
                .arg(root.join(&name))
                .arg("--input-root")
                .arg(root)
                .arg("--asset-root")
                .arg(root)
                .arg("--output-dir")
                .arg(root.join(output))
                .output()
                .unwrap()
        };
        let result = run(role);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let receipt: Value =
            serde_json::from_slice(&fs::read(root.join(role).join("receipt.json")).unwrap())
                .unwrap();
        assert_eq!(receipt["role"], role);
        assert_eq!(receipt["template_definition_sha256"], template["sha256"]);
        assert_eq!(receipt["master_sha256"], manifest["source"]["sha256"]);
        receipts.push(receipt);
        manifest["template_kind"] = "end-credits".into();
        write(&root.join(&name), &manifest);
        assert!(!run(&format!("rejected-{role}")).status.success());
        assert!(!root.join(format!("rejected-{role}")).exists());
        manifest["template_kind"] = "poem-title".into();
        write(&root.join(&name), &manifest);
    }
    assert_ne!(receipts[0]["master_sha256"], receipts[1]["master_sha256"]);
    write(
        &root.join("episode.json"),
        &serde_json::to_value(&episode).unwrap(),
    );
    let segments = ["title-one", "title-two"]
        .iter()
        .map(|role| {
            json!({
                "kind":"episode-presentation","id":role,
                "master":reference(root,&format!("{role}/master.mkv")),
                "source_receipt":reference(root,&format!("{role}/receipt.json")),
                "adoption_manifest":reference(root,&format!("{role}.json"))
            })
        })
        .collect::<Vec<_>>();
    let mut conform = json!({"schema":"reel.episode-conform.v1","episode_id":"episode","language":"es",
        "catalog":reference(root,"catalog.json"),"master_template_definition":reference(root,"master-template.json"),
        "episode":reference(root,"episode.json"),"season_bindings":reference(root,"season.json"),
        "episode_bindings":reference(root,"bindings.json"),"segments":segments,"output_sample_rate":44100});
    write(&root.join("conform.json"), &conform);
    let run_conform = |output: &str| {
        Command::new(env!("CARGO_BIN_EXE_reel-episode-conform"))
            .arg("build")
            .arg(root.join("conform.json"))
            .arg("--input-root")
            .arg(root)
            .arg("--asset-root")
            .arg(root)
            .arg("--output-dir")
            .arg(root.join(output))
            .output()
            .unwrap()
    };
    let result = run_conform("complete");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value =
        serde_json::from_slice(&fs::read(root.join("complete/receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["total_frames"], 48);
    assert_eq!(receipt["decoded_master_matches_ordered_segments"], true);
    assert_eq!(receipt["segments"][0]["id"], "title-one");
    assert_eq!(receipt["segments"][1]["id"], "title-two");
    let first = conform["segments"][0]["master"].clone();
    conform["segments"][0]["master"] = conform["segments"][1]["master"].clone();
    write(&root.join("conform.json"), &conform);
    assert!(!run_conform("swapped-master").status.success());
    assert!(!root.join("swapped-master").exists());
    conform["segments"][0]["master"] = first;
    let mut wrong_episode = serde_json::to_value(&episode).unwrap();
    wrong_episode["presentation"][1]["template_kind"] = "end-credits".into();
    write(&root.join("episode.json"), &wrong_episode);
    conform["episode"] = reference(root, "episode.json");
    write(&root.join("conform.json"), &conform);
    assert!(!run_conform("wrong-kind").status.success());
    assert!(!root.join("wrong-kind").exists());
}
