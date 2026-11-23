# Explicit original synthetic voice design

`reel-voice-design` is a separate Rust crate for authorized private ElevenLabs
design previews and provisionally saved voice candidates. It does not clone a
person, share a voice, choose the best performance, delete voices or alter project
casting. Existing exact-ID speech recording remains in `reel-tig-speech`.

```text
cargo build --locked -p reel-voice-design
reel-voice-design inspect REQUEST
reel-voice-design execute REQUEST NEW_ABSOLUTE_ATTEMPT_DIRECTORY
reel-voice-design review REVIEW_REQUEST NEW_ABSOLUTE_REVIEW_DIRECTORY
reel-editor-host serve NEW_ABSOLUTE_REVIEW_DIRECTORY
```

The bounded local request is at most 64 KiB. Input FileRefs have absolute `path`,
exact `sha256` and `bytes`. The authorization FileRef must contain JSON declaring
`generation_authorized:true`, `public_release:false`,
`synthetic_original_only:true` and a `roles` array naming the exact `role_id`.
This checks scope declarations, not their real-world authority.

Design schema `reel.voice-design-request.v1` includes `role_id`, `description`
(20–1000 characters), `text` (100–1000 characters), explicit `model_id`
(`eleven_multilingual_ttv_v2` or `eleven_ttv_v3`), integer `seed`,
`authorization`, `generation_authorized`, `public_release:false`, and
`timeout_seconds` (1–180). It makes one fixed-host design POST, requests MP3
previews, and retains up to three exact audio previews, the raw provider response,
request, authorization and receipt. No reference audio is accepted.

Save schema `reel.voice-save-request.v1` includes the same role/scope/description
fields, `voice_name`, `design_response` FileRef and `candidate_index` (1–3).
It makes one fixed-host create POST for that declared preview. Saving a provisional
candidate does not declare a listening comparison, production selection or human
approval. A workflow may save ordinal A simply to audition the same identity
in multiple languages; this must not be represented as choosing the best voice.

Only execute reads `ELEVENLABS_API_KEY`. The key goes in the header, never
arguments or saved controls. Redirects and automatic retries are disabled.
Response capture is capped at 16 MiB, individual preview audio at 8 MiB. Credential
reflection refuses. A new directory is reserved before a paid call; no-clobber
attempt/response/receipt files preserve completed, failed and indeterminate work.
Inspect private evidence and provider state before an explicit recovery decision;
a timeout does not establish that the call was unbilled or a voice was not saved.
Never replay the create step merely because a later file write failed.

The provider's default saved-voice privacy is retained; this adapter never requests
sharing. A returned enabled-sharing status refuses success and retains the raw
response for inspection. No automatic deletion or remote rollback follows.

Review schema `reel.voice-audition-review.v1` contains `title`, `notice` and
1–64 `items`, each with unique portable `id`, `character`, `kind`, `language`,
`text` and exact audio `source` FileRef. MP3/WAV files are capped at 8 MiB each
and 256 MiB combined. It freezes named audio into hash-derived routes and exports
the bundled HTML/JS and a `reel.voice-audition-session.v1` control document.
No provider call occurs. The existing native host recognizes only this pinned
frontend and declared audio routes, with its normal loopback capability, ranges,
timeouts, integrity and read-only restrictions. No provider IDs are needed in
the browser roster. Project packets and provider evidence stay outside engine Git.

The private browser supports character/language/recording-type filters, native
audio controls and source text. This is an audition interface, not a scene editor,
phoneme aligner or final casting application. Decoder checks do not establish
accent, perceived age, acting, source fidelity or a real performer's identity.

Primary protocol references:
[design](https://elevenlabs.io/docs/api-reference/text-to-voice/design),
[create](https://elevenlabs.io/docs/api-reference/text-to-voice/create).
