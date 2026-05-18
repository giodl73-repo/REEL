# Pulse 04: Adapter metadata contract

## Outcome

Added optional adapter metadata to the manifest contract without provider
lock-in.

## Changes

- Documented optional `renderer_assumptions.adapters`.
- Added validation that optional adapter ids are known provider-neutral adapter
  boundaries.
- Updated the starter and Ash Vale manifests to name plausible adapter
  boundaries.
- Kept credentials, API endpoints, model names, SDK packages, and binary
  installation details out of the manifest contract.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- adapters
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- smoke
cargo run --quiet -- review-all works
git diff --check
```
