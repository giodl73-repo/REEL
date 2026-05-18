# Pulse 16: Rust review-pack orchestration

## Goal

Move review-pack report and batch-index generation out of Bash and into Rust,
while keeping FFmpeg as the external media renderer.

## Changes

- Updated `reel review-pack` to validate the manifest, call only the media
  adapter scripts for MP4/PNG generation, probe durations, and write the Markdown
  review report directly in Rust.
- Updated `reel review-all` to discover `works/*/manifest.yaml`, render each
  review pack, and write `renders/review-packs/INDEX.md` directly in Rust.
- Removed the Bash review-pack orchestration scripts.
- Updated WSL setup docs and wave status to clarify that Bash is now adapter
  glue, not the review-pack control plane.

## Validation

- `cargo test`
- `cargo run -- review-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml`
- `cargo run -- review-all works`
- `git diff --check`

## Status

Done.
