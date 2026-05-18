# Pulse 12: Adapter dependency policy

## Outcome

Made adapter dependency policy explicit across planning surfaces.

## Changes

- Added `dependency_policy` to adapter descriptors and adapter-plan entries.
- Included dependency policy in text and JSON output for `adapters` and
  `adapter-plan`.
- Included dependency policy in review-pack adapter summaries.
- Documented that policy output keeps binary, SDK, credential, endpoint, and
  provider requirements visible.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- adapters
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- smoke
cargo run --quiet -- review-all works
git diff --check
```
