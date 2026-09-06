# Sung-lyric alignment V1 core

`sung-lyric-align` consumes a path-independent JSON request and writes a
deterministic candidate document. Evidence adapters remain external: known-text
aligners, CTC, phone/vowel extraction, acoustic analysis, score parsing, and
human corrections contribute timestamped observations with immutable IDs.

The V1 core keeps canonical syllables, score events, and performed events
separate; supports repeated occurrences that point to the same canonical
indices; preserves ranked local alternatives and provenance; honors locked
human anchors; reports omissions; and rejects malformed hashes, clocks,
non-monotonic evidence, unlinked lyric events, and lyric identity assigned to
confirmed silence.

This checkpoint is deliberately not a model runner or interactive editor.
Adapters, phrase-bounded re-solve/undo, and the review UI remain later slices.
Machine output is always `machine-candidate-human-listening-required` and grants
no lyric, score, creative, consent, or publication approval.
