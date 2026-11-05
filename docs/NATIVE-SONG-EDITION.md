# Native short editions

`reel-song-edition` implements retained `reel.song-short-edit.v1` requests in
Rust. It makes a deterministic edition from an existing recording and its exact
request, generation evidence, canonical text and independent recognition passes.
It runs no Python, model, recognition service, download or publication operation.

```
reel-song-edition plan INPUT/edit.json
reel-song-edition render INPUT/edit.json NEW_DIRECTORY
reel-song-edition check INPUT/edit.json DIRECTORY
```

The explicit input view contains `source.wav`, `source-request.json`,
`source-generation.json`, `source-analysis.json`, `source.txt` and, for older
recognition evidence, `source-transcripts.zip`. Every input is hash/length-bound
by the request or the retained original request. The view must not escape through
symlinks. The adapter preserves the original private permissions, caption,
canonical lyrics, generation duration and source lock. An empty legacy lock is
retained as unknown; it is not evidence of a named immutable source version.

The supported original request is the existing Spanish
`reel.local-song-request.v1` text2music/cover contract, not a new universal
song-generation schema. Runtime version-manifest locks in retained source
packages remain distinct from the runtime payload hash. Neither establishes
current availability by itself.

## Edition contract

A recipe declares 1–64 ordered, nonoverlapping source windows, a 0–1 second
crossfade, 0–5 second entrance/exit fades and a duration ceiling of at most
600 seconds. Integer sample positions use round-to-nearest, ties-to-even.
Every window must keep a nonempty unchanged core. There is no resampling.

Supported WAVE sources are mono/stereo, 8–96 kHz, ordinary or supported extensible
PCM16/24/32 and float32, at most 600 seconds and 512 MiB. Metadata, archive,
word-count, match-search and output bounds are checked before accepting a packet.
`plan` reports the sample geometry, output bound, memory estimate and lyric gate.
This estimate is admission information, not a filesystem quota.

`render` writes exactly `recording.wav` (float32), `analysis.json` and
`edit-receipt.json` into a new directory, syncs files, reopens the packet and
checks it. Existing destinations refuse. Failure preserves partial files and
never removes the original source. `check` re-derives expected samples and exact
analysis/receipt bytes from the pinned inputs. A forged receipt cannot authorize
a modified recording. All unchanged sample bits, including negative zero, must
match the original. Output signal checks preserve the retained peak/RMS bounds.

## Recognition and acceptance

A word survives only if its entire finite timestamp span is inside one unchanged
core. Cut words and fade/crossfade words supply no evidence. Original pass/token
metadata is preserved. Remapped evidence is not a new recognition or performance.

Unprompted conditions must be explicit in original analysis settings or in the
pinned original transcript archive. For the latter, each named raw pass must
state `canonical_prompt:false`; its words and timestamp pairs must match the
original analysis. ZIP32 stored/deflate streams, lengths and CRCs are checked in
memory under entry/decoded-byte bounds. There is no filesystem extraction.
Missing provenance and contradictory prompt flags refuse; nothing is inferred.

The repeat-aware near-contiguous line gate allows at most one missing word and
two insertions, under the retained phrase-duration limit. Observed tokens cannot
be reused for separate canonical occurrences within a pass. Passes are never
combined to complete a line. Continuous-order diagnostics remain separate.
Optional `max_one_word_gap_lines` can make edition acceptance stricter without
rewriting the original line diagnostic. A technically verified packet may still
fail this quality gate and remains an unselected candidate.

No command asserts acoustic continuity, calls an edge instrumental without
supporting evidence, grants listening approval, selects a master or publishes it.
Use the existing managed job/retained-output workflow for production retention.
Platform qualification and an explicit owned binding are required before shared
lane adoption; a local passing test is not an installed-tool claim.

## Verification and review

`cargo test --locked --lib song_edition::` exercises hand-computed DSP, stereo
core identity, frame rounding, signed zero, WAVE formats, exact lineage,
permissions, whole-word projection, repeated lyrics, independent pass provenance,
ZIP integrity, failed candidates, strict word-gap caps and packet tampering.
The retained-package parser also tests separate runtime-manifest locks.

Producer-applied roles: music reconstruction checks clocks and immutable cores;
sound design/editing requires listening at changed regions; lyric adaptation
checks actual canonical words and independent evidence; rights/provenance keeps
private permissions and prohibits inferred authority. These are review checklists,
not independent review or creative approval.
