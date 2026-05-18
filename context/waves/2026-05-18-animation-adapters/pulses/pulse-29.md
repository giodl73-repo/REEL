# Pulse 29: Artifact manifest verification

## Outcome

Added a verification command for generated artifact manifests.

## Changes

- Added `cargo run -- artifact-check <artifact-json>`.
- Verifies that referenced video and image files exist.
- Verifies stored byte sizes match the files on disk.
- Verifies video durations are positive.
- Reports platform count, checked file count, and total bytes.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- artifact-manifest works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- artifact-check renders\artifacts\0001-ash-vale-last-road-before-winter-artifacts.json
cargo run --quiet -- review-all works
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
git diff --check
```
