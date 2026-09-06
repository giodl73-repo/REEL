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

Evidence adapters may now be imported as independently hash-pinned observation
streams with explicit clock transforms. Observations augment stable performed
event identities with phones, vowel nuclei, normalized tokens, candidates and
per-source confidence; an observation outside the event's recording-time
neighborhood fails closed.

The core uses a deterministic beam dynamic program to recommend and rank three
complete-song canonical paths. Its transition model favors canonical advance,
permits sustained identity, omissions and bounded backward repeat loops, and
uses later forward evidence to resynchronize. Score events provide written
identity, not a globally trusted media clock: callers should collapse ties and
provide phrase-bounded piecewise clock evidence when notation tempo, meter or
performed timing diverge.

Every supported backward repeat opens a new score-time segment. When the path
returns beyond the completed canonical span, the repeat's recording elapsed
time is added to a cumulative offset. All later recording-to-score timing
scores use the baseline plus every completed repeat offset; a later repeat adds
again rather than replacing the earlier correction. Ranked alternatives retain
their own segment history. Anchor-bounded re-solves rebuild segment offsets
inside the solved path while leaving assignments outside the scope frozen.

Evidence streams remain separate in the result. Acoustic activity establishes
sound, independent ASR/CTC establishes heard token identity, forced-alignment
phones may refine boundaries only inside an independently supported phrase,
canonical syllables establish text identity, and score events establish written
note identity. Evidence from these roles is never collapsed into a purported
single observation or double-counted as independent support.

The solver discovers immutable anchor islands from high-confidence, unique
two- or three-word independent-ASR n-grams, including reliable opening and
closing phrases. It seeds islands throughout the recording, retains repeated
matches as distinct performed occurrences, and globally solves forward and
backward gaps between islands. Non-unique canonical phrases are emitted as
ambiguities instead of anchors. Transcript-forced phones and accompaniment
activity cannot create an anchor.

An optional resolve scope supports interactive correction. The event before
and after a partial scope must be immutable human anchors, assignments outside
the scope are frozen, and only the bounded interior is globally re-solved.
Corrections and undo history remain the responsibility of the host review UI;
the solver consumes the resulting immutable anchor set.

This checkpoint is deliberately not a model runner or browser editor.
Machine output is always `machine-candidate-human-listening-required` and grants
no lyric, score, creative, consent, or publication approval.
