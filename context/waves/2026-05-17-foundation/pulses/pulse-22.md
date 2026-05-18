# Pulse 22: Valid starter manifest contract

## Goal

Make the starter `scenario-video.yaml` template a valid example of the Rust
manifest contract.

## Changes

- Updated the starter template's scene and shot duration to match the 60-second
  YouTube/demo export baseline.
- Added Rust regression coverage for validating the starter manifest template.
- Updated README, product validation docs, and the foundation wave record to
  include template validation in the standard checks.

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
