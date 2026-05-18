# Pulse 27: Review-all artifact index

## Outcome

Added artifact-manifest links to the portfolio-style review index.

## Changes

- `cargo run -- review-all <works-root>` now links both each work's review pack
  and its artifact manifest JSON.
- Kept artifact manifests under gitignored `renders\artifacts\`.
- Preserved the repo-local review-pack records as the execution history.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- review-all works
cargo run --quiet -- artifact-manifest works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- review-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
git diff --check
```
