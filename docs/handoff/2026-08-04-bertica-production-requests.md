---
work: bertica-cross-repo-request
stage: handoff
format: short-film
style: illustrated-2d
source_scenario: C:/src/bertica
author: bertica-production
rubric_version: v0.1
created: 2026-08-04
updated: 2026-08-05
sources:
  - C:/src/bertica/production/reel
  - C:/src/bertica/production/visual
  - C:/src/bertica/production/prevoice-scene-packets
---

# BERTICA requests for REEL

Please read this as a consumer-driven improvement request from *El camino de los
caimitos*. BERTICA will continue to own manuscript truth, voice consent, private
photographs, character facts, and approval. REEL should provide a portable
production grammar and deterministic transformations; BERTICA should not need a
runtime dependency on REEL internals.

## Current production evidence

BERTICA has built five private illustrated Spanish animatics through REEL-shaped
YAML manifests: Moro, Herrera arriving in Melena, Don Tancredo/the wedding, Papo
and Cachita, and the Nochebuena Riverita rescue. The experiments exposed four
repeated costs:

1. We must prepare scenes before an approved voice exists, but the current
   contract requires positive scene and shot timing.
2. When Bertica's narration was judged too fast, changing the effective pace to
   85 percent required shot starts/durations, captions, audio segments,
   transitions, and total export duration to be kept synchronized manually.
3. Andresito's poem and Bertica's prose are separate speakers with an important
   protected pause, but the manifest treats narration mainly as shot text and a
   single broad `narration_voice` direction.
4. Character likeness and continuity are increasingly photo-grounded, while the
   original private photographs must remain local and must not be silently sent
   to an image/video provider.

The next BERTICA queue begins with a Mayabeque beach scene: an overloaded launch,
a large ensemble with fixed seating geography, a rising waterline, a family
quarrel, a near-capsize, Amado Rosa pushing the boat back with his shoulder, and
a mother-child reunion. It is a useful stress test for all requests below.

## P0 — Untimed pre-voice manifests

Please add an explicit planning stage in which scenes and ordered shots may exist
without invented durations.

Desired behavior:

- A manifest can declare `timing_status: untimed|guide|conformed|locked`.
- Untimed shots preserve order, source cues, visual purpose, camera/motion intent,
  speaker, and narration/caption text while omitting start/duration.
- `validate` accepts untimed planning manifests but clearly reports that render,
  preview, caption export, and delivery commands are gated.
- `plan` or a dedicated command produces a useful untimed shot/storyboard plan.
- A later deterministic conform operation creates a new timed derivative rather
  than overwriting the planning source.

Acceptance test: validate and plan the Mayabeque sequence before any audio file
or duration exists, while render-oriented commands fail with a precise
`timing not conformed` message.

## P0 — Audio conform and atomic retiming

Please make voice-driven timing a first-class operation.

Desired behavior:

- Accept one or more measured narration cue files, each with speaker/cue identity
  and optional protected head/tail silence.
- Conform shots from cue durations and declared visual allocation rules.
- Support a deterministic global or per-speaker tempo derivative (for example,
  Bertica at 85 percent effective tempo while leaving Andresito and the
  poem-to-prose pause unchanged).
- Recompute as one atomic transformation: scene spans, shot starts/durations,
  narration segments, caption cues, transitions, platform/export duration, and
  review summaries.
- Refuse partial output if any dependent timeline becomes inconsistent.
- Preserve lineage: input manifest, audio hashes, transform parameters, output
  manifest, and tool version.

Acceptance test: take a two-speaker poem/prose scene, slow only Bertica, preserve
the exact Andresito timing and protected pause, and produce a manifest whose
scene, shots, captions, audio, and export totals validate without manual edits.

## P0 — Speaker-aware narration cues

Please separate narration identity from shot prose.

Suggested portable concepts:

- `speakers`: stable IDs, display names, language, pronunciation profile,
  performance direction, consent/approval status reference, and whether the
  asset is human, guide, or synthetic.
- `narration_cues`: stable cue ID, speaker ID, exact text or upstream text
  reference, source block range, shot/scene association, take/audio reference,
  and pause/breath policy.
- Protected transitions such as `poem_to_prose_pause` that conform/retime tools
  cannot change unless explicitly unlocked.

REEL should store approval references and gates, not infer that a voice is
authorized merely because an audio path or speaker name exists.

## P1 — Source-range and omission provenance

Memoir adaptation needs finer provenance than a single scenario path.

Please support:

