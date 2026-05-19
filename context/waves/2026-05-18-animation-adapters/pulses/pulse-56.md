# Pulse 56: Artifact manifest totals

## Outcome

Added aggregate generated artifact manifest paths to batch artifact and review
reports.

## Changes

- `artifact-check-all` JSON now includes `artifact_manifests`, a sorted list of
  generated artifact manifest paths across checked works.
- `artifact-check-all` text output now includes `artifact_manifests=<count>`
  beside work ids, titles, source count, and artifact totals.
- `review-all` JSON now includes aggregate `artifact_manifests`.
- `review-all` Markdown now includes artifact manifest paths in aggregate
  verification totals.

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
