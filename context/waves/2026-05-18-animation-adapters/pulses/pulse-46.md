# Pulse 46: Review-all JSON index timestamp

## Outcome

Made review-all JSON report when the index itself was generated.

## Changes

- Added top-level `generated_unix` to `cargo run -- review-all <works-root>
  --output json`.
- Reused the same timestamp for the Markdown index header so text and JSON agree.
- README now documents the index generation timestamp in review-all JSON output.

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