- Contiguous and discontiguous upstream ranges such as manuscript blocks
  `1065–1073` plus `1115–1140`.
- Per-cue and per-shot source references.
- Explicit omissions and the permitted bridge: silence, title card, archival
  image, or separately approved adaptation.
- A generated source-coverage report that makes invented narration and
  unattributed dialogue easy to detect.

Acceptance test: the Mayabeque manifest discloses blocks 1074–1114 as omitted
and proves that every spoken line comes from one of the selected ranges.

## P1 — Privacy-safe continuity registry

Please add a portable continuity/reference contract that can ground generated
characters without embedding or uploading private photos.

Desired behavior:

- Stable character/entity IDs shared across works and versions.
- Age-at-scene, build, face/hair/clothing observations, confidence, provenance,
  and human-confirmation status.
- Local-only reference asset IDs/hashes and an explicit policy such as
  `provider_transfer: forbidden|approval_required|approved`.
- Prompts and provider handoffs receive approved textual observations by default,
  not the private source file.
- A provider package lists every asset/text field that would leave the machine
  before execution and blocks on missing approval.

Acceptance test: Amado Rosa can remain visually consistent from a local photo-
derived description while the photograph path is absent from an AI-provider
handoff.

## P1 — Variant lineage and review selection

Please make versions such as `v1`, `v2-photo-grounded`, and `v2-85pct` explicit
derivatives rather than filename conventions.

Useful fields/operations:

- parent manifest and transformation reason;
- changed dimensions: voice, pace, likeness, frames, captions, mix, or edit;
- private review candidate versus principal-approved version;
- separate Bertica and Herman findings without collapsing disagreement;
- command/report that identifies the latest review candidate for each scene
  without implying approval.

## P2 — Long-still motion quality controls

Our illustrated audiobook scenes use long cinematic stills. Please consider
manifest-level checks for:

- maximum uninterrupted hold and low-motion warnings;
- safe pan/zoom bounds that never expose blank canvas or crop a face;
- focal point and protected-region metadata;
- parallax/depth-layer hints without requiring a specific renderer;
- continuity checks for left/right position and eye line across adjacent shots;
- a no-lip-sync narration mode;
- clean A/B outputs for narration-only versus effects/music.

The goal is not constant movement. It is controlled, inspectable motion that
protects faces, captions, and the one emotional beat assigned to each frame.

## v0.2.1 follow-up after BERTICA adoption

BERTICA has now exercised REEL v0.2 against all twelve legacy manifests, seven
enriched preferred artifacts, and six native untimed pre-voice scene plans. The
individual-scene contract is strong. The remaining production gap is composing
those scenes into a long-form audiobook series without copying their internals
or weakening source and approval boundaries.

### P0 — Native episodic-series composition

Please add a real `episodic-series` contract that references scene or segment
manifests rather than embedding their shots, cues, private references, or
conformed artifacts.

Required hierarchy and identity:

- stable series, season, and episode IDs, including IDs such as `S2E02`;
- deterministic season/episode order and optional episode parts;
- original manuscript section titles and Andresito poem identities alongside
  working production titles;
- references to child scene manifests by path, work ID, expected hash, and
  accepted timing/review state;
- inheritance of series-wide platform, disclosure, caption, continuity, and
  privacy defaults without erasing scene-specific overrides.

Required episode metadata:

- exact canonical source ranges and disclosed omissions across every child
  scene;
- chronology/place, memory mode, sensitivity/risk, recurring motifs, and
  continuity entry/exit state;
- `untimed`, `guide`, `conformed`, `locked`, and human-review status;
- raw orientation, measured narration, protected pauses, scene duration, and
  total episode/season runtime as distinct values;
- separate Bertica and Herman findings, with no inferred consensus or approval;
- explicit dependencies when an episode continues a poem, source section,
  character state, or unresolved visual motif from another episode.

Validation and reports should detect:

- missing, duplicated, overlapping, or out-of-order source ranges;
- repeated or missing scene-manifest references;
- a poem separated from its associated prose without an explicit approved
  structure;
- incompatible speaker, continuity, platform, privacy, or timing states;
- an episode marked release-ready while any child remains untimed, unreviewed,
  privacy-blocked, or source-incomplete;
- mismatches between child-manifest duration and episode/season totals.

Suggested commands:

