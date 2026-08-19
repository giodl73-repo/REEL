---
skill: roles-check
topic: layered-sprite-library-cache-plan
date: 2026-08-17
roles_used: 4
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL roles check: layered sprite library and cache plan

## Artifact identification

- Type: provider-neutral layered-sprite library, profile-binding, skinning, and cache protocol proposal
- REEL responsibility: schemas, layer composition, anchors, transformations, semantic-token resolution, lineage, validation, rendering, and path-free receipts
- Explicit non-responsibility: hockey vocabulary, player facts, team branding, likeness sourcing, art direction for a customer, or knowledge of any local drive

## Roles selected

1. Animation Director — pose language, layering, attachment, occlusion, and feasibility.
2. Editor — authored pose selection, fallback visibility, and timing preservation.
3. Platform and Audience — portability, cache independence, review/delivery profiles, and disclosure.
4. Story Director — preservation of action causality across semantic resolution.

## Review

### Animation Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Generic bodies, identity/head layers, skins, decals, equipment, props, and effects need distinct z-order and attachment contracts. | P2 | Layer model | Define named layer slots, anchors, pivots, masks/occlusion, and required/optional attachments. |
| 2 | Mirroring must occur before readable decals and asymmetric identity layers are applied. | P2 | Transform order | Encode transform order and add a regression fixture proving numbers remain readable. |
| 3 | Facing may change per keyframe, so it cannot be only a library- or character-level property. | P3 | Pose execution | Permit pose-instance transforms while keeping the source pose immutable. |

### Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Semantic lookup must not change choreography beats, holds, handoffs, or visibility windows. | P2 | Compiler boundary | Resolve art after timing compilation and bind the selected pose IDs into lineage. |
| 2 | Silent nearest-pose fallback could change the meaning of a pass, shot, block, or reaction. | P2 | Fallback policy | Require exact, declared-fallback, or unresolved states and surface all fallbacks in a coverage report. |
| 3 | Pose density is an editorial execution choice, not a library default that should inflate every track. | P3 | Sampling | Preserve binding-level subdivision controls and report emitted keyframe/input counts. |

### Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | REEL cannot serialize any machine-specific drive path into portable manifests. | P2 | Cache portability | Resolve a logical cache namespace through runtime configuration and keep receipts path-free. |
| 2 | Cache absence must not prevent validation of a production's semantic plan. | P3 | Offline behavior | Separate contract validation from asset resolution/render readiness. |
| 3 | Review proxies and delivery masters need independent receipts because cache-backed intermediates do not establish publication quality. | P3 | Export | Preserve exact output profile, hashes, disclosure, and verified inputs. |

### Story Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | REEL should resolve opaque semantic tokens supplied by a domain profile without interpreting “hockey.” | P2 | Domain boundary | Keep token strings and selector dimensions generic; domain meaning stays upstream. |
| 2 | Layer reuse is valuable only if action/contact/response causality remains explicit in choreography. | P3 | Narrative causality | Keep prop handoffs, reactions, and movement motivation authoritative over pose availability. |
| 3 | A cache hit must never become evidence that a pose is narratively appropriate. | P3 | Review boundary | Report technical resolution and coverage without claiming artistic approval. |

## Synthesis

Roles reviewed: 4  
P1 blockers: 0 | P2 issues: 6 | P3 notes: 6

Verdict: **APPROVED-WITH-CONDITIONS**

Top finding: REEL needs a strict transform/layer order and explicit fallback policy so reusable bodies can be mirrored and skinned without reversing numbers or silently changing actions.

Cross-role consensus: REEL should be able to resolve any domain's opaque pose profile, but must not know what those tokens mean or judge whether they are artistically correct.

## Amendments

1. Define generic library, profile-binding, cast/skin, pose-instance, and path-free cache-receipt schemas with exact cross-file hashes.
2. Specify layer/transform order, anchors, z-order, masking/occlusion, per-keyframe facing, and readable-decals-after-mirroring.
3. Add exact/fallback/unresolved coverage reporting and keep semantic validation available when the configured cache is absent.
