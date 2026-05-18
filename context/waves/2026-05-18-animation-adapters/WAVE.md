# Wave: Animation adapters

## Goal

Add a Rust adapter boundary for rendering REEL manifests, keep FFmpeg as the
deterministic baseline adapter, and prepare Remotion, Blender, and AI-video
adapter contracts without provider lock-in.

## Pulse table

| Pulse | Title | Status | Outcome |
|------:|---|---|---|
| 01 | Adapter boundary scaffold | done | Added Rust adapter descriptors and render-operation types while preserving existing FFmpeg CLI behavior. |
| 02 | FFmpeg baseline adapter | done | Moved FFmpeg/ffprobe subprocess invocation and WSL path handling behind the baseline adapter module without changing render outputs. |
| 03 | Adapter planning command | done | Added `cargo run -- adapters` to report implemented and planned adapter boundaries without adding provider dependencies. |
| 04 | Adapter metadata contract | done | Documented optional `renderer_assumptions.adapters` metadata and validate known adapter ids without provider-specific fields. |
| 05 | Remotion adapter stub | done | Added a planned Remotion file/project boundary with deterministic command-shape planning and no Node dependency. |
| 06 | Blender and AI-video stubs | done | Added planned Blender CLI/file and AI-video package boundaries without binaries, SDKs, or credentials. |

## Success criteria

- REEL has an explicit Rust adapter boundary for render operations.
- FFmpeg is represented as the implemented baseline adapter.
- Existing smoke, shot-card, contact-sheet, review-pack, and review-all commands
  keep their current behavior.
- Remotion, Blender, and AI-video remain planned provider-neutral adapters until
  a later pulse chooses a concrete integration.
- Provider SDKs, credentials, and binary dependencies are not required by the
  adapter boundary.
