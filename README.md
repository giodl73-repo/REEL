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

CLI v0.2.46 adds owner-selected `cover` or `contain` visual fit while preserving
`cover` as the default, and teaches motion review to honor limited-animation
`hold_frames`. See
[`docs/visual-fit-and-animation-holds-v0.2.46.md`](docs/visual-fit-and-animation-holds-v0.2.46.md).

CLI v0.2.47 allows a still camera track to begin from a contained full-source
overview before moving through delivery-canvas keyframes. Existing cover-camera
renders remain unchanged. See
[`docs/contained-still-camera-tracks-v0.2.47.md`](docs/contained-still-camera-tracks-v0.2.47.md).

CLI v0.2.48 adds explicit caption-band picture reservation. Captioned still,
video, and limited-animation renders can fit their picture above the declared
caption region instead of placing captions over owner visuals; overlay remains
the default. See
[`docs/caption-safe-picture-layout-v0.2.48.md`](docs/caption-safe-picture-layout-v0.2.48.md).

CLI v0.3.0 lets contained still-camera tracks declare exact source and working
canvas geometry. REEL verifies the source dimensions, emits the same
deterministic fit in FFmpeg, and maps source-space focal/protected geometry into
camera space for render and review evidence. See
[`docs/source-to-camera-mapping-v0.3.0.md`](docs/source-to-camera-mapping-v0.3.0.md).

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

CLI v0.2.23 adds provider-neutral limited-animation shots: an ordered sequence
of individually hashed authored cels, each with an explicit delivery-frame
hold. This supports short fully animated vignettes without coupling REEL to an
image or video generator. See
[`docs/limited-animation-v0.2.23.md`](docs/limited-animation-v0.2.23.md).
The same release adds keyframed sprite animation for efficient pose swaps and
motion over a stable background. See
[`docs/sprite-animation-v0.2.23.md`](docs/sprite-animation-v0.2.23.md).

CLI v0.2.24 adds a strict, additive choreography sidecar and fast abstract
blocking preview. Named stage marks, exact beats, performer approaches, prop
handoffs, reactions, spatial paths, and timing curves compile into a flattened
renderer-neutral plan. See
[`docs/choreography-v0.2.24.md`](docs/choreography-v0.2.24.md).

CLI v0.2.25 adds a strict, additive cross-department craft plan. It records
department intent, evidence, assets, continuity, ownership, status, and explicit
human-review gates; audits structural coverage without scoring art; and exports
least-information department packets. See
[`docs/craft-plan-v0.2.25.md`](docs/craft-plan-v0.2.25.md).

CLI v0.2.26 closes the production-handoff boundary between those features. A
shared SHA-256-pinned production binding maps choreography and craft references
to exact shots and beat markers; choreography can compile pose assets and
beat-synchronized camera phrases into a real sprite production manifest; and
external department packets enforce per-record distribution policy and support
path-free cryptographic receipts. See
[`docs/production-handoff-v0.2.26.md`](docs/production-handoff-v0.2.26.md).

CLI v0.2.27 executes sprite camera tracks in delivery renders and adds a
path-free production-package receipt. Camera centers are crop-clamped at the
requested output shape, while package integrity remains explicitly separate
from human release gates. See
[`docs/delivery-camera-and-production-package-v0.2.27.md`](docs/delivery-camera-and-production-package-v0.2.27.md).

CLI v0.2.28 adds parent-relative sprite tracks and phrase-aware intentional
holds. Small props can follow a performer's changing position and scale through
pose swaps, while motion review excludes only explicitly declared hold spans
and continues measuring unexpected stationary transitions. See
[`docs/sprite-contact-and-holds-v0.2.28.md`](docs/sprite-contact-and-holds-v0.2.28.md).

CLI v0.2.29 adds sprite emissions: an effect resolves once from a named parent
at a specific frame, then detaches into canvas space with independent drift,
scale, rotation, lifetime, layer, and fade. This supports contact snow, dust,
sparks, motion residue, and other world-space aftermath without keeping the
effect glued to its moving source. See
[`docs/sprite-emissions-v0.2.29.md`](docs/sprite-emissions-v0.2.29.md).

