# Pulse 38: Artifact coverage verification

## Outcome

Made artifact verification confirm source-manifest coverage, not just artifact
file integrity.

## Changes

- `artifact-check` now reloads the artifact manifest's source REEL manifest.
- Verification rejects work/title mismatches, non-FFmpeg baseline adapter values,
  missing or extra export platforms, platform aspect/dimension mismatches, scene
  preview count mismatches, scene id mismatches, and scene duration mismatches.
- Artifact-check text and JSON reports include the source manifest path and
  verified scene-preview count.
- Batch artifact and review-all reports aggregate verified scene-preview counts.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- artifact-manifest works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- artifact-check renders\artifacts\0001-ash-vale-last-road-before-winter-artifacts.json
cargo run --quiet -- artifact-check renders\artifacts\0001-ash-vale-last-road-before-winter-artifacts.json --output json
cargo run --quiet -- artifact-check-all works
cargo run --quiet -- artifact-check-all works --output json
cargo run --quiet -- review-all works --output json
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
git diff --check
```
