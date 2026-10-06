# Phase 2 focal-camera execution contract

Implemented in the Phase 2 engine branch; CAIMITOS adoption and full consumer
qualification remain required before claiming that consumer capability.

## Coordinates and transform

An optional Direction `focal_anchor` is a normalized point in the pre-camera
picture viewport. Missing means `(0.5, 0.5)`, preserving Phase 1's center zoom.
The anchor is fixed for the entire direction: zooming around it keeps that point
at its original screen location when pan is zero. It does not automatically
center a detected face or synthesize picture detail.

For example, add `"focal_anchor": {"x": 0.8, "y": 0.4}` to a complete
Direction. In an approach phase, add both
`"pan_from": {"x": 0, "y": 0}` and
`"pan_to": {"x": 0.02, "y": 0}`. Subsequent holds must carry the same
end pan in both endpoints; omitted pan means zero, not inheritance from the
previous phase. These are illustrative fragments, not a standalone Direction.

Each Phase may supply `pan_from` and `pan_to`, normalized displacement points in
the output picture viewport. Both are supplied together or both omitted; missing
means zero displacement. Pan and zoom use the phase's same bounded easing
parameter `t`:

```text
z(t) = zoom_from + (zoom_to - zoom_from) * t
p(t) = pan_from + (pan_to - pan_from) * t
screen(point, t) = anchor + z(t) * (point - anchor) + p(t)
```

Positive pan moves the picture right/down. Coordinates belong to the contained
picture region before the caption band is padded. Transform that region, then
compose the stable band; no post-compose camera moves captions.

## Bounds and continuity

Require finite points, anchor components within `[0,1]`, bounded pan components
and positive supported zoom. Validate source viewport coverage: transformed
left/top edges must remain at or before zero and right/bottom edges at or after
one. Reject exposure of new source edges rather than inventing or extending art.

For each protected rectangle, transform all edges and require containment in the
declared safe area. Because anchor is fixed and pan/zoom are affine in the same
bounded `t`, every transformed rectangle edge is affine in `t`: phase endpoint
checks prove whole-interval containment, including nonlinear monotone easing.
Do not apply this proof to future moving anchors or independently eased axes.

Adjacent phase zoom and pan endpoints must agree. Holds cannot move in either
axis. One-frame moving phases reject. Native-duration fitting changes phase
allocation only; pan values and native cue/effect clocks remain authoritative.
Use the corrected zero-based FFmpeg frame expression and global rational frame
partition; never integrate position from previous frames.

Reduced motion uses identity zoom and zero pan, preserving the selected full
composition and native clocks. Required subjects must remain visible in that
composition; no alternate crop, take or editorial cut is invented.

## Compatibility and required evidence

Omit default values from serialized output, preserving existing Phase 1 fixtures
and legacy job bytes when new fields are absent. Explicitly authored points are
retained in evidence. Keep strict unknown-field validation and whole-direction
inheritance. Carry the new fields through native compiled plans, selected overlays,
future authoring, existing-job upgrades, review samples and cadence expectations.
Cadence must recognize pan movement even when zoom remains constant.

Before shipping: off-center anchor render, pure pan with safe zoom, rejected
source-edge exposure and protected-content crop, endpoint continuity, pan-only
hold/freeze detection, both caption layouts, differing measured ES/EN spans,
short fitted phases, rational FPS, identity reduced motion, byte-stable legacy
fixtures and actual CAIMITOS upgrade/rollback. Retain native rendered frames and
exact source/tool hashes; a transform unit test alone is insufficient proof.

## Explicit picture viewport

An optional `picture_region` in the scene delivery profile propagates unchanged
to the native job. Its output-pixel rectangle reserves space for a side panel or
portrait text area without changing any selected image source hash:

```json
"picture_region": { "x": 0, "y": 30, "width": 720, "height": 405 }
```

The rectangle must be nonempty and fit within the output. When caption-band
reservation is configured it must fit within that reservation's picture area.
Unknown rectangle fields, overflowing coordinates and caption-band overlap
reject. Absent viewport retains the previous geometry and serialization.

The engine fits the original image inside the viewport, applies its camera
there, then pads to the output before text/effect layers. Normalized focal,
safe-area and protected coordinates remain local to the picture viewport.
Legacy or post-compose camera treatments require explicit migration before
using a reserved viewport. The review evidence records the resolved rectangle;
cadence measures the explicit viewport and records its measurement region so
unused text space cannot dilute the camera metric. Viewport support does not
create, bind or qualify poem/caption text or profile-specific effect carriers.
