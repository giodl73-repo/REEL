# Pulse 06: Blender and AI-video stubs

## Outcome

Added planned Blender and AI-video adapter stubs.

## Changes

- Added `src/adapters/blender.rs` with a CLI/Python file-boundary plan.
- Added `src/adapters/ai_video.rs` with a provider-neutral package-boundary
  plan.
- Reported both adapters from `cargo run -- adapters`.
- Kept Blender binaries, provider SDKs, credentials, endpoints, and model names
  out of the baseline contract.

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
