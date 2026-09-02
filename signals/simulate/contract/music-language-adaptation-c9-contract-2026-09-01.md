---
skill: simulate-contract
topic: music-language-adaptation-c9
date: 2026-09-01
gate_result: PASS
---

# Music language adaptation C9 contract verification

## Inputs

- Contract: `docs/music-language-adaptation-v0.3.8.md` and
  `reel.music-language-adaptation.v0.1`.
- Implementation: `crates/reel-music/src/language_adaptation.rs`, root CLI,
  synthetic text fixture, generated accompaniment, and C9 integration tests.
- Upstream contract: C6 model draft, corrected model, selected analyses, and
  immutable source v0.1.

## Schema sweep

| # | Contract element | Actual evidence | Result |
|---|---|---|---|
| 1 | Strict versioned schema | Unknown fields are denied and schema is fixed to v0.1 | PASS |
| 2 | Stable adaptation identity | Non-empty adaptation ID is required | PASS |
| 3 | Exact model-draft bytes | Draft SHA-256 is recomputed | PASS |
| 4 | Exact model semantics | Draft canonical contract and identity must match | PASS |
| 5 | Recursive model validation | C6 dispositions, citations, model, analyses, and source invariants rerun | PASS |
| 6 | Exact accompaniment bytes | File and decoded raw-PCM SHA-256 values are recomputed | PASS |
| 7 | Exact accompaniment format | Sample rate, channels, and sample format are checked | PASS |
| 8 | Exact accompaniment length | Byte count must equal physical bytes | PASS |
| 9 | Model-derived duration | Sample count must equal tempo-map duration with model rounding | PASS |
| 10 | Accompaniment source lineage | Source contract and derivation decision are mandatory | PASS |
| 11 | Distinct text layers | Exactly canonical-source and approved-target kinds are required | PASS |
| 12 | Exact text bytes | Each UTF-8 text file SHA-256 is recomputed | PASS |
| 13 | Distinct languages | Source and target language tags must differ | PASS |
| 14 | Scoped text authorities | Namespace, identity, role, status, and decisions validate | PASS |
| 15 | Target approval | Target authority status is exactly approved and decision-backed | PASS |
| 16 | Unit byte integrity | UTF-8 byte ranges are ordered, in bounds, and match declared text | PASS |
| 17 | Complete unit coverage | Every non-whitespace character belongs to exactly one unit | PASS |
| 18 | Complete source mapping | Translation links flatten to every source unit once and in order | PASS |
| 19 | Complete target mapping | Translation links flatten to every target unit once and in order | PASS |
| 20 | Translation rationale | Every alignment link has a non-empty rationale | PASS |
| 21 | Complete model inheritance | Preserved targets equal the full governed-model target set | PASS |
| 22 | Complete target underlay | Underlay covers every target unit once and in order | PASS |
| 23 | Governed note references | Underlay resolves only inherited melody/vocal notes | PASS |
| 24 | Monotonic musical time | Underlay note starts never move backward | PASS |
| 25 | Exact melisma accounting | Declared melisma count equals cited note count | PASS |
| 26 | Typed stress | Primary, secondary, and unstressed are closed typed values | PASS |
| 27 | Typed prosody divergence | Onset, duration, pitch, melisma, stress, rest, pickup, cadence, and phrase boundary are closed | PASS |
| 28 | Exception decisions | Every prosody exception binds a known translation link, its units/notes, rationale, known roles, and immutable decision | PASS |
| 29 | Unequal-link governance | Unequal source/target unit counts require an exception naming that exact link | PASS |
| 30 | Complete role routing | Reconstruction, arrangement, lyric, sound, editor, and provenance roles are mandatory | PASS |
| 31 | Approval separation | Approval-like review states require separate decisions | PASS |
| 32 | Private report | Validation reports remain `shareable: false` | PASS |
| 33 | Synthetic privacy | Fixtures contain invented text and test-generated PCM only | PASS |
| 34 | Tamper rejection | Tests cover text, translation, model, duration, underlay, exception, and approval tampering | PASS |
| 35 | Execution boundary | Validation performs no translation, synthesis, rendering, listening, upload, or release | PASS |

Schema rows complete. `SCHEMA-DIFF-COMPLETE`.

## Element diff

No contract mismatch was found. C9 proves an adaptation plan and its inherited
music/text lineage. Target-language performance, bilingual listening,
translation judgment, consent, selection, delivery, and release remain external.

## Gate token

- census-distribution: music-language-adaptation-c9/implementation
- gate-provenance: §S5.5-Sub-task-A
- mechanism-distribution: music-language-adaptation-c9/implementation
- mechanism-type-shared: PASS
- gate-status: PASS
- attestation-by: Lyrics/Vocal Adaptation Editor contract lens (simulated)
- attestation-result: Exact approved target text is completely linked and underlaid without mutating the governed source composition.
- verification-by: Independent valid-plan, text, translation, model-inheritance, duration, underlay, prosody, authority, and CLI tests
- verification-result: All same-music language-adaptation invariants passed and all exercised tampering was rejected.

This simulated gate does not represent human review or approval.
