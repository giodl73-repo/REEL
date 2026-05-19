# Pulse 34: Batch artifact verification

## Outcome

Added batch artifact-manifest generation and verification.

## Changes

- Added `cargo run -- artifact-check-all <works-root>`.
- Added `--output json` for machine-readable batch verification summaries.
- For each work manifest under the root, the command generates the artifact
  manifest and runs the same file/byte/duration checks as `artifact-check`.
- Reports aggregate work count, checked file count, total bytes, and per-work
  check reports.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- artifact-check-all works
cargo run --quiet -- artifact-check-all works --output json
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
