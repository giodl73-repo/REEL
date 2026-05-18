# Pulse 26: Metadata manifest field validation

## Goal

Make Rust enforce the documented required metadata sections, not just scene and
shot fields used by draft renderers.

## Changes

- Added Rust validation for required `source_scenario`, `audience`, `audio`,
  `captions`, `renderer_assumptions`, and `review` fields.
- Added regression tests for missing source and review metadata fields.
- Updated the manifest contract docs and foundation wave record.

## Validation

- `cargo test --quiet`
- `cargo run --quiet -- validate manifests\templates\scenario-video.yaml`
- `cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml`
- `cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml`
- `cargo run --quiet -- smoke`
- `cargo run --quiet -- review-all works`
- `git diff --check`

## Status

Done.
