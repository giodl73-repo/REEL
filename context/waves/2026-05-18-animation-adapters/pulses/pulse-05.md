# Pulse 05: Remotion adapter stub

## Outcome

Added a provider-neutral Remotion adapter planning stub.

## Changes

- Added `src/adapters/remotion.rs`.
- Described Remotion as a planned Node/Remotion project boundary.
- Added a deterministic command-shape planner that records manifest path,
  output directory, and platform/export id without executing anything.
- Kept Node, Remotion packages, browser capture, and project scaffolding out of
  baseline REEL dependencies.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- adapters
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- smoke
cargo run --quiet -- review-all works
git diff --check
```
