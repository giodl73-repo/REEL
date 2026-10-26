# Private songbook collection exports

All adapters run as hash-pinned generic Python steps admitted by Rust run-job.
They never select or replace recordings or grant musical approval. Retain their
requests, runtime file hashes, outputs and diagnostics in the project cache.

- `local_songbook_collection.py`: consumes `collection.json` with unique bounded
  entries, immutable source locks, bilingual reading pairs and checked existing
  score PDF descriptors. Produces individual reading-and-score PDFs, a combined
  book and responsive HTML. Every appended source score page must render with
  identical pixels. Combined-page bounds, nonblank renders and QA images remain
  separate from human visual and musical review.
- `local_songbook_native_pdf.py`: consumes `review-request.json`; reuses the
  existing `local_songbook_review.py` presentation of native REEL SVG systems.
  Native score geometry remains unchanged. The optional review `heading` lets a
  collection identify its actual scope. Retains rendered-page QA in a ZIP.
- `local_songbook_legacy_exports.py`: checked score-application PDF, MusicXML
  round trip and editable MIDI for existing measured notation. Pitches and exact
  nominal intervals must survive the XML round trip; MIDI articulation changes
  are reported explicitly. Failed imports cannot be published as exact scores.
- `local_songbook_exact_midi.py`: exact rational nominal-quarter MIDI for retained
  measured MusicXML. Rejects clocks exceeding MIDI PPQ limits; checks exported
  pitch counts and exact note durations. Default MIDI tempo is explicitly
  provisional, never original-recording-clock evidence. This permits preserving
  an existing score and PDF when a score application's import changes timing.

Existing exact type/dot/tuplet duration spelling is preserved by the engraving
adapter. Newly spelled fragments discard stale tuplet and beam notation.
Recognition optionally accepts explicit float16 precision and RMS silence
cropping in memory, preserving full-source clock offsets and original timestamps.
No expected words prompt recognition; source-bound vocal estimates remain
separate from original mixes. A zero-additions pitch refinement exports the
unchanged study and a typed no-additions report instead of validating empty
analysis observations. Unknown notes remain unknown.
