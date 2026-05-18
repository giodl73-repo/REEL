# Pulse 18: Rust shot-card orchestration

## Goal

Move shot-card MP4 generation into Rust while keeping FFmpeg as the external
media engine.

## Changes

- Added `reel shot-cards <manifest> <platform>`.
- Moved shot-card text extraction, wrapping, scene colors, duration scaling,
  temporary clip generation, and concat orchestration into Rust.
- Updated contact sheets and review packs to call Rust shot-card rendering.
- Removed `scripts/render-shot-cards.sh`.
- Updated WSL setup docs and wave status.

## Validation

- `cargo test`
- `cargo run -- shot-cards works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo`
- `cargo run -- shot-cards works\0001-ash-vale-last-road-before-winter\manifest.yaml iphone-social`
- `cargo run -- contact-sheet works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo`
- `cargo run -- review-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml`
- `cargo run -- review-all works`
- `git diff --check`

## Status

Done.
