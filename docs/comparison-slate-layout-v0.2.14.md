# Comparison-slate layout in CLI v0.2.14

Every `comparison-compose` now lays out its opening, variant, and optional
replay slates before starting the comparison FFmpeg graph. The existing strict
`reel.comparison.v0.1` contract and `reel.manifest.v0.2` scene contract are
unchanged.

## Deterministic fitting policy

- Geometry is the verified child delivery width and height.
- The safe area uses an 8% margin on all four sides.
- Initial font size is `height / 18`, clamped to 24–64 px.
- Minimum font size is `height / 36`, clamped to 18–32 px.
- Text wraps at words, with deterministic scalar-bound splitting for a word
  wider than one line.
- Width uses a conservative one-em cell for every Unicode scalar. The rendered
  default sans glyph is therefore bounded without depending on a platform font
  measurement API.
- Opening, variant, and replay slates allow at most 10, 6, and 4 lines.
- Fitting retries one pixel at a time. A half-em padded box must remain inside
  the safe area in both axes.

If no allowed font size fits, composition fails with `cannot fit` before any
comparison slate or final graph is rendered. The output MP4, local artifact,
and receipt remain absent. Successful local
`reel.comparison-artifacts.v0.1` reports retain `slate_layout_policy`, every
`slate_layouts` record, and `maximum_slate_occupied_screen_percent`. Each slate
record includes its role/timing, chosen and minimum font sizes, line spacing,
line limit and actual breaks, safe area, bounding box, and safety result.

## Private layout evidence

```powershell
reel comparison-compose review.yaml --output review.mp4 --format json
reel comparison-layout review.comparison.artifacts.json `
  --output-dir review-comparison-layout --output json
reel comparison-layout-check review-comparison-layout --output json
```

`comparison-layout` first proves the local artifact hash against its sibling
receipt and proves the video against both. It then atomically publishes
`layout.json`, one full-resolution opening PNG, and one full-resolution PNG for
each primary variant slate. The private packet binds artifact, receipt, video,
layout records, frame timestamps, PNG hashes, and byte lengths.

`comparison-layout-check` repeats the artifact/receipt/video verification and
checks every image hash, byte length, role, and timestamp. Modified video,
artifact, packet metadata, or PNG evidence fails verification.

The strict shareable `reel.comparison-receipt.v0.1` remains unchanged. It
contains no slate copy, presented label, screenshot, private instruction,
decode map, or local path. Layout packets are local/private evidence and make
no OCR, device-legibility, translation, preference, consent, or approval claim.

Existing BERTICA manifests and comparison YAML require no migration. Recompose
the comparison with v0.2.14, retain the local artifact and optional layout
packet, and intentionally share only the MP4 plus comparison receipt.
