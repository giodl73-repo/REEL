# Pulse 16: FFmpeg scene preview render

## Outcome

Added the first playable scene preview render path.

## Changes

- Added `cargo run -- scene-preview <manifest> <scene-id> <platform>`.
- Wrote scene previews to `renders\scene-previews\<work>-<scene>-<platform>-preview.mp4`.
- Kept rendering behind the FFmpeg baseline adapter boundary.
- Used the scene plan for dimensions, shot subset, shot timing, and scaled
  render duration.
- Added deterministic visual motion and text treatment without introducing
  Remotion, Blender, Node, SDKs, or provider dependencies.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- scene-preview works\0001-ash-vale-last-road-before-winter\manifest.yaml scene-01 youtube-demo
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
