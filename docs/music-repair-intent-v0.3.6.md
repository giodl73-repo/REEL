# Model-bound repair intent in CLI v0.3.6

REEL v0.3.6 adds `reel.music-repair-intent.v0.1` and
`music-repair-intent-check`. The contract connects a governed editable song
model to a sample-exact repair plan without allowing musical intent to widen
the acoustic edit envelope.

## Exact bindings

The manifest binds both the model draft and repair by file SHA-256, canonical
contract SHA-256, and stable identity. REEL validates both dependency graphs
and then requires their source-manifest, source-contract, and decoded-PCM
identities to match. A correct model for one recording cannot authorize an edit
to another recording.

## Complete repair intent

Every mutating repair operation must occur in exactly one intent. Each intent
names one or more stable model targets, a repair objective, a rationale, and an
immutable human decision. Unknown or non-mutating operations, unknown model
targets, duplicate links, and incomplete operation coverage fail. `keep` and
`lock` operations remain boundaries rather than creative changes.

This relationship is deliberately asymmetric: model targets explain the
musical purpose, while `reel.music-repair.v0.1` remains authoritative for exact
sample ranges. The link cannot expand a changed envelope, trespass a lock, or
weaken complete source coverage.

## Candidate gate

Every intent manifest must declare each gate exactly once:

- exact identity outside changed regions;
- boundary continuity;
- right-tail identity;
- output duration;
- human listening; and
- human selection.

The first four are technical evidence produced by the existing EDL/evidence
pipeline. Listening and selection stay separate human acts. A technical pass
does not select a candidate, and neither selection nor review authorizes
release.

## Usage

```powershell
cargo run -- music-repair-intent-check `
  manifests/fixtures/music-repair-intent/intent.yaml `
  --output json
```

The fixture uses only synthetic PCM and existing synthetic model evidence. Its
single cut is decision-bound to a governed form target and preserves all prior
sample-exact locks and tail protections.

## Boundary

The validator does not invent repairs, judge whether a musical diagnosis is
correct, render audio, listen, select a candidate, approve a performance, or
authorize delivery or publication. Reports remain `shareable: false` because
they retain source and decision lineage.
