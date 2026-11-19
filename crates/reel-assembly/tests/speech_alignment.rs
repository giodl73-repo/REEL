use reel_assembly::scene_authoring::{
    CacheObjectRef, PROVIDER_WORD_IMPORT_SCHEMA, ProviderWordImport,
};
use reel_assembly::speech_alignment::{PROVIDER, import_elevenlabs_characters};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn hash(raw: &[u8]) -> String {
    Sha256::digest(raw)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn reference(raw: &[u8]) -> CacheObjectRef {
    let sha256 = hash(raw);
    CacheObjectRef {
        cache_uri: format!("cache://sha256/{sha256}"),
        sha256,
        bytes: raw.len() as u64,
    }
}
fn fixture(text: &str) -> (Value, ProviderWordImport) {
    let characters: Vec<_> = text.chars().map(|c| c.to_string()).collect();
    let starts: Vec<_> = (0..characters.len()).map(|i| i as f64 / 10.0).collect();
    let ends: Vec<_> = (1..=characters.len()).map(|i| i as f64 / 10.0).collect();
    let value = json!({"characters":characters,"character_start_times_seconds":starts,"character_end_times_seconds":ends});
    let spec = ProviderWordImport {
        schema: PROVIDER_WORD_IMPORT_SCHEMA.into(),
        provider: PROVIDER.into(),
        language: "es".into(),
        cue_id: "cue-1".into(),
        sample_rate: 48_000,
        cue_end_sample: 480_000,
        take: reference(b"selected synthetic take"),
        spoken_text: text.into(),
        spoken_text_sha256: hash(text.as_bytes()),
        alignment_path: "alignment.json".into(),
        alignment: reference(&serde_json::to_vec(&value).unwrap()),
    };
    (value, spec)
}
fn run(
    value: &Value,
    spec: &ProviderWordImport,
    segments: &[&str],
) -> anyhow::Result<reel_assembly::scene_authoring::WordTimingEvidence> {
    let bytes = serde_json::to_vec(value).unwrap();
    let mut spec = spec.clone();
    spec.alignment = reference(&bytes);
    import_elevenlabs_characters(
        &bytes,
        &spec,
        &segments.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    )
}

#[test]
fn original_unicode_text_and_language_local_boundaries_keep_selected_sample_clock() {
    let (value, spec) = fixture("¡Sí!  luz.");
    let evidence = run(&value, &spec, &["¡Sí!", "  ", "luz."]).unwrap();
    assert_eq!(evidence.words.len(), 2);
    assert_eq!(evidence.words[0].word, "¡Sí!");
    assert_eq!(evidence.words[0].start_sample, 0);
    assert_eq!(evidence.words[0].end_sample, 19_200);
    assert_eq!(evidence.words[1].start_sample, 28_800);
    assert_eq!(evidence.selected_take_sha256, spec.take.sha256);
    assert_eq!(
        reel_assembly::scene_authoring::spoken_phrase_sample(
            &evidence,
            &spec.spoken_text,
            "luz",
            None
        )
        .unwrap(),
        28_800
    );
    let (english, mut english_spec) = fixture("Yes, light.");
    english_spec.language = "en".into();
    let english = run(&english, &english_spec, &["Yes,", " ", "light."]).unwrap();
    assert_eq!(
        reel_assembly::scene_authoring::spoken_phrase_sample(
            &english,
            &english_spec.spoken_text,
            "light",
            None
        )
        .unwrap(),
        24_000
    );
    assert_ne!(
        evidence.words[1].start_sample,
        english.words[1].start_sample
    );
    let (value, mut spec) = fixture("看见光");
    spec.language = "zh".into();
    let evidence = run(&value, &spec, &["看见", "光"]).unwrap();
    assert_eq!(evidence.words[1].word, "光");
    assert_eq!(evidence.words[1].start_sample, 9_600);
    assert_eq!(evidence.language, "zh");
}

#[test]
fn refuses_normalized_text_bad_boundaries_and_wrong_retained_alignment() {
    let (mut value, spec) = fixture("Sí luz");
    assert!(run(&value, &spec, &["Si", " ", "luz"]).is_err());
    assert!(run(&value, &spec, &["Sí luz"]).is_err());
    value["characters"][1] = json!("i");
    assert!(run(&value, &spec, &["Sí", " ", "luz"]).is_err());
    let (value, mut spec) = fixture("Sí luz");
    spec.alignment.sha256 = "0".repeat(64);
    assert!(
        import_elevenlabs_characters(
            &serde_json::to_vec(&value).unwrap(),
            &spec,
            &["Sí".into(), " ".into(), "luz".into()]
        )
        .is_err()
    );
}

#[test]
fn refuses_missing_misaligned_backward_and_out_of_take_character_clocks() {
    let (value, spec) = fixture("Sí luz");
    for mutation in 0..6 {
        let mut bad = value.clone();
        match mutation {
            0 => {
                bad.as_object_mut()
                    .unwrap()
                    .remove("character_start_times_seconds");
            }
            1 => {
                bad["character_start_times_seconds"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            }
            2 => bad["character_start_times_seconds"][1] = json!(-1.0),
            3 => bad["character_end_times_seconds"][1] = json!(11.0),
            4 => bad["character_end_times_seconds"][2] = json!(0.1),
            _ => bad["characters"][0] = json!("Sí"),
        }
        assert!(
            run(&bad, &spec, &["Sí", " ", "luz"]).is_err(),
            "mutation {mutation}"
        );
    }
    let mut spec = spec;
    spec.provider = "elevenlabs-forced-alignment".into();
    assert!(run(&value, &spec, &["Sí", " ", "luz"]).is_err());
}
