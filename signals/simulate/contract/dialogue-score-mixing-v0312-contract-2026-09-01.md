---
skill: simulate-contract
topic: dialogue-score-mixing-v0312
date: 2026-09-01
gate_result: PASS
---

# Dialogue-score mixing contract simulation

## Inputs

- Contract: `docs/dialogue-score-mixing-v0.3.12.md` and the engineering commission
- Implementation: `src/production.rs`, `src/audio_mix.rs`,
  `src/audio_preview.rs`, both animatic render paths, CLI, and CI
- Depth: standard

## Schema sweep

| # | Contract element | Implementation evidence | Result |
|---:|---|---|---|
| 1 | First-class dialogue role | `AudioRole::Dialogue` serde route | PASS |
| 2 | Narration semantics unchanged | distinct enum plus legacy compiler path | PASS |
| 3 | Configurable speech detector roles | `AudioDuckingPolicy.detector_roles` | PASS |
| 4 | Optional event automation | default-empty `gain_automation` | PASS |
| 5 | Exactly one point anchor | manifest validation | PASS |
| 6 | Local-time resolution | millisecond resolver | PASS |
| 7 | Beat-marker resolution | timeline-to-event-local resolver | PASS |
| 8 | Finite bounded automation | manifest validation | PASS |
| 9 | Unique ascending resolved points | validation plus negative tests | PASS |
| 10 | Hold interpolation | deterministic expression compiler | PASS |
| 11 | Linear interpolation | deterministic expression compiler | PASS |
| 12 | Smooth interpolation | deterministic smoothstep expression | PASS |
| 13 | Required event processing order | shared compiler graph | PASS |
| 14 | General detector/target policies | role-bus compiler | PASS |
| 15 | Explicit deterministic ordering | manifest order plus unique IDs/targets | PASS |
| 16 | Effects untouched unless targeted | component routing plus graph assertion | PASS |
| 17 | Maximum reduction floor | dry/wet blend and six-dB test | PASS |
| 18 | Legacy ducking compatibility | exact legacy graph and policy-hash shape | PASS |
| 19 | Dialogue stem definition | D = narration + dialogue | PASS |
| 20 | Music stem definition | M = post-declared-duck music | PASS |
| 21 | Effects stem definition | E = ambience + effects | PASS |
| 22 | Premaster/master distinction | filenames and receipt semantics | PASS |
| 23 | Default 48 kHz/24-bit WAV | CLI defaults and PCM codec | PASS |
| 24 | Configurable rate/layout | validated CLI rate and mono/stereo channels | PASS |
| 25 | Exact core sample geometry | WAV parser and sample-count equality | PASS |
| 26 | D+M+E recombination | three-LSB PCM24 proof | PASS |
| 27 | Path-free stem receipt | schema has hashes/IDs/basenames, no paths | PASS |
| 28 | Receipt lineage | manifest/policy/source/tool/output hashes | PASS |
| 29 | Stale source rejection | audio preview checker rehashes every input | PASS |
| 30 | Output/receipt tamper rejection | report receipt hash and per-output hashes | PASS |
| 31 | Overwrite refusal | existing output/report/stem directory gates | PASS |
| 32 | Dynamic-EQ schema | frequency/Q/cut/attack/release validation | PASS |
| 33 | Dynamic-EQ target isolation | policy retains exact detector/target roles | PASS |
| 34 | No false dynamic-EQ render claim | dry-run plan and explicit render failure | PASS |
| 35 | Dialogue-gated loudness evidence | EBU-R128 D-stem measurement under manifest policy | PASS |
| 36 | Speech-active margin | deterministic 100 ms D-to-(M+E) windows | PASS |
| 37 | Mono compatibility | configured maximum downmix-loss evidence | PASS |
| 38 | Small-speaker proxy | derived mono 180 Hz–5.5 kHz WAV and non-silence check | PASS |
| 39 | Clipping/duration evidence | peak sentinel plus exact geometry | PASS |
| 40 | Review variants | full, D, M, E, no-score, mono, small-speaker | PASS |
| 41 | Synthetic pass/fail quality fixtures | loudness/margin/clipping unit cases | PASS |
| 42 | Cross-platform FFmpeg exercise | Linux/Windows matrix synthetic test step | PASS |
| 43 | Shared audio graph | audio-only and still-animatic call same compiler | PASS |
| 44 | Authority boundary | docs and receipts make no creative approval claim | PASS |

SCHEMA-DIFF-COMPLETE

## Gate token

- census-distribution: 44/44 contract elements present and passing
- gate-provenance: §S5.5-Sub-task-A
- mechanism-distribution: 44/44 contract elements present and passing
- mechanism-type-shared: deterministic-role-bus-compile-plus-hash-and-pcm-recheck
- gate-status: PASS
- attestation-by: v0.3.12 contract census owner
- attestation-result: every P0 element and implemented P1 element is located in code or tests
- verification-by: independent schema-sweep witness
- verification-result: no omitted GateTokenSchema row and no blocking mismatch

## Phased P1 finding

Portable speech-keyed dynamic-EQ rendering is not implemented in v0.3.12. The
validated engine-neutral plan is complete and target-specific, and all render
paths fail rather than fake support. This is permitted by the commission's P1
phasing rule and is the only declared P1 omission.

Verdict: **GO** for technical review. This is not creative mix approval,
Golden selection, or release authorization.
