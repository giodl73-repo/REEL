# Pulse 29: Platform and export field validation

## Goal

Make Rust enforce the documented platform and export entry shapes before
rendering.

## Changes

- Added Rust validation for required `platforms[]` fields.
- Added Rust validation for required `exports[]` fields.
- Added regression tests for missing platform and export fields.
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
