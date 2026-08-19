---
skill: roles-check
topic: sprite-render-locality-v0.2.35
date: 2026-08-18
roles_used: 5
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL v0.2.35 sprite render-locality roles check

## Artifact identification

- Type: renderer code, artifact-lineage contract, tests, and production proof
- Scope: shot-local deduplication of repeated raster inputs in the FFmpeg still-animatic adapter
- Evidence: `src/adapters/still_animatic.rs`, its unit tests, the full Rust test suite, and the Karts V8/V9 720p proof artifacts
- Human boundary: this review verifies organization, continuity, and technical evidence. It does not approve the animation as art or promote a Karts review candidate.

## Role selection

- Animation Director: pose timing, asset identity, interpolation, and visual continuity are directly affected.
- Editor: branch-local trims must preserve cut and action timing after input reuse.
- Platform and Audience: render feasibility and output conformance determine whether dense sprite work can reach review devices.
- Story Director: input reuse must not reorder beats or change the scenario.
- Sound Designer: picture optimization must remain isolated from manifest-owned sound and synchronization.

## Animation Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Canonical-path grouping reuses only byte-identical raster sources; logical pose occurrences remain separate. | P3 | shot-local asset cache | Keep canonicalization and occurrence provenance coupled in future refactors. |
| 2 | Per-occurrence scale, rotation, fade, position, and z-order remain downstream of the split, so reused pixels do not imply reused performance state. | P3 | FFmpeg branch construction | Retain branch-local transforms as an invariant test. |
| 3 | Karts V8b and v0.2.35 V8c produced the same output hash, strong evidence that locality optimization preserved the reviewed picture. | P3 | production proof | Preserve this fixture as a regression benchmark. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Every split branch receives its own `trim=start:end`, preventing a shared loop input from leaking across editorial intervals. | P3 | filter graph | Keep explicit trim assertions in unit coverage. |
| 2 | Logical input records remain occurrence-based, preserving the ability to diagnose an exact beat despite physical input reuse. | P3 | artifact inputs | Do not collapse lineage records to unique assets. |
| 3 | V9 action-accent timing is a creative manifest change, not a renderer side effect; the renderer therefore preserves editorial ownership. | P3 | Karts V9 proof | Require human watch before promoting V9 over the current canonical candidate. |

## Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | The stress proof reduced 488 sprite occurrences to 27 unique sprite inputs, making dense limited animation practical on the current workstation. | P3 | locality report | Track both counts in performance regressions. |
| 2 | The checked proof conforms to H.264, yuv420p, 1280x720, 24 fps, and exactly 15 seconds. | P3 | animatic-check | Add other delivery sizes only when a customer manifest requests them. |
| 3 | The new report fields expose optimization effectiveness without local paths or private media identifiers. | P3 | artifact lineage | Keep shared receipts path-free. |

## Story Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Deduplication is shot-local and cannot reorder scenes or shots. | P3 | cache lifetime | Resist cross-shot caching unless ordering and lifetime semantics are separately specified. |
| 2 | Occurrence indices maintain the original action-beat order after inputs are grouped by asset. | P3 | `input_use_index` | Add a multi-shot ordering test before broadening cache scope. |
| 3 | The Karts proof retains the same 15-second scenario and beat markers, so the performance fix does not rewrite canon. | P3 | production proof | Continue to keep choreography decisions in IceLines/Karts-side manifests. |

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Sprite raster reuse changes only picture inputs; audio events and mix assembly are untouched. | P3 | adapter boundary | Keep picture-input caching isolated from audio graph construction. |
| 2 | The proof intentionally has no audio stream, accurately matching its silent review manifest. | P3 | animatic-check | Do not infer audiovisual synchronization approval from this silent fixture. |
| 3 | Artifact checks still report audio-stream and caption counts, guarding against accidental stream introduction. | P3 | receipt validation | Add a scored sprite fixture if future changes share timing utilities with audio. |

## Synthesis

Roles reviewed: 5  
P1 blockers: 0 | P2 issues: 0 | P3 notes: 15

Verdict: **APPROVED-WITH-CONDITIONS**

Top finding: branch-local trims and transforms preserve all 488 logical performances while only 27 raster inputs are opened.

Cross-role consensus: renderer optimization must remain invisible to timing, story order, visual state, and provenance; current tests and the hash-identical V8 proof support that claim.

Condition: a human must watch the V9 action-accent proof before it replaces any canonical Karts artifact. A future renderer change that touches shared timing or audio needs a scored synchronization fixture.

## Amendments

1. Retain the production stress proof and unique-versus-occurrence counters as a performance regression benchmark.
2. Add a multi-shot ordering regression before expanding the cache beyond shot scope.
3. Add a scored sprite fixture if later work shares cached timing logic with audio rendering.
