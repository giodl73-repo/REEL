# Pulse 43: Review-all generated timestamps

## Outcome

Made review-all Markdown show when each artifact manifest was generated.

## Changes

- Added a `Generated unix` column to `renders\review-packs\INDEX.md`.
- The column is populated from the verified artifact-check report for each work.
- README now notes that review-all includes per-work artifact generation
  timestamps alongside verification counts.

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
