# Pulse 20: Batch scene preview rendering

## Outcome

Added platform-wide scene preview rendering.

## Changes

- Added `cargo run -- scene-previews <manifest> <platform>`.
- Reused the existing scene plan and FFmpeg baseline scene-preview renderer.
- Rendered every manifest scene to `renders\scene-previews\`.
- Kept Remotion, Blender, and AI-video as planned handoff boundaries.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- scene-previews works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run --quiet -- scene-preview works\0001-ash-vale-last-road-before-winter\manifest.yaml scene-01 youtube-demo
cargo run --quiet -- scene-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml scene-01 youtube-demo
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
cargo run --quiet -- demo works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- remotion-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo scene-01
cargo run --quiet -- review-all works
git diff --check
```
