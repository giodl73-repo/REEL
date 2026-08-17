---
skill: roles-check
topic: reel-craft-plan-v025
date: 2026-08-16
roles_used: [story-director, editor, sound-designer, animation-director, platform-audience]
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL craft plan v0.2.25 roles check

## Artifact identification

**Type:** additive product contract, validator, structural coverage report,
least-information packet exporter, fictional fixture, documentation, and tests.

**Reviewed:** `src/craft_plan.rs`, CLI routing, the sanitized three-period memoir
fixture, generated costume and cinematography packets, v0.2.25 documentation,
and focused tests.

**Domain signals:** cross-department workflow, continuity, privacy-aware routing,
human authority, editorial timing, sound/score separation, VFX handoff,
accessibility, provenance, and artifact integrity.

## Role selection

- **Story Director:** the plan must preserve narrative intent without silently
  directing the work.
- **Editor:** editorial motivation, eye trace, sound bridges, and protected holds
  must be actionable and time-coherent.
- **Sound Designer:** sound and score must remain distinct human crafts with useful
  cross-cut information.
- **Animation Director:** VFX layers, contacts, depth, continuity, and reconstruction
  disclosure must support a plausible handoff.
- **Platform and Audience:** accessibility, privacy, packet minimization, and the
  difference between structural coverage and audience quality require review.

## Story Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Department intent is preserved as authored text while owner, workflow state, and human approval remain separate. | P3 | department state | Preserve this separation; never infer creative authority from ownership metadata. |
| 2 | Three ordered periods and exact continuity states support a memoir arc without storing manuscript text or real identities. | P3 | sanitized fixture | Keep fixtures fictional and disclose reconstruction status explicitly. |
| 3 | `shot_ref` is currently an unbound string, so a craft decision can outlive or drift from the production-manifest shot it describes. | P2 | editorial/VFX linkage | Add an optional manifest hash plus validated shot-id binding in a later additive packet. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Cut reason, eye trace, sound bridge, protected hold, and movement motivation are all explicit rather than buried in notes. | P3 | editorial decisions | Keep these fields independent so one decision does not substitute for another. |
| 2 | Protected holds now use milliseconds, avoiding an undefined frame-rate dependency. | P3 | `ProtectedHold` | Convert to delivery frames only when binding to a timed manifest. |
| 3 | Department routing allows the same editorial decision to reach editing, sound, accessibility, and directing without exporting the whole plan. | P3 | department packet | Retain explicit routing instead of hard-coded assumptions about departmental need. |

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Sound and score have separate department states, preventing room tone, effects, and composition from collapsing into one undifferentiated field. | P3 | department catalog | Preserve separate ownership and review gates. |
| 2 | `sound_bridge` reaches only explicitly routed departments, and the fixture leaves final music decisions human. | P3 | editorial decisions | Later manifest binding should reference existing audio events and score cues rather than duplicate them. |
| 3 | A structurally complete score department can still be blocked and awaiting human review, correctly resisting a false completion claim. | P3 | coverage report | Keep blocked state and human-review state visible independently. |

## Animation Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | VFX requirements cover layers, depth, occlusion, reflections, particles, contacts, cleanup, evidence, continuity, and assets. | P3 | VFX contract | Add renderer-specific execution only in adapter packets, not the core craft plan. |
| 2 | Controlled screen-direction and reconstruction-disclosure values plus exact match groups make continuity failures machine-checkable. | P3 | continuity | Keep age, wardrobe, hair, props, light, and geography descriptive while retaining controlled fields where finite vocabularies help. |
| 3 | The packet has a source hash but no re-verification command binding the exact exported bytes to the source and selected department. | P2 | packet integrity | Add a path-free receipt/check pair if department packets become exchange artifacts outside the local workspace. |

## Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Coverage always emits `artistic_quality_assessed: false`, and a missing department does not make validation fail. | P3 | coverage semantics | Keep structural completeness visibly distinct from approval and quality. |
| 2 | `not-applicable` status lets small productions acknowledge a department without fabricating work. | P3 | department status | Require intent to explain scope, but do not force an unnecessary creative deliverable. |
| 3 | Explicit routing minimizes packets, but evidence and asset records do not yet declare sensitivity or allowed distribution. | P2 | packet privacy | Add a small distribution policy before packets containing private or licensed references are shared externally. |

## Synthesis

Roles reviewed: 5
P1 blockers: 0 | P2 issues: 3 | P3 notes: 12

**Verdict: APPROVED-WITH-CONDITIONS**

**Top finding:** the craft plan is useful and appropriately non-authoritative,
but production adoption should bind decisions to exact manifest shots rather
than relying indefinitely on free-text `shot_ref` values.

**Cross-role consensus:** all roles support explicit routing and separate human
gates. Multiple roles require the next integration layer to reference existing
manifest timing, audio, and shot identity rather than duplicating them.

## Amend

1. Add optional production-manifest hash and validated shot/beat bindings for
   editorial and VFX decisions.
2. Add evidence/asset distribution policy before using department packets for
   external sharing or licensed/private references.
3. Add a department-packet receipt/check only when packets cross a trust boundary;
   retain the current atomic local export for internal planning.

This simulated roles review complements tests. It does not represent human
approval, artistic judgment, or authority to publish.
