# Sprite emissions — v0.2.29

An emission begins at a contact point on a named canvas-space sprite, then
detaches and lives in the world of the shot. This is useful whenever aftermath
should remain behind while its source continues: dust, snow, sparks, debris,
footprints, motion residue, impact rings, or graphic accents.

```yaml
sprite_animation:
  emissions:
    - id: contact-snow
      asset: effects/snow-burst.png
      parent: performer
      frame: 16
      duration_frames: 8
      offset_x: -0.18
      offset_y: 0.38
      width: 0.14
      end_width: 0.19
      drift_x: -0.03
      drift_y: 0.01
      rotation_degrees: -8
      end_rotation_degrees: -18
      fade_out_frames: 5
      z_index: 24
```

`offset_x` and `offset_y` use the same parent-width geometry as attached child
tracks. REEL resolves the parent's position and width at `frame`; after that
instant the effect uses canvas coordinates and no longer follows the parent.
`drift_x` and `drift_y` are normalized canvas travel over the emission lifetime.
Width remains a fraction of canvas width.

Emissions last at least one frame and must end inside the shot. Fade cannot
exceed lifetime. Geometry, anchors, drift, scale, and rotation are bounded;
parents must exist and use canvas position space; IDs are unique; and a shot may
declare at most 64 emissions. Each emission asset is independently hashed in
the render lineage as `sprite-emission`.

This contract deliberately does not create a particle simulator. The manifest
owns the choreography, asset, contact frame, and aftermath. REEL resolves,
composites, fades, rotates, and verifies those choices without deciding where
an effect deserves to appear.

An emission may keep changing pixels while its parent is inside a declared
intentional hold. In that case `motion-check` can correctly report visual
motion even though the character pose is dramatically held. The cadence gate
remains technical evidence; human review still decides whether the accent is
readable and earns its duration.
