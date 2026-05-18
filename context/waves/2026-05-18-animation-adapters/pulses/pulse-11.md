# Pulse 11: Adapter catalog JSON output

## Outcome

Added machine-readable adapter catalog output.

## Changes

- Added `cargo run -- adapters --output json`.
- Kept text output as the default for humans.
- Reused the adapter descriptor model for JSON serialization.
- Kept provider integrations unchanged; this is a planning/catalog surface only.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- adapters
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- smoke
cargo run --quiet -- review-all works
git diff --check
```
