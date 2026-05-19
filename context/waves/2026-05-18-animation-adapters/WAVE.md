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
| 15 | Scene export planning | done | Added `cargo run -- scene-plan <manifest> <scene-id> <platform>` to derive scene shot subsets, timing, dimensions, and scaled render duration. |
| 16 | FFmpeg scene preview render | done | Added `cargo run -- scene-preview <manifest> <scene-id> <platform>` to render a playable scene MP4 through the FFmpeg baseline adapter. |
| 17 | Demo scene preview integration | done | Added FFmpeg baseline scene-preview players to `cargo run -- demo <manifest>` output with not-final-art labeling. |
| 18 | Remotion scene handoff refinement | done | Extended Remotion handoff props with scene timing, shot subset, frame hints, and caption/action/narration tracks without Node execution. |
| 19 | Full scene acceptance pass | done | Generated and validated the scene plan, FFmpeg scene preview, browser demo, and Remotion scene handoff for `scene-01`. |
| 20 | Batch scene preview rendering | done | Added `cargo run -- scene-previews <manifest> <platform>` to render every manifest scene preview through the FFmpeg baseline adapter. |
| 21 | Demo all-scene previews | done | Updated `cargo run -- demo <manifest>` to render and embed every manifest scene preview for each export platform. |
| 22 | Review-pack scene previews | done | Added a scene-preview table to review packs and render-all review output so reviewers can inspect every baseline scene MP4. |
| 23 | Full work preview render | done | Added `cargo run -- work-preview <manifest> <platform>` to concatenate all scene previews into one continuous platform MP4. |
| 24 | Work preview review surfaces | done | Added full work-preview MP4s to demo and review-pack outputs for each export platform. |
| 25 | Artifact manifest JSON | done | Added `cargo run -- artifact-manifest <manifest>` to render baseline artifacts and write machine-readable paths, durations, and scene coverage. |
| 26 | Artifact manifest review links | done | Linked generated artifact manifests from review-pack and browser-demo outputs so humans and automation share the same artifact index. |
| 27 | Review-all artifact index | done | Added artifact-manifest links to `cargo run -- review-all <works-root>` output beside each review pack. |
| 28 | Artifact manifest byte sizes | done | Added file byte sizes to artifact-manifest video and image entries so automation can detect missing or truncated render outputs. |
| 29 | Artifact manifest verification | done | Added `cargo run -- artifact-check <artifact-json>` to verify artifact files, byte sizes, and positive video durations. |
| 30 | Artifact-check JSON output | done | Added `--output json` to `artifact-check` for machine-readable verification summaries. |
| 31 | Artifact manifest schema version | done | Added `schema_version` to artifact manifests and required the supported version during `artifact-check`. |
| 32 | Artifact-manifest JSON output | done | Added `--output json` to `artifact-manifest` so automation can read the generated artifact manifest from stdout. |
| 33 | Artifact-check identity fields | done | Added work, title, and baseline adapter fields to artifact-check text and JSON reports for downstream routing. |
| 34 | Batch artifact verification | done | Added `cargo run -- artifact-check-all <works-root>` to generate and verify artifact manifests for every work under a root. |
| 35 | Review-all verification summary | done | Added artifact-check file and byte counts to the `review-all` index for each generated work artifact manifest. |
| 36 | Review-all JSON output | done | Added `--output json` to `review-all` with index, review-pack, artifact-manifest, and artifact-check summaries. |
| 37 | Artifact SHA-256 verification | done | Added SHA-256 digests to artifact manifests and enforced digest checks in artifact verification. |
| 38 | Artifact coverage verification | done | Added artifact checks for source manifest identity, export platforms, dimensions, scene ids, and scene durations. |
| 39 | Artifact platform duration verification | done | Added artifact checks that shot-card and work-preview durations match each source export duration. |
| 40 | Review-all aggregate totals | done | Added aggregate verified works, scene previews, files, and bytes to the review-all Markdown index. |
| 41 | Artifact-check text identity | done | Added schema version and source manifest paths to artifact-check and artifact-check-all text reports. |
| 42 | Artifact-check generation timestamp | done | Added the artifact manifest `generated_unix` timestamp to artifact-check JSON and text reports. |
| 43 | Review-all generated timestamps | done | Added each artifact manifest generation timestamp to the review-all Markdown index table. |
| 44 | Artifact-check verification timestamp | done | Added a `checked_unix` timestamp to artifact-check JSON and text reports. |

## Success criteria

- REEL has an explicit Rust adapter boundary for render operations.
- FFmpeg is represented as the implemented baseline adapter.
- Existing smoke, shot-card, contact-sheet, review-pack, and review-all commands
  keep their current behavior.
- Remotion, Blender, and AI-video remain planned provider-neutral adapters until
  a later pulse chooses a concrete integration.
- Provider SDKs, credentials, and binary dependencies are not required by the
  adapter boundary.
