---
skill: roles-check
topic: reel-sprite-emissions-v029
date: 2026-08-17
roles_used: 5
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# Roles check — REEL sprite emissions v0.2.29

## Artifact identification

- Type: renderer-neutral manifest extension, validator, FFmpeg adapter behavior,
  tests, and generic documentation.
- Domain signals: limited animation, choreography aftermath, compositing,
  editorial timing, accessibility, provenance, and mixed-media interoperability.

## Role selection

- Animation Director: owns the distinction between attached character parts and
  world-space aftermath.
- Editor: reviews lifetime, timing, fade, and whether effects preserve authored
  accents.
- Platform and Audience: checks bounded rendering, silent comprehension, and
  delivery interoperability.
- Sound Designer: checks whether visual effects remain independent of later
  sound-event choices.
- Story Director: checks that REEL organizes declared motivation without
  inventing where an effect belongs.

## Findings

### Animation Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Resolve-once then detach correctly models snow, dust, sparks, and residue that should remain in shot space. | P3 | production / adapter | Keep emissions separate from parent-bound sprite tracks. |
| 2 | Scale, drift, rotation, fade, z-order, and anchor controls cover useful limited-animation aftermath without becoming a particle simulator. | P3 | schema | Preserve the deliberately bounded contract. |
| 3 | Parent resolution uses the established parent-width geometry, reducing duplicate coordinate systems. | P3 | adapter | Keep geometry semantics aligned with child tracks. |

### Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Frame-owned spawn and duration make effect timing editorially explicit and reviewable. | P3 | schema | Keep frame timing manifest-owned. |
| 2 | Effects can animate across an intentional character hold, so measured stationary cadence no longer fully represents the dramatic hold. | P2 | motion verification | Document that human timing review remains required for effect-bearing holds. |
| 3 | Fade cannot exceed lifetime and every emission must end within its shot, preventing accidental transition leakage. | P3 | validation | Retain these hard boundaries. |

### Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Emission geometry is normalized and bounded, so the contract is independent of a specific raster size. | P3 | validation | Keep platform dimensions outside the emission contract. |
| 2 | A real mixed-media proof found and fixed a sprite-shot timebase mismatch at video transitions. | P2 | FFmpeg adapter | Retain the `settb=AVTB` normalization and regression assertion. |
| 3 | Emissions are included in hashed visual lineage, preserving delivery verification across platforms. | P3 | artifacts | Keep each emission asset independently hashed. |

### Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Visual emissions do not automatically create sound, so REEL does not invent skate, dust, spark, or impact effects. | P3 | contract boundary | Keep sound events separately authored. |
| 2 | Frame timing provides a precise anchor for a separately declared sound effect when a production wants one. | P3 | interoperability | Document cross-reference by authored frame/beat rather than implicit coupling. |
| 3 | Silent proofs remain visually understandable, which supports sound-off review and later mix iteration. | P3 | Karts proof | Preserve silent technical fixtures. |

### Story Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | The engine executes declared aftermath but never decides that a contact deserves emphasis. | P3 | architecture boundary | Preserve manifest ownership of artistic intent. |
| 2 | The generic dust fixture avoids embedding Karts identities or hockey-specific policy in REEL. | P3 | tests | Keep reusable fixtures sanitized. |
| 3 | The Karts proof demonstrates a meaningful use: residue records a planted edge after the performer departs, supporting cause and response. | P3 | consumer proof | Keep consumer examples outside REEL's normative contract. |

## Synthesis

Roles reviewed: 5  
P1 blockers: 0 | P2 issues: 2 | P3 notes: 13

Verdict: **APPROVED-WITH-CONDITIONS**.

Top finding: spawn-and-detach is the correct reusable abstraction, and the real
mixed-media proof validated the necessary timebase normalization.

Cross-role consensus: REEL should preserve authored timing and provenance while
remaining neutral about artistic placement, sound, and story meaning.

## Amendments

1. Retain the sprite-shot `settb=AVTB` normalization and its dry-run regression
   assertion so emitted shots can transition cleanly to ordinary video.
2. Document that animated emissions can make a dramatic character hold measure
   as moving; technical motion gates do not replace human timing review.
3. Keep emission assets independently hashed and keep sanitized fixtures free
   of consumer identities or project-specific policy.
