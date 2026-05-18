# Pulse 08: Manifest-aware adapter planning

## Outcome

Added manifest-aware adapter planning.

## Changes

- Added `cargo run -- adapter-plan <manifest>`.
- The command validates the manifest and reports each known adapter with a
  `declared` flag based on `renderer_assumptions.adapters`.
- Review-pack adapter summaries now include whether each adapter is declared by
  the manifest.
- Planned adapters remain read-only metadata; only FFmpeg renders media.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- adapters
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- smoke
cargo run --quiet -- review-all works
git diff --check
```
