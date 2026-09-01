---
skill: simulate-contract
topic: music-repair-link-c7
date: 2026-09-01
gate_result: PASS
---

# Music repair link C7 contract verification

## Inputs

- Contract: `docs/music-repair-intent-v0.3.6.md` and
  `reel.music-repair-intent.v0.1`.
- Implementation: `crates/reel-music/src/repair_intent.rs`, root CLI, synthetic
  fixture, crate contract tests, and CLI test.
- Upstream contracts: governed model draft, corrected model, source, and repair
  v0.1.

## Schema sweep

| # | Contract element | Actual evidence | Result |
|---|---|---|---|
| 1 | Strict versioned schema | Unknown fields are denied and schema is fixed to v0.1 | PASS |
| 2 | Stable intent identity | Non-empty `intent_id` is required | PASS |
| 3 | Exact draft bytes | Draft manifest SHA-256 is recomputed | PASS |
| 4 | Exact draft semantics | Draft canonical hash and ID must match | PASS |
| 5 | Recursive draft validation | Full observation census and model provenance are revalidated | PASS |
| 6 | Exact repair bytes | Repair manifest SHA-256 is recomputed | PASS |
| 7 | Exact repair semantics | Repair canonical hash and ID must match | PASS |
| 8 | Recursive repair validation | Changed envelopes, locks, coverage, assets, and operations are revalidated | PASS |
| 9 | Same source contract | Model and repair source contract SHA-256 must match | PASS |
| 10 | Same source manifest | Model and repair source manifest SHA-256 must match | PASS |
| 11 | Same decoded signal | Model and repair decoded-PCM SHA-256 must match | PASS |
| 12 | Complete mutating census | All operations except `keep` and `lock` are collected | PASS |
| 13 | Non-empty intent set | At least one repair intent is required | PASS |
| 14 | Stable intent IDs | Intent IDs are non-empty and unique | PASS |
| 15 | Explicit objective | Objective is a closed typed vocabulary | PASS |
| 16 | Explicit rationale | Every intent requires non-empty rationale | PASS |
| 17 | Immutable decision | Every intent requires artifact ID and SHA-256 | PASS |
| 18 | Operation existence | Unknown and non-mutating operation links fail | PASS |
| 19 | One intent per mutation | Duplicate operation linkage fails | PASS |
| 20 | Complete operation coverage | Every mutating operation must be linked exactly once | PASS |
| 21 | Model target existence | All stable target refs must occur in the governed model | PASS |
| 22 | Exact candidate-gate census | Six required checks must occur exactly once | PASS |
| 23 | Human/technical separation | Listening and selection are distinct from four technical checks | PASS |
| 24 | Role routing | Reconstruction, sound, editor, and provenance roles are mandatory | PASS |
| 25 | Approval separation | Approval-like review status requires immutable decisions | PASS |
| 26 | Private lineage | Report remains `shareable: false` | PASS |
| 27 | Tamper rejection | Tests cover target, operation coverage, candidate gate, binding, decision, and duplicate link failures | PASS |

Schema rows complete. `SCHEMA-DIFF-COMPLETE`.

## Element diff

No contract mismatch was found. Musical intent explains an already bounded
repair but cannot change its samples. Candidate generation, listening,
selection, performance approval, delivery, and release remain outside C7.

## Gate token

- census-distribution: music-repair-link-c7/implementation
- gate-provenance: §S5.5-Sub-task-A
- mechanism-distribution: music-repair-link-c7/implementation
- mechanism-type-shared: PASS
- gate-status: PASS
- attestation-by: Music Reconstruction Engineer contract lens (simulated)
- attestation-result: Every mutating operation is decision-bound to governed model evidence without widening the sample envelope.
- verification-by: Recursive binding, source-lineage, operation-census, target, candidate-gate, tamper, and CLI tests
- verification-result: All model-to-repair linkage and candidate-gate invariants passed.

This simulated gate does not represent human review or approval.
