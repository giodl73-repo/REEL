# Native episode caption sidecars

`reel-episode-captions` assembles existing per-segment SRT tracks on the verified
native episode sample clock. It reads no media, runs no subprocesses, discovers
no cache, and changes no authoring or selection record. Existing caption CLI
commands and conform contracts are unchanged.

```
reel-episode-captions captions.json --input-root staged --output-dir new-captions
```

The strict input contract is `reel.episode-captions.v1`:

```json
{
  "schema": "reel.episode-captions.v1",
  "language": "es",
  "conform_manifest": {"path":"conform.json","sha256":"<exact SHA-256>","bytes":123},
  "conform_receipt": {"path":"receipt.json","sha256":"<exact SHA-256>","bytes":456},
  "tracks": [{
    "segment_id": "intro-01",
    "language": "es",
    "selected_master_sha256": "<same selected master as native segment>",
    "source_caption": {"path":"intro-01.srt","sha256":"<exact SHA-256>","bytes":789},
    "source_words_sha256": "<SHA-256 of UTF-8 JSON array of exact cue text strings>",
    "selection_evidence": {"path":"owner-selection.json","sha256":"<exact SHA-256>","bytes":321},
    "source_frame_rate": {"numerator":25,"denominator":1,"policy":"repeat-only"},
    "vtt_line_percent": 70
  }]
}
```

All FileRefs use the existing scene-delivery contract and must resolve inside
one explicitly staged root, with exact path case, byte length and SHA-256.
The conform recipe and receipt use their existing strict Rust types. There must
be an exact manifest-hash match. For current native Tig composition, recover the
receipt-pinned compiled manifest with [compiled-control inspection](tig-episode-control-inspection.md)
instead of substituting its authored baseline. There must
be exactly one track per ordered native segment, in the same order; no fixed
segment count or Journey identity is built into the adapter. An explicit empty
SRT (with words hash of `[]`) represents a selected caption-free segment. Missing
tracks fail. An SRT's cue indexes must begin at one and be contiguous. Unicode,
Spanish, translations, line breaks and whitespace within cue text survive
unchanged (CRLF is normalized to LF by the shared SRT parser).

The source clock reuses the existing rational frame-rate type. Its `repeat-only`
policy permits no source frame loss; original FPS/counts must agree with native
repeat-only evidence. The output clock comes from the conform recipe, or from
the verified common source clock when that optional recipe field is absent.
Per-segment master hash/bytes, receipt hash, kind, language, upstream verification,
frame/sample cursors, source duration, repeat counts, padding and silent holds
must agree. Source captions cannot exceed complete source picture or unpadded
source audio. No cue is truncated to fit: out-of-range and overlapping cues fail.

Offsets use exact `start_sample / output_sample_rate`, never summed rounded
segment durations. Start times round up and end times round down to milliseconds;
this protects silent holds and can differ by at most one millisecond from a
previous nearest-millisecond exporter. Source cue millisecond durations are
otherwise unchanged. The adapter supports any valid native recipe clock,
including 24 to 25 and 25 to 30 repeat-only conform.

A new or existing empty output directory receives `captions.srt`, `captions.vtt` and
`caption-timing.json`. The private timing receipt binds both native FileRefs,
the exact adapter manifest, ordered source SRT and selection evidence FileRefs,
source text hashes, content/hold sample spans and output hashes/byte counts.
Existing empty directories support managed runners that precreate output parents;
files use exclusive creation and existing content is never overwritten.
Optional per-track VTT `line:N%` preserves an explicitly selected caption-family
placement; no lyric text, translation or speaker credit is synthesized.

The producing repository owns selection, rights, text/translation authority and
approval. `selection_evidence` is retained and hash-checked as an opaque existing
owner record; this adapter does not interpret that repository's record grammar,
verify human approval, or independently rebuild/decode the native conform. The
caller must select the exact native receipt and caption manifest under its own
scoped controls. Technical success grants no publication or creative approval.

## Verification

`cargo test --test episode_captions` covers bilingual 27-segment assembly,
2-second holds after every segment, native 24/25/30 frame clocks, Unicode/text
preservation, inherited VTT placement, and rejection before output creation for
bad hashes, changed words, missing/reordered tracks, wrong language/master clocks,
unsafe paths, stale receipt binding, upstream gaps and hold leakage.

An ignored test supports explicitly staged private retained controls:

```
REEL_CAPTION_PRIVATE_FIXTURE=<closed fixture root>
REEL_CAPTION_PRIVATE_JOURNEY=<fixture take prefix>
cargo test --test episode_captions retained_native_caption_family_comparison -- --ignored --nocapture
```

The root contains exact native `conform.json`, `receipt.json`, original per-unit
`hosts.json` (takes/cues), `films.json` (tracks/cues), and retained `expected.srt`
and `expected.vtt`. The test converts source JSON cues to temporary SRT only for
verification; there is no JSON cue conversion production API. It requires exact
cue payload/order, at most 1ms timing difference and inherited host VTT placement.
Private fixtures and source media are excluded from Git.
