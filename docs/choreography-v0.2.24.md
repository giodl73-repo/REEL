# Choreography sidecars and blocking previews — v0.2.24

REEL choreography describes why multiple screen elements move together before a
renderer decides how they look. The additive `reel.choreography.v0.1` sidecar
owns normalized stage marks, exact beat frames, performer phrases, prop
ownership, spatial paths, and temporal curves. It does not change
`reel.manifest.v0.2` or require existing consumers to adopt choreography.

The first phrase vocabulary is deliberately small and domain-neutral:

- `approach` moves one performer to a named mark between two beats;
- `handoff` transfers one prop from its current owner to a target performer;
- `react` records a named pose at one exact beat.

`linear`, `arc-left`, and `arc-right` paths control spatial travel independently
from `linear`, `ease-in`, `ease-out`, `ease-in-out`, and `hold-then-burst` timing.
This supports smooth travel, designed holds, and intentional pose-to-pose energy
without treating continuous interpolation as the only definition of quality.

## Commands

```powershell
cargo run -- choreography-validate manifests/fixtures/choreography/simple-handoff.yaml

cargo run -- choreography-compile manifests/fixtures/choreography/simple-handoff.yaml `
  --output-path target/choreography-plan.json

cargo run -- choreography-preview manifests/fixtures/choreography/simple-handoff.yaml `
  --output-dir target/choreography-preview
```

`choreography-compile` flattens authored phrases into a
`reel.choreography-plan.v0.1` renderer handoff. The plan contains concrete motion
segments, resolved marks, handoff ownership, and reaction events. The source
sidecar hash binds it to the exact authored input.

`choreography-preview` atomically publishes:

- `blocking-preview.mp4`, a silent H.264 blocking animation;
- `resolved-plan.json`, the exact renderer-neutral plan;
- `path-overlay.png`, showing named marks and performer trajectories;
- `contact-sheet.png`, showing first, middle, and final blocking frames;
- `preview-report.json`, with hashes, sizes, timing, and FFmpeg version.

The preview uses abstract circles and labels by design. It answers blocking and
continuity questions before polished sprites, cels, footage, or a character rig
exist. A finishing adapter can consume the resolved plan without changing the
director-owned choreography.

Every phrase may carry a stable `id`. The compiled motion, handoff, or reaction
retains that identifier so review notes and downstream adapter diagnostics can
refer back to the exact authored intention rather than only a frame number.

## Validation

Validation rejects unknown marks, beats, performers, and props; out-of-range
coordinates and sizes; overlapping performer approaches; overlapping handoffs;
and any handoff attempted by a performer who does not own the prop at that beat.
The compiler also samples every resolved performer and prop frame, rejecting an
arc or handoff that leaves the normalized stage even when its named endpoint
marks are individually valid.

Domain libraries may map their own vocabulary onto these generic mechanics. For
example, a sports library can define a named play as a composition of approaches,
handoffs, and reactions. The sport-specific name does not belong in REEL.
