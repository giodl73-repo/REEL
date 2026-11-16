//! One-attempt ElevenLabs adapter over existing Tig byte custody. Declarations,
//! provider execution, timing qualification, listening and selection stay distinct.
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    time::Duration,
};
use tig_core::Object;
use tig_store::existing::{ByteSource, Profile as ByteProfile, ReadCache, RequiredProtection};
use zeroize::Zeroizing;

const CONTROL_MAX: u64 = 65_536;
const RESPONSE_MAX: u64 = 16 * 1024 * 1024;
const RATE: u64 = 44_100;
const API: &str = "https://api.elevenlabs.io/v1/text-to-speech/";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    byte_source: ByteProfile,
    request: Object,
    authorization: Object,
    max_response_bytes: u64,
    max_audio_samples: u64,
    max_workspace_bytes: u64,
    timeout_seconds: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    stability: f64,
    similarity_boost: f64,
    style: f64,
    use_speaker_boost: bool,
    speed: f64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    provider: String,
    role_id: String,
    voice_id: String,
    model_id: String,
    language_code: String,
    source_text: String,
    source_text_sha256: String,
    audio_tag: Option<String>,
    enable_logging: bool,
    seed: u32,
    voice_settings: Settings,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Authorization {
    schema: String,
    request: Object,
    role_id: String,
    voice_id: String,
    language_code: String,
    consent_evidence: Object,
    casting_evidence: Object,
    generation_authorized: bool,
    permitted_usage: String,
    public_release: bool,
}
struct Prepared {
    profile: Profile,
    request: Request,
    authorization: Authorization,
    request_raw: Vec<u8>,
    authorization_raw: Vec<u8>,
    provider_body: Vec<u8>,
}
fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
fn provider_text(request: &Request) -> String {
    request.audio_tag.as_ref().map_or_else(
        || request.source_text.clone(),
        |tag| format!("[{tag}] {}", request.source_text),
    )
}
fn validate_limits(profile: &Profile) -> Result<()> {
    let required = profile
        .max_response_bytes
        .checked_mul(5)
        .and_then(|n| n.checked_add(16 * 1024 * 1024))
        .context("workspace overflow")?;
    if profile.schema != "reel.tig-speech-profile.experimental-1"
        || profile.byte_source.required != RequiredProtection::PreventMutation
        || profile.max_response_bytes == 0
        || profile.max_response_bytes > RESPONSE_MAX
        || profile.max_audio_samples == 0
        || profile.max_audio_samples > 600 * RATE
        || profile.max_workspace_bytes < required
        || profile.max_workspace_bytes > 128 * 1024 * 1024
        || !(1..=300).contains(&profile.timeout_seconds)
    {
        bail!("speech profile limits refused");
    }
    Ok(())
}
fn validate(profile: &Profile, request: &Request, authorization: &Authorization) -> Result<()> {
    validate_limits(profile)?;
    let settings = &request.voice_settings;
    if request.schema != "reel.speech-request.v1"
        || request.provider != "elevenlabs"
        || !token(&request.voice_id)
        || !token(&request.model_id)
        || !token(&request.role_id)
        || request.language_code.len() != 2
        || !request
            .language_code
            .bytes()
            .all(|b| b.is_ascii_lowercase())
        || request.source_text.trim().is_empty()
        || request.source_text.len() > 16_384
        || request
            .source_text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        || Object::from_bytes(request.source_text.as_bytes())
            .sha256
            .as_str()
            != request.source_text_sha256
        || request.audio_tag.as_ref().is_some_and(|tag| {
            tag.trim().is_empty()
                || tag.len() > 64
                || !tag
                    .bytes()
                    .all(|b| b.is_ascii_alphabetic() || b" -".contains(&b))
        })
        || [
            settings.stability,
            settings.similarity_boost,
            settings.style,
        ]
        .iter()
        .any(|n| !n.is_finite() || !(0.0..=1.0).contains(n))
        || !settings.speed.is_finite()
        || !(0.7..=1.2).contains(&settings.speed)
        || authorization.schema != "reel.speech-authorization.v1"
        || authorization.request != profile.request
        || authorization.role_id != request.role_id
        || authorization.voice_id != request.voice_id
        || authorization.language_code != request.language_code
        || authorization.permitted_usage != "private-audition"
        || authorization.public_release
    {
        bail!("speech request or declared scope refused");
    }
    Ok(())
}
fn read_object(cache: &mut ReadCache, pin: &Object, limit: u64) -> Result<Vec<u8>> {
    if pin.bytes > limit {
        bail!("speech object exceeds budget");
    }
    let mut held = cache.open_verified(pin)?;
    let mut raw = Vec::new();
    held.read_to_end(&mut raw)?;
    Ok(raw)
}
fn prepare(profile_path: &Path) -> Result<(Prepared, ReadCache)> {
    let mut raw = Vec::new();
    fs::File::open(profile_path)?
        .take(CONTROL_MAX + 1)
        .read_to_end(&mut raw)?;
    if raw.len() as u64 > CONTROL_MAX {
        bail!("speech profile exceeds budget");
    }
    let profile: Profile = serde_json::from_slice(&raw)?;
    validate_limits(&profile)?;
    let mut cache = ReadCache::new(profile.byte_source.clone())?;
    let request_raw = read_object(&mut cache, &profile.request, CONTROL_MAX)?;
    let authorization_raw = read_object(&mut cache, &profile.authorization, CONTROL_MAX)?;
    let request: Request = serde_json::from_slice(&request_raw)?;
    let authorization: Authorization = serde_json::from_slice(&authorization_raw)?;
    validate(&profile, &request, &authorization)?;
    // Existing evidence bytes are required; their truth/authority is not inferred by this tool.
    read_object(&mut cache, &authorization.consent_evidence, CONTROL_MAX)?;
    read_object(&mut cache, &authorization.casting_evidence, CONTROL_MAX)?;
    let provider_body = serde_json::to_vec(&json!({
        "text":provider_text(&request), "model_id":request.model_id,
        "language_code":request.language_code,"seed":request.seed,
        "voice_settings":request.voice_settings,"apply_text_normalization":"off"
    }))?;
    Ok((
        Prepared {
            profile,
            request,
            authorization,
            request_raw,
            authorization_raw,
            provider_body,
        },
        cache,
    ))
}
pub fn inspect(profile: &Path) -> Result<Value> {
    let (prepared, _) = prepare(profile)?;
    Ok(
        json!({"schema":"reel.speech-inspection.v1", "request":prepared.profile.request,
        "provider_request":Object::from_bytes(&prepared.provider_body),
        "generation_authorized_declared":prepared.authorization.generation_authorized,
        "credentials_read":false,"provider_called":false,"human_approval":false}),
    )
}
trait Transport {
    fn post(
        &mut self,
        request: &Request,
        body: &[u8],
        key: &str,
        limit: u64,
        seconds: u64,
    ) -> Result<(u16, Vec<u8>)>;
}
struct Http;
impl Transport for Http {
    fn post(
        &mut self,
        request: &Request,
        body: &[u8],
        key: &str,
        limit: u64,
        seconds: u64,
    ) -> Result<(u16, Vec<u8>)> {
        let config = ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(seconds)))
            .build();
        let agent = ureq::Agent::new_with_config(config);
        let url = format!(
            "{API}{}/with-timestamps?output_format=pcm_44100&enable_logging={}",
            request.voice_id, request.enable_logging
        );
        let mut response = agent
            .post(&url)
            .header("xi-api-key", key)
            .header("Content-Type", "application/json")
            .send(body)
            .map_err(|_| anyhow::anyhow!("speech transport failed; no automatic retry"))?;
        let status = response.status().as_u16();
        let mut raw = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(limit + 1)
            .read_to_end(&mut raw)
            .map_err(|_| anyhow::anyhow!("speech response read failed; no automatic retry"))?;
        if raw.len() as u64 > limit {
            bail!("speech response exceeds budget");
        }
        Ok((status, raw))
    }
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn begin(prepared: &Prepared, output: &Path, provider_call_possible: bool) -> Result<()> {
    if !output.is_absolute()
        || output.file_name().is_none()
        || !output.parent().is_some_and(Path::is_dir)
    {
        bail!("new absolute speech attempt directory with existing parent required");
    }
    fs::create_dir(output)?;
    write_new(&output.join("request.json"), &prepared.request_raw)?;
    write_new(
        &output.join("authorization.json"),
        &prepared.authorization_raw,
    )?;
    write_new(
        &output.join("provider-request.json"),
        &prepared.provider_body,
    )?;
    write_new(
        &output.join("attempt.json"),
        &serde_json::to_vec(&json!({
            "schema":"reel.speech-attempt.v1","state":"incomplete-do-not-retry-automatically",
            "request":prepared.profile.request,"authorization":prepared.profile.authorization,
            "provider_call_possible":provider_call_possible,
            "production_selection":false,"human_approval":false
        }))?,
    )?;
    Ok(())
}
fn wave(pcm: &[u8]) -> Result<Vec<u8>> {
    if pcm.is_empty() || pcm.len() % 2 != 0 || pcm.len() > RESPONSE_MAX as usize {
        bail!("invalid PCM16 transport bytes");
    }
    let size = u32::try_from(pcm.len())?;
    let mut bytes = Vec::with_capacity(pcm.len() + 44);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(size + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&(RATE as u32).to_le_bytes());
    bytes.extend_from_slice(&((RATE * 2) as u32).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(pcm);
    Ok(bytes)
}
#[derive(Serialize, Deserialize)]
struct Alignment {
    #[serde(deserialize_with = "bounded_vector")]
    characters: Vec<String>,
    #[serde(deserialize_with = "bounded_vector")]
    character_start_times_seconds: Vec<f64>,
    #[serde(deserialize_with = "bounded_vector")]
    character_end_times_seconds: Vec<f64>,
}
fn bounded_vector<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Bounded<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Bounded<T> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("at most 32768 alignment entries")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Vec<T>, A::Error> {
            let mut items = Vec::new();
            while let Some(item) = seq.next_element()? {
                if items.len() == 32_768 {
                    return Err(serde::de::Error::custom("alignment entry budget exceeded"));
                }
                items.push(item);
            }
            Ok(items)
        }
    }
    deserializer.deserialize_seq(Bounded(std::marker::PhantomData))
}
#[derive(Deserialize)]
struct ProviderResponse {
    audio_base64: String,
    alignment: Option<Alignment>,
    normalized_alignment: Option<Alignment>,
}
fn timing(request: &Request, response: &Value, samples: u64) -> Value {
    let unqualified = || json!({"qualified":false,"reason":"original-character-alignment-not-exact-or-out-of-bounds"});
    let Ok(a) = serde_json::from_value::<Alignment>(response["alignment"].clone()) else {
        return unqualified();
    };
    if a.characters.len() > 32_768
        || a.characters.iter().any(|s| s.chars().count() != 1)
        || a.characters.len() != a.character_start_times_seconds.len()
        || a.characters.len() != a.character_end_times_seconds.len()
    {
        return unqualified();
    }
    let text = a.characters.concat();
    let skip = if text == request.source_text {
        0
    } else if text == provider_text(request) {
        text.chars().count() - request.source_text.chars().count()
    } else {
        return unqualified();
    };
    let mut last_start = 0.0;
    let mut last_end = 0.0;
    for (start, end) in a
        .character_start_times_seconds
        .iter()
        .zip(&a.character_end_times_seconds)
    {
        if !start.is_finite()
            || !end.is_finite()
            || *start < last_start
            || *start < 0.0
            || *end < *start
            || *end < last_end
            || *end > samples as f64 / RATE as f64
        {
            return unqualified();
        }
        last_start = *start;
        last_end = *end;
    }
    let mut words = Vec::new();
    let mut start = None;
    let mut word = String::new();
    for i in skip..a.characters.len() {
        let ch = a.characters[i]
            .chars()
            .next()
            .expect("one character checked");
        if ch.is_whitespace() {
            if let Some(first) = start.take() {
                words.push(json!({"text":word,"start_seconds":a.character_start_times_seconds[first],"end_seconds":a.character_end_times_seconds[i-1]}));
                word.clear();
            }
        } else {
            if start.is_none() {
                start = Some(i);
            }
            word.push(ch);
        }
    }
    if let Some(first) = start {
        words.push(json!({"text":word,"start_seconds":a.character_start_times_seconds[first],"end_seconds":a.character_end_times_seconds.last()}));
    }
    if words
        .iter()
        .any(|word| word["end_seconds"].as_f64() <= word["start_seconds"].as_f64())
    {
        return unqualified();
    }
    json!({"qualified":true,"exact_source_text_sha256":request.source_text_sha256,
        "words":words,"method":"exact original-character alignment; only declared leading tag excluded",
        "performance_text_faithfulness_or_listening_approval":false})
}
fn materialize(prepared: &Prepared, raw: &[u8], output: &Path, origin: &str) -> Result<Value> {
    write_new(&output.join("provider-response.json"), raw)?;
    // Typed decoding avoids the 64 KiB control-document parser and ignores unknown
    // provider fields without allocating arbitrary JSON trees. Known duplicate fields fail.
    let response: ProviderResponse = serde_json::from_slice(raw)?;
    let pcm = STANDARD.decode(&response.audio_base64)?;
    let samples = pcm.len() as u64 / 2;
    if samples > prepared.profile.max_audio_samples {
        bail!("speech audio exceeds sample budget");
    }
    let wav = wave(&pcm)?;
    let alignment_value = json!({"alignment":response.alignment,
        "normalized_alignment":response.normalized_alignment});
    let timings = timing(&prepared.request, &alignment_value, samples);
    let alignment = serde_json::to_vec(&json!({"alignment":alignment_value["alignment"],
        "normalized_alignment":alignment_value["normalized_alignment"],"source_word_timing":timings}))?;
    write_new(&output.join("raw.pcm"), &pcm)?;
    write_new(&output.join("recording.wav"), &wav)?;
    write_new(&output.join("alignment.json"), &alignment)?;
    let receipt = json!({"schema":"reel.speech-candidate-receipt.v1","origin":origin,
        "request":prepared.profile.request,"authorization":prepared.profile.authorization,
        "consent_evidence":prepared.authorization.consent_evidence,
        "casting_evidence":prepared.authorization.casting_evidence,
        "provider_request":Object::from_bytes(&prepared.provider_body),
        "provider_response":Object::from_bytes(raw),"raw_PCM":Object::from_bytes(&pcm),
        "wave":Object::from_bytes(&wav),"alignment":Object::from_bytes(&alignment),
        "sample_rate":RATE,"channels":1,"bits_per_sample":16,"samples":samples,
        "timing_qualified":timings["qualified"],"audio_tag_effect":"advisory; listening required",
        "source_text_bound":true,"generated_speech_verbatim_verified":false,
        "provider_identity_verified_by_import":false,"production_selection":false,
        "public_release":false,"human_approval":false});
    let receipt_raw = serde_json::to_vec(&receipt)?;
    write_new(&output.join("receipt.json"), &receipt_raw)?;
    Ok(
        json!({"schema":"reel.speech-result.v1","receipt":Object::from_bytes(&receipt_raw),
        "samples":samples,"timing_qualified":timings["qualified"],
        "production_selection":false,"human_approval":false,"published":false}),
    )
}
fn perform(
    prepared: &Prepared,
    output: &Path,
    key: &str,
    transport: &mut impl Transport,
) -> Result<Value> {
    if !prepared.authorization.generation_authorized {
        bail!("generation not authorized by declaration");
    }
    if key.len() < 16
        || key.len() > 512
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        bail!("credential unavailable or malformed");
    }
    if [
        &prepared.request_raw,
        &prepared.authorization_raw,
        &prepared.provider_body,
    ]
    .iter()
    .any(|raw| raw.windows(key.len()).any(|w| w == key.as_bytes()))
    {
        bail!("declared inputs contain credential material");
    }
    begin(prepared, output, true)?;
    let result = (|| -> Result<Value> {
        // Exactly one call. Neither transient failures nor provider refusals are retried.
        let (status, raw) = transport.post(
            &prepared.request,
            &prepared.provider_body,
            key,
            prepared.profile.max_response_bytes,
            prepared.profile.timeout_seconds,
        )?;
        if raw.len() as u64 > prepared.profile.max_response_bytes {
            bail!("speech response exceeds budget");
        }
        if raw.windows(key.len()).any(|w| w == key.as_bytes()) {
            bail!("provider response contains credential material");
        }
        write_new(
            &output.join("provider-status.json"),
            &serde_json::to_vec(&json!({"http_status":status}))?,
        )?;
        if status != 200 {
            write_new(&output.join("provider-error-body.bin"), &raw)?;
            bail!("provider refused; no automatic retry");
        }
        materialize(prepared, &raw, output, "native-single-HTTP-attempt")
    })();
    if result.is_err() {
        let _ = write_new(&output.join("failure.json"), br#"{"schema":"reel.speech-failure.v1","state":"failed-or-indeterminate-do-not-retry-automatically","production_selection":false,"human_approval":false}"#);
    }
    result
}
pub fn execute(profile: &Path, output: &Path) -> Result<Value> {
    let (prepared, _) = prepare(profile)?;
    if !prepared.authorization.generation_authorized {
        bail!("generation not authorized by declaration");
    }
    let key =
        Zeroizing::new(std::env::var("ELEVENLABS_API_KEY").context("credential unavailable")?);
    perform(&prepared, output, &key, &mut Http)
}
pub fn capture(profile: &Path, sha256: &str, bytes: u64, output: &Path) -> Result<Value> {
    let (prepared, mut cache) = prepare(profile)?;
    let response = Object {
        sha256: tig_core::Digest::parse(sha256)?,
        bytes,
    };
    let raw = read_object(&mut cache, &response, prepared.profile.max_response_bytes)?;
    begin(&prepared, output, false)?;
    let result = materialize(
        &prepared,
        &raw,
        output,
        "unverified-existing-response-import-no-provider-call",
    );
    if result.is_err() {
        let _ = write_new(&output.join("failure.json"), br#"{"schema":"reel.speech-failure.v1","state":"capture-validation-failed-no-provider-call","production_selection":false,"human_approval":false}"#);
    }
    result
}

#[cfg(test)]
mod tests;
