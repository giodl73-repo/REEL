# Pulse 52: Artifact schema totals

## Outcome

Added aggregate verified artifact schema versions to batch artifact and review
reports.

## Changes

- `artifact-check-all` JSON now includes `schema_versions`, a sorted list of
  verified artifact schema versions across checked works.
- `artifact-check-all` text output now includes `schemas=<versions>` beside works
  and artifact totals.
- `review-all` JSON now includes aggregate `schema_versions`.
- `review-all` Markdown now includes artifact schemas in aggregate verification
  totals.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- artifact-manifest works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- artifact-check renders\artifacts\0001-ash-vale-last-road-before-winter-artifacts.json
cargo run --quiet -- artifact-check renders\artifacts\0001-ash-vale-last-road-before-winter-artifacts.json --output json
cargo run --quiet -- artifact-check-all works
cargo run --quiet -- artifact-check-all works --output json
cargo run --quiet -- review-all works
cargo run --quiet -- review-all works --output json
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
git diff --check
```
