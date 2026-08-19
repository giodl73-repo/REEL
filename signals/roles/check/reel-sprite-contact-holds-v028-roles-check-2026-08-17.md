---
skill: roles-check
topic: reel-sprite-contact-holds-v028
date: 2026-08-17
roles_used: 5
p1_count: 0
verdict: APPROVED
---

# Roles check: REEL sprite contact and holds v0.2.28

## Artifact identification

The artifact is a backward-compatible manifest, renderer, and motion-review
feature. It adds parent-relative sprite position space, shared-cadence contact
validation, reason-bearing intentional holds, a fifty-percent exemption ceiling,
and additive motion-report fields. A sanitized CLI fixture and renderer unit
tests cover valid and rejected states.

## Selected roles

- **Animation Director:** evaluates pose/contact semantics and renderer feasibility.
- **Editor:** evaluates whether held timing remains intentional and auditable.
- **Platform and Audience:** checks delivery compatibility and report clarity.
- **Sound Designer:** checks that the silent proof does not imply a sound policy.
- **Story Director:** checks whether mechanics preserve causal action rather than dictate content.

## Findings

### Animation Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Parent-width offsets express prop contact across pose changes without coupling REEL to hockey or one generator. | P3 | schema | Pass. |
| 2 | Shared cadence and complete parent-keyframe coverage prevent a child from silently missing a pose change. | P3 | validation | Preserve both gates. |
| 3 | Rejecting nested parents keeps v0.2.28 deterministic while leaving deeper rigs as an explicit future version. | P3 | scope | Pass. |

### Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Hold spans require frame boundaries and a reason, making anticipation reviewable. | P3 | intentional holds | Pass. |
| 2 | Motion checking retains total stationary cadence while separately measuring unexpected stationary transitions. | P3 | motion report | Pass. |
| 3 | The fifty-percent ceiling prevents a whole frozen shot from exempting itself. | P3 | validation | Preserve the ceiling. |

### Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Parent offsets are resolved for the requested canvas aspect ratio before compositing. | P3 | renderer | Pass. |
| 2 | Existing manifests default to canvas position space and empty hold lists, preserving compatibility. | P3 | serde defaults | Pass. |
| 3 | JSON reports expose both permitted and unexpected stationary fractions for downstream review interfaces. | P3 | report contract | Surface the reasons in a later UI refinement. |

### Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | The feature is picture-only and does not alter audio-event, ducking, or mastering contracts. | P3 | scope | Pass. |
| 2 | The Karts proof is explicitly silent to isolate motion rather than treating sound as unimportant. | P3 | production proof | Add effects only in a separate controlled pass. |
| 3 | Hold declarations could later inform sound accents but do not automatically assign them. | P3 | future integration | Preserve human sound judgment. |

### Story Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | The feature organizes object ownership and timing without inventing story meaning. | P3 | ownership boundary | Pass. |
| 2 | Reasons document why a beat holds while leaving artistic approval outside the validator. | P3 | hold contract | Pass. |
| 3 | The sanitized fixture proves generic performer/token choreography rather than embedding customer identities. | P3 | tests | Preserve fixture neutrality. |

## Synthesis

Roles reviewed: 5  
P1 blockers: 0 | P2 issues: 0 | P3 notes: 15

**Verdict: APPROVED**

Top finding: v0.2.28 converts two project workarounds—manual prop coordinates
and validator-hostile anticipation—into bounded, tested, renderer-neutral
contracts.

Cross-role consensus: the feature supports animation craft without pretending
to judge whether a pose, hold, or contact choice is artistically good.

## Amendments

1. Add named pose anchors if multiple recurring contact points justify a
   higher-level rig contract.
2. Surface intentional-hold reasons alongside counts in future review UIs.
3. Keep nested rigs and automatic sound accents out of scope until independent
   productions demonstrate a reusable need.
