# Pulse 20: Rust smoke orchestration

## Goal

Move the starter smoke MP4 renderer into Rust so REEL no longer depends on Bash
for active FFmpeg adapters.

## Changes

- Added `reel smoke <manifest>`.
- Moved manifest-fed title-card smoke rendering into Rust-owned FFmpeg
  orchestration.
- Removed `scripts/smoke-render.sh`.
- Updated WSL setup docs and the foundation wave record.

## Validation

- `cargo test --quiet`
- `cargo run --quiet -- smoke`
- `cargo run --quiet -- smoke works\0001-ash-vale-last-road-before-winter\manifest.yaml`
- `cargo run --quiet -- review-all works`
- `git diff --check`

## Status

Done.
