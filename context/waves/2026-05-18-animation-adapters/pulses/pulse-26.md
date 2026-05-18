# Pulse 26: Artifact manifest review links

## Outcome

Surfaced the generated artifact manifest in human review outputs.

## Changes

- `cargo run -- review-pack <manifest>` now generates and links the artifact
  manifest JSON near the top of the review pack.
- `cargo run -- demo <manifest>` now links the same artifact manifest next to
  the review-pack link.
- Kept generated JSON under gitignored `renders\artifacts\`.
- Kept FFmpeg as the only implemented render adapter.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- artifact-manifest works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- review-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- demo works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
cargo run --quiet -- review-all works
git diff --check
```