CLI v0.2.30 adds inclusive performer and sprite visibility windows. A
choreography performer can enter after the opening beat or leave before the
ending beat without remaining frozen on stage, and the compiled sprite manifest
carries the same window into final FFmpeg overlays. See
[`docs/performer-visibility-v0.2.30.md`](docs/performer-visibility-v0.2.30.md).

CLI v0.2.31 adds portable layered-sprite libraries, domain-owned selector
profiles, character skin bindings, and content-derived cache plans. Mirrored
pose layers and post-transform readable decals remain separate, and unresolved
selectors fail instead of silently choosing a nearby pose. See
[`docs/layered-sprite-libraries-v0.2.31.md`](docs/layered-sprite-libraries-v0.2.31.md).

CLI v0.2.32 materializes those plans into deterministic transparent PNG cache
entries using hash-pinned relative recipe catalogs. Physical cache roots remain
outside receipts, reviewed precomposed sprites require an explicit preservation
mode, and a checkerboard contact-sheet command supports visual alpha review.
See [`docs/sprite-materialization-v0.2.32.md`](docs/sprite-materialization-v0.2.32.md).

CLI v0.2.33 bridges materialized sprites into choreography through a portable,
hash-pinned character/request binding. It requires explicit handling for every
performer and pose, verifies physical cache hashes, and writes only a
machine-local staged asset map for the existing choreography compiler. See
[`docs/sprite-choreography-staging-v0.2.33.md`](docs/sprite-choreography-staging-v0.2.33.md).

CLI v0.2.34 makes raster-cache invalidation recipe-local. Each raster key pins
the effective source layers used by that character request, so editing one pose
does not evict unrelated poses merely because they share a catalog. The receipt
continues to pin the complete catalog for provenance. See
[`docs/sprite-cache-locality-v0.2.34.md`](docs/sprite-cache-locality-v0.2.34.md).

CLI v0.2.35 deduplicates repeated sprite assets inside each rendered shot.
Logical pose and emission occurrences remain individually represented in
artifact lineage, while FFmpeg opens each unique raster once, splits that
decoded stream, and trims every branch to its visible segment. Dense pose-cycle
manifests therefore retain exact frame timing without multiplying full-duration
image decodes. See
[`docs/sprite-render-locality-v0.2.35.md`](docs/sprite-render-locality-v0.2.35.md).

CLI v0.2.36 adds strict, provider-neutral production operations: hash-only
generation plans and verified materialization evidence; append-only asset
promotion; incremental picture reuse/regeneration planning with explicit proxy
disclosure; timecoded repair queues; path-free portfolio readiness audits;
explicit voice-take selection and surgical retakes; music provenance with exact
no-score comparison; and sprite selector coverage matrices. Technical
readiness and promotion states never imply creative, principal, rights,
publication, or release approval. See
[`docs/production-operations-v0.2.36.md`](docs/production-operations-v0.2.36.md).

CLI v0.2.37 adds immutable provider-attempt receipts for sanitized evidence
from owner-controlled adapters, independent captured-PNG verification, and a
deterministic hash-pinned resume planner. REEL never calls a provider, stores
provider payloads or paths in portable output, or selects and approves an
output. See
[`docs/provider-attempt-evidence-v0.2.37.md`](docs/provider-attempt-evidence-v0.2.37.md).

CLI v0.2.38 adds owner-issued signed approval attestations and C2PA
verification. `approval-sign` binds a single human decision to an exact target,
policy, scope, and authority registry with an Ed25519 signature over documented
domain-separated canonical bytes; the private key never leaves its local file.
`approval-verify` independently checks a hash-pinned decision chain against a
trusted registry digest and reports cryptographic validity, registry authority,
target integrity, and a current decision only after receiving the complete
sequence-one lineage. `c2pa-verify` verifies an
asset's Content Credentials through an externally supplied, hash-pinned
`c2patool` snapshot and reports current C2PA manifest integrity as valid when
the official `validation_state` is `Valid`. Certificate trust is deliberately
not evaluated in V1: REEL does not invoke `c2patool trust` or load trust
resources, and its fixed private settings disable remote-manifest and other
network retrieval. Future trust requires an explicit hash-pinned input. No
signature or manifest implies rights, publication, or release approval. See
[`docs/signed-approvals-c2pa-v0.2.38.md`](docs/signed-approvals-c2pa-v0.2.38.md).

