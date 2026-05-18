# Wave: Animation adapters

## Goal

Add a Rust adapter boundary for rendering REEL manifests, keep FFmpeg as the
deterministic baseline adapter, and prepare Remotion, Blender, and AI-video
adapter contracts without provider lock-in.

## Pulse table

| Pulse | Title | Status | Outcome |
|------:|---|---|---|
| 01 | Adapter boundary scaffold | done | Added Rust adapter descriptors and render-operation types while preserving existing FFmpeg CLI behavior. |

## Success criteria

- REEL has an explicit Rust adapter boundary for render operations.
- FFmpeg is represented as the implemented baseline adapter.
- Existing smoke, shot-card, contact-sheet, review-pack, and review-all commands
  keep their current behavior.
- Remotion, Blender, and AI-video remain planned provider-neutral adapters until
  a later pulse chooses a concrete integration.
- Provider SDKs, credentials, and binary dependencies are not required by the
  adapter boundary.
