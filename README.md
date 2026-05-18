# REEL

**A movie and video design lab.** REEL studies how moving-image works are
conceived, generated, edited, and evaluated: trailers, shorts, explainer videos,
animated diagrams, game cinematics, social clips, and longer film-like pieces.

The lab is named REEL because the artifact is both the thing we make and the
loop we learn from. The rubric is REEL:
**R**hythm, **E**motion, **E**xecution, **L**egibility. Every video must move in
time, create an effect, hold together technically, and remain understandable to
its intended audience.

Claude Code-driven. Rust-orchestrated. Markdown and YAML remain the human review
surface, while the Rust CLI owns manifest validation, render planning, and
review-pack orchestration. FFmpeg remains an external renderer dependency rather
than something REEL rewrites.

**Review roles:** REEL uses
[ROLES](https://github.com/giodl73-repo/ROLES), the `.roles` convention for
repository-local review panels. The founding panel checks story, animation style,
edit rhythm, sound, and platform fit before renderer work starts.

---

## What REEL is for

- Designing video artifacts before rendering: concepts, scripts, scenes, shots,
  timing, transitions, voice, music, captions, and export targets.
- Producing machine-readable manifests that future tools can render through
  FFmpeg, Remotion, Blender, browser capture, AI-video systems, or native app
  wrappers.
- Adapting Games Design scenarios into animation packages with explicit style,
  camera, shot, sound, caption, and export choices.
- Reviewing canonical and original moving-image works through a panel loop so the
  rubric evolves from actual attempts, not abstract taste.
- Creating reusable video packages for other portfolio repos: product demos,
  game trailers, research explainers, data-story videos, and mobile/social cuts.

## Non-goals

- REEL is not a general-purpose video editor in the first wave.
- REEL does not store large binary renders in git.
- REEL does not make model-provider choices before a research pass.
- REEL does not force sibling repos to depend on a product-specific runtime.

## The REEL dimensions

| | Dimension | Pts | The question |
|-|---|---:|---|
| **R** | Rhythm | 25 | Does the timing, pacing, cut structure, and motion energy serve the piece? |
| **E** | Emotion | 25 | Does the work create the intended feeling, stakes, memory, or desire? |
| **E** | Execution | 25 | Are visuals, audio, prompts, edit decisions, captions, and export choices technically coherent? |
| **L** | Legibility | 25 | Can the intended audience follow what matters on the intended device and platform? |

Advisory threshold: below 60. Binding threshold: 60+.

## Formats

Formats are documented in `formats/`. The starting set is:

- `short-film` - story-driven moving-image work with scene continuity.
- `trailer` - compressed promise, stakes, and mood for a larger work.
- `explainer` - visual explanation with narration, captions, or diagrams.
- `social-clip` - phone-first short-form video with immediate hook and captions.
- `game-cinematic` - world/character/action sequence for games.
- `animated-diagram` - motion graphics or data/story animation.

## Animation styles

Styles are documented in `styles/`. A REEL work should name both its format and
style before scripting or rendering. The starting style catalog is:

- `storyboard-animatic` - timed boards with camera moves, captions, and scratch audio.
- `motion-graphics` - text, shape, icon, chart, and diagram animation.
- `cutout-2d` - character and prop rigs moved in layered 2D space.
- `illustrated-2d` - hand-drawn or painterly frame language.
- `pixel-art` - sprite/tile language for retro or game-adjacent scenarios.
- `isometric-game` - map, settlement, and system views for strategy/game worlds.
- `3d-previs` - rough 3D blocking for camera, action, and scene continuity.
- `cinematic-ai` - prompt-driven generated video with explicit continuity controls.

Games Design scenario flow:

```text
SCENARIO -> REEL BRIEF -> FORMAT + STYLE -> SCRIPT -> SHOTLIST -> MANIFEST -> PANEL -> EXPORT
```

## Production manifest

The production manifest is the handoff from design to rendering. It is defined in
`manifests/README.md` and starts as YAML so humans can review it before tooling
exists. A manifest must name the source scenario, target format and style,
scene/shot order, audio, captions, renderer assumptions, and export targets.

## Renderer direction

The first researched implementation path is Linux-first in WSL2: FFmpeg for
baseline assembly/encoding, then a Remotion adapter for programmatic animation.
Blender and cinematic AI remain style-specific follow-up paths until a concrete
work package requires them.

The REEL CLI is the durable orchestration layer:

```powershell
cargo run -- smoke
cargo run -- adapters
cargo run -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run -- shot-cards works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run -- contact-sheet works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run -- review-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run -- review-all works
```

Rust owns contracts, planning, and subprocess orchestration; FFmpeg, Remotion,
Blender, and future providers stay external adapters.

The adapter boundary now names FFmpeg as the implemented baseline adapter and
keeps Remotion, Blender, and AI-video as provider-neutral planned adapters until
a concrete work package needs them.

Use `cargo run -- adapters` to inspect the implemented baseline and planned
adapter boundaries.

The Remotion boundary is a planned file/project handoff: Rust can describe the
manifest, output directory, platform/export id, and deterministic command shape
without requiring Node or Remotion packages in the baseline repo.

The Blender boundary is planned as a CLI/Python file handoff, and the AI-video
boundary is planned as a provider-neutral package. Neither requires binaries,
SDKs, credentials, endpoints, or model names in the baseline contract.

## Pipeline

```text
BRIEF -> FORMAT -> SCRIPT/SHOTLIST -> MANIFEST -> PANEL -> INNOVATION -> AMENDMENT -> EXPORT
```

## Repository layout

```text
REEL/
├── src/                     Rust CLI orchestration core
├── scoring/                 REEL rubric and innovation log
├── formats/                 Video format grammars
├── styles/                  Animation and visual style grammars
├── manifests/               Production manifest contract and templates
├── .roles/                  Review panel definitions
├── personas/                Filmmaker/editor/reviewer voices and lenses
├── works/                   Numbered canonical and original video works
├── docs/reviews/            Plan and work reviews
├── context/waves/           Repo-local execution history
├── .claude/skills/          REEL wave, pulse, and research skills
└── docs/handoff/            Session resume notes
```

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- smoke
cargo run --quiet -- review-all works
git diff --check
```
