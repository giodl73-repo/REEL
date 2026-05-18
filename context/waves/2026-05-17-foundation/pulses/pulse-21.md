# Pulse 21: Rust CLI command surface docs

## Goal

Make the public REEL docs match the current Rust-owned command surface.

## Changes

- Updated the README renderer direction to show smoke, validation, planning,
  shot-card, contact-sheet, review-pack, and batch review commands.
- Replaced the old documentation-first validation snippets with the current Rust
  CLI validation sequence.
- Updated the product plan and foundation wave record to remove stale Bash
  adapter wording.

## Validation

- `cargo test --quiet`
- `cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml`
- `cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml`
- `cargo run --quiet -- smoke`
- `cargo run --quiet -- review-all works`
- `git diff --check`

## Status

Done.
