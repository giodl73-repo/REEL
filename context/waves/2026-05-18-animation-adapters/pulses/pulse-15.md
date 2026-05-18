# Pulse 15: Scene export planning

## Outcome

Added scene-level export planning.

## Changes

- Added `cargo run -- scene-plan <manifest> <scene-id> <platform>`.
- Derived scene start, duration, platform dimensions, scaled render duration,
  and shot-level source/render timing.
- Rejected unknown scene ids and unknown platforms.
- Kept rendering behavior unchanged; this pulse plans the scene before preview
  rendering.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- scene-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml scene-01 youtube-demo
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
cargo run --quiet -- demo works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- remotion-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run --quiet -- review-all works
git diff --check
```
