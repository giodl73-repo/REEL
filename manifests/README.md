# Production manifest contract

The production manifest is the ordered handoff from REEL design work to a future
renderer. It should be detailed enough for a human or agent to render a draft,
but neutral enough that the renderer can be selected later.

Use YAML for production manifests. New integrations should start from the
sanitized v0.2 planning fixture at
`manifests/fixtures/two-speaker-untimed/planning.yaml`. The
`manifests/templates/scenario-video.yaml` v0.1 template remains a legacy example
for existing canonical works, not the current adoption surface.

Scene production remains `reel.manifest.v0.2`. CLI v0.2.1 also provides the
separate reference-only `reel.series.v0.1` template at
`manifests/templates/episodic-series.yaml`; it does not replace or revise the
scene format. See `docs/episodic-series-v0.1.md`.

CLI v0.2.2 requires no scene-manifest migration. Smooth fractional motion is a
renderer default selected with CLI options, and the v0.2.1 integer-step path is
available through `--motion-quality legacy`. The sanitized acceptance manifest
and grid live under `manifests/fixtures/smooth-motion/`.

CLI v0.2.20 keeps `reel.manifest.v0.2` and adds optional typed mixed-media
fields. Shots may declare
`media_kind: still|video|animation|sprite-animation`, `source_in_seconds`, and a
`beat_marker_id`. Top-level `beat_markers`, `audio_events`, and
`narration_ducking` move edit and mix timing into the portable manifest. See
`../docs/mixed-media-timeline-v0.2.20.md`.

CLI v0.2.21 keeps the manifest schema stable. Selection locks and planning
derivatives are external governance packets; audio-only previews and cached-
picture remux reports bind the unchanged v0.2 manifest by SHA-256. See
`../docs/selection-lock-and-audio-cache-v0.2.21.md`.

CLI v0.2.22 adds an optional top-level `score` block for provider-neutral music
direction. It owns originality policy, creative brief, motifs, instrument
palette, chapter cues, emotion and energy movement, tempo/meter, transitions,
montage/picture notes, and exact sync points. `reel score-plan` compiles those
fields into `reel.score-plan.v0.1`; it does not synthesize or license music. See
`../docs/chapter-score-direction-v0.2.22.md`.

The field reference below documents the legacy timed v0.1 shape retained by the
canonical works. The v0.2 planning/timing lifecycle, including untimed
manifests, speakers, narration cues, and conform state, is documented in
[`docs/production-manifest-v0.2.md`](../docs/production-manifest-v0.2.md).

## Required top-level fields (legacy v0.1)

| Field | Required | Purpose |
|---|---:|---|
| `manifest_version` | yes | Legacy contract version: `reel.manifest.v0.1`. |
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

Optional mixed-media shot fields are `media_kind` (defaults to `still`),
`source_in_seconds` (for video trimming), `animation` (ordered authored cels
with explicit frame holds), `sprite_animation` (a background plus keyframed
pose tracks), and `beat_marker_id` (an exact named
start-time anchor). Optional top-level `audio_events` support `music`,
`ambience`, `effect`, `narration`, and `dialogue` roles; `beat_markers` define
reusable timeline anchors; and `narration_ducking` configures the legacy
narration sidechain. Event `gain_automation` points use exactly one local-time
or beat-marker anchor. Ordered `audio_ducking` policies declare detector and
target role buses, including a bounded maximum reduction. The two ducking
forms are mutually exclusive so legacy behavior cannot change silently.
Optional `audio_mastering` declares final integrated loudness, loudness range,
true peak, and limiter policy after event mixing.

Optional `score` direction is complementary to `audio_events`: it specifies
what original or licensed music should do, while audio events specify which
rendered sources enter the mix and when. A score plan is creative direction,
not proof that the requested instruments, performance, originality, or license
exist in a rendered track.

## Additive planning sidecars

`reel.choreography.v0.1` describes renderer-neutral stage marks, beats,
performer phrases, prop handoffs, and reactions. It compiles to a flattened
blocking plan without changing the production-manifest contract. See
[`../docs/choreography-v0.2.24.md`](../docs/choreography-v0.2.24.md).

`reel.craft-plan.v0.1` records cross-department intent, evidence, assets,
continuity, workflow status, and explicit human-review gates. Its coverage
report measures structural presence and references only; it never scores
artistic quality. Department packets contain only explicitly routed and
referenced information. See
[`../docs/craft-plan-v0.2.25.md`](../docs/craft-plan-v0.2.25.md).

The shared hash-pinned production fixture, choreography asset binding, sprite
manifest compiler, packet distribution rules, and packet receipts introduced
in v0.2.26 are documented in
[`../docs/production-handoff-v0.2.26.md`](../docs/production-handoff-v0.2.26.md).

Both contracts are optional sidecars. Neither silently changes
`reel.manifest.v0.2`, grants approval, or makes a department's creative choice.

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
  Adapter ids must be unique, and their order is preserved by adapter planning.

## Validation

Before rendering, validate a work manifest with the Rust CLI:

```powershell
cargo run -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run -- adapters --output json
cargo run -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
```

The validator checks the supported manifest version; required top-level,
metadata, platform, export, scene, and shot fields; non-empty required values;
non-empty and unique scene, shot, platform, and export identifiers; positive
timing; scene and shot duration totals; shot start offsets; shot-to-scene
references; shot placement within the referenced scene timeline; and
platform/export coverage, duration, aspect ratio, and filename consistency.
When optional `renderer_assumptions.adapters` metadata is present, adapter ids
must be one of the known provider-neutral adapter boundaries.
The adapter plan command reports each known adapter and whether the manifest
declares it, without executing planned providers. Declared adapters are listed
first in manifest order, followed by undeclared known adapters.
Use `--output json` for automation that needs stable adapter ids, status,
operation names, boundaries, and manifest-declared flags where applicable.
Adapter catalog and plan outputs also include dependency policies so provider or
binary requirements remain explicit.
