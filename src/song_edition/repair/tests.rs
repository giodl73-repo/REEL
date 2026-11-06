use super::*;
use crate::song_edition::source::tests::{Fixture, fixture};
use std::path::PathBuf;

fn save(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
pub(super) fn inputs(f: &Fixture) -> (PathBuf, PathBuf, PathBuf, Value) {
    let root = f.dir.path();
    let admission = root.join("repair-admission.json");
    save(&admission, &f.admission);
    let context = source::inspect(&admission).unwrap();
    let mut recipe = json!({"schema":"reel.original-song-repair.v1",
        "sample_rate":8000,"channels":2,"source_frames":8000,
        "patches":[{"start_frame":2000,"end_frame":5000,"fade_frames":2,"source_lines":[1,2]}]});
    for key in [
        "source_version",
        "original_package",
        "original_request",
        "original_generation",
        "canonical_text",
        "recording",
        "sample_rate",
        "channels",
        "source_frames",
    ] {
        recipe[key] = context.context()[key].clone();
    }
    let recipe_path = root.join("repair.json");
    save(&recipe_path, &recipe);
    let raw_audio = wave::encode(&[0.5, -0.5].repeat(8000), 8000, 2).unwrap();
    let raw_path = root.join("raw.wav");
    fs::write(&raw_path, &raw_audio).unwrap();
    let candidates = root.join("candidates.json");
    save(
        &candidates,
        &json!({"schema":"reel.original-song-repair-candidates.v1",
        "recipe":pin(&fs::read(&recipe_path).unwrap()),
        "patches":[{"path":raw_path,"object":pin(&raw_audio)}]}),
    );
    (admission, recipe_path, candidates, recipe)
}
#[test]
fn hand_computed_mask_and_fades_preserve_every_outside_bit_and_original_evidence() {
    let mut f = fixture();
    // Include signed zero in both channels to catch numeric-equality shortcuts.
    let mut source_audio = wave::encode(&[-0.0, 0.125].repeat(8000), 8000, 2).unwrap();
    source_audio.extend(b"JUNK\x04\x00\x00\x00unit");
    let riff_size = source_audio.len() as u32 - 8;
    source_audio[4..8].copy_from_slice(&riff_size.to_le_bytes());
    fs::write(f.file("recording"), &source_audio).unwrap();
    f.admission["recording"]["object"] = serde_json::to_value(pin(&source_audio)).unwrap();
    let audio_pin = pin(&source_audio);
    f.change("original_generation", |g| g["outputs"] = json!([audio_pin]));
    let (admission, recipe, candidates, _) = inputs(&f);
    let request_before = fs::read(f.file("original_request")).unwrap();
    let output = f.dir.path().join("output");
    let report = materialize(&admission, &recipe, &candidates, &output).unwrap();
    assert_eq!(report["outside_mask_sample_bits_exact"], true);
    let audio = wave::decode(&fs::read(output.join("recording.wav")).unwrap()).unwrap();
    let output_raw = fs::read(output.join("recording.wav")).unwrap();
    for (index, (before, after)) in source_audio.iter().zip(&output_raw).enumerate() {
        if !(56 + 2000 * 8..56 + 5000 * 8).contains(&index) {
            assert_eq!(before, after, "protected container byte {index}");
        }
    }
    assert_eq!(source_audio.len(), output_raw.len());
    assert_eq!(audio.samples.len(), 16000);
    for frame in 0..8000 {
        if !(2000..5000).contains(&frame) || frame == 2000 || frame == 4999 {
            assert_eq!(audio.samples[frame * 2].to_bits(), (-0.0f32).to_bits());
            assert_eq!(audio.samples[frame * 2 + 1].to_bits(), 0.125f32.to_bits());
        } else {
            assert_eq!(audio.samples[frame * 2], 0.5);
            assert_eq!(audio.samples[frame * 2 + 1], -0.5);
        }
    }
    let receipt: Value =
        packet::parse(&fs::read(output.join("repair-receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["protected_interleaved_samples"], 10000);
    assert_eq!(receipt["provider_execution_verified"], false);
    assert_eq!(receipt["performed_words_verified"], false);
    assert_eq!(receipt["human_approval"], false);
    assert_eq!(receipt["selected"], false);
    assert_eq!(
        receipt["patches"][0]["supplied_lyrics"],
        "Álpha beta\nDelta echo"
    );
    assert_eq!(
        fs::read(f.file("original_request")).unwrap(),
        request_before
    );
    check(&admission, &recipe, &candidates, &output).unwrap();
    assert!(materialize(&admission, &recipe, &candidates, &output).is_err());
}
#[test]
fn invalid_clock_provenance_masks_and_words_refuse_without_output() {
    for mutation in 0..11 {
        let f = fixture();
        let (admission, recipe_path, candidates, mut recipe) = inputs(&f);
        match mutation {
            0 => recipe["source_version"] = json!("other"),
            1 => recipe["recording"]["sha256"] = json!("0".repeat(64)),
            2 => recipe["sample_rate"] = json!(16000),
            3 => recipe["source_frames"] = json!(7999),
            4 => recipe["patches"][0]["end_frame"] = json!(8001),
            5 => recipe["patches"][0]["fade_frames"] = json!(1500),
            6 => recipe["patches"][0]["source_lines"] = json!([0, 1]),
            7 => recipe["patches"][0]["source_lines"] = json!([1, 3]),
            8 => {
                let duplicate = recipe["patches"][0].clone();
                recipe["patches"].as_array_mut().unwrap().push(duplicate);
            }
            9 => recipe["patches"] = json!([]),
            _ => recipe["patches"][0]["start_frame"] = json!(5000),
        }
        save(&recipe_path, &recipe);
        let output = f.dir.path().join("refused");
        assert!(
            plan(&admission, &recipe_path).is_err(),
            "mutation {mutation}"
        );
        assert!(materialize(&admission, &recipe_path, &candidates, &output).is_err());
        assert!(!output.exists());
    }
}
#[test]
fn wrong_or_unbound_raw_candidates_refuse_before_publication() {
    for mutation in 0..6 {
        let f = fixture();
        let (admission, recipe, candidates_path, _) = inputs(&f);
        let mut candidates: Value = packet::parse(&fs::read(&candidates_path).unwrap()).unwrap();
        if mutation == 0 {
            candidates["recipe"]["sha256"] = json!("0".repeat(64));
        } else if mutation == 1 {
            candidates["patches"] = json!([]);
        } else {
            let raw_path = PathBuf::from(candidates["patches"][0]["path"].as_str().unwrap());
            let (rate, channels, frames) = match mutation {
                2 => (16000, 2, 8000),
                3 => (8000, 1, 8000),
                4 => (8000, 2, 7999),
                _ => (8000, 2, 8000),
            };
            let raw = wave::encode(&vec![0.5; frames * channels as usize], rate, channels).unwrap();
            fs::write(&raw_path, &raw).unwrap();
            if mutation != 5 {
                candidates["patches"][0]["object"] = serde_json::to_value(pin(&raw)).unwrap();
            } else {
                candidates["patches"][0]["object"]["sha256"] = json!("0".repeat(64));
            }
        }
        save(&candidates_path, &candidates);
        let output = f.dir.path().join("refused");
        assert!(
            materialize(&admission, &recipe, &candidates_path, &output).is_err(),
            "mutation {mutation}"
        );
        assert!(!output.exists());
    }
}
#[test]
fn forged_receipt_or_changed_inside_or_outside_samples_cannot_pass_recheck() {
    for at in [0, 6000] {
        let f = fixture();
        let (admission, recipe, candidates, _) = inputs(&f);
        let output = f.dir.path().join("output");
        materialize(&admission, &recipe, &candidates, &output).unwrap();
        let mut raw = fs::read(output.join("recording.wav")).unwrap();
        // Encode a changed but otherwise valid waveform and forge its receipt pin.
        let mut audio = wave::decode(&raw).unwrap();
        audio.samples[at] = 0.375;
        raw = wave::encode(&audio.samples, audio.rate, audio.channels).unwrap();
        fs::write(output.join("recording.wav"), &raw).unwrap();
        let receipt_path = output.join("repair-receipt.json");
        let mut receipt: Value = packet::parse(&fs::read(&receipt_path).unwrap()).unwrap();
        receipt["recording"] = serde_json::to_value(pin(&raw)).unwrap();
        save(&receipt_path, &receipt);
        assert!(check(&admission, &recipe, &candidates, &output).is_err());
    }
}

#[test]
fn linear_fades_have_hand_computed_midpoints_and_empty_fades_copy_directly() {
    for fade in [0, 1, 3] {
        let f = fixture();
        let (admission, recipe_path, candidates_path, mut recipe) = inputs(&f);
        recipe["patches"][0]["fade_frames"] = json!(fade);
        save(&recipe_path, &recipe);
        let mut roster: Value = packet::parse(&fs::read(&candidates_path).unwrap()).unwrap();
        roster["recipe"] = serde_json::to_value(pin(&fs::read(&recipe_path).unwrap())).unwrap();
        save(&candidates_path, &roster);
        let output = f.dir.path().join("output");
        materialize(&admission, &recipe_path, &candidates_path, &output).unwrap();
        let audio = wave::decode(&fs::read(output.join("recording.wav")).unwrap()).unwrap();
        if fade == 0 {
            assert_eq!(audio.samples[4000], 0.5);
        } else {
            assert_eq!(audio.samples[4000], 0.25);
        }
        if fade == 3 {
            assert_eq!(audio.samples[4002], 0.375);
            assert_eq!(audio.samples[4003], -0.3125);
            assert_eq!(audio.samples[9996], 0.375);
        }
    }
}

#[test]
fn full_thirty_second_mask_uses_actual_neighbor_context_and_refuses_one_more_frame() {
    let mut f = fixture();
    let source_audio = wave::encode(&[0.25, -0.125].repeat(60 * 8000), 8000, 2).unwrap();
    fs::write(f.file("recording"), &source_audio).unwrap();
    f.admission["recording"]["object"] = serde_json::to_value(pin(&source_audio)).unwrap();
    f.change("original_generation", |g| {
        g["outputs"] = json!([pin(&source_audio)])
    });
    let (admission, recipe_path, candidates_path, mut recipe) = inputs(&f);
    recipe["patches"][0] =
        json!({"start_frame":80000,"end_frame":320000,"fade_frames":0,"source_lines":[1,2]});
    save(&recipe_path, &recipe);
    plan(&admission, &recipe_path).unwrap();
    let mut samples = Vec::new();
    for frame in 0..320000 {
        let n = frame as f32 / 320000.0;
        samples.extend([n, -n]);
    }
    let raw = wave::encode(&samples, 8000, 2).unwrap();
    let mut candidates: Value = packet::parse(&fs::read(&candidates_path).unwrap()).unwrap();
    let raw_path = PathBuf::from(candidates["patches"][0]["path"].as_str().unwrap());
    fs::write(raw_path, &raw).unwrap();
    candidates["patches"][0]["object"] = serde_json::to_value(pin(&raw)).unwrap();
    candidates["recipe"] = serde_json::to_value(pin(&fs::read(&recipe_path).unwrap())).unwrap();
    save(&candidates_path, &candidates);
    let output = f.dir.path().join("output");
    materialize(&admission, &recipe_path, &candidates_path, &output).unwrap();
    let audio = wave::decode(&fs::read(output.join("recording.wav")).unwrap()).unwrap();
    assert_eq!(audio.samples[80000 * 2], 0.125);
    assert_eq!(audio.samples[80000 * 2 + 1], -0.125);
    assert_eq!(audio.samples[79999 * 2], 0.25);
    assert_eq!(audio.samples[320000 * 2], 0.25);
    recipe["patches"][0]["end_frame"] = json!(320001);
    save(&recipe_path, &recipe);
    assert!(plan(&admission, &recipe_path).is_err());
}

#[test]
fn integer_original_is_not_silently_converted_to_float() {
    let mut f = fixture();
    let mut raw = fs::read(f.file("recording")).unwrap();
    raw[20..22].copy_from_slice(&1u16.to_le_bytes());
    fs::write(f.file("recording"), &raw).unwrap();
    f.admission["recording"]["object"] = serde_json::to_value(pin(&raw)).unwrap();
    f.change("original_generation", |g| g["outputs"] = json!([pin(&raw)]));
    let (admission, recipe, candidates, _) = inputs(&f);
    let err = plan(&admission, &recipe).unwrap_err().to_string();
    assert!(err.contains("original float32 WAVE"), "{err}");
    let output = f.dir.path().join("refused");
    assert!(materialize(&admission, &recipe, &candidates, &output).is_err());
    assert!(!output.exists());
}

#[test]
fn relative_candidate_view_reopens_after_original_output_directory_is_moved() {
    let f = fixture();
    let (admission, recipe, candidates_path, _) = inputs(&f);
    let mut candidates: Value = packet::parse(&fs::read(&candidates_path).unwrap()).unwrap();
    let view = f.dir.path().join("input-view");
    fs::create_dir(&view).unwrap();
    fs::rename(f.dir.path().join("raw.wav"), view.join("raw.wav")).unwrap();
    candidates["patches"][0]["path"] = json!("raw.wav");
    save(&view.join("candidates.json"), &candidates);
    materialize(
        &admission,
        &recipe,
        &view.join("candidates.json"),
        &view.join("output"),
    )
    .unwrap();
    let restored = f.dir.path().join("restored-view");
    fs::rename(&view, &restored).unwrap();
    check(
        &admission,
        &recipe,
        &restored.join("candidates.json"),
        &restored.join("output"),
    )
    .unwrap();
    assert!(!view.exists());
}
#[test]
fn relative_candidate_traversal_refuses_even_when_outside_bytes_match() {
    let f = fixture();
    let (admission, recipe, candidates_path, _) = inputs(&f);
    let mut candidates: Value = packet::parse(&fs::read(&candidates_path).unwrap()).unwrap();
    let view = f.dir.path().join("input-view");
    fs::create_dir(&view).unwrap();
    candidates["patches"][0]["path"] = json!("../raw.wav");
    save(&view.join("candidates.json"), &candidates);
    let output = f.dir.path().join("refused");
    assert!(materialize(&admission, &recipe, &view.join("candidates.json"), &output).is_err());
    assert!(!output.exists());
}
