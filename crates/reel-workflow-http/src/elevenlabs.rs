//! Timed ElevenLabs TTS composition/normalization. No network or voice approval.
use crate::{error, token};
use base64::{Engine, engine::general_purpose::STANDARD};
use reel_workflow::{Artifact, ErrorKind, Result};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::collections::BTreeMap;

pub const OUTPUTS: [(&str, &str); 5] = [
    ("speech.wav", "audio/wav"),
    ("alignment.json", "application/json"),
    ("normalized-alignment.json", "application/json"),
    ("timing.json", "application/json"),
    ("take.json", "application/json"),
];
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceSettings {
    pub stability: f64,
    pub similarity_boost: f64,
    pub style: f64,
    pub speed: f64,
    pub use_speaker_boost: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clone_strength: Option<f64>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub schema: String,
    pub voice_id: String,
    pub language: String,
    pub cue_id: String,
    pub text_input: String,
    pub models_input: String,
    pub admission_input: String,
    pub decision_input: String,
    pub sample_rate: u32,
    pub enable_logging: bool,
    pub seed: Option<u32>,
    pub voice_settings: VoiceSettings,
    pub segments: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Admission {
    schema: String,
    voice_id: String,
    model_id: String,
    language: String,
    text_sha256: String,
    account_subject: String,
    model_access_admitted: bool,
    voice_use_admitted: bool,
    zero_retention_admitted: bool,
    approval_subject: Artifact,
}
#[derive(Deserialize)]
struct Language {
    language_id: String,
}
#[derive(Deserialize)]
struct Model {
    model_id: String,
    can_do_text_to_speech: bool,
    maximum_text_length_per_request: u64,
    languages: Vec<Language>,
    #[serde(default)]
    can_use_style: bool,
    #[serde(default)]
    can_use_speaker_boost: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TimedResponse {
    audio_base64: String,
    alignment: Option<Box<RawValue>>,
    normalized_alignment: Option<Box<RawValue>>,
}
pub struct Input<'a> {
    pub artifact: &'a Artifact,
    pub bytes: &'a [u8],
}
fn selected<'a>(
    inputs: &'a BTreeMap<String, Input<'a>>,
    id: &str,
    media: &str,
) -> Result<&'a Input<'a>> {
    let input = inputs
        .get(id)
        .ok_or_else(|| error(ErrorKind::Missing, "elevenlabs-declared-input"))?;
    if input.artifact.media_type != media
        || input.bytes.len() as u64 != input.artifact.bytes
        || reel_workflow::digest(input.bytes) != input.artifact.sha256
    {
        return Err(error(ErrorKind::Integrity, "elevenlabs-input-binding"));
    }
    Ok(input)
}
impl Spec {
    pub fn check(&self, model: &str) -> Result<()> {
        let settings = &self.voice_settings;
        if self.schema != "reel.elevenlabs-timed.v1"
            || !matches!(model, "eleven_v4" | "eleven_v4_turbo")
            || !token(&self.voice_id)
            || !token(&self.cue_id)
            || self.language.len() != 2
            || !self.language.bytes().all(|c| c.is_ascii_lowercase())
            || ![8000, 16000, 22050, 24000, 32000, 44100, 48000].contains(&self.sample_rate)
            || [
                settings.stability,
                settings.similarity_boost,
                settings.style,
            ]
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || !settings.speed.is_finite()
            || !(0.7..=1.2).contains(&settings.speed)
            || settings
                .clone_strength
                .is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
        {
            return Err(error(ErrorKind::Invalid, "elevenlabs-spec"));
        }
        let ids = [
            &self.text_input,
            &self.models_input,
            &self.admission_input,
            &self.decision_input,
        ];
        if ids.iter().any(|id| id.is_empty())
            || ids.iter().collect::<std::collections::BTreeSet<_>>().len() != 4
        {
            return Err(error(ErrorKind::Invalid, "elevenlabs-input-identities"));
        }
        Ok(())
    }
    pub fn check_endpoint(&self, http: &crate::Config) -> Result<()> {
        let url = reqwest::Url::parse(&http.endpoint)
            .map_err(|_| error(ErrorKind::Invalid, "elevenlabs-endpoint"))?;
        let expected = format!("/v1/text-to-speech/{}/with-timestamps", self.voice_id);
        let local = http.allow_loopback_http
            && url.scheme() == "http"
            && url.host_str().is_some_and(|h| {
                h.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
            });
        let official = url.scheme() == "https"
            && url.port_or_known_default() == Some(443)
            && matches!(
                url.host_str(),
                Some(
                    "api.elevenlabs.io"
                        | "api.us.elevenlabs.io"
                        | "api.eu.residency.elevenlabs.io"
                        | "api.in.residency.elevenlabs.io"
                        | "api.sg.residency.elevenlabs.io"
                )
            );
        if !local
            && (!official
                || !http
                    .secret_headers
                    .keys()
                    .any(|k| k.eq_ignore_ascii_case("xi-api-key")))
        {
            return Err(error(ErrorKind::Denied, "elevenlabs-endpoint-admission"));
        }
        let query: BTreeMap<_, _> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        if url.path() != expected
            || url.query_pairs().count() != 2
            || query.get("output_format") != Some(&format!("pcm_{}", self.sample_rate))
            || query.get("enable_logging") != Some(&self.enable_logging.to_string())
        {
            return Err(error(ErrorKind::Conflict, "elevenlabs-endpoint-options"));
        }
        Ok(())
    }
    pub fn request(&self, model: &str, inputs: &BTreeMap<String, Input<'_>>) -> Result<Vec<u8>> {
        self.check(model)?;
        let text = selected(inputs, &self.text_input, "text/plain")?;
        let text = std::str::from_utf8(text.bytes)
            .map_err(|_| error(ErrorKind::Invalid, "elevenlabs-source-text"))?;
        if text.is_empty()
            || text.chars().count() > 10000
            || self.segments.is_empty()
            || self.segments.len() > 32768
            || self.segments.iter().any(|s| {
                s.is_empty()
                    || !(s.chars().all(char::is_whitespace)
                        || s.chars().all(|c| !c.is_whitespace()))
            })
            || self.segments.concat() != text
        {
            return Err(error(ErrorKind::Invalid, "elevenlabs-source-segments"));
        }
        let models = selected(inputs, &self.models_input, "application/json")?;
        let catalogue: Vec<Model> = reel_workflow::decode(models.bytes)?;
        let chosen: Vec<_> = catalogue.iter().filter(|m| m.model_id == model).collect();
        if chosen.len() != 1
            || !chosen[0].can_do_text_to_speech
            || (self.voice_settings.style != 0.0 && !chosen[0].can_use_style)
            || (self.voice_settings.use_speaker_boost && !chosen[0].can_use_speaker_boost)
            || chosen[0].maximum_text_length_per_request < text.chars().count() as u64
            || !chosen[0]
                .languages
                .iter()
                .any(|l| l.language_id == self.language)
        {
            return Err(error(ErrorKind::Denied, "elevenlabs-model-admission"));
        }
        let admitted = selected(inputs, &self.admission_input, "application/json")?;
        let admission: Admission = reel_workflow::decode(admitted.bytes)?;
        let decision = inputs
            .get(&self.decision_input)
            .ok_or_else(|| error(ErrorKind::Missing, "elevenlabs-decision-subject"))?;
        if admission.schema != "reel.elevenlabs-admission.v1"
            || admission.voice_id != self.voice_id
            || admission.model_id != model
            || admission.language != self.language
            || admission.text_sha256 != reel_workflow::digest(text.as_bytes())
            || !token(&admission.account_subject)
            || !admission.model_access_admitted
            || !admission.voice_use_admitted
            || (!self.enable_logging && !admission.zero_retention_admitted)
            || admission.approval_subject != *decision.artifact
            || decision.bytes.len() as u64 != decision.artifact.bytes
            || reel_workflow::digest(decision.bytes) != decision.artifact.sha256
        {
            return Err(error(
                ErrorKind::Denied,
                "elevenlabs-voice-account-admission",
            ));
        }
        serde_json::to_vec(&serde_json::json!({"text":text,"model_id":model,"language_code":self.language,"voice_settings":self.voice_settings,"seed":self.seed,"apply_text_normalization":"off"})).map_err(|_|error(ErrorKind::Invalid,"elevenlabs-request-json"))
    }
    pub fn normalize(
        &self,
        model: &str,
        text: &[u8],
        response: &[u8],
        maximum_audio_bytes: u64,
    ) -> Result<BTreeMap<String, Vec<u8>>> {
        self.check(model)?;
        if response.len() > 128 * 1024 * 1024 || maximum_audio_bytes > 128 * 1024 * 1024 {
            return Err(error(ErrorKind::Budget, "elevenlabs-response-bytes"));
        }
        let text = std::str::from_utf8(text)
            .map_err(|_| error(ErrorKind::Invalid, "elevenlabs-source-text"))?;
        let reply: TimedResponse = serde_json::from_slice(response)
            .map_err(|_| error(ErrorKind::Integrity, "elevenlabs-response-json"))?;
        if reply.audio_base64.len() as u64
            > maximum_audio_bytes
                .saturating_mul(4)
                .div_ceil(3)
                .saturating_add(8)
        {
            return Err(error(ErrorKind::Budget, "elevenlabs-audio-bytes"));
        }
        let pcm = STANDARD
            .decode(reply.audio_base64)
            .map_err(|_| error(ErrorKind::Integrity, "elevenlabs-audio-base64"))?;
        if pcm.is_empty()
            || pcm.len() % 2 != 0
            || pcm.len() as u64 + 44 > maximum_audio_bytes
            || pcm.len() > u32::MAX as usize - 36
        {
            return Err(error(ErrorKind::Integrity, "elevenlabs-pcm-shape"));
        }
        let frames = pcm.len() as u64 / 2;
        let mut wav = Vec::with_capacity(pcm.len() + 44);
        wav.extend(b"RIFF");
        wav.extend((pcm.len() as u32 + 36).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(self.sample_rate.to_le_bytes());
        wav.extend((self.sample_rate * 2).to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend((pcm.len() as u32).to_le_bytes());
        wav.extend(pcm);
        let alignment = reply
            .alignment
            .ok_or_else(|| error(ErrorKind::Missing, "elevenlabs-original-alignment-required"))?
            .get()
            .as_bytes()
            .to_vec();
        let normalized = reply
            .normalized_alignment
            .map(|a| a.get().as_bytes().to_vec())
            .unwrap_or_else(|| b"null".to_vec());
        if alignment.len() > 1024 * 1024 || normalized.len() > 1024 * 1024 {
            return Err(error(ErrorKind::Budget, "elevenlabs-alignment-bytes"));
        }
        use reel_assembly::scene_authoring::{
            CacheObjectRef, PROVIDER_WORD_IMPORT_SCHEMA, ProviderWordImport,
        };
        let spec = ProviderWordImport {
            schema: PROVIDER_WORD_IMPORT_SCHEMA.into(),
            provider: reel_assembly::speech_alignment::PROVIDER.into(),
            language: self.language.clone(),
            cue_id: self.cue_id.clone(),
            sample_rate: self.sample_rate,
            cue_end_sample: frames,
            take: CacheObjectRef {
                cache_uri: "artifact:speech.wav".into(),
                sha256: reel_workflow::digest(&wav),
                bytes: wav.len() as u64,
            },
            spoken_text: text.into(),
            spoken_text_sha256: reel_workflow::digest(text.as_bytes()),
            alignment_path: "alignment.json".into(),
            alignment: CacheObjectRef {
                cache_uri: "artifact:alignment.json".into(),
                sha256: reel_workflow::digest(&alignment),
                bytes: alignment.len() as u64,
            },
        };
        let timing = reel_assembly::speech_alignment::import_elevenlabs_characters(
            &alignment,
            &spec,
            &self.segments,
        )
        .map_err(|_| error(ErrorKind::Integrity, "elevenlabs-original-timing"))?;
        let timing = serde_json::to_vec(&timing)
            .map_err(|_| error(ErrorKind::Invalid, "elevenlabs-timing-json"))?;
        let take=serde_json::to_vec(&serde_json::json!({"schema":"reel.elevenlabs-candidate-take.v1","provider":"elevenlabs","model_id":model,"voice_id":self.voice_id,"language":self.language,"cue_id":self.cue_id,"sample_rate":self.sample_rate,"channels":1,"frames":frames,"audio_sha256":spec.take.sha256,"spoken_text_sha256":spec.spoken_text_sha256,"response_sha256":reel_workflow::digest(response),"alignment_sha256":spec.alignment.sha256,"selected":false,"approved":false})).map_err(|_|error(ErrorKind::Invalid,"elevenlabs-take-json"))?;
        Ok(BTreeMap::from([
            ("speech.wav".into(), wav),
            ("alignment.json".into(), alignment),
            ("normalized-alignment.json".into(), normalized),
            ("timing.json".into(), timing),
            ("take.json".into(), take),
        ]))
    }
}
