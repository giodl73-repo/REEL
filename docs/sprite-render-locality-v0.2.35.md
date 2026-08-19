# Sprite render locality (v0.2.35)

Dense limited animation commonly reuses a small pose library many times. A
three-cel stride cycle may create hundreds of keyframes in a short scene, but
those keyframes do not represent hundreds of distinct raster assets.

Before v0.2.35, the FFmpeg adapter opened every sprite occurrence as a separate
full-shot looping input. This preserved picture correctness but multiplied
decode and scale work, making high-frequency pose cycles disproportionately
slow.

The adapter now maintains a shot-local cache keyed by the canonical raster
path. It opens each unique pose or emission asset once, uses FFmpeg `split` to
fan that decoded stream out to every occurrence, and applies a segment-specific
`trim` before scaling, rotation, fading, and overlay. Cache scope is one shot so
shots with different durations remain independent.

This is an execution optimization only:

- the `reel.manifest.v0.2` schema is unchanged;
- every logical sprite pose and emission remains in artifact inputs;
- every input hash is still verified;
- position, scale, layer, rotation, fade, visibility, and camera timing are
  unchanged;
- the render report records both logical sprite-asset occurrences and unique
  sprite-asset inputs.

The distinction is intentional. Provenance describes authored occurrences;
render locality describes how efficiently their repeated raster bytes are
decoded.

## Verification

The sprite dry-run fixture requires a shared input split and segment trims while
retaining its original logical input counts. A production stress proof with 490
logical inputs reduced the FFmpeg input list to 28 unique media inputs and
completed at 1280×720/24 fps with all 490 artifact inputs verified.