```text
reel series-validate production/reel/series/el-camino-v1.yaml --output json
reel series-plan production/reel/series/el-camino-v1.yaml --output json
reel series-coverage production/reel/series/el-camino-v1.yaml --output json
reel series-review-queue production/reel/series/el-camino-v1.yaml --output json
reel episode-compose production/reel/series/el-camino-v1.yaml S2E02 \
  --output-dir production/reel/composed/S2E02-v1
```

Composition should publish a new atomic episode packet with its own manifest,
captions, lineage, coverage, and duration report. It must not overwrite or
silently retime child packets.

Acceptance test: import BERTICA's sanitized five-season slate with exactly ten
episodes per season and source coverage from blocks 34–4419. Validation should
confirm 50 ordered episodes and continuous coverage, while leaving all human
approval fields false/open.

### P1 — Caption/cue import

Please add a deterministic `cue-import-srt` operation for upgrading existing
timed proofs:

```text
reel cue-import-srt manifest.yaml captions.es.srt \
  --speaker bertica-narrator \
  --output upgraded-with-cues.yaml
```

It should parse millisecond cue timing, preserve caption text, associate cues
with overlapping shots, require source and speaker assignment, write a new
derivative with lineage, and reject overlaps or duration beyond the declared
work. A mapping file should allow a two-speaker threshold such as Andresito's
poem, a protected silence, and Bertica's prose.

Acceptance test: the sanitized two-speaker fixture imports four poem captions
and prose captions, assigns each to the correct speaker and shots, preserves the
1.5-second poem-to-prose pause, and reproduces the original SRT exactly.

### P1 — Scene-to-episode composition

Please support ordered composition of several conformed child scene packets
into one episode without flattening their source, speaker, privacy, variant, or
review records. Composition may add episode-level cards, credits, and approved
bridges, but these must be separately attributed production units.

Acceptance test: compose a poem threshold, a prose scene, and an end card. The
result preserves every child hash and protected pause, creates continuous
captions, and reports the card as production-authored rather than manuscript
narration.

### P1 — Shared continuity registry

The v0.2 per-manifest entity contract works, but BERTICA now needs a shared,
versioned registry referenced across works. Please allow manifests to cite an
external continuity registry by path, version/hash, and entity ID while keeping
scene-specific `age_at_scene`, clothing, condition, and confidence overrides.

The registry should record approved textual observations and local reference
policies once for recurring people, animals, houses, vehicles, and motif
objects. Provider packaging must still resolve only the approved observations
and never serialize forbidden local paths.

Acceptance test: young Herrera, later Herrera, Bertha María, Herminio, Moro,
Amado Rosa, and the caimito road remain stable across several scene manifests;
a provider package contains the applicable textual observations but no private
photo path.

### Scope discipline

This follow-up does not request a monolithic runtime, a BERTICA-specific schema,
automatic editorial approval, or a requirement that REEL own manuscript truth.
The series layer should remain a portable composition/index contract over
independent scene artifacts. BERTICA continues to own canon, consent, private
assets, historical interpretation, and actual principal decisions.

## v0.2.2 follow-up — smooth long-still motion

BERTICA's project owner observed that the current illustrated scene proofs feel
slightly jittery. The intended treatment is restrained “living still” motion;
the visible stepping is not intentional.

### Reproduced production defect

The affected outputs are valid constant-frame-rate 1280×720 H.264 files at 24
fps. This is not a variable-frame-rate or dropped-frame problem. Both BERTICA's
prototype renderer and REEL v0.2.1 currently scale/crop to delivery resolution
before applying `zoompan`. A typical 3.5–4 percent pan or push therefore travels
only about 45–50 output pixels across a 15–25 second shot. Fractional positions
are quantized: several frames remain nearly stationary, followed by a one-pixel
jump.

Optical-flow sampling of actual private BERTICA v2 exports found the following
fractions of near-stationary frame transitions during declared moving shots:

| Sample | Declared motion | Near-stationary transitions |
|---|---|---:|
| Herrera arrival | push | approximately 93% |
| Don Tancredo mystery box | pan | approximately 90% |
| Don Tancredo opening | push | approximately 83% |
| Moro | push | approximately 66% |

No private render, manuscript text, photograph, or voice asset is required to
reproduce this. A synthetic high-detail illustration or test grid moving about
45 pixels over 20 seconds at 24 fps should expose the same cadence.

### P0 — Subpixel-capable deterministic motion renderer

Please target this fix as REEL v0.2.2 while keeping the scene manifest schema at
`reel.manifest.v0.2` unless a portable motion field is genuinely necessary.

Required behavior:

- A declared moving shot must advance visually on essentially every output frame
  at ordinary audiobook motion rates; it must not alternate long runs of frozen
  frames with integer-pixel jumps.
