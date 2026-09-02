# Effect-pass contract v0.3.19

REEL `0.3.19` adds provider-neutral `effect_passes` to ordinary conformed still
and video shots. The first bounded contract keeps an RGB color carrier separate
from its grayscale alpha matte and optional grayscale occlusion matte.

```yaml
effect_passes:
  - id: road-dust
    color: { path: effects/dust-color.nut, sha256: <64-hex> }
    matte: { path: effects/dust-matte.nut, sha256: <64-hex> }
    occlusion_matte: { path: effects/foreground-keep.nut, sha256: <64-hex> }
    alpha_mode: separate-matte
    composite_operator: over
    color_space: srgb
    alpha_mode_detail: straight
    timing_fps: 24
    duration_frames: 199
    placement: { space: normalized, x: 0, y: 0, width: 1, height: 1 }
    visible_start_frame: 0
    visible_end_frame: 198
    z_index: 10
```

All assets must resolve inside `--asset-root`, match their declared SHA-256,
contain a video stream at the declared frame rate, and match the declared
duration within one frame. The manifest duration must resolve to exactly
`duration_frames`. V1 accepts only `separate-matte`, `over`, `srgb`, `straight`,
and normalized placement. Effects sort by `(z_index, id)`.

`animatic-render` scales the carrier and mattes to their placement rectangle,
multiplies the optional occlusion matte into effect alpha, reconstructs RGBA
with FFmpeg `alphamerge`, and composites with `overlay`. Dry-run artifacts expose
the exact graph. Real artifact reports bind color, matte, optional occlusion,
manifest, and completed output hashes. Verify a real result with:

```text
reel effect-pass-check candidate.artifacts.json --output json
```

The command rechecks ordinary animatic lineage and requires a complete effect
pass. Clean-picture outputs use `--disclosure ""` and `--no-captions`; review
labels and checkerboards remain separate derivatives.

The synthetic test generates its own background, color, matte, and foreground
occlusion. It proves deterministic output, changed-matte lineage/output,
dry-run graph visibility, and fail-closed wrong hash, duration, and asset-root
escape behavior. No production media or project-specific effect semantics are
stored in REEL.
