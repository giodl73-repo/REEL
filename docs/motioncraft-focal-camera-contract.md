# Phase 2 focal-camera execution contract

Design checkpoint, not an implemented capability. Implement and verify this
contract before exposing it through the CAIMITOS adapter/runbook.

## Coordinates and transform

An optional Direction `focal_anchor` is a normalized point in the pre-camera
picture viewport. Missing means `(0.5, 0.5)`, preserving Phase 1's center zoom.
The anchor is fixed for the entire direction: zooming around it keeps that point
at its original screen location when pan is zero. It does not automatically
center a detected face or synthesize picture detail.

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
and legacy job bytes. Keep strict unknown-field validation and whole-direction
inheritance. Carry the new fields through native compiled plans, selected overlays,
future authoring, existing-job upgrades, review samples and cadence expectations.
Cadence must recognize pan movement even when zoom remains constant.

Before shipping: off-center anchor render, pure pan with safe zoom, rejected
source-edge exposure and protected-content crop, endpoint continuity, pan-only
hold/freeze detection, both caption layouts, differing measured ES/EN spans,
short fitted phases, rational FPS, identity reduced motion, byte-stable legacy
fixtures and actual CAIMITOS upgrade/rollback. Retain native rendered frames and
exact source/tool hashes; a transform unit test alone is insufficient proof.
