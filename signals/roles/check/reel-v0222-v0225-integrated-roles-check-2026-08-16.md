---
skill: roles-check
topic: reel-v0222-v0225-integrated
date: 2026-08-16
roles_used: [story-director, editor, sound-designer, animation-director, platform-audience]
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# Integrated REEL v0.2.22–v0.2.25 roles check

## Artifact identification

**Type:** consolidated architecture, contract, implementation, CLI, fixture,
artifact, documentation, and test review.

**Scope reviewed:**

- v0.2.22 chapter score direction and provider-neutral score plans;
- v0.2.23 authored cel sequences and keyframed sprite animation;
- v0.2.24 choreography sidecars, compilation, and blocking previews;
- v0.2.25 cross-department craft plans, structural coverage, and department
  packets;
- their combined CLI, manifest, timing, lineage, privacy, accessibility, and
  human-authority boundaries.

The review includes the existing feature-specific role checks, current source
contracts, documentation, sanitized fixtures, generated choreography and
department packets, and the complete automated test/lint surface.

## Role selection

- **Story Director:** checks whether the combined system preserves story intent
  without silently directing, inventing, or approving the work.
- **Editor:** checks the interaction among shots, frames, milliseconds, beat
  markers, holds, cuts, and motion phrases.
- **Sound Designer:** checks score direction, audio-event boundaries, sound
  bridges, silence, and picture synchronization.
- **Animation Director:** checks cel, sprite, choreography, camera, VFX,
  continuity, asset lineage, and renderer feasibility.
- **Platform and Audience:** checks backward compatibility, preview/delivery
  separation, accessibility claims, privacy, packet distribution, and review
  semantics.

## Story Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Score cues require narrative function, choreography phrases express causal action, and craft departments declare intent; story reasoning is no longer confined to generic notes. | P3 | score/choreography/craft contracts | Preserve distinct fields rather than replacing them with a single prose brief. |
| 2 | Customer story and real identities remain outside reusable REEL mechanics; both new fixtures are fictional or abstract. | P3 | ownership and fixtures | Continue requiring customer projects to own the selected story and private media. |
| 3 | Human authority is consistently explicit: score direction is not composition, coverage is not quality, role checks are not approval, and review gates are not inferred. | P3 | authority boundaries | Keep these negative claims in docs and machine reports. |
| 4 | `movement_motivation`, choreography phrase IDs, and production shots describe related story intent but have no shared binding, so they can drift while each file still validates. | P2 | cross-contract story identity | Bind craft and choreography records to an exact production-manifest hash and shot ID before production adoption. |
| 5 | The three-period fixture proves period progression and continuity without prescribing performance emotion or cultural authenticity. | P3 | craft fixture | Preserve this restraint when a real customer instantiates the contract. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Authored cels have exact frame holds, sprite tracks have explicit movement modes, and choreography separates spatial paths from temporal curves. | P3 | v0.2.23–24 motion | Preserve pose-to-pose holds as a deliberate option rather than treating smooth interpolation as universal quality. |
| 2 | Score picture hits already reuse production beat markers, giving music the strongest timing integration in the new stack. | P3 | score sync | Use this shared-marker model for choreography and craft integration. |
| 3 | REEL now has several valid clocks—manifest seconds, production beat milliseconds, cel/sprite frames, choreography frames, and craft hold milliseconds—but no single binding proves they refer to the same conformed shot revision. | P2 | timing authority | Add one hash-bound shot/beat mapping and reject stale sidecars. |
| 4 | A craft protected hold is renderer-neutral in milliseconds but cannot yet be checked against the referenced shot duration because `shot_ref` is free text. | P2 | protected holds | When bound, require the hold to fit inside the exact shot and map deterministically to delivery frames. |
| 5 | `cut_reason`, eye trace, sound bridge, protected hold, and movement motivation are independently reviewable and routed only to named departments. | P3 | editorial craft | Keep department routing explicit and least-information. |

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Chapter score direction now carries motifs, instrumentation, articulation, mood, energy, tempo, narrative function, transitions, and exact picture hits without claiming execution. | P3 | v0.2.22 score | Keep renderer/composer evidence in a separate future execution packet. |
| 2 | Manifest audio events, ducking, mastering, and score intent remain distinct; energy is not conflated with loudness. | P3 | audio architecture | Preserve this separation in adapters and review reports. |
| 3 | Blocking previews are silent by default, correctly avoiding a false impression of finished sound design. | P3 | choreography preview | Add cue pips or scratch sound only as explicit review options. |
| 4 | Craft `sound_bridge`, choreography beats, score cues, and manifest audio events cannot yet cite one verified shared beat/shot identity. | P2 | audiovisual synchronization | Resolve all four through existing manifest beat IDs instead of copying timestamps into parallel fields. |
| 5 | Sound and score have separate craft departments, and a structurally present score department may remain blocked and pending human review. | P3 | craft coverage | Retain the distinction between presence, readiness, and artistic approval. |

