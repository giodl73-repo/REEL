# Pulse 35: Review-all verification summary

## Outcome

Made the generated review-all index show artifact verification results.

## Changes

- `cargo run -- review-all <works-root>` now verifies each generated artifact
  manifest before writing the index row.
- The index table includes checked file count and total bytes per work, making
  review output visibly tied to artifact-check health.
- README notes that `review-all` includes artifact verification counts.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- review-all works
cargo run --quiet -- artifact-check-all works
cargo run --quiet -- artifact-check-all works --output json
cargo run --quiet -- artifact-check renders\artifacts\0001-ash-vale-last-road-before-winter-artifacts.json --output json
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
git diff --check
```
