use reel::cue_relative::{self, Contract};
use serde_json::{Value, json};

fn production() -> Value {
    json!({"manifest_version":"reel.manifest.v0.2","work":"clock-fixture",
        "title":"Recording clock","shots":[{"id":"picture","scene_id":"scene"}],
        "audio_events":[{"id":"recording","role":"music","source":"recording.wav","start_seconds":0}]})
}

fn contract() -> Contract {
    serde_json::from_value(json!({
        "schema":cue_relative::AUDIO_EVENT_CONTRACT_SCHEMA,"id":"clock-fixture",
        "production_manifest":"production.json","sample_rate":48000,
        "frame_rate":{"numerator":25,"denominator":1},
        "cues":[{"cue_id":"recording","duration_samples":5520000}],
        "attachments":[{"id":"chart","target":{"kind":"overlay","shot_id":"picture","overlay_id":"chart"},
            "start":{"kind":"cue-start","cue_id":"recording","offset_samples":1507200},
            "end":{"kind":"cue-start","cue_id":"recording","offset_samples":1632000}}]
    })).unwrap()
}

fn compile(c: &Contract, p: &Value) -> anyhow::Result<cue_relative::CompiledConform> {
    cue_relative::compile_with_manifest_bytes(c, &serde_json::to_vec(p).unwrap())
}

#[test]
fn recording_clock_resolves_retained_frame_window_without_narration_or_markers() {
    let result = compile(&contract(), &production()).unwrap();
    assert_eq!(result.schema, cue_relative::AUDIO_EVENT_COMPILED_SCHEMA);
    assert_eq!(result.duration_samples, 5520000);
    assert_eq!(result.cues.len(), 1);
    let span = &result.attachments[0];
    assert_eq!((span.start_frame, span.end_frame_exclusive), (785, 850));
    assert_eq!((span.start_sample, span.end_sample), (1507200, 1632000));
}

#[test]
fn schema_selects_one_namespace_without_fallback_and_binds_compiled_identity() {
    let mut c = contract();
    let mut p = production();
    c.schema = cue_relative::CONTRACT_SCHEMA.into();
    assert!(compile(&c, &p).is_err());
    p["narration_cues"] = json!([{"id":"recording","speaker_id":"narrator"}]);
    let narration = compile(&c, &p).unwrap();
    c.schema = cue_relative::AUDIO_EVENT_CONTRACT_SCHEMA.into();
    let audio = compile(&c, &p).unwrap();
    assert_ne!(
        serde_json::to_vec(&narration).unwrap(),
        serde_json::to_vec(&audio).unwrap()
    );
    assert_eq!(
        narration.production_manifest_sha256,
        audio.production_manifest_sha256
    );
    p["audio_events"] = json!([]);
    assert!(compile(&c, &p).is_err());
    c.schema = "unknown-clock-domain".into();
    assert!(compile(&c, &p).is_err());
}

#[test]
fn duplicate_audio_events_and_spoken_markers_refuse() {
    let mut p = production();
    let duplicate = p["audio_events"][0].clone();
    p["audio_events"].as_array_mut().unwrap().push(duplicate);
    assert!(
        compile(&contract(), &p)
            .unwrap_err()
            .to_string()
            .contains("duplicate IDs")
    );
    let mut c = contract();
    c.cues[0].markers.push(cue_relative::CueMarker {
        id: "invented-word".into(),
        kind: cue_relative::MarkerKind::Word,
        start_sample: 0,
        end_sample: 100,
    });
    assert!(
        compile(&c, &production())
            .unwrap_err()
            .to_string()
            .contains("cannot claim")
    );
    c.cues[0].markers[0].kind = cue_relative::MarkerKind::Phrase;
    assert!(compile(&c, &production()).is_err());
}

#[test]
fn recording_replacement_never_silently_rescales_retained_offsets() {
    let mut c = contract();
    c.cues[0].duration_samples *= 2;
    let result = compile(&c, &production()).unwrap();
    assert_eq!(
        (
            result.attachments[0].start_frame,
            result.attachments[0].end_frame_exclusive
        ),
        (785, 850)
    );
    c.cues[0].duration_samples = 100;
    assert!(compile(&c, &production()).is_err());
}

#[test]
fn caption_targets_follow_the_explicit_clock_namespace() {
    let mut c = contract();
    let mut p = production();
    p["narration_cues"] = json!([{"id":"voice","speaker_id":"narrator"}]);
    c.attachments[0].target = cue_relative::Target::Caption {
        cue_id: "voice".into(),
        caption_id: "caption".into(),
    };
    assert!(compile(&c, &p).is_err());
    c.attachments[0].target = cue_relative::Target::Caption {
        cue_id: "recording".into(),
        caption_id: "caption".into(),
    };
    assert!(compile(&c, &p).is_ok());
}
