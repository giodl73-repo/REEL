---
skill: simulate-contract
topic: music-score-export-c2
date: 2026-09-01
gate_result: PASS
---

# Music score export C2 contract verification

## Inputs

- Contract: `docs/music-score-export-v0.3.1.md` and
  `reel.music-score-export-plan.v0.1`.
- Implementation: `crates/reel-music/src/export.rs`, `src/music_score.rs`, the
  three `music-score-export-*` CLI commands, and
  `tests/music_score_export_cli_v031.rs`.
- Witness model: the generated, non-private
  `manifests/fixtures/music-model-corrected/model.yaml` fixture.

## Schema sweep

| # | Contract element | Actual evidence | Result |
|---|---|---|---|
| 1 | Plan has a strict versioned schema | `ScoreExportPlan` denies unknown fields and fixes `reel.music-score-export-plan.v0.1` | PASS |
| 2 | Plan binds exact model bytes | `model_manifest_sha256` is rebuilt from the validated model path | PASS |
| 3 | Plan binds canonical model meaning | `model_contract_sha256` uses canonical JSON hashing | PASS |
| 4 | Quantization is explicit | `exact-model-ticks`; expressive timing application is explicitly false | PASS |
| 5 | MIDI capability is bounded | PPQ must fit the SMF division; voices are limited to 1–16 | PASS |
| 6 | MusicXML capability is bounded | Notes may not overlap inside one part voice | PASS |
| 7 | Output publication is atomic | All artifacts and the receipt are validated in a sibling temporary directory before rename | PASS |
| 8 | Existing output is protected | Renderer refuses an existing packet directory; plan writer refuses overwrite | PASS |
| 9 | MIDI retains structure | Conductor/part tracks encode duration, tempo, meter, form, notes, voices, velocity, and lyric bindings | PASS |
| 10 | MusicXML retains structure | Score-partwise output encodes divisions, parts, timing movement, notes, directions, meter, and lyric bindings | PASS |
| 11 | Guide is deterministic | Integer tick/sample conversion, fixed millihertz table, and integer phase accumulation write fixed PCM bytes | PASS |
| 12 | Guide purpose is bounded | Plan and receipt call it rehearsal-only and not a performance master | PASS |
| 13 | MIDI is independently re-imported | The checker parses SMF chunks, VLQs, metadata, and note-on/off pairs from output bytes | PASS |
| 14 | MusicXML is independently re-imported | The checker parses actual pitch, duration, voice, timing, direction, meter, and binding elements | PASS |
| 15 | Tampering is rejected | Unit tests mutate MIDI pitch; the CLI test mutates MusicXML pitch and observes check failure | PASS |
| 16 | Receipt binds all outputs | Exact artifact hashes, byte counts, adapters, versions, model, plan, and comparison outcomes are retained | PASS |
| 17 | Authority remains separate | Receipt is non-shareable and disclaims approval, rights, selection, performance, and publication | PASS |

Schema rows complete: all required plan, packet, comparison, lineage, and
authority-boundary elements are represented. `SCHEMA-DIFF-COMPLETE`.

## Element diff

No blocking or advisory mismatch was found between the documented C2 contract
and the implementation. The documented single implicit MusicXML measure,
absence of lyric underlay, non-overlapping-voice restriction, and utilitarian
square-wave guide are enforced or disclosed rather than silently exceeded.

## Gate token

- census-distribution: music-score-export-c2/implementation
- gate-provenance: §S5.5-Sub-task-A
- mechanism-distribution: music-score-export-c2/implementation
- mechanism-type-shared: PASS
- gate-status: PASS
- attestation-by: Music Reconstruction Engineer contract lens (simulated)
- attestation-result: The implementation satisfies every declared C2 structural export element.
- verification-by: Independent CLI and tamper-test witness
- verification-result: MIDI, MusicXML, WAV, receipt, atomicity, and mutation rejection passed on the synthetic fixture.

This simulated gate is implementation evidence, not human musical approval.
