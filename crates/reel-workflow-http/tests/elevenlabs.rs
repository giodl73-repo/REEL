use base64::{Engine, engine::general_purpose::STANDARD};
use reel_workflow::Artifact;
use reel_workflow_http::elevenlabs::{Input, Spec};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn spec(language: &str, text: &str) -> Spec {
    serde_json::from_value(json!({
        "schema":"reel.elevenlabs-timed.v1","voice_id":"fixture","language":language,
        "cue_id":"cue-1","text_input":"text","models_input":"models",
        "admission_input":"admission","decision_input":"decision","sample_rate":8000,
        "enable_logging":true,"seed":17,"segments":[text],
        "voice_settings":{"stability":0.5,"similarity_boost":0.5,"style":0.0,
            "speed":1.0,"use_speaker_boost":false}
    }))
    .unwrap()
}
fn artifact(id: &str, bytes: &[u8], media: &str) -> Artifact {
    Artifact {
        logical_id: id.into(),
        sha256: reel_workflow::digest(bytes),
        bytes: bytes.len() as u64,
        media_type: media.into(),
    }
}
fn inputs(text: &str, language: &str) -> BTreeMap<String, (Artifact, Vec<u8>)> {
    let decision = b"synthetic frozen voice-use decision".to_vec();
    let decision_artifact = artifact("decision", &decision, "text/plain");
    let admission = serde_json::to_vec(&json!({"schema":"reel.elevenlabs-admission.v1",
        "voice_id":"fixture","model_id":"eleven_v4","language":language,
        "text_sha256":reel_workflow::digest(text.as_bytes()),"account_subject":"fixture-account",
        "model_access_admitted":true,"voice_use_admitted":true,"zero_retention_admitted":false,
        "approval_subject":decision_artifact}))
    .unwrap();
    let models = serde_json::to_vec(
        &json!([{"model_id":"eleven_v4","can_do_text_to_speech":true,
        "maximum_text_length_per_request":10000,"languages":[{"language_id":language}]}]),
    )
    .unwrap();
    [
        ("text", text.as_bytes().to_vec(), "text/plain"),
        ("models", models, "application/json"),
        ("admission", admission, "application/json"),
        ("decision", decision, "text/plain"),
    ]
    .into_iter()
    .map(|(id, b, m)| (id.into(), (artifact(id, &b, m), b)))
    .collect()
}
fn request(
    s: &Spec,
    inputs: &BTreeMap<String, (Artifact, Vec<u8>)>,
) -> reel_workflow::Result<Vec<u8>> {
    s.request(
        "eleven_v4",
        &inputs
            .iter()
            .map(|(id, (a, b))| {
                (
                    id.clone(),
                    Input {
                        artifact: a,
                        bytes: b,
                    },
                )
            })
            .collect(),
    )
}
fn replace(inputs: &mut BTreeMap<String, (Artifact, Vec<u8>)>, id: &str, value: Value) {
    let bytes = serde_json::to_vec(&value).unwrap();
    inputs.insert(id.into(), (artifact(id, &bytes, "application/json"), bytes));
}
#[test]
fn exact_source_and_frozen_admission_are_required_before_request() {
    let text = "¡Sí!";
    let s = spec("es", text);
    let mut i = inputs(text, "es");
    let body: Value = serde_json::from_slice(&request(&s, &i).unwrap()).unwrap();
    assert_eq!(body["text"], text);
    assert_eq!(body["model_id"], "eleven_v4");
    assert_eq!(body["language_code"], "es");
    assert_eq!(body["apply_text_normalization"], "off");
    assert!(body.get("approval_subject").is_none());
    assert!(s.check("eleven_multilingual_v2").is_err());
    let mut admission: Value = serde_json::from_slice(&i["admission"].1).unwrap();
    admission["voice_use_admitted"] = json!(false);
    replace(&mut i, "admission", admission);
    assert!(request(&s, &i).is_err());
    let mut i = inputs(text, "es");
    i.get_mut("decision").unwrap().1.push(b'!');
    assert!(request(&s, &i).is_err());
    let mut i = inputs(text, "en");
    assert!(request(&s, &i).is_err());
    let models = json!([{"model_id":"eleven_v4","can_do_text_to_speech":false,
        "maximum_text_length_per_request":10000,"languages":[{"language_id":"es"}]}]);
    replace(&mut i, "models", models);
    assert!(request(&s, &i).is_err());
    let mut s = spec("es", text);
    s.enable_logging = false;
    assert!(request(&s, &inputs(text, "es")).is_err());
}
fn alignment(text: &str, end: f64) -> String {
    let chars: Vec<_> = text.chars().map(|c| c.to_string()).collect();
    serde_json::to_string_pretty(&json!({"characters":chars,
        "character_start_times_seconds":vec![0.0;chars.len()],
        "character_end_times_seconds":vec![end;chars.len()]}))
    .unwrap()
}
fn response(pcm: &[u8], alignment: &str) -> Vec<u8> {
    format!("{{\"audio_base64\":\"{}\",\"alignment\":{},\"normalized_alignment\":{{\"distinct\":true}}}}",STANDARD.encode(pcm),alignment).into_bytes()
}
#[test]
fn language_local_candidates_keep_exact_original_alignment_and_pcm_clock() {
    for (language, text, end) in [("es", "¡Sí!", 0.5), ("en", "Yes!", 0.75)] {
        let s = spec(language, text);
        let a = alignment(text, end);
        let outputs = s
            .normalize(
                "eleven_v4",
                text.as_bytes(),
                &response(&vec![0; 16000], &a),
                20000,
            )
            .unwrap();
        assert_eq!(outputs["alignment.json"], a.as_bytes());
        assert_eq!(
            outputs["normalized-alignment.json"],
            br#"{"distinct":true}"#
        );
        let wav = &outputs["speech.wav"];
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 8000);
        assert_eq!(wav.len(), 16044);
        let take: Value = serde_json::from_slice(&outputs["take.json"]).unwrap();
        assert_eq!(take["frames"], 8000);
        assert_eq!(take["language"], language);
        assert_eq!(take["selected"], false);
        assert_eq!(take["approved"], false);
        assert_eq!(take["audio_sha256"], reel_workflow::digest(wav));
        let timing: Value = serde_json::from_slice(&outputs["timing.json"]).unwrap();
        assert_eq!(timing["language"], language);
        assert!(
            String::from_utf8(outputs["timing.json"].clone())
                .unwrap()
                .contains(text)
        );
    }
}
#[test]
fn missing_original_or_invalid_clock_cannot_use_normalized_fallback() {
    let s = spec("en", "Yes!");
    assert!(
        s.normalize("eleven_v4", b"Yes!", &response(&[0, 0], "null"), 20000)
            .is_err()
    );
    assert!(
        s.normalize(
            "eleven_v4",
            b"Yes!",
            &response(&[0], &alignment("Yes!", 0.0)),
            20000
        )
        .is_err()
    );
    assert!(
        s.normalize(
            "eleven_v4",
            b"Yes!",
            &response(&[0, 0], &alignment("Yes!", 1.0)),
            20000
        )
        .is_err()
    );
    assert!(
        s.normalize(
            "eleven_v4",
            b"Yes!",
            &response(&[0, 0], &alignment("Sí!", 0.0)),
            20000
        )
        .is_err()
    );
}

