# Pulse 19: Full scene acceptance pass

## Outcome

Completed the first full-scene acceptance pass for `scene-01`.

## Artifacts

- Scene plan:
  `cargo run -- scene-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml scene-01 youtube-demo`
- FFmpeg baseline scene preview:
  `renders\scene-previews\0001-ash-vale-last-road-before-winter-scene-01-youtube-demo-preview.mp4`
- Browser demo with embedded scene previews:
  `renders\demo\0001-ash-vale-last-road-before-winter-demo.html`
- Remotion scene handoff package:
  `renders\remotion\0001-ash-vale-last-road-before-winter\youtube-demo`

## Notes

- This is a working baseline preview, not final art.
- FFmpeg remains the implemented adapter.
- Remotion remains a handoff package with scene-focused props; no Node,
  Remotion package, npm install, browser runtime, or provider execution is
  required.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- scene-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml scene-01 youtube-demo
cargo run --quiet -- scene-preview works\0001-ash-vale-last-road-before-winter\manifest.yaml scene-01 youtube-demo
cargo run --quiet -- demo works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- remotion-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo scene-01
cargo run --quiet -- review-all works
git diff --check
```
