# Still-plate camera tracks (v0.2.45)

REEL v0.2.45 can execute a frame-keyed camera path over an owner-authored still
plate. The owner system remains authoritative for the image, waypoint meaning,
labels, narration, claims, and publication review. REEL owns only timing,
interpolation, bounded crop execution, and artifact lineage.

## Manifest contract

`camera_track` is optional and valid only on a `still` shot. It replaces the
named `motion` treatment for that shot.

```yaml
media_kind: still
visual_asset: corridor-map.png
camera_track:
  timing_fps: 24
  keyframes:
    - frame: 0
      center_x: 0.20
      center_y: 0.50
      zoom: 1.15
      curve_to_next: ease-in-out
    - frame: 216
      center_x: 0.52
      center_y: 0.48
      zoom: 1.80
      curve_to_next: hold-then-burst
    - frame: 431
      center_x: 0.82
      center_y: 0.46
      zoom: 1.45
```

The track:

- requires `timing_fps` from 1 through 60;
- requires at least two keyframes and a real center or zoom change;
- starts at frame zero and uses strictly increasing frame numbers;
- keeps every keyframe inside the conformed shot duration;
- accepts normalized centers from 0 through 1 and zoom from 1 through 4;
- supports `linear`, `ease-in-out`, `ease-out`, and `hold-then-burst`;
- cannot be combined with `motion` or attached to video, animation, or sprite
  animation shots.

The renderer center-fits the plate to the delivery aspect ratio, interpolates
the authored states, and clamps every crop to available picture bounds. The
default smooth mode uses frame-evaluated cubic perspective sampling; legacy
mode retains deterministic integer `zoompan` behavior. Motion review treats the
first 65 percent of each `hold-then-burst` segment and any span after the final
keyframe as authored holds.

A camera track does not establish semantic correspondence, map truth, waypoint
meaning, accessibility, privacy, claim validity, selection, or release
approval.

## Evidence

Animatic artifact reports count executed still camera tracks in
`mixed_media.still_camera_tracks`. Motion lineage names the per-shot treatment
`camera-track`, and crop-safety evidence samples each authored segment against
declared focal points and protected regions.
