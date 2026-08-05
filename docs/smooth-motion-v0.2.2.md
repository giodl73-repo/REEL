# Smooth long-still motion in CLI v0.2.2

REEL CLI v0.2.2 repairs slow illustrated pans, pushes, and pulls without changing
the `reel.manifest.v0.2` scene contract. Existing conformed manifests require
no migration.

## Rendering strategy

Smooth mode scales and crops each source to the delivery canvas, then uses
FFmpeg's `perspective` filter with `eval=frame` and cubic interpolation. Each
frame samples a fractional source rectangle. A cosine ease-in/out curve changes
that rectangle continuously, so a 45-pixel move over 480 frames does not wait
for an integer crop coordinate before changing the image.

The sampled rectangle always remains inside the scaled canvas. Push and pull
stay centered; pans use the existing 3.5 percent overscan. `hold` and
`hold-dark` bypass the transform entirely, and `hold-dark` retains only its
appearance adjustment.

Smooth mode is the default:

```text
reel animatic-render manifest.yaml --asset-root assets --audio mix.wav --captions captions.srt --output review-smooth-v022.mp4 --motion-quality smooth --motion-curve ease-in-out --format json
```

Legacy v0.2.1 reproduction remains explicit:

```text
reel animatic-render manifest.yaml --asset-root assets --audio mix.wav --captions captions.srt --output review-legacy-v021.mp4 --motion-quality legacy --format json
```

REEL refuses to overwrite an existing video or sibling artifact report. Use a
new derivative filename so approved and legacy renders remain preserved.

## Published cadence metric

`motion-analyze` decodes adjacent frame differences through FFmpeg
`tblend=all_mode=difference,signalstats`. A transition is near-stationary when
its mean absolute luma difference (`YAVG`) is below `0.001`. A declared moving
shot passes when no more than 10 percent of transitions are near-stationary.

```text
reel motion-analyze review-smooth-v022.mp4 --output json
```

On the sanitized 20-second 1280x720, 24-fps fixture:

| Backend | Near-stationary transitions | Fraction | Result |
|---|---:|---:|---|
| Legacy zoompan | 333 / 479 | 69.52% | fail |
| Smooth perspective | 4 / 479 | 0.84% | pass |
| Smooth 25-second push | 33 / 599 | 5.51% | pass |
| Smooth 25-second pull | 33 / 599 | 5.51% | pass |

The metric is intentionally a cadence gate, not an artistic score. Exact holds
are expected to fail the moving-shot gate because they should remain still.

## Cost and limits

Measured on WSL2 with FFmpeg 6.1.1, libx264 `slow`, CRF 18, and the 20-second
fixture:

| Output | Wall time | Maximum RSS |
|---|---:|---:|
| 1280x720 at 24 fps | 6.53 s | 540 MiB |
| 1920x1080 at 24 fps | 7.12 s | 1,309 MiB |

Artifact reports carry a conservative memory estimate. Smooth and legacy
renders are bounded to 2,073,600 pixels (1080p landscape or portrait) and 60
fps. Larger requests fail before FFmpeg runs and publish no partial video or
report.

## Artifact retention

Keep the sibling `*.artifacts.json` with every rerender. Its additive motion
lineage records:

- REEL tool and FFmpeg versions;
- backend, interpolation, quality, and effective curve;
- sampling strategy, working dimensions, fps, and resource limits;
- per-shot treatment and frame count;
- manifest, visual, caption, and audio hashes;
- exact FFmpeg arguments;
- output hash, byte size, and measured duration.

The report schema remains `reel.animatic-artifacts.v0.1`; the motion fields are
additive. Output publication uses temporary files, verifies duration within the
existing one-frame tolerance, and only then publishes the video and report.
