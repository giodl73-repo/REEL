---
skill: roles-check
topic: reel-choreography-v024
date: 2026-08-16
roles_used: [animation-director, editor, platform-audience, story-director, sound-designer]
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL choreography v0.2.24 roles check

## Artifact identification

**Type:** additive product contract, compiler, abstract preview renderer, CLI,
fixture, documentation, and tests.

**Reviewed:** `src/choreography.rs`, CLI routing in `src/main.rs`, the
`simple-handoff.yaml` fixture, generated blocking preview packet, v0.2.24
documentation, and focused regression tests.

The artifact is renderer-neutral infrastructure. It does not select a hockey
story, encode a hockey-specific verb, or claim publication approval.

## Role selection

- **Animation Director:** phrase grammar, pose timing, continuity, path/timing
  separation, and renderer feasibility are the center of the change.
- **Editor:** exact beat frames and `hold-then-burst` timing affect rhythm and
  action clarity.
- **Platform and Audience:** the blocking packet is a visual review surface with
  explicit dimensions, labels, and a contact sheet.
- **Story Director:** semantic action phrases must preserve authored intention
  rather than collapse back into unexplained coordinates.
- **Sound Designer:** future camera, effects, and music events must synchronize to
  the same beats without forcing sound into this initial renderer.

## Animation Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Spatial paths and timing curves are independent, retaining both designed arcs and intentional holds. | P3 | `SpatialPath`, `TimingCurve` | Preserve this separation in every adapter. |
| 2 | Ownership, overlap, and resolved-frame bounds checks make the abstract blocking trustworthy. | P3 | validation | Keep checks renderer-neutral and add collision policy only when a use case can distinguish intended from accidental contact. |
| 3 | The resolved plan is not yet consumed by the production sprite-animation or a Remotion adapter. | P2 | adapter handoff | Add an asset-binding sidecar and one adapter proof before calling choreography production-ready. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Strictly increasing named beats make action order and duration inspectable. | P3 | beats | Retain exact frames; do not replace them with prose timing. |
| 2 | `hold-then-burst` provides a useful limited-animation rhythm without enforcing smooth movement. | P3 | timing curves | Evaluate the curve against more than one vignette before adding presets. |
| 3 | A choreography sidecar has no explicit binding to a production-manifest shot or its beat markers, so duplicate timelines could drift. | P2 | manifest boundary | Add a hash-bound shot/beat binding when the first production adapter is introduced. |

## Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | MP4, contact sheet, path overlay, and resolved JSON provide complementary review surfaces. | P3 | preview packet | Keep all four; none alone exposes timing, geography, and lineage. |
| 2 | Performer label rows are deterministic, but dense blocking can still make mark labels visually collide with performers. | P3 | abstract raster | Add optional mark-label suppression or a legend if denser scenes demonstrate the need. |
| 3 | The preview validates canvas bounds but does not claim phone legibility or audience-facing accessibility. | P3 | preview scope | Keep the packet explicitly internal; run platform review on the finished film rather than treating blocking labels as delivery graphics. |

## Story Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | `approach`, `handoff`, and `react` describe causal screen action without importing a sport or customer. | P3 | phrase vocabulary | Keep new core verbs relational and cross-domain. |
| 2 | Optional phrase IDs survive compilation, allowing notes to cite authored intention instead of only coordinates. | P3 | plan lineage | Require IDs only when a downstream review or adapter needs stable references. |
| 3 | Reactions record a pose but not yet a target, cause, facing, or gaze relationship. | P3 | `react` | Add relational reaction fields only after a concrete second vignette proves which ones are reusable. |

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Named exact beats are suitable future anchors for effects and music hits. | P3 | beats | Reuse beat IDs rather than creating a parallel choreography-only sound clock. |
| 2 | The silent preview correctly avoids implying a finished sound design. | P3 | blocking MP4 | Keep blocking silence the default; optional metronome or cue pips must be explicitly requested. |
| 3 | Choreography beats are not yet bound to production-manifest audio events or camera phrases. | P2 | cross-event synchronization | Prove one shared-beat binding with the production adapter rather than duplicating audio fields in the sidecar. |

## Synthesis

Roles reviewed: 5
P1 blockers: 0 | P2 issues: 3 | P3 notes: 12

**Verdict: APPROVED-WITH-CONDITIONS**

**Top finding:** the planning and blocking loop is useful now, but production use
requires a hash-bound adapter proof that consumes the resolved plan with real
assets.

**Cross-role consensus:** Animation, editing, and sound agree that the next layer
must synchronize through existing production-manifest beats instead of creating
parallel shot, camera, or audio timelines.

## Amend

1. Add a separate asset/pose binding and prove one resolved choreography plan
   through the existing sprite renderer or Remotion adapter.
2. Bind the choreography source/plan hash and exact beat mapping to one
   production-manifest shot, then add a re-verification command for that packet.
3. Add camera following and audio-hit synchronization only through shared beat
   references; do not duplicate production audio or hockey vocabulary in the
   choreography schema.

This simulated roles review complements tests and artifact probing. It does not
represent human approval or authority to publish.
