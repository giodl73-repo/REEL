# Motioncraft Phase 2 — Camera, Validation and Episode Proof

Plan date: 2026-10-06. Status: role-reviewed execution plan; implementation evidence remains subject to the gates below.

## Objective

Complete the missing production capabilities: native Windows cadence, reproducible comparisons, bounded focal-point camera movement, layer-aware temporal validation, and full bilingual episode qualification. Every supported field must travel from CAIMITOS `episode.json` defaults and `scene.json` overrides through compilation, selected delivery and the native REEL scene engine. Existing episodes use immutable upgrades with an exact rollback route.

The detailed consumer plan is maintained in CAIMITOS at `projects/animation-vfx/MOTIONCRAFT-PHASE-2.md`. This REEL plan records the shared engine scope and release gates; checkpoint results belong in [the progress record](motioncraft-phase-2-progress.md).

## Ordered delivery and acceptance

| Phase | Work | Completion evidence |
|---|---|---|
| P2.1 Native cadence | Resolve and record a native Windows FFmpeg analyzer without WSL; retain Linux behavior and existing thresholds. | Moving, hold, reduced-motion and deliberate-freeze checks on actual decoded frames; invalid executable resolution fails explicitly; exact tool identity retained. |
| P2.2 Comparison package | Produce still, directed and reduced variants from one frozen selection and native clock, with indexed frames and synchronized playback. | Exact audio stems, cue/effect placement and content identity; portable package verification, tamper rejection, fresh canonical-cache consumer reproduction and original-revision rollback. |
| P2.3 Focal camera | Optional normalized focal anchor and bounded pan/zoom, explicit transform order and interpolation; propagate through defaults, shot overrides, selected overlays and existing-job upgrades. | Whole-path protected-region, source-edge and caption-band checks; off-center subjects, rational frame rates, short phases and different ES/EN spans; native rendered evidence, unchanged legacy output and reduced-motion identity. |
| P2.4 Layer validation | Bind each layer's moving/hold/visibility expectations to selected carrier hashes and native frame intervals. Prove contribution survives in the final composite. | Deliberate visible freezes, incorrect timing/composition and stale sources fail. Moving layers and declared holds pass. Transparent, hidden or imperceptible evidence is inconclusive. Alpha-edge and fresh-cache consumer checks retained. Unsupported compositions have explicit dispositions. |
| P2.5 Episode qualification | Freeze one complete existing CAIMITOS bilingual episode, including manuscript scope, selected art, alignments, captions, opening/title/credits and layer inputs before rendering. | All 12 combinations: ES/EN × landscape/portrait × still/directed/reduced. Exact clocks/stems/frame counts, every cut reviewed at normal speed, phone captions, protected subjects, poems, sound bridges and repetition. Future authoring and existing upgrades reach the same engine; full evidence rehydrates from canonical cache. |

Each audition names the narrative beat, focal detail, protected context and reason to move or hold. A successful source-layer analysis alone cannot establish final visibility. An isolated shot or synthetic fixture cannot establish complete-episode qualification.

## Compatibility and shipping

Absent direction retains legacy behavior. Reject unknown, stale or unsupported intent rather than silently dropping it. Preserve selected source identities and language-local narration samples; never fit speech to a camera duration. Portrait composition requires explicit layout and protection, with profile-specific effect carriers when necessary.

Retain source revisions, jobs, reports, exact engine/adapter/tool identities and cache-backed assets. Test rollback against the original accepted input. Reuse already approved shipped assets within their recorded authority; new art or changed portrayal requires its own authority.

Closeout requires all five acceptance gates, native Windows/Linux checks, actual CAIMITOS dependency-pin integration, runbook examples, rejection and rollback coverage, validated cache handoff, reviewed PRs and main-branch integration records. Technical qualification does not select a creative master or grant publication approval.

Springs, new transitions, generated parallax, character animation, automatic art redesign and mass episode upgrades are deferred.

## Role review

[Role findings and dispositions](../signals/roles/check/motioncraft-phase-2-plan-roles-check.md) are incorporated above. The review is the current assistant applying repository checklists; it is not independent agent review or human approval. Consumer principal findings remain separately recorded in the CAIMITOS plan.

## Goal instruction

> Complete Motioncraft Phase 2 using this plan and the CAIMITOS consumer plan. Implement native cadence, reproducible comparisons, compatible focal camera direction and layer-aware validation through authoring, adapters and the native scene engine. Qualify one frozen complete bilingual episode across all 12 combinations. Preserve native clocks, protected content, selections, rollback and cache provenance. Update the runbook, validate exact consumer pins and ship reviewed changes. Report technical completion separately from creative selection; keep unmet gates explicit.
