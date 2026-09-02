mod common;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use common::music_language::authority;
use reel_music::{
    hash::sha256_path,
    perspective::{MatchPolicy, ModelBinding, PerspectiveComparison},
};
use tempfile::tempdir;

fn write_model(root: &Path, id: &str, piano: bool) -> PathBuf {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_path = repository.join("manifests/fixtures/music-model-corrected/model.yaml");
    let source_parent = source_path.parent().unwrap();
    let mut model = reel_music::model::load(&source_path).unwrap();
    model.source.manifest = source_parent.join(&model.source.manifest);
    for analysis in &mut model.analyses {
        analysis.manifest = source_parent.join(&analysis.manifest);
    }
    model.lyric_layers[0].path = source_parent.join(&model.lyric_layers[0].path);
    model.model_id = id.into();
    if piano {
        model.parts[0].notes[1].start_tick += 60;
        model.parts[0].notes[1].duration_ticks -= 60;
        let mut added = model.parts[0].notes[3].clone();
        added.id = "piano-only-note".into();
        added.start_tick = 3_840;
        model.parts[0].notes.push(added);
        model.lead_sheet = None;
    }
    let path = root.join(format!("{id}.yaml"));
    fs::write(&path, serde_yaml::to_string(&model).unwrap()).unwrap();
    reel_music::model::validate(&path).unwrap();
    path
}

fn binding(path: PathBuf) -> ModelBinding {
    let report = reel_music::model::validate(&path).unwrap();
    ModelBinding {
        manifest: path.clone(),
        manifest_sha256: sha256_path(&path).unwrap(),
        contract_sha256: report.contract_sha256,
        model_id: report.model_id,
    }
}

fn fixture(root: &Path) -> PathBuf {
    fs::create_dir_all(root).unwrap();
    let recovered = write_model(root, "recovered-perspective", false);
    let piano = write_model(root, "piano-perspective", true);
    let manifest = PerspectiveComparison {
        schema: reel_music::perspective::SCHEMA.into(),
        comparison_id: "synthetic-recovered-versus-piano".into(),
        recovered_model: binding(recovered),
        piano_model: binding(piano),
        recovered_melody_part_id: "melody".into(),
        piano_melody_part_id: "melody".into(),
        policy: MatchPolicy {
            onset_tolerance_ticks: 120,
            duration_tolerance_ticks: 120,
            pitch_tolerance_semitones: 0,
        },
        authority: authority(
            "synthetic-perspective-authority",
            '7',
            "fixture-only",
            false,
        ),
        review: reel_music::model::Review {
            status: "not-reviewed".into(),
            required_roles: vec![
                "music-reconstruction-engineer".into(),
                "score-arrangement-director".into(),
                "sound-designer".into(),
                "editor".into(),
                "rights-provenance-steward".into(),
            ],
            decision_refs: vec![],
        },
    };
    let path = root.join("comparison.yaml");
    fs::write(&path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    path
}

#[test]
fn cli_compares_and_rechecks_recovered_and_piano_melodies() {
    let temporary = tempdir().unwrap();
    let comparison = fixture(temporary.path());
    let report = temporary.path().join("report.json");
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("music-perspective-compare")
        .arg(&comparison)
        .arg("--output-path")
        .arg(&report)
        .arg("--output")
        .arg("json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["exact_matches"], 3);
    assert_eq!(value["tolerance_matches"], 1);
    assert_eq!(value["piano_only_note_ids"][0], "piano-only-note");
    assert_eq!(value["agreement_millionths"], 800_000);

    let checked = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("music-perspective-check")
        .arg(&comparison)
        .arg(&report)
        .arg("--output")
        .arg("json")
        .output()
        .unwrap();
    assert!(checked.status.success());
    assert!(
        !Command::new(env!("CARGO_BIN_EXE_reel"))
            .arg("music-perspective-compare")
            .arg(&comparison)
            .arg("--output-path")
            .arg(&report)
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn rejects_policy_source_and_report_tampering() {
    let temporary = tempdir().unwrap();
    let comparison = fixture(temporary.path());
    let report = temporary.path().join("report.json");
    reel_music::perspective::write(&comparison, &report).unwrap();

    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    value["exact_matches"] = 99.into();
    fs::write(&report, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    assert!(reel_music::perspective::check(&comparison, &report).is_err());

    let comparison = fixture(&temporary.path().join("policy"));
    let mut manifest = reel_music::perspective::load(&comparison).unwrap();
    manifest.policy.onset_tolerance_ticks = 2_000;
    fs::write(&comparison, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    assert!(reel_music::perspective::build(&comparison).is_err());

    let comparison = fixture(&temporary.path().join("source"));
    let manifest = reel_music::perspective::load(&comparison).unwrap();
    let piano = manifest.piano_model.manifest.clone();
    let mut model = reel_music::model::load(&piano).unwrap();
    model.parts[0].notes[0].midi_note += 1;
    fs::write(&piano, serde_yaml::to_string(&model).unwrap()).unwrap();
    assert!(reel_music::perspective::build(&comparison).is_err());
}