CLI v0.2.39 adds provider-neutral economics reconciliation over a complete,
hash-pinned attempt chain. It keeps quote, reservation, and realized charge
independent; derives queue, execution, capture, and total observed latency from
canonical observations; counts retry, retake, remix, and extension operations
separately; and evaluates an owner-authored budget as pass, warn, or block.
Unavailable realized charges remain unavailable, currencies cannot mix with
provider credits, and no result grants spending or creative authority. See
[`docs/provider-economics-v0.2.39.md`](docs/provider-economics-v0.2.39.md).

CLI v0.2.40 adds deterministic OpenTimelineIO export for validated conformed or
locked picture timelines. Every shot becomes one offline `Clip.2` at an exact
1000-unit millisecond timebase, with stable REEL identity and asset status in
namespaced metadata. V1 exports no media paths, prompts, audio, transitions,
effects, selection, or approval claims. See
[`docs/otio-export-v0.2.40.md`](docs/otio-export-v0.2.40.md).

CLI v0.2.41 adds media-generic, content-addressed changed-only build planning
over owner-authored dependency graphs. It verifies current recipe, direct
input, and reusable output bytes; derives canonical action keys from exact
dependency outputs; and distinguishes reuse, rebuild, and blocked downstream
work without executing builds or mutating state. See
[`docs/changed-only-build-v0.2.41.md`](docs/changed-only-build-v0.2.41.md).

CLI v0.2.42 replaces manual changed-only state hashing with immutable
owner-result receipts and receipt-bound state advancement. REEL regenerates the
exact plan from current evidence, measures every expected output, writes a
path-free receipt, and re-verifies those bytes before advancing local state.
Execution, result choice, approval, and rollback remain owner-controlled. See
[`docs/changed-only-results-v0.2.42.md`](docs/changed-only-results-v0.2.42.md).

CLI v0.2.43 adds strict owner-authored animation exposure sheets. Exact
inclusive frame spans for drawings, poses, props, effects, camera states, and
dialogue relationships bind to one SHA-pinned production shot and emit a
path-free coverage report. REEL validates timing but does not choose drawings,
render frames, mutate DCC projects, or claim a delivery frame rate. See
[`docs/animation-exposure-sheets-v0.2.43.md`](docs/animation-exposure-sheets-v0.2.43.md).

CLI v0.2.44 adds immutable evidence for owner-created multi-surface product
demo captures. REEL verifies exact state-document and PNG bytes, dimensions,
distinct captures, ordered CLI/TUI/Web coverage, and path-free publication.
It does not execute commands, control browsers, create captures, verify visible
semantics or redaction, select footage, or approve release. See
[`docs/screen-demo-capture-evidence-v0.2.44.md`](docs/screen-demo-capture-evidence-v0.2.44.md).

CLI v0.2.45 adds frame-keyed camera tracks for ordinary still plates. Owner
systems retain image semantics and waypoint meaning while REEL validates
bounded center/zoom paths, executes crop-clamped FFmpeg motion, and records the
track in artifact lineage. See
[`docs/still-plate-camera-tracks-v0.2.45.md`](docs/still-plate-camera-tracks-v0.2.45.md).

CLI v0.2.21 adds proof selection locks and fast audio revision. `animatic-lock`
creates an atomic packet containing the selected verified artifact, a locked
manifest derivative, and a receipt binding both hashes without invalidating the
manifest that produced the proof. `planning-derive` explicitly unlocks a new
lineage-bearing revision. `animatic-audio-render` compiles only manifest-owned
audio events, while `animatic-remux` stream-copies previously verified picture
and replaces only its audio. See
[`docs/selection-lock-and-audio-cache-v0.2.21.md`](docs/selection-lock-and-audio-cache-v0.2.21.md).

CLI v0.3.14 adds a deterministic raw-PCM repair materializer for the complete
v0.1 repair vocabulary: keep, cut, insert, replace, repeat, move, crossfade,
preserve-tail, match-gain, hash-bound match-EQ, extend-bars, and lock. Its
path-free receipt binds exact outside-region identity, resolved beat alignment,
clipping, ambience/loudness, tail/phase correlation, and spectral seam evidence.
The legacy cut-only FFmpeg EDL and hashes remain unchanged. See
[`docs/music-repair-materialization-v0.3.14.md`](docs/music-repair-materialization-v0.3.14.md).

