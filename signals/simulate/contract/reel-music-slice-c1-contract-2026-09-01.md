---
skill: simulate-contract
topic: reel-music-slice-c1
date: 2026-09-01
gate_result: PASS
---

# Contract simulation: `reel-music` Slice C1

## Inputs

- Specification: `docs/music-reconstruction-crate-proposal.md`, contract-family
  and corrected-model sections plus the first two Slice C bullets.
- Implementation: `analysis.rs`, `model.rs`, CLI v0.2.28, generated fixture,
  tests, and release documentation.
- Deferred by contract: MIDI/MusicXML adapters, notation re-import, audible
  guide, timeline export, language adaptation, and arrangement.

## Gate token

```yaml
census-distribution: shared
gate-provenance: §S5.5-Sub-task-A
gate-status: PASS
attestation-by: Music Reconstruction Engineer
attestation-result: Analysis estimates remain immutable evidence and corrected model events retain exact lineage and provenance state.
verification-by: Rights and Provenance Steward
verification-result: Validation is local and side-effect-free, model authority is explicit, and human corrections require immutable decision references.
mechanism-distribution: shared
mechanism-type-shared: PASS
```

The domain contracts live in `reel-music`; the root package supplies only CLI
dispatch. Role names are simulated lenses, not human approval.

## Element diff

| # | Contract element | Implementation evidence | Severity | Result |
|---|---|---|---|---|
| 1 | Analyzer output remains separate evidence. | `reel.music-analysis.v0.1` is immutable and separately hash-bound from `reel.music-model.v0.1`. | P2 | Match |
| 2 | Bind exact source lineage. | Both contracts revalidate source manifest, canonical contract, and decoded PCM hashes. | P2 | Match |
| 3 | Record analyzer provenance. | Analyzer ID, adapter, version, model revision, parameter SHA-256, license, and denied network policy are required. | P2 | Match |
| 4 | Record source-local observations. | Each typed observation has analyzer ID, half-open sample range, confidence millionths, and uncertainty. | P2 | Match |
| 5 | Avoid ground-truth claims. | Reports say verified, not correct; limitations are mandatory and reviews remain separate. | P2 | Match |
| 6 | Treat separated stems as evidence. | Optional raw-PCM stems require exact identity, matching timebase, mixture consistency, bleed, and uncertainty. | P2 | Match |
| 7 | Build a separate corrected model. | Model binds one or more current analysis contracts without modifying them. | P2 | Match |
| 8 | Carry explicit model authority. | Top-level `AuthorityRef` scopes model identity/status/roles and governed decisions. | P2 | Match |
| 9 | Represent musical time deterministically. | PPQ and rounding match the immutable source; ticks and durations are integers. | P2 | Match |
| 10 | Represent tempo, meter, and form. | Point maps start at zero; form sections are contiguous and cover complete duration. | P2 | Match |
| 11 | Represent editable notes and parts. | Parts contain canonical ordered notes with voice, onset, duration, pitch, velocity, and provenance. | P2 | Match |
| 12 | Represent harmony, rhythm, hooks, and expressive timing. | Typed structures validate ranges, element references, onsets, and bounded note adjustments. | P2 | Match |
| 13 | Keep unknowns visible. | Model carries an explicit unknowns ledger; fixture names omitted expressive facts. | P3 | Match |
| 14 | Distinguish observed/inferred/corrected. | Every event has a state, rationale, evidence references, and optional correction reference. | P2 | Match |
| 15 | Require evidence for observations/inferences. | References must resolve to exact current analysis/observation IDs; unknown and stale evidence tests fail. | P2 | Match |
| 16 | Require decisions for human correction. | `human-corrected` cannot validate without an immutable correction reference. | P2 | Match |
| 17 | Preserve exact lyric authority. | Vocal parts require a hash-verified canonical/as-sung layer with separate authority. | P2 | Match |
| 18 | Prove with non-private material. | Checked and generated fixtures contain only unsigned PCM and generic synthetic musical assertions. | P2 | Match |
| 19 | Preserve operating-system portability. | Canonical hashes are pinned in CLI tests and validated on Windows and WSL/Linux. | P2 | Match |
| 20 | Keep unfinished export work deferred. | v0.2.28 docs explicitly exclude MIDI, MusicXML, guide render, timeline export, translation, and arrangement. | P3 | Match |

## Residual risks

The synthetic model is structurally useful but not evidence that an editable
score is playable, recognizable, or emotionally faithful. Those claims remain
blocked until notation round trips and actual human musical review exist.
Analysis v0.1 deliberately permits only network-denied provenance.

## Gate result

**PASS** — C1 satisfies the analysis-evidence and corrected-model subset of
Slice C. It does not satisfy or approve Slice C2 exports, a real transcription,
a musical correction, a vocal, an arrangement, or publication.