- Use a genuinely subpixel-capable transform and interpolation path. An affine,
  perspective, browser/Remotion, or rigorously validated adaptive-supersampling
  implementation is acceptable. Merely increasing output fps while retaining
  integer crop coordinates is not.
- Apply deterministic ease-in/ease-out motion by default so pushes and pans do
  not start or stop mechanically. The curve and parameters must be recorded in
  the artifact report.
- Preserve exact `hold` and `hold-dark` semantics. The fix must not introduce
  constant motion where a still frame is artistically preferable.
- Preserve safe crop, focal point, protected-region, portrait/landscape,
  disclosure, caption, transition, audio, duration, and privacy behavior.
- Keep output constant-frame-rate and preserve the conformed timeline to within
  the existing one-frame duration tolerance.
- Record the motion backend/version, interpolation method, curve, working
  resolution or sampling strategy, fps, and any quality override in the sibling
  `*.artifacts.json` report.
- Provide an explicit legacy-render option for deterministic reproduction of old
  proofs, while making smooth motion the default for new v0.2.2 renders.
- Bound memory and render cost, fail clearly when a requested quality mode is
  infeasible, and leave no partial output or artifact report on failure.

Suggested CLI shape (names may change to fit REEL conventions):

```text
reel animatic-render manifest.yaml ... \
  --motion-quality smooth \
  --motion-curve ease-in-out

reel animatic-render manifest.yaml ... \
  --motion-quality legacy
```

### Automated acceptance tests

1. Render a sanitized 20-second, 1280×720, 24-fps pan that travels approximately
   45 pixels. Frame analysis must show continuous subpixel movement without a
   periodic one-pixel step cadence. Define and publish the metric and threshold;
   the old renderer must fail the test and the new renderer must pass it.
2. Repeat for centered `push` and `pull` treatments, including a long 25-second
   shot representative of narration slowed to 85 percent effective tempo.
3. Verify that a `hold` produces no transform drift and that `hold-dark` changes
   appearance without changing position.
4. Verify no blank canvas, protected-region violation, caption/disclosure
   regression, duration drift, variable frame rate, or audio/caption desync.
5. Run production CLI tests on the supported Windows/WSL and Linux paths and
   inspect the artifact report for complete motion lineage.
6. Produce a short side-by-side or alternating legacy/smooth sanitized proof so
   the cadence improvement can be judged visually rather than only by unit tests.

### BERTICA migration and consumption

BERTICA should not need to edit its current v0.2 manifests. After v0.2.2 lands,
it will first rerender one short existing proof through `animatic-render`, verify
the cadence metric and human-visible improvement, and only then regenerate the
five private reviewer scenes. Existing v2 files must remain preserved as legacy
artifacts until the smooth versions are approved. Voice choice, source timing,
visual content, and review status must not change as a side effect of this motion
repair.

Please respond with:

- the selected subpixel transform strategy and why it avoids integer stepping;
- the exact smooth and legacy commands BERTICA should run;
- the automated cadence metric, threshold, and before/after results;
- performance and memory cost at 1280×720 and 1920×1080;
- confirmation that current `reel.manifest.v0.2` files need no migration;
- the artifact-report fields BERTICA should retain with each rerender.

## Recommended implementation order

1. Untimed planning manifests.
2. Speaker-aware cues plus protected pauses.
3. Audio conform/atomic retiming.
4. Source coverage and omission reporting.
5. Privacy-safe continuity/provider package.
6. Variant lineage and long-still quality checks.

For v0.2.1, the recommended order is:

1. Native episodic-series schema and validation.
2. Scene-to-episode composition with atomic lineage.
3. Caption/cue import.
4. Shared external continuity registry.

For v0.2.2, the recommended order is:

1. Reproduce and measure the integer-step cadence with a sanitized fixture.
2. Implement a subpixel-capable smooth path plus explicit legacy mode.
3. Add motion lineage, duration/atomicity checks, and cross-platform tests.
4. Deliver a short legacy-versus-smooth visual proof and consumption commands.

## Cross-repo handoff request

Please respond with:

- which requests already exist in another form;
- the smallest schema/CLI slice you recommend implementing now;
- any BERTICA manifest you want copied as a fixture (prefer a sanitized textual
  fixture, not manuscript text, private photos, source voice, or binary renders);
- migration implications for the five current BERTICA manifests;
- the exact REEL artifact/command BERTICA should consume after the change.
