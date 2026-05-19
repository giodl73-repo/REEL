# Pulse 47: Review-all batch checked timestamp

## Outcome

Added a top-level review-all verification timestamp.

## Changes

- Added top-level `checked_unix` to `cargo run -- review-all <works-root>
  --output json`.
- Added `Checked unix` to the Markdown index verification totals.
- README now documents index generation and verification timestamps in
  review-all JSON output.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- review-all works
cargo run --quiet -- review-all works --output json
cargo run --quiet -- artifact-check-all works
cargo run --quiet -- artifact-check renders\artifacts\0001-ash-vale-last-road-before-winter-artifacts.json --output json
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
git diff --check
```