CLI v0.3.13 executes the optional speech-keyed dynamic EQ declared by v0.3.12.
The FFmpeg graph isolates the requested presence band on the declared target
bus, keys its bounded attenuation from the declared detector bus, then applies
the policy's broadband ducking. Non-target roles remain outside both paths.
The existing cross-platform synthetic stem test now exercises this graph. See
[`docs/speech-keyed-dynamic-eq-v0.3.13.md`](docs/speech-keyed-dynamic-eq-v0.3.13.md).

CLI v0.3.12 adds dialogue-aware score mixing without changing legacy narration
behavior. Events can use the `dialogue` role and deterministic local/beat-keyed
gain automation. Ordered `audio_ducking` policies route declared detector roles
to declared target roles with a maximum reduction floor. Optional stem delivery
writes post-duck/pre-master D, M, E and pre-master WAVs plus a mastered full mix,
no-score, mono, and small-speaker review variants with path-free receipts and a
sample-level recombination proof. Dynamic-EQ intent is validated and visible in
dry-run plans. See
[`docs/dialogue-score-mixing-v0.3.12.md`](docs/dialogue-score-mixing-v0.3.12.md).

CLI v0.2.22 adds manifest-owned chapter score direction and a deterministic
`score-plan` handoff. Films can express original-music policy, motifs,
instrument families and articulations, mood/energy movement, tempo, story or
location palettes, transitions, montage notes, and exact picture hits tied to
beat markers without coupling the manifest to a composer or model provider.
Existing v0.2 manifests remain valid. See
[`docs/chapter-score-direction-v0.2.22.md`](docs/chapter-score-direction-v0.2.22.md).

The integrated v0.2.22–v0.2.25 planning line also adds optional episode runtime
budgets and timing audit, a hash-bound showrunner-control sidecar, explicit
selected-media readiness, and provider-neutral exact-lyrics song-generation
contracts. See
[`docs/series-runtime-planning-v0.2.22.md`](docs/series-runtime-planning-v0.2.22.md),
[`docs/showrunner-control-v0.2.23.md`](docs/showrunner-control-v0.2.23.md),
[`docs/asset-readiness-v0.2.24.md`](docs/asset-readiness-v0.2.24.md), and
[`docs/song-generation-v0.2.25.md`](docs/song-generation-v0.2.25.md).

The integrated music-reconstruction line adds the provider-neutral `reel-music`
workspace crate. v0.2.26 freezes raw-PCM source identity and validates bounded
repair plans; v0.2.27 compiles and renders sample-exact cut-only repairs with
strict local evidence; and v0.2.28 separates analyzer estimates from a corrected
editable music model with event-level provenance. See
[`docs/music-reconstruction-v0.2.26.md`](docs/music-reconstruction-v0.2.26.md),
[`docs/music-repair-render-v0.2.27.md`](docs/music-repair-render-v0.2.27.md), and
[`docs/music-analysis-model-v0.2.28.md`](docs/music-analysis-model-v0.2.28.md).

CLI v0.3.1 adds Slice C2 score export. A validated corrected model can be
compiled atomically to deterministic Standard MIDI, editable MusicXML, and a
utilitarian WAV rehearsal guide. The retained local receipt binds every output
and independently re-imports MIDI and MusicXML to detect lost duration, tempo,
meter, form, notes, or lyric-layer identities. See
[`docs/music-score-export-v0.3.1.md`](docs/music-score-export-v0.3.1.md).

CLI v0.3.2 adds a local interchange intake for outputs people already produce
with separation, transcription, feature-analysis, and notation tools. It binds
WAV/FLAC stems, MIDI, MusicXML, CSV/lab tables, JAMS, RDF, NPZ, and sonification
audio without running or replacing those tools. See
[`docs/music-interchange-intake-v0.3.2.md`](docs/music-interchange-intake-v0.3.2.md).

CLI v0.3.3 compares competing admitted evidence without automatically ranking
it. It emits a deterministic private queue for human selection and artifact-
specific corrections, and requires separate immutable decisions before either
can close. See
[`docs/music-evidence-comparison-v0.3.3.md`](docs/music-evidence-comparison-v0.3.3.md).

