# Pulse 30: Artifact-check JSON output

## Outcome

Added machine-readable output for artifact verification.

## Changes

- Extended `cargo run -- artifact-check <artifact-json>` with `--output json`.
- Preserved text output as the default.
- Reused the existing verification report shape for JSON output:
  artifact manifest path, platform count, checked file count, and total bytes.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- artifact-manifest works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- artifact-check renders\artifacts\0001-ash-vale-last-road-before-winter-artifacts.json
cargo run --quiet -- artifact-check renders\artifacts\0001-ash-vale-last-road-before-winter-artifacts.json --output json
cargo run --quiet -- review-all works
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
git diff --check
```
