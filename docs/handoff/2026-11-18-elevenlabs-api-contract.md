# ElevenLabs speech adapter contract check

Official documentation checked on 2026-11-18. This is an integration reference,
not an implemented transport or service qualification. No project text, voice,
audio or credentials were submitted.

The [model catalogue](https://elevenlabs.io/docs/overview/models) names
`eleven_v4` and `eleven_v4_turbo`. It lists a 10,000-character request limit for
`eleven_v4`. Preserve an explicitly selected model ID; do not interpret the
request for v4 as a new HTTP API version or silently fall back to another model.
These documented limits do not establish a particular account's access.

The [model-list API](https://elevenlabs.io/docs/api-reference/models/list)
exposes `GET /v1/models`, including TTS capability, language IDs and
`maximum_text_length_per_request`. A provider adapter should preserve the
observed model metadata alongside its request configuration. Account/model
admission must be explicit; inspection of a frozen workflow cannot query a
service automatically.

The [timed TTS endpoint](https://elevenlabs.io/docs/api-reference/text-to-speech/convert-with-timestamps)
uses `POST /v1/text-to-speech/{voice_id}/with-timestamps`. A request can specify
`model_id`, `language_code`, voice settings, versioned pronunciation dictionaries
and an optional seed. The documented default model is not v4, so omission is
unsafe for the requested workflow. Seed reproducibility is best effort. The
response contains base64 audio and optional character timing for original and
normalized text. Logging defaults on; the documented zero-retention option is
enterprise-only. A privacy configuration must describe actual supported behavior.

REEL must bind audio, exact submitted text, original/normalized alignment and
request/response evidence to one task/attempt/take. The existing
`reel-assembly::scene_authoring::import_provider_words` accepts an exact-text
forced-alignment word response, not TTS character arrays. A separate checked
conversion is required before TTS timing can drive semantic cue events. Missing
alignment cannot become invented word times, and normalized text cannot silently
replace project source text. Each language keeps its own trigger binding and
selected-take sample clock.

Already recorded season-5 takes remain reusable inputs. Selecting one must not
create a synthesis request. New synthesis remains a candidate operation with
project voice/likeness consent and selection separate from technical validation.
Direct API, proxy and manual response paths must converge on the same bounded
response validation and native retention lifecycle.
