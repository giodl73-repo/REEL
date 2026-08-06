# Animatic verification in CLI v0.2.3

REEL CLI v0.2.3 hardens the v0.2.2 smooth-motion workflow without changing
`reel.manifest.v0.2`. Existing manifests require no migration.

## Render preflight

For smooth motion, REEL estimates the simultaneous memory cost of every moving
shot's perspective filter. The artifact report records the filter count, peak
estimate, and 2048 MiB maximum. A render above the maximum fails before FFmpeg
starts and recommends splitting the render or explicitly choosing the legacy
backend.

REEL also samples the full transform range for each shot. The source rectangle
must remain inside the canvas, and any declared `focal_point` or
`protected_regions` must remain visible at every sampled extreme. A violation
fails before publication. Holds use the full frame and no perspective filter.

## Manifest-aware cadence

```text
reel motion-check manifest.yaml review.mp4 --output json
```

The command trims the rendered video to every conformed shot and applies the
same adjacent-frame luma metric as `motion-analyze`. Moving treatments must have
at most 10% near-stationary transitions. `hold` and `hold-dark` treatments must
have at least 85%, so an intentional hold no longer looks like a failed pan.
The report also includes transform-safety evidence and fails the process when
any shot fails.

## Artifact verification

```text
reel animatic-check review.artifacts.json --output json
```

The verifier rejects dry-run or unknown reports and checks:

- every recorded input hash and the output hash and byte count;
- manifest work, duration, shot order, motion lineage, and safety evidence;
- ordered, non-overlapping SRT cues within the conformed duration;
- exactly one H.264/yuv420p video stream at the reported dimensions and
  constant frame rate;
- duration within one frame and the expected silent or single-audio-stream
  policy.

The artifact report schema remains `reel.animatic-artifacts.v0.1`; the added
lineage fields are additive. Verification output uses
`reel.animatic-check.v0.1`, and cadence output uses
`reel.motion-check.v0.1`.

## CI reproducibility

CI installs the FFmpeg 6.1 GPL build through an immutable setup-action commit,
verifies the installed version and required filters/encoder, applies a 20-minute job
timeout, and cancels superseded runs for the same branch. The real-FFmpeg gate
renders the sanitized proof, runs both verification commands, and uploads their
JSON evidence.
