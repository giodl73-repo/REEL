---
skill: roles-check
topic: reel-chapter-score-direction
date: 2026-08-16
roles_used: 5
p1_count: 0
verdict: APPROVED
---

# Roles check: REEL chapter score direction

## Artifact identification

- Type: additive production-manifest contract, validator, CLI compiler, fixture,
  tests, and documentation
- Domain signals: sound design, edit synchronization, narrative emotion,
  provider-neutral handoff, backward compatibility, and platform mix intent

## Role selection

| Role | Why selected |
|---|---|
| Sound Designer | Owns music, effects, silence, mix priority, and emotional rhythm. |
| Editor | Score sync points and montage notes must serve picture rhythm. |
| Story Director | Chapter cues express narrative function and emotional movement. |
| Platform and Audience | Music direction must not weaken sound-off or accessibility contracts. |
| Animation Director | The handoff must remain renderer/provider neutral and feasible. |

## Review findings

### Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | The score is now structural: cues own narrative function, mood/energy movement, instruments, motifs, transitions, and silence-shaped handoffs. | P3 | `ScoreDirection` | Preserve this separation from the legacy prose `music_direction`. |
| 2 | Instrument direction includes family, role, timbre, and articulation, allowing richer asks without pretending to encode a finished performance. | P3 | `ScoreInstrument` | Add execution evidence only in a separate future contract. |
| 3 | `original-only` plus an explicit avoid list gives productions a place to reject copied melodies and artist imitation. | P3 | Policy/docs | Keep licensing and listening approval external and human. |

### Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Exact sync points can bind to existing beat markers, preventing score intent from drifting from a conformed edit. | P3 | Validation | Preserve millisecond alignment checks. |
| 2 | `montage_intent` and `picture_notes` can ask music to preserve complete calls or avoid accenting every cut. | P3 | `ScoreCue` | Keep these notes advisory rather than silently generating cuts. |
| 3 | Overlapping cues remain legal, which supports layered transitions and stems; timeline bounds still fail safely. | P3 | Validation | Document overlap policy if a renderer begins consuming cues directly. |

### Story Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Each cue must state a narrative function, preventing place/style tags from becoming the whole story decision. | P3 | Validation | Retain this required field. |
| 2 | Mood and energy endpoints make escalation and reversals reviewable across chapters. | P3 | Cue contract | Human review should still judge whether the rendered music earns the arc. |
| 3 | Motifs can recur under different orchestrations, supporting story continuity without reusing one recording unchanged. | P3 | Motif contract | Keep motif IDs stable across score revisions. |

### Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | The optional score block does not alter captions, sound-off policy, export geometry, or existing manifests. | P3 | Compatibility | Continue testing score-free fixtures. |
| 2 | Score plans retain duration and timing status without leaking local paths. | P3 | `reel.score-plan.v0.1` | Keep shareability/privacy claims explicit if receipts are added later. |
| 3 | Tempo and energy are bounded, reducing malformed provider requests while leaving platform mastering to the existing audio contract. | P3 | Validation | Do not conflate score energy with loudness targets. |

### Animation Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | The score plan names no composer, model, synthesis engine, sample library, or renderer. | P3 | Provider boundary | Preserve provider neutrality. |
| 2 | Picture hits reuse beat markers already understood by visual and audio timelines. | P3 | Shared timing | Future adapters should consume the same marker IDs, not copy timestamps into sidecars. |
| 3 | The synthetic fixture proves the contract without adding binary assets or a provider dependency. | P3 | Fixture | Keep fixtures text-only unless execution evidence specifically requires media. |

## Synthesis

Roles reviewed: 5

P1 blockers: 0  |  P2 issues: 0  |  P3 notes: 15

Verdict: **APPROVED**

Top finding: REEL now expresses film-score intent precisely enough for a human
composer, music model, or renderer adapter while maintaining the critical
boundary that direction is not execution, originality proof, licensing, or
listening approval.

Cross-role consensus: shared beat markers and narrative-function-first cues
connect music, picture, and story without making the manifest provider-specific.

## Amendments

1. Preserve score direction as an optional additive v0.2 field so current
   BERTICA and other consumer manifests need no migration.
2. When rendered-score evidence is added, bind it through a separate hashed
   execution packet rather than claiming the plan proves performance.
3. If a renderer begins consuming overlapping cues, document stem/overlap mix
   behavior before making it a delivery guarantee.
