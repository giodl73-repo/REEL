# Pulse 03: Adapter planning command

## Outcome

Added a read-only adapter planning command.

## Changes

- Added `cargo run -- adapters`.
- Reports FFmpeg as `implemented-baseline`.
- Reports Remotion, Blender, and AI-video as `planned`.
- Lists operation coverage for implemented adapters and keeps planned adapters
  dependency-free.
- Updated README and product validation docs with the new command.

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
