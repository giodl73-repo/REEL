# Pulse 24: Work preview review surfaces

## Outcome

Surfaced full work-preview MP4s in the review surfaces.

## Changes

- `cargo run -- review-pack <manifest>` now renders and lists each platform's
  full work-preview MP4.
- `cargo run -- demo <manifest>` now embeds a full work-preview player before
  the per-scene preview list for each export platform.
- Kept the artifacts under gitignored `renders\work-previews\`.
- Kept FFmpeg as the only implemented render adapter.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- work-preview works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run --quiet -- review-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- demo works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- review-all works
cargo run --quiet -- scene-previews works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
git diff --check
```
