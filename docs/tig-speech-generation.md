# Private speech attempts over existing Tig objects

`reel-tig-speech` is an experimental, explicit ElevenLabs adapter. It produces
unselected private candidates, not scene wrappers, approved performances or
production assets. Build with `cargo build --locked --features speech-provider
--bin reel-tig-speech`. No project runtime binding changes automatically.

```text
reel-tig-speech inspect PROFILE
reel-tig-speech capture PROFILE RESPONSE_SHA256 RESPONSE_BYTES NEW_ABSOLUTE_OUTPUT
reel-tig-speech execute PROFILE NEW_ABSOLUTE_OUTPUT
```

Inspection verifies existing objects without reading credentials, writing an
attempt or contacting a provider. Capture imports an existing exact response;
it cannot establish who generated it. Execute alone reads `ELEVENLABS_API_KEY`
from the environment and makes one HTTPS POST. Never put a credential in an
argument, profile, evidence object or request. Execution rejects reflected
credential bytes before saving inputs or a provider response. Error output is
static; detailed request and failure artifacts are private.

## Inputs and scope

The profile is a bounded JSON control document:

```json
{
  "schema": "reel.tig-speech-profile.experimental-1",
  "byte_source": {
    "places": [{"root": "/existing/owned/store", "layout": "flat"}],
    "required": "prevent-mutation",
    "limits": {
      "object_bytes": 1048576, "total_bytes": 2097152,
      "requests": 5, "verify_millis": 30000, "live_readers": 1
    }
  },
  "request": {"sha256": "<exact request hash>", "bytes": 0},
  "authorization": {"sha256": "<exact scope hash>", "bytes": 0},
  "max_response_bytes": 1048576,
  "max_audio_samples": 88200,
  "max_workspace_bytes": 33554432,
  "timeout_seconds": 30
}
```

Replace placeholders and use an existing absolute store root. This example does
not initialize a store. Protected reads currently require Windows; unsupported
platforms fail rather than weaken protection. Every read checks exact hash and
length through the existing Tig byte source. Corruption fails.

The request uses `schema: reel.speech-request.v1`, `provider: elevenlabs`,
`role_id`, `voice_id`, `model_id`, two-letter lower-case `language_code`, exact
`source_text`, its UTF-8 `source_text_sha256`, nullable `audio_tag`, explicit
`enable_logging`, integer `seed`, and `voice_settings` containing `stability`,
`similarity_boost`, `style`, `use_speaker_boost`, and `speed`. No default casting
or model is inferred. The tag, if declared, is a leading advisory delivery tag;
its effect requires listening. A seed does not guarantee deterministic output.

The scope object uses `schema: reel.speech-authorization.v1`, the exact request
object pin, matching `role_id`, `voice_id`, `language_code`, existing
`consent_evidence` and `casting_evidence` object pins, explicit boolean
`generation_authorized`, `permitted_usage: private-audition`, and
`public_release: false`. Both evidence objects must be present and verified.
The tool checks these declarations and their binding, not their authority or
truth. It cannot grant consent, establish a performer's identity, change an
existing assignment or turn style feedback into casting approval.

Control objects and evidence are limited to 64 KiB each; source text to 16 KiB.
The response cap is at most 16 MiB, audio at most 600 seconds at 44.1 kHz, and
timeout 1–300 seconds. The declared workspace allowance must be at least five
times the response cap plus 16 MiB, and at most 128 MiB. This is an allocation
planning guard, not an OS memory sandbox. Alignment arrays are capped at 32768
entries. The provider body is decoded into typed fields, without allocating
unknown provider JSON trees.

## Provider protocol and failure boundary

Execution fixes the host to `api.elevenlabs.io` and uses the
[convert-with-timestamps endpoint](https://elevenlabs.io/docs/api-reference/text-to-speech/convert-with-timestamps)
with `output_format=pcm_44100`. Identifiers cannot inject a URL path or query.
The credential appears only in the `xi-api-key` header. Redirects are disabled;
non-200 status, timeout, refusal, read failure and over-budget responses fail
without an automatic second attempt. Explicit logging policy is sent as a query
parameter; the provider restricts disabling logging to eligible accounts. An
unsupported policy or model must be resolved by the producer, not retried with
weaker settings. Text normalization is explicitly off.

The output directory must be new, absolute and have an existing parent. Every
file uses create-new semantics. An attempt marker and exact declared inputs are
saved before execution; partial or indeterminate attempts remain for review.
There is no resume or automatic retry. A bounded non-200 response is retained
privately as `provider-error-body.bin`, after checking for credential reflection.
Transport errors have a static failure record. Do not infer that a failed or
timed-out attempt was unbilled or never reached the provider.

## Candidate outputs and qualification

On a successful decode, the attempt contains the raw response, original PCM16
mono 44100 Hz bytes, a WAV wrapper with unchanged samples, original and normalized
character alignment, and a receipt pinning the exact inputs and outputs. There
is no pitch shift, gain change, time stretch, resampling or scene-clock edit.

Source-word timing qualifies only when original character alignment matches the
exact source or the source with its declared leading tag, every entry is one
Unicode scalar, times are monotonic and within the actual audio duration, and
each word has positive duration. Normalized alignment is retained but never
substituted for source alignment. Missing or mismatched timing stays unqualified;
no forced word timings are invented. Malformed responses remain failed attempts.
Even qualified timing does not prove that the spoken words are faithful.

An execution receipt records one native HTTP attempt; a capture receipt explicitly
records unverified import origin. Neither claims human approval, a verified
performer identity, production selection, public release or publication. Listen
for pronunciation, language, acting, source fidelity and contextual timing before
adopting a candidate. Existing project approvals remain separate.

After review, retain private payloads using the existing
[`reel-tig-job publish-file` lifecycle](tig-job-runner.md), and author scoped
selection/provenance records in the producing project. This command does not
publish, initialize caches, modify selections, install runtimes or render scenes.

## Offline qualification

`cargo test --lib --features speech-provider speech_generation::tests` uses
synthetic text, PCM and mocked transport. It checks exact source/scope binding,
single-attempt failures, credential reflection, no overwrite, large responses,
Unicode timing, absent timing and malformed responses. Windows tests also read
actual protected temporary objects, inspect and capture, then reject corruption.
These are technical tests, not live service, listening, casting or consent review.
