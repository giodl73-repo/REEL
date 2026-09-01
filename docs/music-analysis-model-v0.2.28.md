# External analysis and corrected music model in CLI v0.2.28

REEL v0.2.28 implements Slice C1: immutable analyzer evidence and a separate,
human-reviewable corrected music model. It adds no analyzer, separator, model
download, notation exporter, or network execution.

```powershell
reel music-analysis-validate analysis.yaml --output json
reel music-model-validate model.yaml --output json
```

## External analysis evidence

`reel.music-analysis.v0.1` binds the immutable source manifest, canonical source
contract, and decoded PCM identity. Every analyzer records adapter, version,
model revision, parameter hash, license, and denied-network policy. Every
observation names that analyzer, an exact source sample range, integer-scaled
confidence, uncertainty disclosure, and one typed value: tempo, meter, beat,
bar, form, pitch, harmony, bass, rhythm, hook, instrumentation, or vocal token
alignment.

Optional raw-PCM stem evidence records exact bytes and decoded identity, source-
matching timebase, role, mixture-consistency estimate, bleed estimate, and
uncertainty. A stem is evidence from a separator; it is never described as an
original multitrack.

At least one explicit limitation is required. Reviewed/approved-like statuses
require immutable decision references. Validation performs no analysis and no
network or model operation.

## Corrected editable model

`reel.music-model.v0.1` binds the source plus one or more current analysis
contracts. It represents:

- integer PPQ, duration, tempo changes, and meter changes;
- contiguous form sections covering the whole model;
- editable parts and notes with voice, onset, duration, MIDI pitch, and
  velocity;
- harmony spans, rhythm cells, hooks, exact lyric layers, and expressive timing;
- explicit unknowns and model authority; and
- the required reconstruction, score, sound, edit, and provenance review panel.

Every musical event carries a rationale and provenance state. `observed` and
`inferred` events must cite valid immutable observations. `human-corrected`
events require an immutable correction reference and remain distinct from
overall review or approval. The model itself also carries a scoped authority
record. No event is silently promoted from analyzer output.

Vocal parts require at least one exact hash-bound canonical or as-sung lyric
layer with its own authority. This does not establish translation, voice-model
consent, performed-word fidelity, or permission to generate a vocal.

Expressive timing adjustments must reference known notes and remain positive
and within model duration. Tempo and meter start at tick zero, form coverage is
gap-free, IDs and evidence references are unique, and all bound source and
analysis hashes are revalidated on every check.

## Synthetic fixture and limits

`manifests/fixtures/music-model-corrected/` reuses the existing generated
unsigned PCM source. Its external-analysis fixture contains seven deliberately
limited observations. The editable model contains two form sections, four
melody notes, tonic harmony, a quarter-note rhythm cell, and one inferred hook.
One pitch is marked `human-corrected` using a clearly fixture-only correction
reference; this is a schema test, not a simulated approval.

The checked-in canonical hashes and generated failure tests prove stale
analysis rejection, unknown-evidence rejection, confidence bounds, correction-
reference requirements, exact lyric requirements for vocal parts, and immutable
source lineage on Windows and Linux.

MIDI, MusicXML, score round-trip comparison, and an audible rehearsal guide are
implemented by Slice C2 in REEL v0.3.1. Sample/tick interchange beyond the
current score adapters, language adaptation, lyric underlay, and arrangement
remain later work. No BERTICA audio, lyrics, titles, identity, paths, or
creative judgment appears in the fixture.
