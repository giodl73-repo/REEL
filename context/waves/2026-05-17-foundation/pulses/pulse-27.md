# Pulse 27: Required value validation

## Goal

Ensure required manifest fields carry content, not just keys.

## Changes

- Added Rust validation that required metadata fields are not empty strings,
  lists, or maps.
- Added Rust validation that required scene and shot fields are not empty.
- Added regression tests for empty metadata, scene, and shot values.
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
