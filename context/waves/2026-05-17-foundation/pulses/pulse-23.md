# Pulse 23: Manifest identity validation

## Goal

Harden the Rust manifest validator before more render adapters depend on it.

## Changes

- Added validation for non-empty and unique scene, shot, platform, and export
  identifiers.
- Added validation for positive scene and shot durations and non-negative shot
  starts.
- Added validation that every platform has a matching export.
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
