---
skill: simulate-contract
topic: music-model-draft-c6
date: 2026-09-01
gate_result: PASS
---

# Music model draft C6 contract verification

## Inputs

- Contract: `docs/music-model-draft-v0.3.5.md` and
  `reel.music-model-draft.v0.1`.
- Implementation: `crates/reel-music/src/model_draft.rs`, CLI, fixture, and C6
  contract tests.
- Upstream contracts: analysis and corrected editable model v0.1.

## Schema sweep

| # | Contract element | Actual evidence | Result |
|---|---|---|---|
| 1 | Strict versioned schema | Unknown fields are denied and schema is fixed to v0.1 | PASS |
| 2 | Exact model bytes | Model manifest SHA-256 is recomputed | PASS |
| 3 | Exact model semantics | Model canonical contract and ID are revalidated | PASS |
| 4 | Full upstream validation | Model validation recursively revalidates source, analyses, imports, and corrections | PASS |
| 5 | Complete observation census | Every observation from every bound analysis is loaded | PASS |
| 6 | One disposition each | Disposition count and unique evidence keys must equal the census | PASS |
| 7 | No foreign observations | Unknown analysis or observation IDs fail | PASS |
| 8 | Explicit mapping | Mapped observations require one or more unique stable target refs | PASS |
| 9 | Stable target namespace | Tempo, meter, form, notes, harmony, rhythm, hooks, and expressive timing are named | PASS |
| 10 | Target existence | Every declared target must exist in the bound model | PASS |
| 11 | Evidence citation | Target provenance must cite the disposition observation | PASS |
| 12 | State equality | Declared observed/inferred/corrected state must equal model provenance | PASS |
| 13 | Correction equality | Human-corrected mapping and model must reference the same immutable decision | PASS |
| 14 | No false correction | Observed and inferred mappings reject correction references | PASS |
| 15 | Decision-bound omission | Omission requires rationale and immutable decision | PASS |
| 16 | Exact unknown preservation | Unknown text must occur verbatim in `model.unknowns` | PASS |
| 17 | Reverse citation check | Every model evidence citation requires a matching target mapping | PASS |
| 18 | No silent cherry-picking | Missing dispositions and undeclared model citations fail | PASS |
| 19 | Scoped authority | Draft authority namespace, content identity, roles, status, and decisions validate | PASS |
| 20 | Complete review routing | Reconstruction, arrangement, sound, editor, and provenance roles are mandatory | PASS |
| 21 | Approval separation | Approval-like review status requires separate decisions | PASS |
| 22 | Private evidence | Report is always `shareable: false` | PASS |
| 23 | Tamper rejection | Tests cover missing disposition, wrong correction, missing target mapping, absent unknown, and matched omission | PASS |

Schema rows complete. `SCHEMA-DIFF-COMPLETE`.

## Element diff

No contract mismatch was found. Model authoring, musical judgment, listening
approval, translation, arrangement, rendering, and release remain outside C6.

## Gate token

- census-distribution: music-model-draft-c6/implementation
- gate-provenance: §S5.5-Sub-task-A
- mechanism-distribution: music-model-draft-c6/implementation
- mechanism-type-shared: PASS
- gate-status: PASS
- attestation-by: Music Reconstruction Engineer contract lens (simulated)
- attestation-result: Every analysis observation has an explicit disposition and every model citation is declared.
- verification-by: Independent census, target, correction, omission, unknown, reverse-citation, and CLI tests
- verification-result: All observation-to-model governance invariants passed.

This simulated gate does not represent human review or approval.
