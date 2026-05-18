# Wave: Foundation studio

## Goal

Create REEL as the portfolio's Design Labs repo for movies and videos, with the
minimum durable structure needed to start making video packages without locking
into a renderer too early.

## Pulse table

| Pulse | Title | Status | Outcome |
|------:|---|---|---|
| 01 | Repo scaffold and intake | done | Created REEL docs, skills, initial rubric, validation contract, and TRACKER intake. |
| 02 | Animation style grammar and roles review | done | Added animation style catalog, Games Design scenario video flow, `.roles` panel, and foundation plan review. |
| 03 | Production manifest sketch | done | Defined the renderer-neutral YAML manifest contract and starter template for scenario videos. |
| 04 | Renderer research | done | Selected WSL2 Ubuntu + FFmpeg as the baseline path, Remotion as the first animation prototype boundary, and Blender/AI-video as deferred style-specific paths. |
| 05 | WSL FFmpeg smoke render | done | Added a WSL setup guide and manifest-fed FFmpeg smoke renderer that writes gitignored MP4 drafts. |
| 06 | First original video package | done | Created the Ash Vale trailer package from BANISH scenario evidence and rendered its first FFmpeg smoke draft. |
| 07 | FFmpeg shot-card draft | done | Added a manifest-driven shot-card renderer that turns each shot into a timed draft card sequence. |
| 08 | Shot-card export variants | done | Added 16:9 YouTube and 9:16 iPhone/social shot-card exports from the same manifest. |
| 09 | Manifest validation gate | done | Added a WSL-friendly manifest validator for required fields, timeline consistency, and export/platform targets. |
| 10 | Manifest-driven export timing | done | Updated the shot-card renderer to validate first and derive aspect ratios and duration scaling from manifest exports. |
| 11 | Scene-aware draft cards | done | Added scene-colored shot cards with visual prompts for clearer animation review. |
| 12 | Contact sheet review exports | done | Added contact-sheet PNG generation from shot-card draft cuts for fast rhythm review. |
| 13 | Review pack renderer | done | Added a one-command review pack that renders all platform cuts, contact sheets, and a summary report. |
| 14 | Batch review-pack index | done | Added a batch command that renders every work manifest's review pack and writes an index. |
| 15 | Rust CLI orchestration core | done | Added a Rust CLI for manifest validation, export planning, and review-pack adapter orchestration. |
| 16 | Rust review-pack orchestration | done | Moved review-pack reports and batch indexes into Rust while keeping FFmpeg media generation external. |
| 17 | Rust contact-sheet orchestration | done | Moved contact-sheet FFmpeg orchestration into Rust and removed the Bash contact-sheet adapter. |
| 18 | Rust shot-card orchestration | done | Moved shot-card MP4 FFmpeg orchestration into Rust and removed the Bash shot-card adapter. |
| 19 | Rust validation canonical path | done | Removed the legacy Bash manifest validator and documented `cargo run -- validate` as the canonical pre-render gate. |
| 20 | Rust smoke orchestration | done | Moved the starter smoke MP4 renderer into Rust and removed the last Bash FFmpeg adapter. |
| 21 | Rust CLI command surface docs | done | Updated README and product validation docs to show the current Rust-owned smoke, validation, planning, render, and review commands. |
| 22 | Valid starter manifest contract | done | Updated the starter scenario-video template so it passes Rust validation and added regression coverage. |
| 23 | Manifest identity validation | done | Added Rust checks for duplicate IDs, positive timing, and platform/export coverage before rendering. |
| 24 | Scene span validation | done | Added Rust checks that each shot falls within the timeline span of its referenced scene. |
| 25 | Nested manifest field validation | done | Added Rust checks for documented required scene and shot fields before rendering. |

## Success criteria

- REEL has a clear movie/video product thesis and explicit non-goals.
- The REEL rubric and production vocabulary are documented.
- Repo-local wave, pulse, and research skills exist.
- Repo-local `.roles` panel reviews story, animation style, edit, sound, and platform fit.
- REEL can describe multiple animation styles for Games Design scenario videos before renderer selection.
- REEL has a renderer-neutral production manifest contract covering scenario source, format, style, scenes, shots, audio, captions, continuity, assumptions, and exports.
- REEL has a researched first renderer path that keeps Linux video tooling in WSL2 and avoids provider lock-in.
- REEL can produce a gitignored FFmpeg smoke MP4 from the starter scenario manifest through Rust orchestration.
- REEL has a first original Games Design scenario video package with brief, script, shotlist, manifest, and panel review.
- REEL can render a gitignored shot-card MP4 that follows a work manifest's shot order and durations.
- REEL can render platform-specific shot-card drafts for YouTube/demo and iPhone/social review.
- REEL can validate manifest structure, timeline continuity, and export targets before rendering.
- REEL can derive draft export timing from manifest platform/export targets.
- REEL can render scene-aware shot cards with visual prompts from the manifest.
- REEL can render gitignored contact sheets from draft cuts for quick review.
- REEL can render a gitignored review pack for all manifest platforms.
- REEL can batch-render review packs for all work manifests under `works/`.
- REEL has a Rust CLI core so contracts and orchestration fit the wider portfolio while renderers remain external dependencies.
- REEL writes review-pack reports and indexes in Rust.
- REEL renders contact sheets through Rust-owned FFmpeg orchestration.
- REEL renders shot-card MP4s through Rust-owned FFmpeg orchestration.
- REEL uses Rust manifest validation as the canonical pre-render gate.
- REEL renders smoke MP4s through Rust-owned FFmpeg orchestration.
- REEL documents its current Rust CLI command surface for validation and review rendering.
- REEL's starter manifest template passes the same Rust validation contract as work manifests.
- REEL rejects duplicate manifest IDs, non-positive timing, and platform/export coverage gaps.
- REEL rejects shots whose timelines fall outside their referenced scene spans.
- REEL rejects scenes and shots that omit documented required manifest fields.
- Validation commands are named before renderer-specific tooling is chosen.
- TRACKER records REEL as a Design Labs repo and dependency intake candidate.
