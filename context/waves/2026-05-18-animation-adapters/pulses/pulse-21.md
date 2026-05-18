# Pulse 21: Demo all-scene previews

## Outcome

Expanded the browser demo from a first-scene preview to all scene previews.

## Changes

- `cargo run -- demo <manifest>` now renders every manifest scene preview for
  each export platform.
- Demo HTML embeds each scene preview with scene id and render duration.
- Kept generated media under gitignored `renders\`.
- Kept FFmpeg as the only implemented render adapter.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- demo works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- scene-previews works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run --quiet -- scene-preview works\0001-ash-vale-last-road-before-winter\manifest.yaml scene-01 youtube-demo
cargo run --quiet -- scene-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml scene-01 youtube-demo
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- smoke
cargo run --quiet -- remotion-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo scene-01
cargo run --quiet -- review-all works
git diff --check
```
