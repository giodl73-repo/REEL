# Source-to-camera mapping (v0.3.0)

REEL v0.3.0 can bind a contained still-camera track to exact source and
working-canvas geometry:

```yaml
visual_fit: contain
camera_track:
  timing_fps: 24
  geometry:
    source_width: 960
    source_height: 420
    canvas_width: 1280
    canvas_height: 520
  keyframes:
    - { frame: 0, center_x: 0.50, center_y: 0.50, zoom: 1.00 }
    - { frame: 120, center_x: 0.50, center_y: 0.50, zoom: 1.50 }
focal_point: { x: 0.50, y: 0.50 }
protected_regions:
  - { id: owner-label, x: 0.42, y: 0.44, width: 0.16, height: 0.12 }
```

The source dimensions must match the actual visual stream reported by
FFprobe. The canvas dimensions must match the active camera working canvas:
the delivery dimensions for overlay composition or the picture-region
dimensions for a reserved caption band. A mismatch fails before FFmpeg render.
Mapped sources must also use square pixels and normalized orientation metadata;
REEL rejects autorotation and non-square sample-aspect transforms until those
coordinate spaces can be represented explicitly.

REEL calculates one centered contain fit from those integers. The 960x420
source above becomes an 1188x520 fitted source at `(46,0)` in a 1280x520
camera canvas. The generated scale and pad filters use those exact dimensions,
and mapped inputs are converted to RGBA before padding so chroma subsampling
cannot round the declared offset. Safety evidence and render execution therefore
share one mapping.

Motion lineage records `source_to_canvas` with the source, canvas, and fitted
pixel geometry. Source-space focal points and protected regions are transformed
through that mapping before each sampled camera window is checked.
`animatic-check` independently revalidates the bound source dimensions, canvas,
and mapping.

## Compatibility and boundaries

- This release is v0.3.0 because adding `geometry` to the public Rust
  `StillCameraTrack` structure is source-incompatible for downstream struct
  literals. The manifest field remains optional and existing YAML continues to
  load unchanged.
- `geometry` is opt-in. Existing camera tracks and default overlay renders are
  unchanged.
- V1 supports `geometry` only on still camera tracks with
  `visual_fit: contain`.
- Cover-fit source mapping, video camera tracks, sprite coordinates, and
  per-keyframe safety declarations are not inferred. Rotated or non-square-pixel
  sources must be normalized before they can declare camera geometry.
- Focal points and protected regions remain shot-wide. A camera tour of
  disjoint regions cannot claim that one point remains visible throughout; it
  must use separate shots or await an explicit segment-level safety contract.
- Source dimensions establish geometry, not semantic importance. Visual owners
  still select focal/protected regions and retain claim and publication
  authority.
