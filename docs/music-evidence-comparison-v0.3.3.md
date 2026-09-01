# Music evidence comparison and review queue (v0.3.3)

REEL v0.3.3 adds `reel.music-evidence-comparison.v0.1` and the
`music-evidence-compare` command. The contract compares competing artifacts
already admitted by `reel.music-interchange-intake.v0.1`; it does not run an
analyzer, parse tool-specific semantics, calculate a winner, or alter an
upstream artifact.

## Boundary

Every comparison binds both the exact YAML bytes and canonical contract hash of
one validated interchange intake. Each comparison set declares one artifact
purpose and references at least two distinct intake artifacts with that same
purpose. This prevents a sonification, stem, score, or annotation from being
silently treated as an equivalent candidate.

Candidate measurements are externally supplied evidence. Millionth-scale
coverage, confidence, bleed, and mixture-consistency values are range checked;
alignment error and event count remain integer observations. Missing values
remain `null`. REEL records these facts but deliberately computes no aggregate
score or ordering.

Each set also requires at least one explicit finding. A finding names the
candidate artifacts it compares, a typed dimension, an `agrees`, `disagrees`,
or `inconclusive` outcome, and a human-readable detail. An optional evidence
hash can bind a separately retained measurement or inspection artifact.

## Corrections and selection

A correction request identifies its candidate, target, category, and exact
requested inspection or change. It remains in the generated queue until a
separate immutable decision artifact is referenced. Corrections never mutate
the imported bytes.

Selection is optional. While it is absent, the queue contains a selection item
and every candidate remains coequal evidence. A selection must reference a
candidate in the set and a separate immutable human decision. REEL rejects
selection of a candidate that still has an open correction. It does not infer
selection from measurements, manifest order, simulated role review, or a
general review status.

The report is deterministic and always `shareable: false` because it retains
private source lineage, local artifact hashes, assessments, findings, and review
reasons.

## Example

```powershell
cargo run -- music-evidence-compare `
  manifests/fixtures/music-interchange-intake/comparison.yaml `
  --output json
```

The synthetic fixture compares two CSV note-event candidates, records one pitch
disagreement, and emits one correction plus one selection task. It contains no
consumer audio, lyrics, model output, or third-party-generated evidence.

## Guarantees

- stale intake byte or canonical hashes fail;
- unknown, duplicated, or wrong-purpose candidates fail;
- findings must compare at least two candidates in their own set;
- millionth measurements cannot exceed 1,000,000;
- resolved corrections and selections require immutable decision references;
- a selected candidate cannot retain an open correction; and
- validation performs no network access, analyzer execution, semantic import,
  artifact mutation, approval inference, or publication.

Human listening and source comparison remain required before a candidate can
shape the corrected editable model.
