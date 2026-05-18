# Pulse 23: Full work preview render

## Outcome

Added a continuous full-work preview render path.

## Changes

- Added `cargo run -- work-preview <manifest> <platform>`.
- Reused `scene-previews` output and concatenated all scene previews in manifest
  order.
- Wrote full preview MP4s to `renders\work-previews\`.
- Kept the full preview under the FFmpeg baseline adapter path; no Remotion,
  Blender, Node, provider SDK, or credential dependency is required.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- work-preview works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run --quiet -- scene-previews works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run --quiet -- review-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml
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
