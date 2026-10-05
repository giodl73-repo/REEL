# Motioncraft execution checkpoint

## State

Goal: execute the full reviewed Motioncraft plan, including scene-engine
consumption and CAIMITOS new/existing episode adoption. Status: active;
no completion claim. This first implementation checkpoint changes authoritative
code and establishes the current-engine baseline.

Worktree: `C:/Users/giodl/.codex/worktrees/motioncraft/REEL`.
Branch: `codex/motioncraft`. Baseline: `4effab1`.

## Verified progress

- Imported the reviewed v4 plan and its twelve role finding dispositions into
  the current REEL codebase.
- Added `reel-assembly::motioncraft` with strict authored phases, bounded
  geometry/curves, explicit native/working/delivery clocks, optional static
  elements, reduced motion, and review-frame schedules.
- Verified five focused tests and all ten assembly library unit tests with
  `cargo test -p reel-assembly --lib`. These checks prove compiler behavior
  only; they do not prove scene execution or CAIMITOS adoption.
- Current dependencies are locally usable: Cargo, FFmpeg, FFprobe and Python
  are available. No media rendered yet.
- Recorded reuse/gap map in `docs/motioncraft-reuse-map.md`.

## Next bounded implementation

Audit how `resolve_scene` fingerprints selected inputs and language event
spans. Attach optional motion intent to episode/scene authoring without changing
legacy serialized output. Resolve episode defaults, scene overrides and stable
shot/event overrides explicitly. Compile native event spans into motion plans
in `scene_delivery_compile`; bind evidence to exact immutable input hashes.
Extend `scene_delivery::PictureMotion` and its validation/render/check paths
to consume the phased plan rather than handing a detached sidecar to users.
Use real synthetic scene-render tests before claiming slice B.

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

Authoring integration, selected-graph/hash binding, actual scene renderer,
review extraction, queue comparison and quality scores, consumer changes,
upgrade/rollback, hydrated production package, and full final audit are all
still required. No human review, approval, or viewer comprehension is claimed.
