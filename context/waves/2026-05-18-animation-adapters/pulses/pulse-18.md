# Pulse 18: Remotion scene handoff refinement

## Outcome

Made the Remotion handoff scene-aware without adopting Node execution.

## Changes

- Extended `cargo run -- remotion-pack <manifest> <platform> <scene-id>`.
- Kept the old default path usable by defaulting the scene id to `scene-01`.
- Added scene metadata, render dimensions, render duration, fps/frame hints, and
  scene shot subset to `props.json`.
- Included caption, action, camera, narration, and visual-prompt tracks for the
  selected scene.
- Kept Remotion as a planned handoff package; no Node, npm, browser runtime, or
  provider dependency is executed.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- remotion-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo scene-01
cargo run --quiet -- scene-preview works\0001-ash-vale-last-road-before-winter\manifest.yaml scene-01 youtube-demo
cargo run --quiet -- scene-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml scene-01 youtube-demo
cargo run --quiet -- demo works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
cargo run --quiet -- review-all works
git diff --check
```
