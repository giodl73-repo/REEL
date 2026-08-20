# Animation exposure sheets (v0.2.43)

REEL v0.2.43 validates an owner-authored, renderer-neutral exposure sheet for
one exact production shot. The sheet records explicit inclusive frame spans for
drawings, poses, props, effects, camera states, and dialogue relationships.

REEL does not create drawings, choose poses or mouth shapes, interpolate
motion, render frames, mutate a DCC project, or grant creative approval.

## Contract

```yaml
schema: reel.exposure-sheet.v0.1
sheet_id: example-shot-xsheet
fps: 24
duration_frames: 96
shot_ref: primary
production_binding:
  manifest: ../production/manifest.yaml
  manifest_sha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
  work: owner-work
  shots: { primary: shot-001 }
tracks:
  - track_id: character-a
    kind: drawing
    coverage: complete
    exposures:
      - { start_frame: 0, end_frame: 7, exposure_id: pose-a }
      - { start_frame: 8, end_frame: 15, exposure_id: pose-b }
```

`fps` is an animation working timebase, not a delivery-frame-rate claim.
`duration_frames` must align with the bound shot's canonical whole-millisecond
duration within half a frame. The report exposes the exact error against that
canonical duration as `duration_delta_milli_frames`, where 500 is exactly half
a frame regardless of frame rate. Production binding does not preserve or
claim sub-millisecond source precision.

Tracks and exposures must be strictly ordered. Frame spans are inclusive,
non-overlapping, and within `[0, duration_frames - 1]`. A `complete` track must
cover every frame without gaps. A `sparse` track may have gaps, which remain
explicit in the report. Adjacent identical bindings must be merged.

Each exposure has an owner-defined portable `exposure_id`. `asset_sha256` is
optional because an exposure sheet may precede final art; the report separates
declared asset hashes from planned exposures and explicitly says that it did
not verify asset bytes. Optional sorted `cue_ids` must name narration cues
attached to the same production shot.

## Check and publish

```powershell
cargo run --bin reel -- exposure-sheet-check shot-xsheet.yaml `
  --output-path shot-xsheet-report.json --output json
```

The no-clobber report is deterministic and path-free. It records exact sheet
and production-manifest hashes, shot/timebase identity, per-track frame
coverage and gaps, declared-hash versus planned exposure counts, and explicit
authority boundaries. Every identity copied into the report must use portable
ASCII letters, numbers, hyphens, or underscores; path-like work and shot
identities are rejected.

The report is technical timing evidence only. Exposure IDs and asset hashes do
not imply selection, approval, rights clearance, publication, or release.
