# Timed speech candidate adapter

`reel-workflow-http::elevenlabs` composes explicit `eleven_v4` or
`eleven_v4_turbo` requests and checks timed responses. Native provider config
accepts `"service":{"kind":"elevenlabs-timed","spec":...}`. Omitting service
preserves the existing proxy contract and workspace identity. This is component
verification; complete built-in native lifecycle and real-account qualification
remain open.

## Frozen inputs and request

The native task is `speech-candidate`, provider `elevenlabs`, with matching
language. Spec schema `reel.elevenlabs-timed.v1` freezes `voice_id`, `language`,
`cue_id`, `sample_rate`, `enable_logging`, optional `seed`, `voice_settings`,
authored `segments`, and four input IDs: `text_input`, `models_input`,
`admission_input`, `decision_input`. Exact UTF-8 `text/plain` source, JSON model
catalogue, JSON admission and immutable decision are admitted native inputs.
Only text is named in `send_inputs`; authority/catalogue bytes stay local.
Segments reproduce source bytes and explicit language-local word boundaries.

Admission schema `reel.elevenlabs-admission.v1` binds `voice_id`, `model_id`,
`language`, `text_sha256`, `account_subject`, `model_access_admitted`,
`voice_use_admitted`, `zero_retention_admitted`, and exact Artifact
`approval_subject`. These are frozen caller declarations, not authentication of
consent or service entitlements. Producer authority, speaker scope, audience,
retention and reuse still require actual decision review. Disabling logging
requires the zero-retention declaration. The frozen catalogue must contain
exactly one selected TTS model with sufficient text capacity, requested language
and capabilities for any enabled style or speaker boost. No model fallback.

Endpoint: `/v1/text-to-speech/VOICE/with-timestamps`, exactly
`output_format=pcm_RATE` and `enable_logging=BOOL`. HTTPS is restricted to the
adapter's official service host allowlist and a named `xi-api-key` secret.
Explicit literal-loopback HTTP supports fixtures. Ordinary transport validation
still rejects credentials/fragments, redirects and retries. PCM rates: 8000,
16000, 22050, 24000, 32000, 44100, 48000 Hz. Speed 0.7–1.2 is the adapter's local
subset, not a claim about the full API range. Text normalization is off; seeds
do not promise repeatability. Account format/model eligibility remains separate.

## Outputs and recovery

Required outputs: `provider-request.json`, `provider-response.json`,
`provider-config.json`, `http.json`, `alignment.json`,
`normalized-alignment.json`, `timing.json`, `take.json` (all JSON), and
`speech.wav` (`audio/wav`). Existing evidence bounds and owned workspace
admission apply. Response identifiers/status are flushed before body capture.
Complete captured responses resume under the original operation without another
POST; incomplete calls need original-operation reconciliation.

PCM must be nonempty mono signed 16-bit little-endian. The adapter wraps it in
WAV, measures frames and converts exact original alignment into language-local
word timing against that clock. Original and normalized alignment remain
separate. Missing original alignment, source mismatch, backward time or timing
beyond the take refuses conversion; normalized data is never a fallback.
Take metadata remains `selected:false`, `approved:false`. The existing timing
schema's `selected_take_sha256` binds these candidate bytes; a consumer must
explicitly select the exact take before use. Existing season-5 recordings remain
reusable assets; synthesis is only an explicit configured task.

## Evidence and producer checks

Thirteen HTTP component/CLI tests, 48 default native tests and strict Clippy
for both isolated crates pass. Twenty native effect fixtures remain opt-in and
were not rerun for this component checkpoint. Tests cover ES/EN
timing, exact original alignment preservation, admission/endpoint refusals,
invalid PCM/clock rejection and identifiers retained before body truncation.
No real credentials, voices or project media were used. The previously qualified
generic native proxy fixture does not qualify this built-in adapter end to end.

Producer-applied sound-design/editing checks require selected-take timing before
edit decisions. Rights/provenance checks require frozen scope evidence, explicit
text egress, private raw evidence and distinct consent/selection/release gates.
These checklist findings grant no listening, creative, translation, voice or
public-release approval.

Official contracts checked during implementation:
[timed TTS](https://elevenlabs.io/docs/api-reference/text-to-speech/convert-with-timestamps),
[model catalogue](https://elevenlabs.io/docs/api-reference/models/list),
[models](https://elevenlabs.io/docs/overview/models), and
[audio formats](https://elevenlabs.io/docs/help-center/troubleshooting/what-audio-formats-do-you-support).
