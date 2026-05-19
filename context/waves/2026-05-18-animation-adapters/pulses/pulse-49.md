# Pulse 49: Artifact media counts

## Outcome

Split verified artifact counts into video and image totals for review automation.

## Changes

- `artifact-check` reports now include `video_files` and `image_files` alongside
  total files, bytes, and duration.
- `artifact-check-all` aggregates video and image file counts across works.
- `review-all` JSON and Markdown now surface aggregate video/image counts, and
  the per-work verification table shows the media split.
- Text output for artifact-check commands now includes `videos=<count>` and
  `images=<count>`.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- artifact-manifest works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- artifact-check renders\artifacts\0001-ash-vale-last-road-before-winter-artifacts.json
cargo run --quiet -- artifact-check renders\artifacts\0001-ash-vale-last-road-before-winter-artifacts.json --output json
cargo run --quiet -- artifact-check-all works
cargo run --quiet -- artifact-check-all works --output json
cargo run --quiet -- review-all works
cargo run --quiet -- review-all works --output json
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
git diff --check
```