CLI v0.3.4 validates adapter-normalized semantic events from an explicitly
selected artifact, recomputes integer sample/microsecond/musical-tick mappings,
and atomically writes analysis observations with exact import-event lineage.
See [`docs/music-semantic-import-v0.3.4.md`](docs/music-semantic-import-v0.3.4.md).

CLI v0.3.5 requires an explicit mapped, omitted, or unknown disposition for
every analysis observation entering an editable model, and checks every model
evidence citation in reverse. See
[`docs/music-model-draft-v0.3.5.md`](docs/music-model-draft-v0.3.5.md).

CLI v0.3.6 binds each mutating repair operation exactly once to governed model
targets and an immutable human decision. It also requires the complete
technical, listening, and selection gate for every future candidate without
claiming that validation selects or approves one. See
[`docs/music-repair-intent-v0.3.6.md`](docs/music-repair-intent-v0.3.6.md).

CLI v0.3.7 recursively rechecks an exact repair candidate, C7 intent, repair,
EDL, and technical evidence before applying separate listening and selection
gates. Failed candidates remain auditable rejections and can never be promoted
to selected status. See
[`docs/music-repair-candidate-v0.3.7.md`](docs/music-repair-candidate-v0.3.7.md).

CLI v0.3.8 adds a strict same-music language-adaptation plan. It binds exact
canonical-source and approved-target text, complete ordered translation links,
the complete governed model, an exact-duration accompaniment, target-unit note
underlay, and decision-backed prosody exceptions. Validation does not translate,
perform, or approve either wording or release. See
[`docs/music-language-adaptation-v0.3.8.md`](docs/music-language-adaptation-v0.3.8.md).

CLI v0.3.9 binds a target-language vocal candidate to that exact adaptation,
then keeps performed-text audit, voice consent, creation provenance, lyric
listening, bilingual comparison listening, selection/rejection, and release as
separate gates. See
[`docs/music-language-performance-v0.3.9.md`](docs/music-language-performance-v0.3.9.md).

CLI v0.3.10 adds a complete score-driven limited-ensemble arrangement plan. It
classifies every governed model target, maps every source part and non-omitted
note, checks ranges and polyphony, and requires later score, audible comparison,
recognition, and selection gates. See
[`docs/music-arrangement-plan-v0.3.10.md`](docs/music-arrangement-plan-v0.3.10.md).

CLI v0.3.11 adds `music-arrangement-candidate-check` for the first exact
arrangement proof. It recursively binds the C11 plan, a materialized arranged
model, deterministic MIDI/MusicXML and audible score round trips, a blind
source comparison, and separate human listening, recognition, and
selection/rejection states. Validation is local and read-only; it does not
render, listen, select, publish, or make a private artifact shareable. See
[`docs/music-arrangement-candidate-v0.3.11.md`](docs/music-arrangement-candidate-v0.3.11.md).

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
cargo run -- changed-only-plan graph.json state.json --output-path plan.json
cargo run -- changed-only-result-receipt graph.json state.json plan.json result.json --output-path receipt.json
cargo run -- changed-only-state-advance graph.json state.json plan.json result.json receipt.json --output-path next-state.json
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

## Portfolio reuse contract

REEL's bounded reusable contract is the
[`reel.manifest.v0.2`](docs/production-manifest-v0.2.md) production schema and
the CLI's corresponding `validate` and `plan` behavior. Source repositories
retain scenario truth, rights decisions, and release authority; REEL owns
manifest validation and renderer-neutral planning. Internal Rust modules,
personas, scores, review prose, and work-specific production assets are not
shared APIs.

[ICELINES](https://github.com/giodl73-repo/ICELINES) is the current portfolio
adopter. Its hockey-film templates record REEL provenance separately and declare
`reel.manifest.v0.2` as their production handoff target. Those templates are
consumer-owned pre-conform overlays, not valid REEL manifests until converted
and accepted by the pinned REEL CLI. Compatibility follows the manifest version
and validation behavior rather than the REEL crate version alone.

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

REEL uses separate licenses for software and content. Source code,
executable scripts, tests, configuration, and ordinary software
documentation are MIT-licensed (copyright Gio Della-Libera). Original
non-software content is licensed CC BY-NC 4.0 (copyright Gio Della-Libera);
commercial use of that content requires separate written permission.
Third-party material remains under its own terms.
See [LICENSE](./LICENSE) for the complete notice.
