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

Production manifest v0.2 adds an explicit timing lifecycle, speaker-aware
narration cues, protected pauses, deterministic voice conform, fine-grained
source coverage, privacy-safe continuity references, derivative lineage, and
long-still motion controls. See
[`docs/production-manifest-v0.2.md`](docs/production-manifest-v0.2.md).

CLI v0.2.18 adds an opt-in `montage` edit mode with true hard cuts and crop-safe
`punch-in` / `punch-out` treatments for sub-second photo bursts. The default
`cinematic` mode is unchanged. See
[`docs/fast-cut-photo-montage-v0.2.18.md`](docs/fast-cut-photo-montage-v0.2.18.md).

CLI v0.2.19 adds opt-in trailer motion without changing the manifest schema or
cinematic defaults: `slam-in`, `whip-left`, and `whip-right` shot treatments,
plus the global `--motion-curve ease-out` setting. See
[`docs/trailer-motion-v0.2.19.md`](docs/trailer-motion-v0.2.19.md).

CLI v0.2.20 adds manifest-owned still/video source semantics, timed music,
ambience, effect, and narration events, named beat-marker validation, and
narration-driven sidechain ducking. It also supports manifest-owned final audio
mastering, intentional caption-free renders, bounded large-timeline execution,
and selectable medium/slow encoding. Existing pre-mixed `--audio` renders
remain compatible. See
[`docs/mixed-media-timeline-v0.2.20.md`](docs/mixed-media-timeline-v0.2.20.md).

CLI v0.2.21 adds proof selection locks and fast audio revision. `animatic-lock`
creates an atomic packet containing the selected verified artifact, a locked
manifest derivative, and a receipt binding both hashes without invalidating the
manifest that produced the proof. `planning-derive` explicitly unlocks a new
lineage-bearing revision. `animatic-audio-render` compiles only manifest-owned
audio events, while `animatic-remux` stream-copies previously verified picture
and replaces only its audio. See
[`docs/selection-lock-and-audio-cache-v0.2.21.md`](docs/selection-lock-and-audio-cache-v0.2.21.md).

CLI v0.2.22 adds optional episode and season runtime plans without changing
`reel.series.v0.1`. A plan names a runtime class, minimum/target/maximum
duration, and reviewable component budgets. `series-timing-audit` compares those
plans with declared, orientation, or planned timing; projects season and series
runtime; and reports neighboring-episode drift without turning an intentional
creative exception into a validation failure. See
[`docs/series-runtime-planning-v0.2.22.md`](docs/series-runtime-planning-v0.2.22.md).

The main BERTICA-driven workflow is:

```powershell
cargo run -- validate planning.yaml --output json
cargo run -- plan planning.yaml --output json
cargo run -- conform planning.yaml --cues cue-measurements.yaml `
  --speaker-tempo narrator=85 --output-dir production/conformed/scene-v2
cargo run -- source-coverage production/conformed/scene-v2/manifest.yaml --output json
cargo run -- quality-check production/conformed/scene-v2/manifest.yaml --output json
cargo run -- caption-check production/conformed/scene-v2/captions.srt --output json
cargo run -- animatic-render production/conformed/scene-v2/manifest.yaml `
  --asset-root C:/src/consumer --audio production/audio/master.wav `
  --captions production/conformed/scene-v2/captions.srt `
  --caption-presentation production/conformed/scene-v2/caption-presentation.yaml `
  --caption-profile youtube-review --speaker-label-policy first-entrance `
  --output production/video/private-review-smooth-v026.mp4 `
  --motion-quality smooth --motion-curve ease-in-out
cargo run -- motion-analyze production/video/private-review-smooth-v026.mp4 --output json
cargo run -- animatic-check production/video/private-review-smooth-v026.artifacts.json --output json
cargo run -- animatic-receipt production/video/private-review-smooth-v026.artifacts.json `
  --output production/video/private-review-smooth-v026.receipt.json --format json
cargo run -- animatic-receipt-check production/video/private-review-smooth-v026.receipt.json `
  production/video/private-review-smooth-v026.mp4 --output json
