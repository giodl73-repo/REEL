# Pulse 28: Artifact manifest byte sizes

## Outcome

Added file byte sizes to the generated artifact manifest.

## Changes

- Added `bytes` to each artifact video entry.
- Added `bytes` to each artifact image entry.
- Preserved existing path, duration, platform, and scene-preview fields.
- Kept generated artifacts under gitignored `renders\`.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- artifact-manifest works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- review-all works
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
git diff --check
```
