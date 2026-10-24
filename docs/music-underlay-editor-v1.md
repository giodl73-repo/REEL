# Exact-text lyric underlay workshop

The underlay workflow assigns existing native melody notes to spans of an
explicitly supplied lyric file. It preserves that file's exact bytes and the
model's lyric-layer identity, language, kind, hash and authority. It writes the
existing `lead_sheet.underlay` contract in a new unreviewed native music model.
It changes no notes, performance, source wording or translation.

```text
reel-music-editor export-underlay model.yaml lyrics.txt new-editor-directory
reel-music-editor apply-underlay model.yaml lyrics.txt music-underlay-edit.json new-proposal.json
```

The model must already declare a lead sheet, melody/vocal part and selected
lyric layer. The explicit lyric file must match that layer's native resolved path
and byte hash. An incomplete underlay can be opened for repair; a completed
candidate must pass the shared native underlay validator before publication.
Only the two named files and the supplied request are read. No other lyric
layer, source recording, analysis manifest, cache or archive is discovered.

## Editing

Export creates private static HTML/JavaScript/session files. Use an explicitly
requested local static server; export starts no service. The interface shows
the selected layer's language and canonical/as-sung kind, with read-only text.
Select a span, highlight source text and choose **Use selected text**. Assign
the first/last notes from the shared note list, choose syllable treatment, and
add/remove/reorder groups as needed. Several notes can share a text span for a
melisma. Existing notes remain fixed. Explain changed underlay in the proposal
reason, then save a locked request. Undo, reset, valid-draft autosave and
import/export remain bound to this exact model and lyric layer.

The interface maps displayed selection positions back to original UTF-8 byte
boundaries, including accents, emoji and CRLF/bare-CR line endings. It never
normalizes the source file. Surrogate halves, invalid UTF-8 slices, blank spans,
overlapping/out-of-order text and duplicate/missing/foreign note references
refuse. Native order validation includes notes inside the first melisma, fixing
an earlier gap which checked order only after the first text group. An existing
boundary inside a CRLF pair is preserved but cannot be highlighted directly in
the normalized textarea; selecting new text is an explicit change.

## Request and candidate

`reel.music-underlay-session.v1` supplies a `reel.music-underlay-edit.v1` request:

- exact `expected_model_sha256`, `expected_lyric_sha256` and `lyric_layer_id`;
- a new `candidate_model_id` and a `rationale` for changed mappings;
- native `underlay` entries: `id`, `note_ids`, UTF-8 `text_start_byte`/
  `text_end_byte`, and `syllabic` (`single`, `begin`, `middle`, `end`).

No replacement lyric text, note changes or approval fields are accepted. Every
melody note must occur exactly once, in nondecreasing native onset order, with
unique span IDs and non-overlapping source text ranges. Musical syllabification
and the adequacy of coverage still require actual lyric/vocal review; structural
validation does not infer word boundaries, pronunciation or a sung performance.

Models, requests, sessions and model outputs retain the 2 MiB control limit;
the selected lyric file is bounded to 128 KiB. At most 1000 spans and 1000 total
note references are accepted, within the existing 64-part/1000-note editor
scope. IDs are bounded to 256 UTF-8 bytes and reasons to 4096. Span controls are
paged in groups of 100 and share one bounded note-choice list. Input reparse/
symlink files refuse. Hash locks are optimistic content checks, not a filesystem
lease; stale bytes, another layer/path, unknown fields and existing outputs fail.

Candidate publication uses the shared native-model editor I/O beside the source
model, preserving its relative references. All note/provenance records and lyric
authorities stay unchanged. Model authority/review statuses reset to
`not-reviewed`, old model decision references clear, and the candidate's required
review roles include `lyrics-vocal-adaptation-editor`. This records a review
requirement, not an actual person's approval. The exact request/reason stays
separate from lyric content; retain it with the original model and lyric asset
when registering the new version in the producing project's asset system.

## Evidence and remaining gates

Success summaries contain counts and state flags only.
`lyric_bytes_verified: true` means the supplied lyric file matches its declared
byte identity; it does not verify text authority or rights.
`audio_resolved: false` and `native_validation: "not-run"` remain explicit. Full
source/analysis/other-layer validation, native score export/check, musical review,
browser acceptance, private-song compatibility and runtime adoption are separate.

Synthetic integration applies a source-phrase melisma, invokes the full native
model validator and the existing MIDI/MusicXML/guide/SVG renderer/checker, and
compares every original fixture byte. Both the original note-editor round trip
and the new underlay case remain in CI. Handler tests exercise exact selections,
source locks, repair, undo/import/export and bounded pagination using a synthetic
DOM; they are not browser visual acceptance. No private source text or credits
belong in public fixtures, logs or conversation output.
