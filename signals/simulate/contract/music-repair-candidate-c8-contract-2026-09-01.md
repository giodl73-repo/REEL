---
skill: simulate-contract
topic: music-repair-candidate-c8
date: 2026-09-01
gate_result: PASS
---

# Music repair candidate C8 contract verification

## Inputs

- Contract: `docs/music-repair-candidate-v0.3.7.md` and
  `reel.music-repair-candidate.v0.1`.
- Implementation: `crates/reel-music/src/repair_candidate.rs`, evidence loader,
  root CLI, generated synthetic fixtures, and C8 tests.
- Upstream contracts: C7 repair intent, repair, EDL, evidence, model draft,
  corrected model, analyses, and source v0.1.

## Schema sweep

| # | Contract element | Actual evidence | Result |
|---|---|---|---|
| 1 | Strict versioned schema | Unknown fields are denied and schema is fixed to v0.1 | PASS |
| 2 | Stable candidate identity | Non-empty `candidate_id` is required | PASS |
| 3 | Exact intent bytes | Intent manifest SHA-256 is recomputed | PASS |
| 4 | Exact intent semantics | Intent canonical hash and ID must match | PASS |
| 5 | Recursive intent validation | C7 model, source, repair, operation, decision, and candidate-gate invariants rerun | PASS |
| 6 | Exact candidate bytes | Candidate PCM SHA-256 is recomputed | PASS |
| 7 | Exact evidence bytes | Evidence manifest SHA-256 is recomputed | PASS |
| 8 | Exact evidence semantics | Evidence canonical contract SHA-256 must match | PASS |
| 9 | Intent repair equality | Evidence repair bytes, contract, and ID must equal the C7 repair | PASS |
| 10 | EDL reconstruction | Current EDL is rebuilt from the bound repair and compared exactly | PASS |
| 11 | Evidence reconstruction | Technical metrics are recomputed from current EDL, repair, and candidate | PASS |
| 12 | Adapter lineage | Saved adapter and version participate in exact evidence equality | PASS |
| 13 | Candidate/evidence equality | Evidence candidate SHA-256 must equal the candidate binding | PASS |
| 14 | Outside-region evidence | Segment identity is recomputed by the evidence layer | PASS |
| 15 | Boundary evidence | Join metrics and violations are recomputed | PASS |
| 16 | Right-tail evidence | Exact right-tail identity and minimum length are recomputed | PASS |
| 17 | Duration evidence | Candidate byte count must equal the EDL output duration | PASS |
| 18 | Listening state vocabulary | Pending, passed, and failed are closed typed states | PASS |
| 19 | Listening decision rule | Pending forbids a decision; passed/failed require one | PASS |
| 20 | Selection state vocabulary | Pending, selected, and rejected are closed typed states | PASS |
| 21 | Selection decision rule | Pending forbids a decision; selected/rejected require one | PASS |
| 22 | No technical auto-selection | Technical pass alone never changes selection state | PASS |
| 23 | Selection eligibility | Selected requires technical pass and passed listening | PASS |
| 24 | Explicit rejection | Rejected requires completed listening and a separate decision | PASS |
| 25 | Failed candidate retention | Failed evidence can validate only as pending or explicitly rejected, never selected | PASS |
| 26 | Scoped authority | Candidate authority namespace, identity, roles, status, and decisions validate | PASS |
| 27 | Complete review routing | Reconstruction, sound, editor, and provenance roles are mandatory | PASS |
| 28 | Approval separation | Approval-like review status requires separate decisions | PASS |
| 29 | Private report | Candidate validation reports remain `shareable: false` | PASS |
| 30 | Tamper rejection | Tests cover PCM, evidence hash, listening shortcut, and failed-candidate selection tampering | PASS |
| 31 | Cross-platform proof | Generated fixtures avoid checked-in absolute EDL paths and run from temporary roots | PASS |

Schema rows complete. `SCHEMA-DIFF-COMPLETE`.

## Element diff

No contract mismatch was found. Rendering, listening itself, creative
preference, delivery, and release remain external. The contract verifies exact
records of those gates without performing or inferring them.

## Gate token

- census-distribution: music-repair-candidate-c8/implementation
- gate-provenance: §S5.5-Sub-task-A
- mechanism-distribution: music-repair-candidate-c8/implementation
- mechanism-type-shared: PASS
- gate-status: PASS
- attestation-by: Music Reconstruction Engineer contract lens (simulated)
- attestation-result: Exact candidate evidence remains recursively bound to C7 intent and cannot self-select.
- verification-by: Independent selected, rejected, recursive-lineage, state-transition, tamper, and CLI tests
- verification-result: All candidate evidence, listening, rejection, and selection invariants passed.

This simulated gate does not represent human review or approval.
