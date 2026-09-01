---
skill: roles-check
topic: reel-music-slice-c1-v028
date: 2026-09-01
roles_used: [music-reconstruction-engineer, score-arrangement-director, sound-designer, editor, rights-provenance-steward, lyrics-vocal-adaptation-editor]
p1_count: 0
verdict: APPROVED
---

# Roles check: `reel-music` Slice C1 v0.2.28

## Artifact and role selection

Artifact: strict external-analysis and corrected-model Rust contracts, CLI,
synthetic fixtures, tests, and documentation. Reconstruction, score, sound,
editing, provenance, and lyric roles are selected because the model carries
musical structure, source evidence, exact text-layer hooks, and future notation
lineage. Picture/story/platform roles are outside this contract.

## Music Reconstruction Engineer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Source, decoded signal, analysis manifest, and canonical analysis identities are all revalidated before the model is accepted. | P2 resolved | `analysis.rs`, `model.rs` | Preserve this full chain in export receipts. |
| 2 | Analyzer estimates retain exact sample region, integer confidence, uncertainty, engine/model/parameter evidence, and limitations. | P2 resolved | Analysis contract | Add sample↔tick anchors before picture-timeline export. |
| 3 | Observed/inferred events cannot cite unknown observations, and human correction requires a separate immutable reference. | P2 resolved | Provenance validation | Round-trip comparisons must retain provenance IDs. |

## Score and Arrangement Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | The editable model explicitly represents form, tempo, meter, notes, harmony, rhythm cells, hooks, and unknowns. | P2 resolved | Model schema | Add rehearsal marks, repeats, pickups, ties, and articulation only when required by C2. |
| 2 | Form covers the complete duration and point maps begin at zero, preventing partial score claims. | P2 resolved | Model validation | Make export loss for either structure a blocking receipt failure. |
| 3 | The fixture is structurally inspectable but is not represented as playable, recognizable, or arrangement-approved. | P3 | v0.2.28 docs | Keep those claims behind editable round trip and human score/listening review. |

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Optional stem evidence carries mixture-consistency, bleed, uncertainty, exact format, and synchronized timebase rather than a multitrack claim. | P2 resolved | `StemEvidence` | Add audible mixture checks when real stem adapters arrive. |
| 2 | Expressive timing is distinct from nominal score timing and cannot move a note outside the model. | P2 resolved | `ExpressiveTiming` | Ensure guide rendering records whether offsets were applied. |
| 3 | Dynamics, articulation, timbre, and performance feeling remain explicit unknowns in the fixture. | P3 | Fixture | Do not synthesize defaults into authoritative model facts. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Contiguous form sections make musical information order and duration inspectable. | P2 resolved | Form validation | Preserve section IDs in score markers and timeline export. |
| 2 | Canonical note ordering and bounded half-open ranges prevent adapter-dependent event order. | P2 resolved | Part/note validation | Treat reordered or dropped notes as round-trip failure. |
| 3 | Hook element references resolve to exact model elements rather than prose-only intent. | P2 resolved | Hook validation | Compare hook membership separately from human hook recognition. |

## Rights and Provenance Steward

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Analysis records model revision, parameter identity, declared license, and denied network policy; validation executes nothing. | P2 resolved | Analyzer contract | Add local tool/version evidence to later execution receipts. |
| 2 | The corrected model now has its own authority record in addition to event correction references and review state. | P2 resolved | `MusicModel.authority` | Require actual project decisions for governed statuses. |
| 3 | Reports contain private lineage and correctly remain `shareable: false`; technical verification does not imply approval. | P2 resolved | Reports/docs | Design redacted export receipts separately. |

## Lyrics and Vocal Adaptation Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Vocal models cannot validate without an exact hash-bound lyric layer and its own authority. | P2 resolved | Lyric validation | Add token/syllable underlay only in the governed language slice. |
| 2 | Canonical and as-sung layers are distinct enum values; target-language text is not smuggled into this model contract. | P2 resolved | `LyricLayerKind` | Keep corrected and translated layers in separately versioned contracts. |
| 3 | A lyric-layer request makes no claim about performed words, translation quality, or voice consent. | P2 resolved | v0.2.28 docs | Require listening and speaker consent evidence before vocal generation. |

## Synthesis

Roles reviewed: 6
P1 blockers: 0 | P2 issues: 0 open (15 resolved/passing) | P3 notes: 3

Verdict: **APPROVED**

Top finding: C1 prevents analyzer estimates from silently becoming corrected
musical authority while still producing an exportable, integer-timed model.

Cross-role consensus: notation and guide adapters may consume this model, but
must prove what survived their round trips and must not claim playability,
recognition, performed-word fidelity, or creative approval.

## Amendments applied

1. Added explicit top-level model authority so event corrections do not stand
   in for authority over the corrected model as a whole.
2. Required vocal parts to bind at least one exact, separately authorized lyric
   layer.
3. Bounded expressive onset/duration adjustments so they cannot create negative
   or out-of-model performed timing.

Human approval recorded: **none**.
