# Pulse 13: Browser demo page

## Outcome

Added a browser-openable demo page for a REEL manifest.

## Changes

- Added `cargo run -- demo <manifest>`.
- The command writes `renders/demo/<work>-demo.html`.
- The demo includes the manifest metadata, review-pack link, adapter summary,
  FFmpeg baseline MP4s, and contact sheets.
- Generated demo files remain under gitignored `renders/`.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- adapters
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- smoke
cargo run --quiet -- demo works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- review-all works
git diff --check
```
