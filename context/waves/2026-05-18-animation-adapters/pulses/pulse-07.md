# Pulse 07: Review-pack adapter summary

## Outcome

Added adapter status to review-pack reports.

## Changes

- Review packs now include an adapter summary table.
- FFmpeg is shown as the implemented baseline render adapter.
- Remotion, Blender, and AI-video are shown as planned adapters with their
  boundaries and operation coverage.
- Rendered media outputs remain FFmpeg-only in this wave.

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
