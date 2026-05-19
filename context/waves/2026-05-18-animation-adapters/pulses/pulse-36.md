# Pulse 36: Review-all JSON output

## Outcome

Added machine-readable `review-all` output for automation.

## Changes

- Added `cargo run -- review-all <works-root> --output json`.
- Added a structured review-all report containing the generated index path,
  per-work review pack path, artifact manifest path, and artifact-check report.
- Kept text output compatible by printing only the generated index path.
- Documented the JSON command in README and the validation contract.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- review-all works
cargo run --quiet -- review-all works --output json
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
