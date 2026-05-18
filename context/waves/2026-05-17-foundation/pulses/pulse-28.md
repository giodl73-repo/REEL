# Pulse 28: Top-level value validation

## Goal

Make the Rust validator reject invalid top-level manifest identity values before
rendering.

## Changes

- Added validation for the supported `manifest_version`.
- Added validation that top-level scalar fields are strings and not empty.
- Added regression tests for empty titles and unsupported manifest versions.
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
