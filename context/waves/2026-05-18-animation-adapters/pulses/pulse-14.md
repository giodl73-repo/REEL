# Pulse 14: Remotion handoff package

## Outcome

Added a Remotion handoff package generator.

## Changes

- Added `cargo run -- remotion-pack <manifest> <platform>`.
- Writes `renders/remotion/<work>/<platform>/manifest.yaml`.
- Writes manifest-derived `props.json` for a future Remotion composition.
- Writes `README.md` with the planned command shape and dependency policy.
- Does not install Node, Remotion packages, browser runtimes, or execute a
  Remotion render.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- adapters
cargo run --quiet -- adapters --output json
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- smoke
cargo run --quiet -- demo works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- remotion-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run --quiet -- review-all works
git diff --check
```
