---
skill: roles-check
topic: reel-selection-lock-audio-cache-v021
date: 2026-08-15
roles_used: 5
p1_count: 0
verdict: APPROVED
---

# REEL v0.2.21 selection-lock and audio-cache role review

## Artifact identification

- Type: Rust CLI/code, artifact contracts, tests, and operator documentation.
- Domain signals: moving-image continuity, edit governance, sound mixing,
  private artifact lineage, local rendering, and platform delivery.
- Reviewed artifacts: `src/selection_lock.rs`, `src/audio_preview.rs`, their CLI
  surfaces in `src/main.rs`, tests, README changes, and the v0.2.21 guide.

## Role selection

- Animation Director: cached picture must preserve the exact selected visual.
- Editor: lock and derivative boundaries govern when an edit may change.
- Sound Designer: audio-only output must match full-render mix semantics.
- Platform and Audience: output formats, duration, privacy, and delivery state matter.
- Story Director: work identity and revision intent must remain legible.

## Animation Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Remux uses H.264 stream-copy, so no frame generation or visual recompression occurs. | P3 | `remux_picture` | Retain stream-copy as a verified lineage field. |
| 2 | The picture artifact is fully rechecked before reuse, including its manifest and visual inputs. | P3 | `remux_picture` | Keep cached-picture reuse explicit rather than automatic. |
| 3 | Remux verification binds both the source picture artifact and final output hashes. | P3 | `check_picture_remux` | Preserve the two-sided verification contract. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Locking no longer mutates the manifest that produced the selected proof. | P3 | `lock_selection` | Use the packet as the immutable edit-selection boundary. |
| 2 | A later change requires an explicit conformed derivative with reason and changed dimensions. | P3 | `derive_planning_manifest` | Keep overwrite refusal and locked-source enforcement. |
| 3 | Picture and mix must agree within 50 ms before remux, preventing accidental pacing drift. | P3 | `remux_picture` | Retain the bounded duration gate. |

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Audio-only compilation reproduces trims, gains, fades, placement, role buses, ducking, and mastering. | P3 | `render_audio_preview` | Keep full-render and preview filter semantics synchronized. |
| 2 | Every audio source, the manifest, audio policy, and output are hash-bound and rechecked. | P3 | audio preview reports | Retain audio-policy hashing as a visible contract. |
| 3 | The preview uses a 192 kb/s AAC review master, while the legacy full render encodes 128 kb/s AAC. | P3 | encoder settings | Document the intentional review-master bitrate rather than claiming byte identity. |

## Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | `.m4a` is required for audio previews and `.mp4` remains the remux delivery container. | P3 | CLI validation | Keep extensions deterministic for operators and players. |
| 2 | Outputs and reports use no-overwrite publication; report failure removes a newly published media file. | P3 | publication paths | Preserve atomic/no-clobber behavior. |
| 3 | Audio and lock reports contain local paths and are private production evidence. | P3 | v0.2.21 guide | Direct external sharing through existing path-free receipt commands. |

## Story Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Picture and audio from different works cannot be remuxed. | P3 | `remux_picture` | Keep work identity as a hard boundary. |
| 2 | Selecting a proof does not silently assert principal or human approval. | P3 | lock lineage | Keep approval separate from mechanical verification. |
| 3 | Planning derivatives record why the story/edit is reopening and which dimensions may change. | P3 | derivative lineage | Require specific, human-readable reasons. |

## Synthesis

Roles reviewed: 5
P1 blockers: 0 | P2 issues: 0 | P3 notes: 15

Verdict: APPROVED

Top finding: proof selection must preserve the source manifest hash instead of
rewriting it after render.

Cross-role consensus: the workflow is safe because reuse and revision are both
explicit, hash-bound operations rather than inferred state changes.

## Amendments applied

1. Removed automatic `principal_approved: true` from lock creation so tooling
   cannot manufacture human approval.
2. Added semantic verification that a locked derivative changes only timing
   lock and lineage governance fields, never production content.
3. Clarified the 192 kb/s preview encoding and private/path-bearing nature of
   local audio evidence in the operator guide.
