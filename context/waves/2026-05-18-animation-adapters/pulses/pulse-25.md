# Pulse 25: Artifact manifest JSON

## Outcome

Added a machine-readable artifact manifest for generated baseline renders.

## Changes

- Added `cargo run -- artifact-manifest <manifest>`.
- Renders shot-card videos, contact sheets, scene previews, and full work
  previews for every export platform.
- Writes `renders\artifacts\<work>-artifacts.json`.
- Records artifact paths, video durations, platform dimensions, adapter identity,
  and per-scene preview coverage.
- Kept FFmpeg as the only implemented render adapter.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- artifact-manifest works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- work-preview works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run --quiet -- review-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- demo works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
cargo run --quiet -- review-all works
git diff --check
```