#[test]
fn endpoint_requires_exact_service_options_or_explicit_literal_loopback() {
    use reel_workflow_http::{Config, SecretHeader};
    let s = spec("es", "Sí");
    let mut c=Config {schema:"reel.workflow-http.v1".into(),
        endpoint:"https://api.elevenlabs.io/v1/text-to-speech/fixture/with-timestamps?output_format=pcm_8000&enable_logging=true".into(),
        timeout_seconds:10,request_byte_limit:1024,response_byte_limit:1024,
        secret_headers:BTreeMap::new(),allow_loopback_http:false};
    assert!(s.check_endpoint(&c).is_err());
    c.secret_headers.insert(
        "xi-api-key".into(),
        SecretHeader {
            variable: "VOICE_KEY".into(),
            prefix: "".into(),
        },
    );
    s.check_endpoint(&c).unwrap();
    c.endpoint = c
        .endpoint
        .replace("api.elevenlabs.io", "unadmitted.example");
    assert!(s.check_endpoint(&c).is_err());
    c.endpoint="http://127.0.0.1:8000/v1/text-to-speech/fixture/with-timestamps?output_format=pcm_8000&enable_logging=true".into();
    assert!(s.check_endpoint(&c).is_err());
    c.allow_loopback_http = true;
    s.check_endpoint(&c).unwrap();
    c.endpoint.push_str("&output_format=pcm_8000");
    assert!(s.check_endpoint(&c).is_err());
    let mut s = spec("es", "Sí");
    s.voice_settings.style = 0.4;
    assert!(request(&s, &inputs("Sí", "es")).is_err());
    s.voice_settings.style = 0.0;
    s.voice_settings.use_speaker_boost = true;
    assert!(request(&s, &inputs("Sí", "es")).is_err());
}
