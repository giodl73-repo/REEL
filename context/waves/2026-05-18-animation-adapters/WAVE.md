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
| 07 | Review-pack adapter summary | done | Added adapter summaries to review-pack reports while keeping FFmpeg as the only rendered adapter. |
| 08 | Manifest-aware adapter planning | done | Added `cargo run -- adapter-plan <manifest>` and review-pack declaration flags for manifest-selected adapter boundaries. |
| 09 | Adapter metadata determinism | done | Preserved manifest-declared adapter order and rejected duplicate optional adapter ids for deterministic planning. |
| 10 | Adapter-plan JSON output | done | Added optional `--output json` for machine-readable adapter plans while keeping text output as the default. |
| 11 | Adapter catalog JSON output | done | Added optional `--output json` for the adapter catalog while keeping text output as the default. |
| 12 | Adapter dependency policy | done | Added explicit dependency/provider policy to catalog, adapter-plan, JSON, and review-pack summaries. |
| 13 | Browser demo page | done | Added `cargo run -- demo <manifest>` to render a browser-openable HTML demo from FFmpeg baseline artifacts and adapter summaries. |
| 14 | Remotion handoff package | done | Added `cargo run -- remotion-pack <manifest> <platform>` to write manifest-derived Remotion props and command-shape docs without running Node. |

## Success criteria

- REEL has an explicit Rust adapter boundary for render operations.
- FFmpeg is represented as the implemented baseline adapter.
- Existing smoke, shot-card, contact-sheet, review-pack, and review-all commands
  keep their current behavior.
- Remotion, Blender, and AI-video remain planned provider-neutral adapters until
  a later pulse chooses a concrete integration.
- Provider SDKs, credentials, and binary dependencies are not required by the
  adapter boundary.
