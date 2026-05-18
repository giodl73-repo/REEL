# Pulse 02: FFmpeg baseline adapter

## Outcome

Moved FFmpeg invocation mechanics behind the baseline adapter boundary.

## Changes

- Added `FfmpegAdapter` as the implemented baseline adapter wrapper.
- Moved FFmpeg/ffprobe subprocess execution, WSL path conversion, shell quoting,
  and concat-path escaping into `src/adapters/ffmpeg.rs`.
- Kept smoke, shot-card, contact-sheet, review-pack, and review-all command
  behavior unchanged.
- Left Remotion, Blender, and AI-video as planned adapters with no dependencies.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- smoke
cargo run --quiet -- shot-cards works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run --quiet -- shot-cards works\0001-ash-vale-last-road-before-winter\manifest.yaml iphone-social
cargo run --quiet -- contact-sheet works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run --quiet -- review-all works
git diff --check
```
