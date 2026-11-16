use super::*;

fn fixture() -> Prepared {
    let request = Request {
        schema: "reel.speech-request.v1".into(),
        provider: "elevenlabs".into(),
        role_id: "fixture-role".into(),
        voice_id: "FixtureVoice012345678".into(),
        model_id: "fixture-model".into(),
        language_code: "es".into(),
        source_text: "¡Hola, niño!".into(),
        source_text_sha256: Object::from_bytes("¡Hola, niño!".as_bytes())
            .sha256
            .as_str()
            .into(),
        audio_tag: None,
        enable_logging: false,
        seed: 7,
        voice_settings: Settings {
            stability: 0.5,
            similarity_boost: 0.7,
            style: 0.0,
            use_speaker_boost: true,
            speed: 1.0,
        },
    };
    let request_raw = serde_json::to_vec(&request).unwrap();
    let authorization = Authorization {
        schema: "reel.speech-authorization.v1".into(),
        request: Object::from_bytes(&request_raw),
        role_id: request.role_id.clone(),
        voice_id: request.voice_id.clone(),
        language_code: request.language_code.clone(),
        consent_evidence: Object::from_bytes(b"consent"),
        casting_evidence: Object::from_bytes(b"casting"),
        generation_authorized: true,
        permitted_usage: "private-audition".into(),
        public_release: false,
    };
    let authorization_raw = serde_json::to_vec(&authorization).unwrap();
    let profile: Profile = serde_json::from_value(json!({
        "schema":"reel.tig-speech-profile.experimental-1",
        "byte_source":{"places":[],"required":"prevent-mutation","limits":{}},
        "request":Object::from_bytes(&request_raw),"authorization":Object::from_bytes(&authorization_raw),
        "max_response_bytes":1048576,"max_audio_samples":88200,
        "max_workspace_bytes":33554432,"timeout_seconds":30
    })).unwrap();
    let provider_body = serde_json::to_vec(&json!({"text":provider_text(&request)})).unwrap();
    Prepared {
        profile,
        request,
        authorization,
        request_raw,
        authorization_raw,
        provider_body,
    }
}
fn response(text: &str) -> Vec<u8> {
    let characters = text.chars().map(|c| c.to_string()).collect::<Vec<_>>();
    let count = characters.len();
    let pcm = (0..4410i16).flat_map(i16::to_le_bytes).collect::<Vec<_>>();
    serde_json::to_vec(&json!({"audio_base64":STANDARD.encode(pcm),"alignment":{
        "characters":characters,
        "character_start_times_seconds":(0..count).map(|i|i as f64 * 0.001).collect::<Vec<_>>(),
        "character_end_times_seconds":(0..count).map(|i|(i+1) as f64 * 0.001).collect::<Vec<_>>()
    },"normalized_alignment":null}))
    .unwrap()
}
struct Mock {
    calls: usize,
    status: u16,
    raw: Vec<u8>,
}
impl Transport for Mock {
    fn post(
        &mut self,
        request: &Request,
        body: &[u8],
        _key: &str,
        _limit: u64,
        _seconds: u64,
    ) -> Result<(u16, Vec<u8>)> {
        self.calls += 1;
        assert_eq!(request.voice_id, "FixtureVoice012345678");
        assert!(!body.is_empty());
        Ok((self.status, self.raw.clone()))
    }
}
#[test]
fn request_scope_and_exact_unicode_source_are_bound() {
    let mut p = fixture();
    validate(&p.profile, &p.request, &p.authorization).unwrap();
    p.request.source_text = "Hola, nino!".into();
    assert!(validate(&p.profile, &p.request, &p.authorization).is_err());
    p = fixture();
    p.authorization.voice_id.make_ascii_lowercase();
    assert!(validate(&p.profile, &p.request, &p.authorization).is_err());
    p = fixture();
    p.authorization.public_release = true;
    assert!(validate(&p.profile, &p.request, &p.authorization).is_err());
    p = fixture();
    p.request.voice_id = "x/../../other?key".into();
    assert!(validate(&p.profile, &p.request, &p.authorization).is_err());
    p = fixture();
    p.profile.max_workspace_bytes = 1;
    assert!(validate(&p.profile, &p.request, &p.authorization).is_err());
}
#[test]
fn permission_gate_and_existing_output_refuse_before_provider_call() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("attempt");
    let mut p = fixture();
    p.authorization.generation_authorized = false;
    let mut transport = Mock {
        calls: 0,
        status: 200,
        raw: response(&p.request.source_text),
    };
    assert!(perform(&p, &output, "fixture-key-not-a-secret", &mut transport).is_err());
    assert_eq!(transport.calls, 0);
    assert!(!output.exists());
    p.authorization.generation_authorized = true;
    fs::create_dir(&output).unwrap();
    fs::write(output.join("sentinel"), b"preserve").unwrap();
    assert!(perform(&p, &output, "fixture-key-not-a-secret", &mut transport).is_err());
    assert_eq!(transport.calls, 0);
    assert_eq!(fs::read(output.join("sentinel")).unwrap(), b"preserve");
}
#[test]
fn successful_attempt_preserves_raw_pcm_and_requires_later_selection() {
    let p = fixture();
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("attempt");
    let raw = response(&p.request.source_text);
    let mut transport = Mock {
        calls: 0,
        status: 200,
        raw: raw.clone(),
    };
    let result = perform(&p, &output, "fixture-key-not-a-secret", &mut transport).unwrap();
    assert_eq!(transport.calls, 1);
    assert_eq!(
        fs::read(output.join("provider-response.json")).unwrap(),
        raw
    );
    let pcm = fs::read(output.join("raw.pcm")).unwrap();
    let wav = fs::read(output.join("recording.wav")).unwrap();
    assert_eq!(&wav[44..], &pcm);
    assert_eq!(&wav[..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 44100);
    assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 1);
    assert_eq!(u16::from_le_bytes(wav[34..36].try_into().unwrap()), 16);
    assert_eq!(result["samples"], 4410);
    assert_eq!(result["timing_qualified"], true);
    assert_eq!(result["human_approval"], false);
    assert_eq!(result["production_selection"], false);
    let receipt: Value =
        serde_json::from_slice(&fs::read(output.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["generated_speech_verbatim_verified"], false);
    assert_eq!(receipt["public_release"], false);
    assert!(
        !fs::read_to_string(output.join("receipt.json"))
            .unwrap()
            .contains("fixture-key-not-a-secret")
    );
}
#[test]
fn refusals_and_oversized_responses_are_not_retried_or_published() {
    for (status, size) in [(302, 10), (429, 10), (503, 10), (200, 1048577)] {
        let p = fixture();
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("attempt");
        let mut transport = Mock {
            calls: 0,
            status,
            raw: vec![b'x'; size],
        };
        assert!(perform(&p, &output, "fixture-key-not-a-secret", &mut transport).is_err());
        assert_eq!(transport.calls, 1);
        assert!(output.join("failure.json").exists());
        assert!(!output.join("receipt.json").exists());
        assert!(!output.join("recording.wav").exists());
    }
}
#[test]
fn reflected_credential_bytes_never_reach_attempt_files() {
    let p = fixture();
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("attempt");
    let key = "fixture-key-not-a-secret";
    let mut transport = Mock {
        calls: 0,
        status: 200,
        raw: format!("{{\"echo\":\"{key}\"}}").into_bytes(),
    };
    assert!(perform(&p, &output, key, &mut transport).is_err());
    for item in fs::read_dir(output).unwrap() {
        let raw = fs::read(item.unwrap().path()).unwrap();
        assert!(!raw.windows(key.len()).any(|w| w == key.as_bytes()));
    }
}
#[test]
fn alignment_keeps_unicode_and_only_excludes_the_declared_tag() {
    let mut p = fixture();
    p.request.audio_tag = Some("softly".into());
    let raw = response(&provider_text(&p.request));
    let value: Value = serde_json::from_slice(&raw).unwrap();
    let qualified = timing(&p.request, &value, 4410);
    assert_eq!(qualified["qualified"], true);
    assert_eq!(qualified["words"][0]["text"], "¡Hola,");
    assert_eq!(qualified["words"][1]["text"], "niño!");
    let altered: Value = serde_json::from_slice(&response("Hola, nino!")).unwrap();
    assert_eq!(timing(&p.request, &altered, 4410)["qualified"], false);
    let mut bad = value.clone();
    bad["alignment"]["character_end_times_seconds"][0] = json!(100.0);
    assert_eq!(timing(&p.request, &bad, 4410)["qualified"], false);
    let mut bad = value;
    bad["alignment"]["character_start_times_seconds"][2] = json!(-1.0);
    assert_eq!(timing(&p.request, &bad, 4410)["qualified"], false);
}
#[test]
fn absent_alignment_does_not_invent_word_timings() {
    let p = fixture();
    let value = json!({"audio_base64":STANDARD.encode([0u8;200])});
    assert_eq!(timing(&p.request, &value, 100)["qualified"], false);
    assert!(wave(&[]).is_err());
    assert!(wave(&[0]).is_err());
}

#[test]
fn credential_in_declared_inputs_refuses_without_writes_or_calls() {
    for field in 0..3 {
        let mut p = fixture();
        let key = "fixture-key-not-a-secret";
        match field {
            0 => p.request_raw.extend_from_slice(key.as_bytes()),
            1 => p.authorization_raw.extend_from_slice(key.as_bytes()),
            _ => p.provider_body.extend_from_slice(key.as_bytes()),
        }
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("attempt");
        let mut transport = Mock {
            calls: 0,
            status: 200,
            raw: response(&p.request.source_text),
        };
        assert!(perform(&p, &output, key, &mut transport).is_err());
        assert_eq!(transport.calls, 0);
        assert!(!output.exists());
    }
}

#[test]
fn large_audio_responses_are_supported_without_inventing_timings() {
    let p = fixture();
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("attempt");
    let raw =
        serde_json::to_vec(&json!({"audio_base64":STANDARD.encode(vec![0u8;100000])})).unwrap();
    assert!(raw.len() > CONTROL_MAX as usize);
    let mut transport = Mock {
        calls: 0,
        status: 200,
        raw,
    };
    let result = perform(&p, &output, "fixture-key-not-a-secret", &mut transport).unwrap();
    assert_eq!(result["samples"], 50000);
    assert_eq!(result["timing_qualified"], false);
    assert_eq!(result["production_selection"], false);
}

#[test]
fn malformed_and_duplicate_audio_responses_keep_failed_attempts_only() {
    for raw in [
        b"not JSON".to_vec(),
        br#"{"audio_base64":"AAA=","audio_base64":"AAA="}"#.to_vec(),
        serde_json::to_vec(&json!({"audio_base64":"not base64"})).unwrap(),
    ] {
        let p = fixture();
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("attempt");
        let mut transport = Mock {
            calls: 0,
            status: 200,
            raw: raw.clone(),
        };
        assert!(perform(&p, &output, "fixture-key-not-a-secret", &mut transport).is_err());
        assert_eq!(transport.calls, 1);
        assert_eq!(
            fs::read(output.join("provider-response.json")).unwrap(),
            raw
        );
        assert!(output.join("failure.json").exists());
        assert!(!output.join("receipt.json").exists());
    }
}

#[cfg(windows)]
#[test]
fn protected_existing_objects_support_inspection_and_unverified_capture() {
    let p = fixture();
    let temp = tempfile::tempdir().unwrap();
    let objects = temp.path().join("objects");
    fs::create_dir(&objects).unwrap();
    let raw = response(&p.request.source_text);
    let pin = Object::from_bytes(&raw);
    for bytes in [
        &p.request_raw[..],
        &p.authorization_raw,
        b"consent",
        b"casting",
        &raw,
    ] {
        fs::write(
            objects.join(Object::from_bytes(bytes).sha256.as_str()),
            bytes,
        )
        .unwrap();
    }
    let profile = json!({"schema":"reel.tig-speech-profile.experimental-1",
        "byte_source":{"places":[{"root":temp.path(),"layout":"flat"}],
            "required":"prevent-mutation","limits":{"object_bytes":1048576,"total_bytes":2097152,
                "requests":5,"verify_millis":30000,"live_readers":1}},
        "request":p.profile.request,"authorization":p.profile.authorization,
        "max_response_bytes":1048576,"max_audio_samples":88200,
        "max_workspace_bytes":33554432,"timeout_seconds":30});
    let profile_path = temp.path().join("profile.json");
    fs::write(&profile_path, serde_json::to_vec(&profile).unwrap()).unwrap();
    let inspection = inspect(&profile_path).unwrap();
    assert_eq!(inspection["credentials_read"], false);
    assert_eq!(inspection["provider_called"], false);
    let output = temp.path().join("captured");
    capture(&profile_path, pin.sha256.as_str(), pin.bytes, &output).unwrap();
    let receipt: Value =
        serde_json::from_slice(&fs::read(output.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(
        receipt["origin"],
        "unverified-existing-response-import-no-provider-call"
    );
    assert_eq!(receipt["provider_identity_verified_by_import"], false);
    // Corruption must fail; an existing matching object is not trusted by filename.
    fs::write(objects.join(p.profile.request.sha256.as_str()), b"corrupt").unwrap();
    assert!(inspect(&profile_path).is_err());
}
