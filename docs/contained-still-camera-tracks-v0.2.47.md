# Contained still camera tracks (v0.2.47)

REEL v0.2.47 can execute a still `camera_track` after fitting the complete
owner-authored source inside the delivery canvas.

```yaml
media_kind: still
visual_asset: wide-owner-diagram.svg
visual_fit: contain
camera_track:
  timing_fps: 24
  keyframes:
    - frame: 0
      center_x: 0.50
      center_y: 0.50
      zoom: 1.00
      curve_to_next: ease-in-out
    - frame: 72
      center_x: 0.24
      center_y: 0.50
      zoom: 2.20
      curve_to_next: ease-in-out
```

`contain` scales down and centers the source on a black delivery-sized canvas
before the existing camera path runs. A `zoom` of `1` therefore shows the
complete source rather than a cover-cropped view. Smooth perspective and legacy
zoompan execution use the same fit-before-camera order. `cover` remains the
default, and existing cover-camera filter commands are unchanged.

Camera centers are normalized delivery-canvas coordinates and remain
crop-clamped. At zoom `z`, an unclamped center on either axis lies between
`1 / (2z)` and `1 - 1 / (2z)`. Authors should keep moving keyframes inside that
range; otherwise multiple requested states can resolve to the same edge crop
and fail motion review.

## Safety boundary

`focal_point` and `protected_regions` describe source-space geometry. Padding
changes the source's position inside the delivery canvas, so v0.2.47 rejects
either declaration when `visual_fit: contain` and `camera_track` are combined.
REEL does not guess the source-to-canvas mapping.

Caption-safe picture regions are also outside this contract. Captions remain a
separate delivery overlay and can obscure owner-authored annotations even when
the complete source is preserved.

The visual owner remains authoritative for diagram semantics, labels, claims,
camera meaning, accessibility, and publication review. REEL owns only
fit-before-camera execution, timing, crop clamping, motion review, and artifact
lineage.
