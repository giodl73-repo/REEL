# Pulse 10: Adapter-plan JSON output

## Outcome

Added machine-readable adapter-plan output.

## Changes

- Added `cargo run -- adapter-plan <manifest> --output json`.
- Kept text output as the default.
- Serialized adapter ids, statuses, declared flags, operation names, and
  boundaries through the existing Rust adapter plan model.
- Added `serde_json` only for CLI/report serialization, not for provider
  integration.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- adapters
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- smoke
cargo run --quiet -- review-all works
git diff --check
```
