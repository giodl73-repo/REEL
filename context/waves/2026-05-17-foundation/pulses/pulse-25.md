# Pulse 25: Nested manifest field validation

## Goal

Make Rust enforce the documented required scene and shot fields, not just the
fields currently needed by draft renderers.

## Changes

- Added Rust validation for required `scenes[]` fields.
- Added Rust validation for required `shots[]` fields.
- Added regression tests for missing scene and shot contract fields.
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
