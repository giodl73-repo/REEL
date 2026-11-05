# Native short-edition migration

`reel::song_edition` is the shared Rust in-memory edit core. This is one component
of the native replacement for the archived `tools/local_song_edit.py` workflow;
it is not yet a Melena production command or a replacement for the complete
recording, recognition and selection process. No Python is executed by this core.

`plan(recipe, sample_rate, source_frames)` accepts 1–64 ordered nonoverlapping
source windows, a 0–1 second crossfade, 0–5 second entrance/exit fades and a hard
duration ceiling of at most 600 seconds. Source intake is mono/stereo at
8–96 kHz, at most 600 seconds. Seconds become integer sample frames using
round-to-nearest, ties-to-even, matching the retained recipe contract. Every
segment must retain a nonempty unchanged core after its fades. The validated
plan cannot be constructed or changed through public fields or deserialization.

`render(plan, decoded_float32_pcm, channels)` creates the ordered edition without
resampling or modifying unchanged cores. Crossfades and edge ramps operate only
in declared changed regions. `verify_cores` checks dimensions, finite samples and
every unchanged sample's exact float32 bits. Container decoding/encoding,
source/output hashes, signal acceptance, memory/workspace admission and retained
publication belong to the caller's next adapter. Bitwise equality also preserves
negative zero; changed regions are not represented as unchanged evidence.

`remap_passes(plan, original_passes)` retains a word only if its entire finite,
ordered timestamp span lies inside one unchanged core. It shifts that span to
the edition clock and preserves the token's other metadata. Null/malformed spans,
cut words, partial words and fade/crossfade words provide no evidence. Passes and
tokens are bounded; duplicate pass names and explicitly prompt-conditioned passes
refuse. Original pass metadata is retained separately. Missing prompt provenance
is **not** converted into an `unprompted` assertion. An admitting adapter must
validate the exact original recognition source and its unprompted conditions.
Derived passes state no new recognition, performance or human approval.

## Remaining adapter requirements

Before Melena can adopt a complete native short-edition entry:

1. Bind exact source recording, generation request, generation proof, analysis,
   canonical text and source-version lock, preserving candidate IDs and history.
   Require matching permissions, caption, original generation duration and lyrics.
2. Decode supported source WAVE to float32 under explicit byte/memory limits;
   encode a FLOAT WAVE and re-open it before checking every unchanged core.
3. Recompute the retained repeat-aware near-contiguous line-presence gate from
   filtered original independent passes, without combining passes to fill a line
   or reusing observations for separate source occurrences. Preserve continuous
   source-order diagnostics separately. No prompt text supplies recognition.
4. Produce hash-bound analysis and edit receipts through an explicit bounded CLI,
   retain all outputs using the existing managed job and Tig admission path,
   and exercise current/prior restoration and owned-binding rollback.
5. Qualify the actual image on supported platforms before adopting an optional
   Melena binding. Acoustic continuity, listening, selection and delivery remain
   distinct from deterministic correspondence and lyric screening.

The core tests cover a hand-computed crossfade/edge envelope, exact stereo cores,
ties-to-even rounding, one-sample fades, unchanged identity including negative
zero, invalid source shapes/bounds, word-span exclusion, pass metadata retention,
prompt-conditioned rejection and evidence budgets. Run
`cargo test --lib song_edition::tests --locked`; no models, media downloads,
providers or services are needed.

Producer-applied role review: music reconstruction checks the sample clock and
immutable cores; sound design/editor checks identify joins and fades as changed
regions requiring listening; lyric adaptation preserves original words and
excludes changed spans; rights/provenance forbids inferred prompt provenance or
new authority. These are checklists, not independent review or human approval.
The outstanding adapter requirements above prevent production-adoption claims.
