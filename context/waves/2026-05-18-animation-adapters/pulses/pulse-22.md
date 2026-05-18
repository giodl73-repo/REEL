# Pulse 22: Review-pack scene previews

## Outcome

Added all scene-preview artifacts to review packs.

## Changes

- `cargo run -- review-pack <manifest>` now renders every scene preview for every
  export platform.
- Review-pack markdown includes a scene-preview table with platform, scene id,
  MP4 path, and render duration.
- `cargo run -- review-all works` inherits the same artifact coverage.
- Kept all media under gitignored `renders\` and FFmpeg as the only implemented
  render adapter.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- review-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- review-all works
cargo run --quiet -- demo works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- scene-previews works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
git diff --check
```
