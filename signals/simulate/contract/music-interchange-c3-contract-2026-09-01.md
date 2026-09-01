---
skill: simulate-contract
topic: music-interchange-c3
date: 2026-09-01
gate_result: PASS
---

# Music interchange C3 contract verification

## Inputs

- Contract: `docs/music-interchange-intake-v0.3.2.md` and
  `reel.music-interchange-intake.v0.1`.
- Implementation: `crates/reel-music/src/interchange.rs`,
  `music-interchange-validate`, fixtures, and C3 tests.
- External grounding: `signals/discover/websearch/music-interchange-c3-websearch-2026-09-01.md`.

## Schema sweep

| # | Contract element | Actual evidence | Result |
|---|---|---|---|
| 1 | Intake uses a strict versioned schema | Unknown fields are denied and the schema is fixed to v0.1 | PASS |
| 2 | Intake binds immutable source bytes and contract | Source manifest, canonical contract, and decoded PCM hashes are revalidated | PASS |
| 3 | Intake retains scoped authority | Authority namespace, artifact, content hash, status, roles, and decisions use the shared authority validator | PASS |
| 4 | Producer identity is exact | Adapter, version, and executable SHA-256 are required | PASS |
| 5 | Producer parameters are exact | Parameter SHA-256 is required | PASS |
| 6 | Model identity is coherent | Revision, exact hash, and license are an all-or-none group | PASS |
| 7 | Software and dataset disclosures are explicit | Software license and non-empty dataset disclosure are required | PASS |
| 8 | External execution remains local | Every producer and PCM decoder requires denied network policy | PASS |
| 9 | Artifact identity is exact | Each artifact binds path, SHA-256, and non-zero byte count | PASS |
| 10 | Purpose is separate from format | Typed purpose/format combinations reject incompatible declarations | PASS |
| 11 | Existing audio outputs are recognized | WAV and FLAC signatures support stem and sonification purposes | PASS |
| 12 | Existing event/score outputs are recognized | MIDI, MusicXML, CSV, lab, JAMS, RDF, and NPZ shapes are checked | PASS |
| 13 | Semantic interpretation stays provisional | Every artifact requires roles plus explicit uncertainty | PASS |
| 14 | Container stems require normalization | Stem purpose cannot validate without a normalized raw-PCM binding | PASS |
| 15 | Normalized PCM is exact | Decoder/version/parameters, timebase, raw format, hashes, and byte length are checked | PASS |
| 16 | Normalized stems align to the source | PCM timebase must equal the immutable source timebase | PASS |
| 17 | Review and approval remain distinct | Required reconstruction, sound, and provenance roles are present; approval-like states require decisions | PASS |
| 18 | Private identity is not projected as shareable | The validation report is always marked `shareable: false` | PASS |
| 19 | Tampering is rejected | Tests change external bytes and false-declare CSV as MIDI | PASS |
| 20 | Third-party execution is absent | Validation only reads, parses, hashes, and compares local files | PASS |

Schema rows complete. `SCHEMA-DIFF-COMPLETE`.

## Element diff

No mismatch was found. The implementation intentionally stops before semantic
CSV/JAMS namespace conversion, decoder execution, analyzer execution, quality
judgment, or analysis-model promotion, matching the documented C3 boundary.

## Gate token

- census-distribution: music-interchange-c3/implementation
- gate-provenance: §S5.5-Sub-task-A
- mechanism-distribution: music-interchange-c3/implementation
- mechanism-type-shared: PASS
- gate-status: PASS
- attestation-by: Music Reconstruction Engineer contract lens (simulated)
- attestation-result: Existing-tool artifacts are strictly bound without being promoted to musical truth.
- verification-by: Independent fixture, mutation, normalization, and CLI tests
- verification-result: Format identity, source lineage, PCM binding, privacy state, and tamper rejection passed.

This simulated gate does not represent human review or approval.
