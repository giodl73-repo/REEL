#[path = "support/lesson_fixture.rs"]
mod fixture;
use fixture::ScoreFixture;
use reel_music::{export, model, validation_inputs::ValidationInputs};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[test]
fn admitted_model_and_plan_preserve_original_validation_without_disk_fallback() {
    let f = ScoreFixture::in_current_directory();
    let namespace = f
        .temp
        .path()
        .strip_prefix(std::env::current_dir().unwrap())
        .unwrap()
        .to_path_buf();
    let model_name = namespace.join("model.json");
    let legacy = model::validate(&f.model()).unwrap();
    let expected = export::build(&f.model()).unwrap();
    let mut calls = BTreeMap::new();
    let mut read = |path: &Path| {
        *calls.entry(path.to_path_buf()).or_insert(0) += 1;
        f.bytes(path.strip_prefix(&namespace)?)
    };
    let mut inputs = ValidationInputs::admitted(&mut read);
    let actual = model::validate_with_inputs(&model_name, &mut inputs).unwrap();
    assert_eq!(legacy, actual);
    assert_eq!(
        expected,
        export::build_with_inputs(&model_name, &mut inputs).unwrap()
    );
    drop(inputs);
    assert_eq!(calls.len(), 4);
    assert!(calls.values().all(|n| *n == 1));
    let mut refused = 0;
    let mut read = |_: &Path| {
        refused += 1;
        anyhow::bail!("explicit provider refusal")
    };
    assert!(
        model::validate_with_inputs(&model_name, &mut ValidationInputs::admitted(&mut read))
            .is_err()
    );
    assert_eq!(refused, 1);
    assert!(model::validate(&model_name).is_ok());
    let mut read = |p: &Path| {
        let p = p.strip_prefix(&namespace)?;
        let mut raw = f.bytes(p)?;
        if p == Path::new("source.raw") {
            raw[0] ^= 1;
        }
        Ok(raw)
    };
    let error =
        model::validate_with_inputs(&model_name, &mut ValidationInputs::admitted(&mut read))
            .err()
            .unwrap()
            .to_string();
    assert!(error.contains("media sha256"), "{error}");
}

#[test]
fn admitted_names_and_object_size_refuse_before_fallback_or_reuse() {
    let mut calls = 0;
    let mut read = |_: &Path| {
        calls += 1;
        Ok(vec![0; 1024 * 1024 + 1])
    };
    let mut input = ValidationInputs::admitted(&mut read);
    assert!(input.read(Path::new("../outside")).is_err());
    assert!(input.read(Path::new("huge")).is_err());
    assert!(input.read(Path::new("huge")).is_err());
    drop(input);
    assert_eq!(calls, 2);
    let mut read = |_: &Path| Ok(vec![1]);
    let mut input = ValidationInputs::admitted(&mut read);
    for n in 0..32 {
        input.read(&PathBuf::from(format!("entry-{n}"))).unwrap();
    }
    assert!(input.read(Path::new("entry-32")).is_err());
    let mut read = |_: &Path| Ok(vec![1; 1024 * 1024]);
    let mut input = ValidationInputs::admitted(&mut read);
    for n in 0..16 {
        input.read(&PathBuf::from(format!("chunk-{n}"))).unwrap();
    }
    assert!(input.read(Path::new("chunk-16")).is_err());
}

#[test]
fn unsupported_admitted_layout_and_semantic_import_refuse_before_following_paths() {
    let f = ScoreFixture::new();
    let mut model: serde_json::Value =
        serde_json::from_slice(&f.files[Path::new("model.json")]).unwrap();
    model["lead_sheet"] = serde_json::json!({"title":"Unsupported fixture","melody_part_id":"melody","lyric_layer_id":null,"underlay":[]});
    let model_raw = serde_json::to_vec(&model).unwrap();
    let mut count = 0;
    let mut read = |_: &Path| {
        count += 1;
        Ok(model_raw.clone())
    };
    let error = reel_music::model::validate_with_inputs(
        Path::new("model.json"),
        &mut ValidationInputs::admitted(&mut read),
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("lyric or vocal layouts"));
    assert_eq!(count, 1);
    let mut analysis: serde_json::Value =
        serde_json::from_slice(&f.files[Path::new("analysis.json")]).unwrap();
    analysis["imports"] = serde_json::json!([{"import_id":"unsupported","manifest":"never-open.json","manifest_sha256":"0".repeat(64),"contract_sha256":"0".repeat(64)}]);
    let raw = serde_json::to_vec(&analysis).unwrap();
    let mut count = 0;
    let mut read = |_: &Path| {
        count += 1;
        Ok(raw.clone())
    };
    let error = reel_music::analysis::validate_with_inputs(
        Path::new("analysis.json"),
        &mut ValidationInputs::admitted(&mut read),
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("semantic imports"));
    assert_eq!(count, 1);
}
