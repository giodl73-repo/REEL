---
skill: roles-check
topic: reel-delivery-camera-production-package-v027
date: 2026-08-16
roles_used: [story-director, editor, sound-designer, animation-director, platform-audience]
p1_count: 0
verdict: APPROVED
---

# REEL v0.2.27 delivery-camera and production-package roles check

## Artifact identification

**Type:** manifest contract, renderer implementation, release-integrity schema,
CLI, documentation, tests, and real render proofs.

**Evidence:** source diff, 16:9 and 9:16 four-second camera renders, artifact
lineage, package receipt/check tests, full test suite, and strict lint.

## Role selection

- **Story Director:** creative authority and release-gate semantics.
- **Editor:** camera timing, movement motivation, and conformed-shot behavior.
- **Sound Designer:** package completeness without conflating audio approval.
- **Animation Director:** sprite/camera execution and aspect-ratio feasibility.
- **Platform and Audience:** crop safety, privacy, accessibility, and receipts.

## Findings

### Story Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Camera execution consumes authored beat-bound phrases rather than inventing a shot move. | P3 | choreography compiler | Preserve authored camera authority. |
| 2 | Package `release_ready` requires explicit review gates and never follows from a successful render alone. | P3 | package receipt | Keep integrity and approval distinct. |
| 3 | Approved gates require a bound evidence component and accountable owner. | P3 | review gates | Never infer evidence from prose or filenames. |

### Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Camera keyframes share the sprite timing FPS and must begin at frame zero, increase, and remain within the shot. | P3 | production validation | Retain blocking frame validation. |
| 2 | Follow, whip, hold, and settle preserve intentional curve differences through delivery. | P3 | FFmpeg compilation | Keep curves explicit in lineage-bearing manifests. |
| 3 | The final camera applies after sprite compositing, keeping player/puck paths independent from framing. | P3 | filter graph | Preserve this ordering. |

### Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Score plans, render artifacts, video, captions, and review evidence can be bound without merging their contracts. | P3 | package components | Keep component kinds controlled. |
| 2 | A verified score-plan component proves bytes, not composition quality, mix approval, or rights. | P3 | package semantics | Preserve the documented negative claim. |
| 3 | Camera execution changes picture only and leaves existing audio-event and synchronization paths intact. | P3 | renderer | Continue regression coverage for mixed-media audio. |

### Animation Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Choreography now emits real camera keyframes alongside pose, path, and handoff tracks. | P3 | asset compiler | Preserve the semantic-to-render separation. |
| 2 | Crop centers are clamped during compilation and bounded again by the FFmpeg expression. | P3 | crop safety | Keep both defenses. |
| 3 | Real 640×360 and 360×640 renders completed from the same choreography manifest. | P3 | render proof | Maintain dual-aspect release probes. |
| 4 | Artifact lineage explicitly counts sprite camera tracks. | P3 | auditability | Version-gate future lineage additions similarly. |

### Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Output aspect ratio is an execution input; one proof no longer implies crop safety for another shape. | P3 | delivery | Require every intended delivery proof. |
| 2 | Package component paths must be relative and cannot traverse above the package root. | P3 | containment | Preserve canonical containment checks. |
| 3 | Receipts omit paths while binding IDs, kinds, hashes, bytes, work, revision, scope, and gates. | P3 | privacy/integrity | Keep the receipt externally shareable. |
| 4 | Component tampering and receipt/package disagreement fail closed. | P3 | verification | Preserve exact-byte comparison and atomic output. |

## Synthesis

Roles reviewed: 5
P1 blockers: 0 | P2 issues: 0 | P3 notes: 17

**Verdict: APPROVED**

**Top finding:** camera choreography has crossed the final delivery boundary at
multiple aspect ratios while production-package integrity remains explicitly
separate from human release authority.

**Cross-role consensus:** editor, animation, and platform lenses agree that
camera movement is now executable and crop-bounded; story, sound, and platform
agree that a verified package cannot silently approve itself.

## Amend

No blocking amendment. Future non-blocking additions:

1. Add optional square render proof to a release matrix when a customer needs it.
2. Add a package-assembly helper only after repeated customer use proves the
   required component-selection policy.
3. Add scratch camera-safe guides to previews without changing delivery crops.

This simulated review is not artistic judgment, rights clearance, human
approval, or authority to publish.
