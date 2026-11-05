# Inspect an older original song source

`reel-song-source inspect <admission.json> <new-context.json>` checks retained
source correspondence without generating audio or rewriting the original request.
It accepts explicit named files and exact byte pins. It performs no cache lookup,
hydration, model setup, network request, selection or publication.

The private admission schema is `reel.song-source-interpretation.v1`. Its five
inputs are `original_package`, `original_request`, `original_generation`, `canonical_text` and
`recording`. Each has an absolute `path` and an `object` with lowercase `sha256`
and `bytes`. Ordinary RIFF WAVE decoding checks the recording's finite samples,
channels and native clock. Existing source-duration and PCM-format bounds apply.
Metadata, text and recording sizes are independently bounded. On Windows, named
inputs remain open with read sharing only through context publication.

`source_version` must match the version in the original package, which must also
bind the exact request, generation evidence and recording pins.
`interpretation` names the exact package, request and generation schema strings and these
JSON pointers:

- `package_version_pointer`, `package_request_pointer`,
  `package_generation_pointer`, `package_recording_pointer`;
- `canonical_lyrics_pointer`, `render_lyrics_pointer`;
- `private_only_pointer`, `external_upload_pointer`,
  `identifiable_imitation_pointer`, `public_release_pointer`;
- `generation_request_sha256_pointer`,
  `generation_canonical_sha256_pointer`, `generation_render_sha256_pointer`,
  `generation_outputs_pointer`;
- `generation_private_flag_pointer`, `generation_external_upload_pointer`.

The operator supplies and audits this interpretation; matching a schema string
does not independently establish what its fields mean. The report explicitly
leaves mapping authority unverified. Original private-use flags must be true;
upload, imitation and public-release flags must be false. Missing or nonboolean
values refuse. Request restriction pointers must be distinct.

The generation evidence must hash-bind the exact original request and both its
canonical and rendered lyric strings. Its output roster must contain the recording exactly
once, with consistent hash and byte count. Canonical source text must match the
request exactly. The optional `allow_single_terminal_lf_difference` permits only
one terminal LF difference; it performs no whitespace, spelling, Unicode or
punctuation normalization. Both original text layers and their byte pins survive.
Supplied lyric text is never presented as proof of the performed words.

The new private output uses `reel.song-repair-source-context.v1`, with separate
original evidence pins, the admitted interpretation, source text, native audio
clock and restrictive private scope. It is a new correspondence context. It is
not an original generation request or a retroactive statement of approval. Voice
consent, human approval, performed-word verification and provider execution remain
false. Existing generated-guide validators are unchanged.

The in-memory inspection result is opaque and retains the read handles until it
is dropped. Writing a context requires a new absolute filename in an existing
directory and uses a no-clobber atomic publication. The CLI prints a summary that
omits lyrics and input paths; the context itself remains private and unshareable.

This entry supplies one prerequisite for bounded same-source lyric repair. It
does not execute a repair range, qualify a provider, preserve samples by itself,
clear acoustic or exact-word diagnostics, or finish managed-result custody.

Producer-applied rights/provenance and lyric review: retain the original bytes,
keep the interpretation explicit, separate supplied and performed words, and
carry private-use and approval holds into any later candidate operation.
