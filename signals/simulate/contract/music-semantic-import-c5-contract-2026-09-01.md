---
skill: simulate-contract
topic: music-semantic-import-c5
date: 2026-09-01
gate_result: PASS
---

# Music semantic import C5 contract verification

## Inputs

- Contract: `docs/music-semantic-import-v0.3.4.md` and
  `reel.music-semantic-import.v0.1`.
- Implementation: `crates/reel-music/src/semantic_import.rs`, additive analysis
  import lineage, two CLI commands, fixtures, and C5 tests.
- Upstream contracts: interchange intake and evidence comparison v0.1.

## Schema sweep

| # | Contract element | Actual evidence | Result |
|---|---|---|---|
| 1 | Strict versioned import schema | Unknown fields are denied and schema is fixed to v0.1 | PASS |
| 2 | Exact intake bytes | Intake manifest SHA-256 is recomputed | PASS |
| 3 | Exact intake semantics | Intake canonical contract is fully revalidated | PASS |
| 4 | Exact comparison bytes | Comparison manifest SHA-256 is recomputed | PASS |
| 5 | Exact comparison semantics | Comparison canonical contract and intake lineage are revalidated | PASS |
| 6 | Explicit selected candidate | Import set and artifact must match the comparison's decision-bound selection | PASS |
| 7 | Purpose-scoped bridge | Only event, annotation, and score candidates are admitted | PASS |
| 8 | Semantic mapping authority | Namespace, content identity, roles, status, and decisions are explicit | PASS |
| 9 | Adapter identity | ID, name, version, executable hash, parameters hash, model revision, and license are required | PASS |
| 10 | Local-only adapter boundary | Adapter network policy must be denied; REEL executes no adapter | PASS |
| 11 | Native locator retained | Every event keeps a non-empty upstream source locator | PASS |
| 12 | Typed native time | Samples, integer microseconds, and musical ticks are distinct strict variants | PASS |
| 13 | Integer conversion | Checked rational arithmetic uses no floating-point seconds | PASS |
| 14 | Declared rounding | Immutable source rounding policy controls conversion | PASS |
| 15 | Exact source range | Recomputed range must equal mapped source samples and fit the source | PASS |
| 16 | Typed observation value | Existing analysis value validators constrain tempo, meter, pitch, text hashes, and labels | PASS |
| 17 | Bounded confidence | Confidence rejects values above 1,000,000 | PASS |
| 18 | Limits stay visible | At least one unique limitation is required | PASS |
| 19 | Review routing complete | Reconstruction, sound, editor, and provenance roles are mandatory | PASS |
| 20 | Atomic output | Existing output is rejected and temporary analysis validates before no-clobber persistence | PASS |
| 20a | Portable output paths | Cross-root output is rejected rather than serializing machine-specific absolute paths | PASS |
| 21 | Import binding retained | Generated analysis binds import raw and canonical hashes plus import ID | PASS |
| 22 | Analyzer lineage retained | Generated analyzer declares the semantic import ID | PASS |
| 23 | Event lineage retained | Every generated observation binds one import event | PASS |
| 24 | Exact mirror enforced | Sample range, confidence, uncertainty, and value must match the import event | PASS |
| 25 | Full event census | Every import event must occur exactly once in analysis | PASS |
| 26 | Backward compatibility | Empty optional import fields are omitted from legacy canonical serialization | PASS |
| 27 | Private evidence | Validation and write reports are always `shareable: false` | PASS |
| 28 | Tamper rejection | Tests cover time, selection, comparison, import, and generated-observation tampering | PASS |

Schema rows complete. `SCHEMA-DIFF-COMPLETE`.

## Element diff

No contract mismatch was found. Native tool parsing, stem semantics, automatic
model authoring, correction, listening approval, translation, arrangement, and
release remain outside C5.

## Gate token

- census-distribution: music-semantic-import-c5/implementation
- gate-provenance: §S5.5-Sub-task-A
- mechanism-distribution: music-semantic-import-c5/implementation
- mechanism-type-shared: PASS
- gate-status: PASS
- attestation-by: Music Reconstruction Engineer contract lens (simulated)
- attestation-result: Selected normalized events enter analysis without losing native locator, exact time mapping, or import lineage.
- verification-by: Independent fixture, conversion, atomic-write, compatibility, and tamper tests
- verification-result: All declared lineage and mapping invariants passed.

This simulated gate does not represent human review or approval.
