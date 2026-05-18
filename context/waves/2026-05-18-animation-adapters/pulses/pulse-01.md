# Pulse 01: Adapter boundary scaffold

## Outcome

Added REEL's first Rust adapter boundary scaffold.

## Changes

- Added `src/adapters/` with adapter descriptors for FFmpeg, Remotion, Blender,
  and AI-video.
- Marked FFmpeg as the only implemented baseline adapter.
- Added renderer-neutral render operation types for smoke renders, shot-card
  renders, contact sheets, and review packs.
- Left existing CLI behavior and FFmpeg render paths unchanged.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- smoke
cargo run --quiet -- review-all works
git diff --check
```