## Animation Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | REEL now has a coherent cost ladder: still motion, authored limited-animation cels, reusable sprites, semantic choreography, and planned richer adapters. | P3 | animation architecture | Select the cheapest representation that preserves the intended pose and action readability. |
| 2 | Asset-root containment, per-asset hashing, exact frame caps, ownership checks, overlap checks, and resolved stage bounds make the implemented render paths auditable. | P3 | validation and lineage | Preserve these checks when adding asset bindings. |
| 3 | The choreography compiler produces a useful resolved plan and preview, but no production sprite or Remotion adapter consumes it with real pose assets. | P2 | choreography execution | Add a separate pose/asset binding and prove one real adapter round trip before calling choreography production-integrated. |
| 4 | Camera remains a shot-level background treatment rather than a synchronized choreography participant that can hold, follow, whip, and settle on shared beats. | P2 | camera choreography | Add generic camera phrases only after binding choreography to production beats; do not invent a second camera timeline. |
| 5 | Craft VFX requirements explicitly cover layers, depth, occlusion, reflections, particles, contacts, cleanup, evidence, continuity, and reconstruction disclosure. | P3 | VFX handoff | Keep renderer-specific settings in adapter packets rather than the core craft plan. |

## Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | All four releases are additive: old manifests remain valid, and choreography/craft contracts are optional sidecars. | P3 | compatibility | Continue running score-free and sidecar-free fixtures in the full suite. |
| 2 | Choreography MP4, contact sheet, path overlay, and resolved plan form a useful internal review packet without claiming audience-facing legibility. | P3 | preview scope | Keep delivery accessibility and phone legibility in the final-film review path. |
| 3 | Craft coverage always reports `artistic_quality_assessed: false`; missing or not-applicable departments do not masquerade as artistic failure. | P3 | coverage semantics | Preserve this machine-readable disclaimer. |
| 4 | Department routing minimizes exported information, but evidence and asset records lack sensitivity and allowed-distribution policy. | P2 | packet privacy | Add distribution classification before using packets with private, licensed, or externally shared references. |
| 5 | Local department packets are atomic and source-hashed but lack a path-free receipt/check for external recipients. | P3 | packet integrity | Add receipt/check only when packets cross a trust boundary; do not burden local planning prematurely. |

## Synthesis

Roles reviewed: 5
P1 blockers: 0 | P2 issues: 7 | P3 notes: 18

**Verdict: APPROVED-WITH-CONDITIONS**

The feature stack is approved to merge as additive, explicitly experimental
planning and rendering infrastructure. It is **not yet approved to claim that
choreography or craft sidecars are production-integrated handoffs**.

**Top finding:** the strongest individual features now need one shared identity
and timing spine. A source hash, production-manifest hash, shot ID, and existing
beat-marker mapping should connect story intent, choreography, camera, craft
holds, sound bridges, score hits, and the rendered asset.

**Cross-role consensus:** Story, editing, sound, and animation all identified the
same risk: valid parallel contracts can drift from one another unless they bind
to the exact conformed production shot. Platform review separately requires an
explicit distribution policy before department packets leave the local trust
boundary.

## Amend

1. **Add one optional production binding shared by choreography and craft.** It
   should carry the production-manifest SHA-256, work ID, shot ID, and explicit
   sidecar-beat to manifest-beat mapping. Validation must reject stale hashes,
   unknown shots, out-of-shot holds, and mismatched timing.
2. **Prove the choreography execution boundary.** Add a separate pose/asset
   binding, compile one choreography plan through the existing sprite renderer
   or Remotion adapter, and bind the output artifact back to the plan and exact
   manifest shot. Camera phrases should join this same beat spine.
3. **Harden department packets only at the sharing boundary.** Add evidence and
   asset sensitivity/distribution fields before private or licensed use; add a
   path-free receipt/check when packets are sent externally. Keep local atomic
   exports simple.

## Merge recommendation

**Proceed with merge** if the release notes continue to describe choreography
as a blocking/compiler preview and craft plans as local structural handoffs.
Track the three amendments as the next integration increment. Do not describe
the unresolved bindings as already implemented.

This simulated roles review complements automated tests and artifact probes. It
does not represent human approval, artistic judgment, licensing clearance, or
authority to publish.
