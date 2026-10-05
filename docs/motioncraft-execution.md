# Motioncraft execution checkpoint

## State

Goal: execute the full reviewed Motioncraft plan, including scene-engine
consumption and CAIMITOS new/existing episode adoption. Status: active;
no completion claim. The compiler, authoring integration, actual scene camera
execution and indexed evidence are implemented; the complete goal is not done.

Worktree: `C:/Users/giodl/.codex/worktrees/motioncraft/REEL`.
Branch: `codex/motioncraft`. Baseline: `4effab1`.

## Verified progress

- Imported the reviewed v4 plan and its twelve role finding dispositions into
  the current REEL codebase.
- Added `reel-assembly::motioncraft` with strict authored phases, bounded
  geometry/curves, explicit native/working/delivery clocks, optional static
  elements, reduced motion, and review-frame schedules.
- Verified the full `reel-assembly` suite: 42 tests across unit, authored-trigger,
  graph-contract and text-trigger checks.
- Verified 19 scene-authoring tests and 18 scene-delivery tests including the
  ignored FFmpeg checks. Tests cover inherited direction, language/event scope,
  fingerprints, native timing, rendered hold/push pixels, reduced motion,
  unchanged audio bytes and indexed review frames.
- Verified all three scene-build CLI tests. The independent-scene test upgrades
  episode direction, rejects its old job, compiles selected delivery, builds
  through the actual scene engine and restores the old authoring revision.
- The latest input-snapshot provenance fix passes the same independent-scene
  integration test. Formatting and diff checks pass.
- Current dependencies are locally usable: Cargo, FFmpeg, FFprobe and Python
  are available. Synthetic FFmpeg renders have passed; retained queue-comparison
  and consumer-package artifacts have not yet been produced.
- Recorded reuse/gap map in `docs/motioncraft-reuse-map.md`.

## Implemented handoff

Episode default direction and scene defaults/language-event overrides resolve
with whole-object precedence. They affect only consuming language fingerprints;
absent direction preserves legacy fingerprints. The selected native scene
compiler emits phased camera plans into actual delivery jobs, preserving native
sample clocks. Scene build compares authored direction with the selected job;
the scene engine validates the plan against native spans and allocated frames.

The renderer uses bounded fractional perspective sampling. Directed scene
outputs automatically include exact indexed PNGs, phase/attachment metadata,
contact sheet and silent quarter-speed playback. Receipt/check consumes all
additional artifacts; PNG pixels are independently compared with indexed
decoding of the rendered picture. The test also compares an extracted PNG with
a separate full-video decode, avoiding a shared-indexing false positive.

Compiler receipts now hash the exact parsed authoring/profile/graph/alignment-
manifest buffers. Native alignment bytes remain checked against scoped selected
asset hashes. Jobs likewise use the exact parsed job buffer for plan identity.

See `docs/motioncraft-authoring.md` for actual fields, supported treatment,
commands, exact-duration defaults and upgrade/rollback procedure.

## Next bounded implementation

Finish visual-direction/profile support and protected geometry validation.
Resolve reusable episode defaults for differently timed native shots explicitly
rather than silently stretching narration. Then produce the persistent matched
queue-explainer variants, review real images/playback, and record the quality
comparison. Complete consumer integration when its lane is established.

The initial direction type currently describes bounded zoom and geometric
review intent. Other element motion and typography need explicit execution
contracts before the queue-explainer proof; unsupported intent must fail.

## Pending consumer lane

Read-only CAIMITOS diagnostics found `production-engineering` absent from the
current registry and `animation-vfx` gated as requiring a dedicated worktree.
Requested a lane assignment from the user asynchronously. No CAIMITOS code or
registry has been changed. This affects consumer writes only; REEL work can
continue. Do not classify the whole goal as blocked while useful REEL work
remains.

## Outstanding acceptance

Remaining: complete immutable intent/selected-revision binding audit,
typography/palette and delivery-profile direction, protected geometry,
retained queue comparison and quality evaluation, CAIMITOS adapter adoption,
consumer new/upgrade/rollback canary, cache hydration and verified package,
and requirement-by-requirement final audit. No human review, approval or
viewer comprehension is claimed.