```

Untimed manifests validate and produce useful storyboard plans while render and
delivery commands remain explicitly gated. Conform packets are published
atomically and include a conformed manifest, captions, input/output hashes,
transform parameters, and tool version.

The sanitized `manifests/fixtures/vertical-sound-off/` derivative exercises a
real 9:16, caption-complete output with no audio stream and records a complete
five-role panel decision without implying principal approval.

CLI v0.2.1 adds the reference-only `reel.series.v0.1` layer, atomic episode
packets, deterministic SRT cue import, and shared hashed continuity registries
without changing `reel.manifest.v0.2`. See `docs/episodic-series-v0.1.md` and
`docs/cue-import-and-continuity-v0.1.md`.

CLI v0.2.2 keeps both contracts unchanged and repairs slow long-still cadence.
`animatic-render` now defaults to frame-evaluated cubic subpixel motion with
ease-in/out; `--motion-quality legacy` reproduces the v0.2.1 zoompan path.
`motion-analyze` applies the published adjacent-frame cadence gate. See
[`docs/smooth-motion-v0.2.2.md`](docs/smooth-motion-v0.2.2.md) and the fully
synthetic `manifests/fixtures/smooth-motion/` proof.

CLI v0.2.3 keeps `reel.manifest.v0.2` unchanged and closes the verification
loop around those renders. `motion-check` evaluates moving shots and intentional
holds separately against the manifest timeline. `animatic-check` verifies
hashed inputs/output, H.264/yuv420p CFR delivery, dimensions, duration, audio
policy, captions, shot lineage, and transform safety. Smooth multi-shot renders
also fail preflight when their concurrent perspective-filter estimate exceeds
the published 2048 MiB budget. See
[`docs/animatic-verification-v0.2.3.md`](docs/animatic-verification-v0.2.3.md).

CLI v0.2.4 keeps `reel.manifest.v0.2` unchanged and makes the render environment
self-diagnosing. `render-doctor` verifies FFmpeg, ffprobe, the exact filters used
by composition and verification, cubic perspective interpolation, and libx264
before BERTICA or another consumer starts an expensive render. Real
`animatic-render` operations enforce the same gate. See
[`docs/render-environment-v0.2.4.md`](docs/render-environment-v0.2.4.md).

CLI v0.2.5 keeps `reel.manifest.v0.2` unchanged and binds the successful render
environment to each real `*.artifacts.json` report. The nested evidence records
transport, FFmpeg/ffprobe versions, all seven required capabilities, and a
deterministic SHA-256 fingerprint. `animatic-check` requires and validates this
lineage for v0.2.5+ artifacts. See
[`docs/render-lineage-v0.2.5.md`](docs/render-lineage-v0.2.5.md).

CLI v0.2.6 keeps `reel.manifest.v0.2` unchanged and adds a privacy-safe sharing
boundary. `animatic-receipt` verifies the full local artifact, then writes a
path-free `reel.animatic-receipt.v0.1` containing binding hashes, delivery and
motion facts, generic input counts, transport, and the render-environment
fingerprint. It carries no work ID, filenames, local paths, or input IDs/hashes
and does not imply approval. See
[`docs/privacy-safe-receipt-v0.2.6.md`](docs/privacy-safe-receipt-v0.2.6.md).

CLI v0.2.7 keeps `reel.manifest.v0.2` unchanged and lets a recipient verify an
intentionally shared video without the private artifact report.
`animatic-receipt-check` strictly rejects unknown receipt fields, hashes and
probes the video, and checks its H.264/yuv420p delivery, dimensions, CFR,
duration, byte length, and audio state. Its JSON report is also path-free. See
[`docs/receipt-check-v0.2.7.md`](docs/receipt-check-v0.2.7.md).

CLI v0.2.8 keeps `reel.manifest.v0.2` unchanged and adds a deterministic caption
accessibility gate. `caption-check` validates strict SRT order/timing and audits
minimum display time, characters per line, lines per cue, and reading speed.
Thresholds are explicit and configurable, while JSON output omits caption text
and local paths. See
[`docs/caption-accessibility-v0.2.8.md`](docs/caption-accessibility-v0.2.8.md).

CLI v0.2.9 keeps `reel.manifest.v0.2` unchanged and runs that caption policy
automatically before every `animatic-render`. A strict, separately versioned
caption-presentation sidecar maps delivery cues to existing narration cues and
audience-facing speaker labels. Deterministic `none`, `first-entrance`,
`persistent`, and timed-reintroduction policies render a separate badge without
mutating SRT text; artifact verification reconstructs every caption and
presentation hash. See
[`docs/speaker-caption-presentation-v0.2.9.md`](docs/speaker-caption-presentation-v0.2.9.md).

CLI v0.2.10 adds an artifact-bound `caption-layout` evidence packet without
changing `reel.manifest.v0.2`. It records per-cue declared caption/badge boxes,
pixel sizing, margins, renderer-declared contrast treatment, overlap and frame
safety, and maximum occupied screen percentage. Deterministic first/middle/last
frames and a contact sheet assist human review; the report makes no OCR,
translation, device-legibility, or accessibility-expertise claim. See
[`docs/caption-layout-v0.2.10.md`](docs/caption-layout-v0.2.10.md).

CLI v0.2.11 adds a deterministic, local `audio-check` gate. It records EBU R128
integrated loudness, loudness range and true peak; sample peak/count evidence;
stream and duration facts; leading/trailing/internal silence; optional stem
margin; and a SHA-256 binding under audiobook, podcast, YouTube-audiobook, and
private-review profiles. Its strict JSON omits paths and creative identity, and
a successful report can be bound to animatic artifact lineage without changing
audio. See [`docs/audio-quality-v0.2.11.md`](docs/audio-quality-v0.2.11.md).

CLI v0.2.12 adds a strict, separate `reel.comparison.v0.1` contract for
controlled A/B/C review videos. It verifies every child receipt/video and local
artifact evidence, enforces declared fixed and changed dimensions, creates
neutral slates with optional chime/protected silence/replay, and supports
descriptive or deterministically blinded labels. A local parent artifact embeds
each child receipt and private decode evidence; a separate path-free parent
receipt deliberately omits IDs and labels. See
[`docs/comparison-composer-v0.2.12.md`](docs/comparison-composer-v0.2.12.md).

CLI v0.2.13 adds strict, local, append-only `review-record` derivatives bound
to an exact video/artifact/receipt hash. Independent advisory findings remain
separate; an explicit final-authority resolution must cite the hashes of an
actual multi-reviewer disagreement. A separate hash-bound review index lets
`series-review-queue` report missing findings, disagreement, resolution, and
remaining gates without exposing private reasons or inferring approval. See
[`docs/review-decisions-v0.2.13.md`](docs/review-decisions-v0.2.13.md).

CLI v0.2.14 preflights every comparison opening, variant, and replay slate
against the actual output geometry and an 8% safe area before starting FFmpeg.
Copy wraps deterministically and scales only to a documented font/line floor;
infeasible copy leaves no video, artifact, or receipt. The local comparison
artifact records lines, font geometry, bounds, and occupancy. Opt-in
`comparison-layout` and `comparison-layout-check` commands create and verify a
private artifact/video-bound PNG packet, while the existing shareable receipt
remains free of copy, labels, images, and paths. See
[`docs/comparison-slate-layout-v0.2.14.md`](docs/comparison-slate-layout-v0.2.14.md).

CLI v0.2.15 adds a strict `reel.voice-performance.v0.1` sidecar and deterministic
performance-plan receipt. Exact cue substrings can now carry controlled dramatic
actions, intensity, pace, pitch-shape intent, onset, stress and pauses while the
engine compiler distinguishes executed controls from advisory direction. This
prevents a prose note from being mistaken for a parameter the voice engine
actually received. See
[`docs/voice-performance-v0.2.15.md`](docs/voice-performance-v0.2.15.md).

CLI v0.2.16 additively separates emotion scope, baseline register, pitch
contour, terminal boundary, relative semitone targets and span joins. New
prosody-evidence commands bind external per-span measurements to the exact plan
and rendered-audio hash, detect rising/falling contour mismatches, and preserve
human listening as the approval gate. See
[`docs/voice-prosody-evidence-v0.2.16.md`](docs/voice-prosody-evidence-v0.2.16.md).

CLI v0.2.17 adds an approved cross-scene voice profile and a strict
`voice-consistency-check` preflight. Auditions and full scenes now inherit stable
speaker identities, narrator/poet/cast modes, measured speaking-rate envelopes,
and minimum cue gaps. Stale bindings, identity drift, incomplete coverage, fast
delivery, and compressed pauses fail before final assembly. See
[`docs/voice-consistency-v0.2.17.md`](docs/voice-consistency-v0.2.17.md).

## Renderer direction

The first researched implementation path is Linux-first in WSL2: FFmpeg for
baseline assembly/encoding, then a Remotion adapter for programmatic animation.
Blender and cinematic AI remain style-specific follow-up paths until a concrete
work package requires them.

The REEL CLI is the durable orchestration layer:

```powershell
cargo run -- smoke
cargo run -- adapters
cargo run -- adapters --output json
cargo run -- render-doctor
cargo run -- render-doctor --output json
cargo run -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run -- scene-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml scene-01 youtube-demo
cargo run -- scene-preview works\0001-ash-vale-last-road-before-winter\manifest.yaml scene-01 youtube-demo
cargo run -- scene-previews works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run -- work-preview works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run -- artifact-manifest works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run -- artifact-manifest works\0001-ash-vale-last-road-before-winter\manifest.yaml --output json
cargo run -- artifact-check renders\artifacts\0001-ash-vale-last-road-before-winter-artifacts.json
cargo run -- artifact-check renders\artifacts\0001-ash-vale-last-road-before-winter-artifacts.json --output json
cargo run -- artifact-check-all works
cargo run -- artifact-check-all works --output json
cargo run -- corpus works
cargo run -- corpus works --output json
cargo run -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run -- shot-cards works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run -- contact-sheet works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run -- review-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run -- demo works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run -- remotion-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo scene-01
cargo run -- review-all works
cargo run -- review-all works --output json
cargo run -- validate manifests\fixtures\two-speaker-untimed\planning.yaml --output json
cargo run -- plan manifests\fixtures\two-speaker-untimed\planning.yaml --output json
cargo run -- conform manifests\fixtures\two-speaker-untimed\planning.yaml --cues manifests\fixtures\two-speaker-untimed\cue-measurements.yaml --speaker-tempo narrator=85 --output-dir target\fixture-conform
cargo run -- source-coverage manifests\fixtures\two-speaker-untimed\planning.yaml --output json
cargo run -- provider-package manifests\fixtures\two-speaker-untimed\planning.yaml --output target\fixture-provider.json --format json
cargo run -- quality-check manifests\fixtures\two-speaker-untimed\planning.yaml --output json
cargo run -- series-timing-audit manifests\templates\episodic-series.yaml --output json
```

Rust owns contracts, planning, and subprocess orchestration; FFmpeg, Remotion,
Blender, and future providers stay external adapters.

Install the versioned CLI with `cargo install --path . --locked` or use a tagged
Windows/Linux release binary. See [`docs/setup/install.md`](docs/setup/install.md).

The adapter boundary now names FFmpeg as the implemented baseline adapter and
keeps Remotion, Blender, and AI-video as provider-neutral planned adapters until
a concrete work package needs them.

Use `cargo run -- adapters` to inspect the implemented baseline and planned
adapter boundaries.
Use `cargo run -- adapter-plan <manifest>` to see which of those adapter
boundaries a manifest declares under `renderer_assumptions.adapters`.
Add `--output json` when automation needs a machine-readable adapter catalog or
manifest adapter plan.
Both text and JSON outputs include each adapter's dependency policy so reviewers
can see whether an external binary, SDK, credential, or provider choice is
required.

The Remotion boundary is a planned file/project handoff: Rust can describe the
manifest, output directory, platform/export id, and deterministic command shape
without requiring Node or Remotion packages in the baseline repo.

The Blender boundary is planned as a CLI/Python file handoff, and the AI-video
boundary is planned as a provider-neutral package. Neither requires binaries,
SDKs, credentials, endpoints, or model names in the baseline contract.

Review packs include an artifact-manifest link, adapter summary,
work-preview table, and scene-preview table so reviewers can see the FFmpeg
baseline used for rendered outputs and the planned animation adapter boundaries.
Demo pages under `renders\demo\` provide a browser-openable view of the FFmpeg
baseline MP4s, full work previews, all baseline scene previews, contact sheets,
review pack, artifact manifest, and adapter summary.
Remotion handoff packages under `renders\remotion\` provide manifest-derived
props, scene timing hints, and a planned command shape without installing or
running Node.
Scene planning derives a single scene's shot subset, timing, platform
dimensions, and scaled render duration before rendering a full scene preview.
Scene previews under `renders\scene-previews\` are deterministic FFmpeg baseline
MP4s with shot timing, text treatment, and simple animated motion.
Use `scene-previews` to render every manifest scene for one platform.
Work previews under `renders\work-previews\` concatenate the platform's scene
previews into a continuous baseline MP4.
Artifact manifests under `renders\artifacts\` provide a schema version,
machine-readable paths, byte sizes, SHA-256 digests, durations, dimensions, and
scene-preview coverage for automation.
Add `--output json` to print the generated artifact manifest to stdout.
Use `artifact-check` to verify that a generated artifact manifest still points to
files with matching byte sizes, matching SHA-256 digests, and positive video
durations, and still matches the source manifest's work identity, export
platforms, dimensions, platform video durations, scene ids, and scene durations.
Add `--output json` when automation needs the verification summary.
Check reports include schema version, generation timestamp, verification
timestamp, source manifest, work identity, and the baseline adapter for
downstream routing, plus aggregate verified video/image file counts and video
duration. Batch reports also aggregate verified artifact manifest paths, source
manifest paths, artifact schema versions, baseline adapter identities, work
ids/titles, and platform counts across works.
Use `artifact-check-all` to generate and verify artifact manifests for every
work under a works root.
Use `corpus` to validate and summarize every work manifest under a works root
without rendering media. This gives automation a fast inventory of work ids,
manifest paths, manifest versions, titles, source repos, source ids, formats,
source paths, source commits, audience primary/context/effect values, styles,
alternate styles, platform names, platform counts, scene counts, shot counts,
export counts, and manifest timing totals before expensive artifact or
review-pack generation.
Use `review-queue` to validate and summarize manifest-owned review status and
required roles, including required-role manifest/work id/title lists, across a
works root without rendering media, plus status-to-role and role-by-status work
id lists for outstanding assignment routing by manifest, id, title, and role.
Generated review packs include manifest-owned review status, required review
roles, and role-specific focus before adapter and FFmpeg artifact sections. The
`review-all` index links each work's review pack and artifact manifest, then
shows per-work artifact generation and verification timestamps plus per-work and
aggregate verification counts for generated scene previews, files, bytes, and
review pack/review status/required role/work id/title/artifact manifest/source
manifest/artifact schema/baseline adapter/platform/video/image split counts,
status counts, review-status artifact manifests/review packs/work ids/titles,
required-role counts, required-role status counts, plus video duration.
Add `--output json` to emit the generated index path, index generation and
verification timestamps, review-pack paths, artifact-manifest paths, and
artifact-check summaries plus review handoff metadata for automation.

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
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo run --quiet -- validate manifests\fixtures\two-speaker-untimed\planning.yaml --output json
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- smoke
cargo run --quiet -- demo works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- remotion-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml youtube-demo
cargo run --quiet -- review-all works
git diff --check
```

## License

[MIT](LICENSE) — © 2026 Gio Della-Libera.
