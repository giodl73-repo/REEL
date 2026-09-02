---
skill: simulate-contract
topic: music-arrangement-candidate-c12
date: 2026-09-01
gate_result: PASS
---

# C12 arrangement-candidate contract simulation

## Inputs

- Contract: `docs/music-arrangement-candidate-v0.3.11.md`
- Implementation: `src/arrangement_candidate.rs`, CLI wiring, and
  `tests/music_arrangement_candidate_cli_v0311.rs`
- Depth: standard

## Schema sweep

| # | Contract element | Implementation evidence | Result |
|---:|---|---|---|
| 1 | Exact C11 manifest hash | `ArrangementBinding.manifest_sha256` and file rehash | PASS |
| 2 | Exact C11 contract and identity | recursive plan validation plus arrangement ID comparison | PASS |
| 3 | Exact arranged-model hash | `ModelBinding.manifest_sha256` and file rehash | PASS |
| 4 | Exact arranged-model contract and identity | recursive model validation plus model ID comparison | PASS |
| 5 | Source lineage inheritance | source and analysis identity fields compared | PASS |
| 6 | Non-note inheritance | timebase, duration, tempo, meter, form, harmony, rhythm, hooks, lyrics, expression, unknowns compared | PASS |
| 7 | Declared non-note scope | v0.1 rejects non-preserve non-note dispositions | PASS |
| 8 | Exact instrument parts | actual parts equal instruments used by mapped notes | PASS |
| 9 | Exact mapped-note coverage | all mapping IDs materialized exactly once | PASS |
| 10 | Exact mapped-note values | part, role, voice, tick, duration, pitch, and velocity compared | PASS |
| 11 | Score-plan binding | plan file and canonical contract hashes checked | PASS |
| 12 | Score-receipt binding | receipt bytes and arranged-model contract checked | PASS |
| 13 | MIDI round trip | existing independent score checker required to pass | PASS |
| 14 | MusicXML round trip | existing independent score checker required to pass | PASS |
| 15 | Audible round trip | candidate audio hash must equal receipt rehearsal-guide hash | PASS |
| 16 | Creation provenance | adapter ID/version must equal audible export adapter | PASS |
| 17 | Network/egress boundary | local forbids decision; external requires decision | PASS |
| 18 | Source comparison binding | source reference and authority hashes checked | PASS |
| 19 | Blind comparison | distinct labels required | PASS |
| 20 | Complete listening lenses | eight required dimensions, unique and complete | PASS |
| 21 | Listening decision semantics | pending forbids, complete requires decision | PASS |
| 22 | Recognition decision semantics | pending forbids, complete requires decision | PASS |
| 23 | Recognition ordering | recognition requires completed listening | PASS |
| 24 | Positive recognition | `recognized` requires passed listening | PASS |
| 25 | Selection | selected requires passed listening and recognition | PASS |
| 26 | Rejection | rejected requires completed listening or recognition | PASS |
| 27 | Authority state | candidate/selected/rejected matches selection | PASS |
| 28 | Required review panel | six required role slugs checked | PASS |
| 29 | Privacy | report is always `shareable: false` | PASS |
| 30 | Non-execution | check path contains no renderer, listener, uploader, or network call | PASS |
| 31 | Synthetic positive fixture | CLI proves four notes, one part, three round trips | PASS |
| 32 | Tamper diagnostics | plan, model, score, audio, gates, and review shortcuts rejected | PASS |

SCHEMA-DIFF-COMPLETE

## Gate token

- census-distribution: 32/32 contract elements present and passing
- gate-provenance: §S5.5-Sub-task-A
- mechanism-distribution: 32/32 contract elements present and passing
- mechanism-type-shared: recursive-hash-validation-plus-independent-round-trip
- gate-status: PASS
- attestation-by: C12 contract census owner
- attestation-result: all declared contract elements were located in implementation or tests
- verification-by: independent schema-sweep witness
- verification-result: no omitted GateTokenSchema row and no blocking mismatch

## Mismatch findings

The standard-depth sweep produced thirty-two findings. All are satisfied. The
one deliberate limitation is explicit rather than a mismatch: v0.1 candidate
scores preserve non-note model layers and use the deterministic rehearsal guide
as the audible round trip. A later contract may govern expressive non-note
transformation and performance-master evidence without weakening this gate.

Verdict: **GO** for synthetic C12 integration. This is not creative approval,
human recognition, candidate selection, or release authorization.
