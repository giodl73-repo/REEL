# Offline instrumental source studies

`tools/local_instrumental_score.py` projects explicitly bound, strictly validated
multi-instrument note events into the existing native MusicSource, Analysis and
MusicModel contracts. It uses REEL's existing MIDI/MusicXML/SVG/guide exporter and
independent round-trip check. It introduces no second notation or scene grammar.

The request fixes the complete recording, measured-note asset, native executable,
FFmpeg identity, provisional pulse/meter and optional print-lead instrument.
The adapter preserves every measured note identity, pitch and sounding interval,
audits rounding within one millisecond, and retains a per-note source ledger.
It rejects invalid captures, duplicate identities, absent requested instruments,
out-of-range spans and notes that collapse at the declared score clock.

An instrument chosen for the printable projection is a producer proposal, not
proof of the actual lead. Other captured parts remain in the ensemble MIDI and
MusicXML. No missing durations, harmony, lyrics or confidence scores are invented.
The score pulse is explicitly provisional and does not retime the recording.
Native structural/clock checks do not approve melody completeness, instrument
labels, musical form, readability, arrangement treatment or publication.

The real private fixture retained by the consuming project exported a complete
90-second source study with 1,160 measured events in five parts, preserving every
captured pitch and interval within the stated clock tolerance. Its sparse lead
projection remains unreviewed. Private requests, audio and raw results stay in
that project's cache/LFS rather than this repository.

Nine synthetic projection tests plus the existing nine measured-event and five
source-layer tests cover invalid spans, false lead selection, lost/transposed
notes, retiming, polyphony and deterministic source preservation. No Rust core
code changes are needed. Rendering all eight arrangement styles and producing
readable reviewed books remain downstream qualification work.
