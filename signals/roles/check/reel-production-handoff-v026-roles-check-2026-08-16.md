---
skill: roles-check
topic: reel-production-handoff-v026
date: 2026-08-16
roles_used: [story-director, editor, sound-designer, animation-director, platform-audience]
p1_count: 0
verdict: APPROVED
---

# REEL production-handoff v0.2.26 roles check

## Artifact identification

**Type:** integrated contract, implementation, CLI, fixture, render-proof,
documentation, and regression review.

**Scope:** the corrective increment following the consolidated v0.2.22–25
review: shared production binding, choreography asset execution, synchronized
camera phrases, craft distribution policy, and external packet receipts.

Evidence reviewed includes the source contracts, sanitized fixtures, generated
sprite production manifest, 4-second sprite-render MP4, CLI reports, stale-hash
and tamper tests, complete automated suite, and strict lint result.

## Role selection

- **Story Director:** verifies exact story/shot identity without transferring
  creative authority to REEL.
- **Editor:** verifies the shared shot, beat, frame, and hold timing spine.
- **Sound Designer:** verifies that picture action and craft references can use
  existing production beat identity without conflating score and sound.
- **Animation Director:** verifies asset binding, pose execution, handoff
  motion, camera direction, lineage, and real renderer feasibility.
- **Platform and Audience:** verifies compatibility, privacy boundaries,
  least-information packets, receipts, and non-approval semantics.

## Story Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Choreography and craft records now resolve through one exact production-manifest hash and work ID, closing the prior parallel-story drift risk. | P3 | production binding | Preserve strict hash equality; never auto-refresh a stale binding. |
| 2 | Local shot references map explicitly to production shot IDs instead of relying on title or prose similarity. | P3 | shot identity | Keep mapping authored and reviewable. |
| 3 | The compiler carries source choreography, asset-binding, production-manifest, and shot identity into generated-manifest lineage. | P3 | execution lineage | Preserve all four identities in future adapters. |
| 4 | REEL continues to organize declared intent without choosing performance, story worth, authenticity, or approval. | P3 | authority boundary | Keep negative authority claims machine-readable and documented. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Sidecar beats must map to existing production beat markers and agree within half a delivery frame. | P3 | beat synchronization | Retain the frame-derived tolerance rather than a loose millisecond constant. |
| 2 | Bound choreography duration must match its exact shot duration, preventing a valid plan from silently targeting another conformed revision. | P3 | shot timing | Keep duration failure blocking. |
| 3 | Craft protected holds are checked against the duration of the exact bound shot. | P3 | protected holds | Add placement within a shot only if the craft schema later gains start offsets. |
| 4 | Stale production hashes are rejected before compilation or output publication and are covered by a CLI regression test. | P3 | atomicity | Preserve preflight-before-write behavior. |

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Choreography now cites the production beat IDs already available to score picture hits, so action and music can share identity without copying timestamps. | P3 | audiovisual sync | Continue resolving IDs through the production manifest. |
| 2 | Craft editorial and VFX records resolve to real production shots while `sound_bridge` remains intentional craft direction, not an invented audio event. | P3 | sound/craft boundary | Keep execution events in the production manifest. |
| 3 | The sprite proof remains intentionally silent and does not imply finished effects, mix, or score. | P3 | proof scope | Make scratch cues explicit and opt-in if added later. |
| 4 | Score direction, audio events, choreography, and craft planning remain separate contracts joined by production identity rather than collapsed into one brief. | P3 | architecture | Preserve this separation of concerns. |

## Animation Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | A strict asset sidecar pins the choreography hash and requires exact performer, prop, and reaction-pose coverage. | P3 | asset binding | Preserve exact-set validation and asset-root containment. |
| 2 | The choreography compiler emits an actual validated sprite production manifest with sampled performer paths, pose swaps, prop handoff, and auditable lineage. | P3 | execution adapter | Keep the semantic plan separate from renderer-specific tracks. |
| 3 | That manifest completed a real 4-second MP4 render, proving the existing sprite path can consume the compiled result. | P3 | renderer proof | Retain a small sanitized render probe for release verification. |
| 4 | Camera hold, follow, whip, and settle phrases use the choreography beat spine, validate targets and zoom bounds, compile into the resolved plan, and appear in the blocking preview. | P3 | camera choreography | Promote camera framing into a delivery renderer only when its crop behavior can be validated per aspect ratio. |

## Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Production binding, asset execution, and distribution controls are additive; sidecar-free production manifests retain prior behavior. | P3 | compatibility | Continue running legacy fixtures in the full suite. |
| 2 | Every craft evidence and asset record declares `internal-only`, `approval-required`, or `shareable`, and policy activates at the explicit external boundary. | P3 | privacy | Keep local planning simple and external intent explicit. |
| 3 | External exports reject internal-only material and require a non-empty approval reference for selected approval-required material. | P3 | distribution gate | Do not treat the reference as proof of rights or approval. |
| 4 | Department packet receipts are path-free and bind packet hash, byte count, source-plan hash, department, schema, and distribution scope; tampering is rejected. | P3 | packet integrity | Preserve strict parsing and exact-byte checks. |
| 5 | Coverage and validation still state that structural success is not artistic, cultural, historical, licensing, or human approval. | P3 | audience trust | Keep this distinction in reports and docs. |

## Synthesis

Roles reviewed: 5
P1 blockers: 0 | P2 issues: 0 | P3 notes: 21

**Verdict: APPROVED**

**Top finding:** v0.2.26 establishes one cryptographically pinned production
identity and timing spine across choreography and craft, then proves the
choreography boundary through the existing sprite renderer.

**Cross-role consensus:** story, editing, sound, and animation agree that stale
parallel contracts are now rejected rather than trusted. Platform review finds
that packet sharing now has an explicit least-information distribution gate and
externally verifiable integrity without exposing local paths.

## Amend

No P1 or P2 amendment remains from the prior integrated review. Recommended
future refinements are deliberately non-blocking:

1. Add aspect-ratio-aware camera execution after crop-safety validation; the
   current camera contract, compiler, and blocking preview are sufficient for
   synchronized direction but do not claim final camera rendering.
2. Add optional scratch cue pips for internal choreography review while keeping
   previews silent by default.
3. Add protected-hold placement only if a later craft use case needs a hold at
   a specific offset inside a shot.

This simulated roles review complements tests and artifact verification. It is
not human approval, artistic judgment, rights clearance, or authority to
publish.
