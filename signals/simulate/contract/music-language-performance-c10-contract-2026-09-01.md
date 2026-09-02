---
skill: simulate-contract
topic: music-language-performance-c10
date: 2026-09-01
gate_result: PASS
---

# Music language performance C10 contract verification

## Inputs

- Contract: `docs/music-language-performance-v0.3.9.md` and
  `reel.music-language-performance.v0.1`.
- Implementation: `crates/reel-music/src/language_performance.rs`, root CLI,
  shared C9 fixture builder, generated raw-PCM takes, and C10 tests.
- Upstream contract: C9 adaptation, C6 model draft/model/analysis, and source.

## Schema sweep

| # | Contract element | Actual evidence | Result |
|---|---|---|---|
| 1 | Strict versioned schema | Unknown fields denied; schema fixed to v0.1 | PASS |
| 2 | Stable performance identity | Non-empty performance ID required | PASS |
| 3 | Exact adaptation bytes | Adaptation manifest SHA-256 recomputed | PASS |
| 4 | Exact adaptation semantics | Canonical contract and identity must match | PASS |
| 5 | Recursive adaptation validation | C9 text, links, model, accompaniment, underlay, prosody, and upstream chain rerun | PASS |
| 6 | Exact vocal bytes | File and decoded raw-PCM hashes independently checked | PASS |
| 7 | Exact vocal format | Format, channels, and sample rate equal accompaniment | PASS |
| 8 | Exact vocal duration | Sample count and byte count equal accompaniment | PASS |
| 9 | Separate performed text | As-performed text has independent path, hash, language, and authority | PASS |
| 10 | Exact performed bytes | UTF-8 file SHA-256 recomputed | PASS |
| 11 | Language equality | Performed language equals approved target language | PASS |
| 12 | Complete performed units | Every non-whitespace UTF-8 character covered once in order | PASS |
| 13 | Complete target audit | Every approved target unit has one ordered disposition | PASS |
| 14 | Complete performed audit | Every performed unit is consumed once in order | PASS |
| 15 | Exact match semantics | Matched requires one byte-equal unit and forbids a decision | PASS |
| 16 | Changed semantics | Changed requires performed text and decision | PASS |
| 17 | Omitted semantics | Omitted forbids performed text and requires decision | PASS |
| 18 | Uncertain semantics | Uncertain remains explicit and decision-backed | PASS |
| 19 | Separate lyric listening | Pending/passed/failed state has exact decision rules | PASS |
| 20 | Typed creation method | Human, synthetic, and non-identifiable fixture methods are closed | PASS |
| 21 | Adapter provenance | Creating adapter and version are mandatory | PASS |
| 22 | Synthetic model provenance | Synthetic voice requires checkpoint hash and license | PASS |
| 23 | Seed provenance | Present seed cannot be empty | PASS |
| 24 | Creation egress | External creation requires an independent approval decision | PASS |
| 25 | Consent scope | Subject, operation, runtime, audience, retention, and reuse are mandatory | PASS |
| 26 | Consent state decisions | Pending forbids decisions; completed states require them | PASS |
| 27 | No consent waiver | Human/synthetic voices cannot use fixture-only exemption | PASS |
| 28 | Exact source-reference bytes | Source file/decoded hashes and byte count checked | PASS |
| 29 | Source-reference authority | Source comparison media has separately validated authority | PASS |
| 30 | Same comparison duration | Source reference equals accompaniment timebase and format | PASS |
| 31 | Same comparison model | Source reference declares exact recursively checked model contract | PASS |
| 32 | Correct languages | Source/target comparison languages equal distinct adaptation languages | PASS |
| 33 | Distinct comparison labels | Source and target labels cannot collide | PASS |
| 34 | Complete listening rubric | Lyrics, prosody, recognition, accompaniment, and mix lenses required exactly once | PASS |
| 35 | Separate comparison listening | Pending/passed/failed state has exact decision rules | PASS |
| 36 | No automatic selection | Technical validation never changes selection state | PASS |
| 37 | Selection eligibility | Selection requires both listening passes and satisfied consent | PASS |
| 38 | Explicit rejection | Rejection requires completed listening or denied consent | PASS |
| 39 | Authority/state agreement | Authority status exactly matches candidate/selected/rejected state | PASS |
| 40 | Complete role routing | Seven reconstruction, music, lyric, sound, editor, provenance, and audience roles mandatory | PASS |
| 41 | Approval separation | Approval-like review state requires independent decisions | PASS |
| 42 | Private report | Output remains path-free and `shareable: false` | PASS |
| 43 | Fixture privacy | Only invented text and temporary constant-value tones used | PASS |
| 44 | Tamper rejection | Audio, text, lineage, audit, provenance, consent, comparison, selection, and roles exercised | PASS |
| 45 | Execution boundary | Validator performs no generation, transcription, listening, upload, delivery, or release | PASS |

Schema rows complete. `SCHEMA-DIFF-COMPLETE`.

## Element diff

No mismatch was found. The contract verifies exact evidence records and gate
transitions; actual lyric judgment, musical recognition, consent, selection,
delivery, publication, and release remain human/project responsibilities.

## Gate token

- census-distribution: music-language-performance-c10/implementation
- gate-provenance: §S5.5-Sub-task-A
- mechanism-distribution: music-language-performance-c10/implementation
- mechanism-type-shared: PASS
- gate-status: PASS
- attestation-by: Lyrics/Vocal Adaptation Editor contract lens (simulated)
- attestation-result: Exact performed text and target-unit dispositions remain separate from the approved translation and audible lyric judgment.
- verification-by: Independent valid, rejected, audio/text/lineage, audit, provenance, consent, comparison, selection, role, and CLI tests
- verification-result: All declared C10 invariants passed and every exercised shortcut or mutation was rejected.

This simulated gate does not represent human review or approval.
