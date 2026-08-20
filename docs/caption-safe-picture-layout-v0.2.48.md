# Caption-safe picture layout (v0.2.48)

REEL v0.2.48 can reserve the caption profile's declared lower band from active
picture:

```powershell
reel animatic-render manifest.yaml `
  --asset-root . `
  --silent `
  --captions captions.srt `
  --caption-picture-layout reserve-caption-band `
  --output review.mp4
```

`overlay` remains the default and preserves existing render commands and
caption lineage. `reserve-caption-band` is explicit and requires captions.
REEL derives a top-anchored, full-width picture region ending at the selected
caption profile's `caption_region.y`. It renders each shot into that region,
pads to the requested delivery frame, and applies captions afterward.

For the 1280x720 private-review profile, the picture region is
`x=0, y=0, width=1280, height=520`; the caption region begins at `y=520`.
Smooth and legacy motion use the same picture dimensions before final padding.

Artifact caption lineage records the strategy and exact pixel picture region.
`caption-layout` reports the region and fails if it intersects the declared
caption region. These are geometry and integrity claims, not OCR-derived proof
that every source label is readable.

## V1 boundaries

- Still, video, and limited-animation shots are supported.
- Sprite-animation shots are rejected because their normalized canvas geometry
  has not been mapped into a smaller picture region.
- `focal_point` and `protected_regions` require the explicit contained-camera
  geometry mapping introduced in v0.3.0; unmapped or cover-fitted geometry is
  rejected.
- Speaker badges remain overlays; this option reserves only the caption band.
- REEL does not infer semantic importance, select a source crop, rewrite owner
  labels, or establish accessibility or publication approval.

The visual owner remains authoritative for source semantics, camera meaning,
label importance, captions, claims, and release review. REEL owns the explicit
delivery composition, caption-profile geometry, motion execution, and artifact
evidence.
