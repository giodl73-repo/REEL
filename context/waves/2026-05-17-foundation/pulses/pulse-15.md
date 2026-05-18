# Pulse 15: Rust CLI orchestration core

## Goal

Move REEL's durable contracts and orchestration into Rust while keeping FFmpeg as
an external renderer dependency instead of rewriting video tooling.

## Changes

- Initialized a Rust binary crate for REEL.
- Added `reel validate` for typed manifest validation.
- Added `reel plan` for manifest-derived export planning.
- Added `reel review-pack` and `reel review-all` commands that delegate media
  rendering to the existing FFmpeg-backed scripts.
- Added typed tests for the Ash Vale manifest and export plan.
- Updated README, product plan, WSL setup docs, and wave status to make Rust the
  orchestration layer.

## Validation

- `cargo test`
- `cargo run -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml`
- `cargo run -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml`
- `cargo run -- review-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml`
- `cargo run -- review-all works`
- `git diff --check`

## Status

Done.
