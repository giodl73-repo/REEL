# Sprite materialization (v0.2.32)

REEL can materialize a `reel.sprite-cache-plan.v0.1` into content-addressed,
transparent PNGs without recording the physical cache root in any portable
artifact.

A `reel.sprite-recipe-catalog.v0.1` binds logical recipe IDs to hash-pinned,
catalog-relative source images or to an explicit transparent layer. Asset
recipes may provide request-named variants. This supports different carry,
pass, shot, and reaction images while retaining one stable character recipe.

By default, pose-space sources inherit the resolved horizontal mirror and
post-transform sources do not. `mirror_behavior: preserve` is an explicit
migration mode for a reviewed precomposed source whose final orientation,
identity, and readable markings must remain unchanged. It is not evidence that
the source has been separated into native layers.

As of v0.2.34, raster cache keys include the composition key, an effective
source fingerprint for the selected recipe layers, dimensions, contain-fit
policy, sRGB color-space declaration, and straight-alpha policy. Earlier
v0.2.32 keys used the whole catalog hash. Existing entries are reused only when
their hashes match the deterministic render; a mismatch fails rather than
overwriting cache content.

```text
reel sprite-cache-materialize <catalog> <cache-plan> <output-root> \
  --width 512 --height 512 [--receipt-path <receipt.json>]

reel sprite-cache-contact-sheet <receipt> <cache-root> <sheet.png> \
  --columns 5 --tile-size 256 [--report-path <report.json>]
```

The materialization receipt and contact-sheet report contain logical keys,
hashes, dimensions, and character/request labels, but no absolute paths. The
contact sheet uses a checkerboard backing for alpha review; its report maps
every row-major cell to the corresponding logical cache entry.
