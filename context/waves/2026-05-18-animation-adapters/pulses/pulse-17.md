# Pulse 17: Demo scene preview integration

## Outcome

Added scene-preview playback to the browser demo.

## Changes

- `cargo run -- demo <manifest>` now renders the first manifest scene preview
  for each export platform.
- Demo HTML embeds each FFmpeg baseline scene preview as a playable video.
- Demo labels the preview as "not final art" and keeps artifacts under
  gitignored `renders\`.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- demo works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- scene-preview works\0001-ash-vale-last-road-before-winter\manifest.yaml scene-01 youtube-demo
cargo run --quiet -- scene-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml scene-01 youtube-demo
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
cargo run --quiet -- remotion-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run --quiet -- review-all works
git diff --check
```
