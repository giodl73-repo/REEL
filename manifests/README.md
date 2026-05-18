# Production manifest contract

The production manifest is the ordered handoff from REEL design work to a future
renderer. It should be detailed enough for a human or agent to render a draft,
but neutral enough that the renderer can be selected later.

Use YAML for foundation manifests. The first template lives at
`manifests/templates/scenario-video.yaml`.

## Required top-level fields

| Field | Required | Purpose |
|---|---:|---|
| `manifest_version` | yes | Contract version. Start with `reel.manifest.v0.1`. |
| `work` | yes | REEL work id, usually `NNNN-slug`. |
| `title` | yes | Human-readable video title. |
| `source_scenario` | yes | Upstream repo/path/id that owns scenario truth. |
| `format` | yes | Video format, such as `trailer` or `game-cinematic`. |
| `style` | yes | Animation style, such as `isometric-game` or `storyboard-animatic`. |
| `audience` | yes | Intended viewer and context. |
| `platforms` | yes | Export/viewing targets. |
| `continuity` | yes | Characters, places, factions, props, and canon constraints. |
| `scenes` | yes | Ordered scene list. |
| `shots` | yes | Ordered shot list with timing and visual/audio intent. |
| `audio` | yes | Narration, music, effects, silence, and mix priorities. |
| `captions` | yes | Caption policy and key on-screen text. |
| `renderer_assumptions` | yes | Renderer-neutral assumptions and optional candidates. |
| `exports` | yes | Deliverable cuts, aspect ratios, durations, and filenames. |
| `review` | yes | Required `.roles` checks before rendering. |

## Scene contract

Each scene must define:

- `id`
- `purpose`
- `duration_seconds`
- `story_beat`
- `location`
- `characters`
- `continuity_notes`

## Metadata section contracts

`source_scenario` must define `repo`, `path`, `id`, and `source_commit`.
`audience` must define `primary`, `context`, and `desired_effect`.
`audio` must define `narration_voice`, `music_direction`, `effects_direction`,
and `silence_notes`. `captions` must define `required`, `style`, and
`language`. `renderer_assumptions` must define `candidates` and `blockers`.
`review` must define `required_roles` and `status`.

## Platform and export contracts

Each platform must define `name`, `aspect_ratio`, `target_duration_seconds`, and
`sound_optional`. Each export must define `id`, `filename`, `aspect_ratio`, and
`duration_seconds`.

## Shot contract

Each shot must define:

- `id`
- `scene_id`
- `start_seconds`
- `duration_seconds`
- `camera`
- `action`
- `visual_prompt`
- `style_constraints`
- `audio`
- `captions`
- `transition_out`

## Games Design scenario rules

- The source game repo owns scenario canon.
- REEL may adapt tone, order, and presentation, but must not silently change
  scenario facts.
- Character/place/object continuity constraints belong in `continuity` and each
  shot's `style_constraints`.
- Style variants of the same scenario should use separate manifests or a named
  `alternate_styles` list.

## Renderer rules

- Do not require a provider-specific field in v0.1.
- Use `renderer_assumptions.candidates` to list plausible paths such as FFmpeg,
  Remotion, Blender, browser capture, or cinematic AI.
- Use `renderer_assumptions.blockers` for unknowns that require `reel-research`.
- Optionally use `renderer_assumptions.adapters` for known adapter ids:
  `ffmpeg`, `remotion`, `blender`, or `ai-video`. This list names possible
  adapter boundaries only; it must not require provider credentials, API
  endpoints, model names, SDK packages, or binary installation details.

## Validation

Before rendering, validate a work manifest with the Rust CLI:

```powershell
cargo run -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
```

The validator checks the supported manifest version; required top-level,
metadata, platform, export, scene, and shot fields; non-empty required values;
non-empty and unique scene, shot, platform, and export identifiers; positive
timing; scene and shot duration totals; shot start offsets; shot-to-scene
references; shot placement within the referenced scene timeline; and
platform/export coverage, duration, aspect ratio, and filename consistency.
When optional `renderer_assumptions.adapters` metadata is present, adapter ids
must be one of the known provider-neutral adapter boundaries.
