# Pulse 17: Rust contact-sheet orchestration

## Goal

Move contact-sheet PNG generation into Rust while keeping FFmpeg itself as the
external media engine.

## Changes

- Added `reel contact-sheet <manifest> <platform>`.
- Updated review-pack rendering to call Rust contact-sheet orchestration instead
  of the Bash contact-sheet script.
- Kept `scripts/render-shot-cards.sh` as the remaining FFmpeg video-card adapter.
- Removed `scripts/render-contact-sheet.sh`.
- Updated WSL setup docs and wave status.

## Validation

- `cargo test`
- `cargo run -- contact-sheet works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo`
- `cargo run -- contact-sheet works\0001-ash-vale-last-road-before-winter\manifest.yaml iphone-social`
- `cargo run -- review-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml`
- `cargo run -- review-all works`
- `git diff --check`

## Status

Done.
