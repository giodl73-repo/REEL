# Motioncraft execution checkpoint

## State

Goal: execute the full reviewed Motioncraft plan, including scene-engine
consumption and CAIMITOS new/existing episode adoption. Status: active;
no completion claim. The compiler, authoring integration, actual scene camera
execution and indexed evidence are implemented; the complete goal is not done.

Worktree: `C:/Users/giodl/.codex/worktrees/motioncraft/REEL`.
Branch: `codex/motioncraft`. Baseline: `4effab1`.

## Initial implementation evidence (historical checkpoint)

- Imported the reviewed v4 plan and its twelve role finding dispositions into
  the current REEL codebase.
- Added `reel-assembly::motioncraft` with strict authored phases, bounded
  geometry/curves, explicit native/working/delivery clocks, optional static
  elements, reduced motion, and review-frame schedules.
- Verified the full `reel-assembly` suite: 42 tests across unit, authored-trigger,
  graph-contract and text-trigger checks.
- Verified 19 scene-authoring tests and 18 scene-delivery tests including the
  ignored FFmpeg checks. Tests cover inherited direction, language/event scope,
  fingerprints, native timing, rendered hold/push pixels, reduced motion,
  unchanged audio bytes and indexed review frames.
- Verified all three scene-build CLI tests. The independent-scene test upgrades
  episode direction, rejects its old job, compiles selected delivery, builds
  through the actual scene engine and restores the old authoring revision.
- The latest input-snapshot provenance fix passes the same independent-scene
  integration test. Formatting and diff checks pass.
- Current dependencies are locally usable: Cargo, FFmpeg, FFprobe and Python
  are available. Subsequent retained study/package and consumer-baseline
  evidence is recorded below.
- Recorded reuse/gap map in `docs/motioncraft-reuse-map.md`.

## Implemented handoff

Episode default direction and scene defaults/language-event overrides resolve
with whole-object precedence. They affect only consuming language fingerprints;
absent direction preserves legacy fingerprints. The selected native scene
compiler emits phased camera plans into actual delivery jobs, preserving native
sample clocks. Scene build compares authored direction with the selected job;
the scene engine validates the plan against native spans and allocated frames.

The renderer uses bounded fractional perspective sampling. Directed scene
outputs automatically include exact indexed PNGs, phase/attachment metadata,
contact sheet and silent quarter-speed playback. Receipt/check consumes all
additional artifacts; PNG pixels are independently compared with indexed
decoding of the rendered picture. The test also compares an extracted PNG with
a separate full-video decode, avoiding a shared-indexing false positive.

Compiler receipts now hash the exact parsed authoring/profile/graph/alignment-
manifest buffers. Native alignment bytes remain checked against scoped selected
asset hashes. Jobs likewise use the exact parsed job buffer for plan identity.

See `docs/motioncraft-authoring.md` for actual fields, supported treatment,
commands, exact-duration defaults and upgrade/rollback procedure.

## Next bounded implementation

Complete playback-based evaluation of the retained matched queue-explainer
variants and record the quality comparison. Visual-direction/profile support,
protected geometry and explicit native-duration fitting are now implemented.
Complete consumer integration when its lane is established.

The initial direction type currently describes bounded zoom and geometric
review intent. Other element motion and typography need explicit execution
contracts before the queue-explainer proof; unsupported intent must fail.

## Pending consumer lane

Read-only CAIMITOS diagnostics found `production-engineering` absent from the
current registry and `animation-vfx` gated as requiring a dedicated worktree.
Requested a lane assignment from the user asynchronously. No CAIMITOS code or
registry has been changed. This affects consumer writes only; REEL work can
continue. Do not classify the whole goal as blocked while useful REEL work
remains.

## Outstanding acceptance

Remaining: complete immutable intent/selected-revision binding audit,
queue playback comparison and quality evaluation, CAIMITOS adapter adoption,
consumer new/upgrade/rollback canary, cache hydration and verified package,
and requirement-by-requirement final audit. No human review, approval or
viewer comprehension is claimed.

## Native-duration and scene-engine checkpoint

Explicit `fit_native_duration` now resolves reusable phase templates against
native shot durations without changing audio samples. Delivery-profile safe
areas and declared protected regions are validated through camera transforms.
Typed palette/typography/reference intent is retained in indexed evidence;
selected artwork remains responsible for realizing those visual choices.
Eight focused Motioncraft compiler tests pass.

The synthetic landscape queue study has compiled four selected semantic events
and rendered through `reel-scene-build`, producing native stems, master/review
media, indexed frames, a contact sheet and quarter-speed playback. Artifacts
are under `target/motioncraft-demo-landscape/render-revised`. The contact sheet
has been inspected for framing and text retention. This is one revised render,
not the matched comparison or evidence of a quality-score improvement.

CAIMITOS new-episode authoring, existing-episode upgrade and rollback through
its actual adapter and the REEL scene engine remain completion requirements.
No CAIMITOS readiness or full project completion is claimed at this checkpoint.

## Matched study checkpoint

All six native renders now exist: baseline, revised and reduced for 1280x720
and 720x1280. Each has 450 decoded frames at 30 fps and 720,000 native samples.
`tools/motioncraft_study_review.py` verifies identical D/E/M/mix WAV bytes and
picture timing within each profile, then reuses the compiler's revised review
schedule to extract matched baseline/revised/reduced frames. It produces an
indexed inventory, captioned muted derivatives and labelled matched boards.
Original selected graphics/audio and runtime media remain outside git.

