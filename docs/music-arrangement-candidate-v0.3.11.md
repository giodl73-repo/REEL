# Governed arrangement candidates (v0.3.11)

`reel.music-arrangement-candidate.v0.1` materializes a validated C11
arrangement plan as one exact, inspectable score-and-audio candidate.

Run:

```text
reel music-arrangement-candidate-check candidate.yaml --output json
```

The validator recursively checks the bound arrangement plan and arranged music
model. The v0.1 candidate score must preserve every non-note model layer and
materialize every planned note exactly once in the instrument part named by its
mapping. This deliberately keeps the first candidate contract narrow: timbre
and part recasting may change, while form, pulse, melody/harmony declarations,
hooks, lyrics, and expressive timing cannot drift silently.

The score export binding is rechecked against the exact arranged-model
contract. MIDI and MusicXML must independently round-trip, and the candidate's
audible artifact must be byte-identical to the score export's deterministic
rehearsal guide. That guide is evidence that the score can be heard; it is not
a performance master, orchestration approval, or release asset.

The blind audible comparison requires form, pulse, melody, harmony, hooks,
emotional arc, instrumentation, and mix-balance lenses. Listening,
recognition, and candidate selection/rejection are independent human gates:

- pending gates forbid decisions;
- completed gates require immutable decision references;
- recognition requires completed listening, and `recognized` requires a
  passed listening result;
- selection requires passed listening and human recognition; and
- rejection requires completed listening or recognition evidence.

The authority status must remain `candidate`, `selected`, or `rejected` in
lockstep with the selection gate. Required review roles cover reconstruction,
score/arrangement, sound, editing, rights/provenance, and platform/audience.

The validator performs no rendering, network access, listening, recognition,
selection, upload, or publication. Its report is private (`shareable: false`).
The synthetic fixture contains no consumer lyrics, audio, titles, identities,
paths, or creative judgments.
