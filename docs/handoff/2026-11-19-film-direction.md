# Frozen visual direction, cel targets and drawing cadence

Film scene controls now have an explicit v2 path. A request with schema
`reel.workflow-film-scene-control.v2` must name `direction_path`; the exact JSON
asset must be declared in task inputs/source bindings. The task must additionally
declare required `direction.json` output (`application/json`, at most 1 MiB).
Legacy v1 requests omit the new field and retain their five-output protocol.
An old runtime refuses the new request rather than silently ignoring direction.

`reel-workflow-native preview-film-direction PACKET NATIVE_PROFILE` reads exact
available snapshot bytes and emits a private preview without accepting claims,
calling providers, rendering or publishing outputs. It requires v2 and does not
acquire missing bytes. Composite tasks still require their existing child-claim
protocol; this job-independent command does not bypass it.

Example settings:

```json
{
  "schema": "reel.workflow-film-direction-settings.v1",
  "policy": {
    "schema": "reel.scene-policy.v1",
    "policy_id": "project-normal",
    "target_composition_seconds_min": 0.1,
    "target_composition_seconds_max": 2.0,
    "hard_unchanged_composition_seconds_max": 4.0,
    "semantic_cuts_required": true
  },
  "desired_seconds_per_cel": {"numerator": 1, "denominator": 3},
  "delivery_fps": {"numerator": 24, "denominator": 1},
  "cadence": {"kind": "every-frames", "frames": 2},
  "raster_size": [1920, 1080],
  "visual_school": "realistic",
  "performance_style": "theatrical",
  "animation_technique": "drawn animation",
  "references": [],
  "budget": {
    "drawing_slots": 1024,
    "stored_bytes": 16777216,
    "raw_rgb_bytes": 1073741824,
    "estimated_bytes_per_candidate": 1000,
    "cost": {"unit": "authored-estimate-units", "microunits_per_candidate": 7}
  }
}
```

The effective policy ID must match the authored scene override or episode policy.
Projects can supply explicit effective files for different scenes/sequences;
there is no implicit inheritance or substitution of current settings. Visual
school, performance and technique are separate bounded authored descriptions,
so other schools do not require a new renderer enum. Up to sixteen exact reference
descriptors may be named; their bytes must also be declared inputs. This grants
no egress permission, style/likeness rights or creative approval.

## Production checks and forecast

The producer uses the selected-take clocks and resolved language-local semantic
spans, converts different cue rates to an exact common integer clock, and reuses
`ScenePolicy` composition auditing. Adjacent primary drawings with identical byte
hashes form a continuous hold even across logical aliases or cue boundaries.
The hard hold maximum refuses production controls; preferred duration bounds
produce hints. This audit measures selected primary drawings: camera/VFX changes
do not count as new drawings in this forecast.
The preview's `sample_rate`/`duration_samples` describe common analysis ticks;
they do not change or resample the original recordings.

The desired cel duration gives a composition-count target, not permission to
invent semantic cuts or alter narration. `held-cels` forecasts one drawing per
composition; `every-frames` specifies drawing exposure separately from delivery
rate. At 24 delivery fps, exposure every two frames represents approximately
twelve drawings per second within shots. The preview flags missing composition
density and `exposure_asset_binding_required`; it never pretends those additional
drawings exist or have been selected.

Frame starts/shared boundaries use floor and the final boundary uses ceiling,
matching the existing sample-to-delivery-frame convention. Zero-frame compositions
and density above the delivery grid refuse. The forecast reports actual per-run
drawing slots and a conservative target-slot upper bound allowing each desired
composition to restart its cadence. Selected immutable drawing bytes are reused;
additional candidates contribute the authored byte estimate and optional authored
cost rate. The rate is input data, not a live service quote. Raw RGB demand is
frames × width × height × 3, not an encoded movie-size or CPU-time prediction.
The storage estimate covers drawing assets; audio, fonts, control objects and
replication require their own admission accounting.
Drawing/storage/raw-frame budgets refuse rather than silently reduce density.

Limits include 4,096 spans, at most one hour, source rates through 384 kHz, a common
clock fitting u32, at most 240 delivery fps and one million frames, raster axes
through 8,192 pixels, and bounded metadata/estimates. Actual native lifecycle and
renderer admission limits still apply independently.

The sixth output freezes settings/hash, language, clock, per-run ranges, targets,
estimates and unresolved exposure demand. The receipt binds it to the exact
request/scene/input set; native output budgeting now includes its extra object.
It follows the same owned deterministic dispatch, candidate ingestion, retained
proof and replay as the existing five controls. V1 compiled control bytes remain
unchanged when the new setting is absent.

## Remaining renderer integration

This adds direction-aware control generation and planning, not an animated frame
sequence renderer. Existing selected semantic cuts are preserved. The future
scene-control-to-render adapter must consume the direction requirement and bind
actual exposure assets/renderer support before claiming cadence was rendered.
Current graph/control outputs alone are not such a claim. Automated storyboard
expansion, drawing generation and binding to the root exposure-sheet format remain
separate work. No media production or provider call occurs during forecasting.

Applied animation-direction, editing and rights/provenance producer checklists:
style and feasibility are explicit; hard timing constraints and language clocks
are preserved; unmet density remains visible; technical pass grants no creative,
translation, reference-use or release decision.

## Qualification

Source `2132f9b80802db2814bf8a6fc64d9518854c1ed3` has 94 passing
workflow tests, including ES/EN targets, unchanged legacy control bytes, drawing
cadence independent of delivery fps, reuse without regeneration, cross-cue alias
coalescing, clock overflow and budget/invalid-reference refusal. Strict workflow
Clippy and strict native Clippy with/without film support pass. The standard native
regression suite passed 58 tests and all 247 SDK source pins before the final
preflight CLI/reuse refinements; those refinements received pure tests, rebuilt
tools, strict Clippy and the explicit native execution below. All 1,897 archived
source files verified.

The existing-pool Spanish native fixture passed in 416.11 test seconds (416.79
wall seconds), retaining and reopening six controls and replaying its original
worker. The native preview command left claims unchanged and called no provider.
The measured cuts remained 2,400/8,000 samples; the preview froze 24 delivery frames,
three desired compositions, thirteen drawing slots for actual cuts and a fourteen
slot conservative target. Style remained `realistic`, performance `theatrical`,
with unresolved exposure binding explicitly true. The preflight ran after fixture
input publication, not before those bytes existed. No footage was rendered.

Preserved roots: `F:/src/tig-dev-temp/.tmpUnSUy9` (operation) and
`F:/src/tig-dev-temp/.tmpVXJjT6` (source). Frozen tools, source, controls and raw
check/fixture logs are pinned by
`F:/src/tig-dev-temp/reel-film-direction-20261119-v1/qualified-tools.json`,
SHA256 `6f8d57a86fe964d67e1e1fef9bf51f087aa563b8abeec28877222fc658d7eae5`.