Generate a new study with `py tools/motioncraft_demo.py <new-root> --font
C:/Windows/Fonts/arial.ttf`, adding `--width 720 --height 1280` for portrait.
Compile each `compile-<variant>.json` using `reel-scene-authoring
compile-delivery`, then render `compiled-<variant>/build.json` with
`reel-scene-build build`. Supply the study as project and asset root. The build
manifest argument is repository-relative. Finally run `py
tools/motioncraft_study_review.py <study-root> --font C:/Windows/Fonts/arial.ttf`.
All outputs are new-only. `--output-name` retains revised derivative sets.

The first caption set overlapped the landscape lower label. Current review
candidates use `review-comparison-v2`; inspected caption frames retain the
diagram label in both profiles. The earlier set is preserved as failure
evidence. Matched boards and sampled caption frames have been inspected;
playback evaluation and dimension scores remain open. No quality-target pass
is claimed from file presence or still images.

Current checks: full reel-assembly suite (45 tests), scene-authoring suite (19),
scene-delivery suite including real FFmpeg (18), independent scene-build tests
(2) and Rust format check pass.

## Verified package and clean reproduction

The study package inventories 627 components across both profiles using the
existing production-package contract plus additive source-asset/render-audio
kinds. REEL receipt/check passes with creative selection pending. A clean
landscape workspace hydrated authored inputs and selected objects, then
recompiled all six JSON outputs byte-for-byte and rendered through the scene
engine. All 450 decoded picture frames, four native WAV files and selected
review PNG pixels match the packaged reference. See
[commands and evidence scope](motioncraft-study-handoff.md).

Three Python transport failure/no-clobber tests and three Rust package tests
pass. Actual CAIMITOS adapter deployment, consumer upgrade/rollback and the
48,001/47,999-sample canary remain required; this generic study transport does
not replace them. Portrait clean reproduction and scored playback review also
remain open.

## Binding audit and craft evaluation checkpoint

The independent scene-build integration test now exercises current authored
intent changes, stale selected-graph locks and edited motion job bytes. Each
fails before creating an output directory. Restored exact bytes render
successfully. Motion is bound by the semantic snapshot's selected graph lock,
event/picture bindings and exact job hash; scene build additionally checks
the authored effective direction. No second selection system was added.

Clean portrait reproduction now passes the same six compiled files, four
native audio files, 450 decoded picture frames and 44 review samples as the
landscape proof. The simulated core-role review now includes normal/quarter
playback and phone-width inspection. Its bounded REEL judgment meets the
study target at landscape 69 to 72 and portrait 70 to 73; see
[findings and score evidence](../signals/roles/check/reel-motioncraft-queue-comparison-2026-10-05.md).
This is not a human comprehension test, creative approval, or full project
acceptance. Actual CAIMITOS adapter deployment and new/upgrade/rollback
canary remain required.

## Actual consumer baseline checkpoint

The current CAIMITOS sanitized adapter has now produced a legacy baseline in
the REEL-owned worktree. REEL render/check and independent decoded PCM/frame
inspection pass at 48,001/47,999 native cue samples, 96,000 total samples,
48 frames and effect sample offset 123. See
[exact commands, hashes and scope](motioncraft-consumer-baseline.md).
Consumer code remains read-only: the latest lane diagnostic still requires
Asset Authority provisioning before production writes. This is baseline
evidence only; no Motioncraft adapter adoption or upgrade is claimed.

## Requirement audit checkpoint

The full-plan [acceptance audit](motioncraft-acceptance-audit.md) now separates
REEL evidence from actual consumer requirements. It also identifies open
cadence-check and caption-band contract integration instead of assuming
indexed extraction covers those gates. The compiler now samples concurrent
moving-element overlap interiors with bounded pairwise work; nine focused
Motioncraft tests and the full 46-test assembly suite pass.

The scene-build fixture's duplicate handwritten phase schedule was removed
in favor of the shared authored fixture. Its exact compiled job hash matches
the pre-removal capture, and the actual scene-build test passes. Two phased
delivery tests including FFmpeg pass after the overlap sampler change. These
are progress checkpoints; the full goal remains incomplete.

## Native-frame cadence checkpoint

The additive `reel-scene-cadence` command verifies existing scene receipts and
reuses the legacy motion-check luma analyzer and thresholds with exact frame
trims. All six study renders pass; reports are retained under each profile
root at `cadence-r1/{baseline,revised,reduced}.json`, outside render receipts.
Revised shots each have one near-stationary moving transition: 1/10 for the
first shot and 1/14 for later shots, within the existing maximum 0.10.
All intended holds are stationary. Static and reduced shots also pass.

Regression checks reject frozen moving intervals even beside valid holds, and
refuse whole-frame hold allowances for post-compose/layered treatments. The
patterned camera/reduced FFmpeg render and existing sprite/camera expectation
tests pass. These are cadence measurements, not viewer comprehension or final
creative approval. Caption-band mapping and actual CAIMITOS adoption remain
open completion requirements. Existing handoff-r1 is unchanged; cadence reports
will need inclusion in a new final package, with the final producer pin.
