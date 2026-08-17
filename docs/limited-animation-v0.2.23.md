# Limited animation shots — v0.2.23

REEL can assemble an ordered set of authored cels as one timed shot. The
manifest owns the pose order and timing; the image-generation, drawing, or 3D
provider remains outside REEL.

```yaml
shots:
  - id: hit-to-box
    scene_id: penalty-sequence
    start_seconds: 0.0
    duration_seconds: 4.0
    media_kind: animation
    animation:
      timing_fps: 24
      frames:
        - asset: frames/001-anticipation.png
          hold_frames: 18
          pose: anticipation
        - asset: frames/002-contact.png
          hold_frames: 3
          pose: impact
        - asset: frames/003-referee.png
          hold_frames: 15
          pose: whistle-and-signal
        - asset: frames/004-penalty-box.png
          hold_frames: 60
          pose: reaction-hold
```

The sum of `hold_frames / timing_fps` must match `duration_seconds` within half
one timing frame. Each frame asset is resolved under `--asset-root`, prevented
from escaping that root, and independently recorded with a SHA-256 digest in
the animatic artifact report.

`animation` is deliberately a limited-animation contract rather than a model
API. Fast impact inserts can hold for one to three frames while anticipation,
reaction, and environment poses hold longer. A 5–15 second vignette commonly
uses 8–30 designed poses while REEL expands the holds to the delivery frame
rate. This makes stylized cel timing deterministic and reproducible.
