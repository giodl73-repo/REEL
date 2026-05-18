# Pulse 24: Scene span validation

## Goal

Ensure shot-to-scene references are temporal, not just nominal.

## Changes

- Added Rust validation that computes scene timeline spans from ordered scene
  durations.
- Rejected shots whose start/end range falls outside the referenced scene span.
- Added a regression test for a shot assigned to the wrong scene.
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
