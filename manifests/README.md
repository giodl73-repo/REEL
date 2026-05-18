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

## Validation

Before rendering, validate a work manifest with the Rust CLI:

```powershell
cargo run -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
```

The validator checks required top-level fields, non-empty and unique scene, shot,
platform, and export identifiers, positive timing, scene and shot duration
totals, shot start offsets, shot-to-scene references, and platform/export
coverage, duration, aspect ratio, and filename consistency.
