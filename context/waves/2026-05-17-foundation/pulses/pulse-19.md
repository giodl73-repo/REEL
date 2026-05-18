# Pulse 19: Rust validation canonical path

## Goal

Make Rust manifest validation the only documented pre-render validation gate.

## Changes

- Removed the legacy `scripts/validate-manifest.sh` Bash parser.
- Updated manifest and WSL setup docs to use `cargo run -- validate`.
- Updated the foundation wave record to mark Rust validation as canonical.

## Validation

- `cargo test --quiet`
- `cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml`
- `cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml`
- `cargo run --quiet -- review-all works`
- `git diff --check`

## Status

Done.
