# Sprite animation shots — v0.2.23

Sprite animation is the economical option between camera motion on one still
and a complete authored cel sequence. A shot owns one stable background and one
or more sprite tracks. Each track moves linearly, in deliberate steps, or by
hard holds between normalized keyframe positions and may change its asset at
any keyframe to swap poses.

```yaml
media_kind: sprite-animation
sprite_animation:
  background: backgrounds/rink.png
  timing_fps: 24
  sprites:
    - id: passer
      z_index: 10
      anchor_x: 0.5
      anchor_y: 0.92
      movement: stepped
      movement_steps: 3
      keyframes:
        - { frame: 0, asset: sprites/passer-carry.png, x: 0.20, y: 0.64, width: 0.24 }
        - { frame: 36, asset: sprites/passer-release.png, x: 0.38, y: 0.58, width: 0.20, z_index: 5 }
    - id: puck
      z_index: 30
      keyframes:
        - { frame: 0, asset: sprites/puck.png, x: 0.26, y: 0.70, width: 0.025 }
        - { frame: 48, asset: sprites/puck.png, x: 0.72, y: 0.54, width: 0.025 }
```

`x` and `y` are normalized sprite-center coordinates. `width` is a fraction of
the output canvas width and interpolates between keyframes to create perspective
growth or retreat. Tracks begin at frame zero; keyframe numbers increase
strictly and must remain inside the shot. `anchor_x` and `anchor_y` choose the
point that stays on the keyframed path; a hockey skater normally uses a centered
x anchor and a y anchor near the skates. Manifest order is independent of
layering: track `z_index` sets the default overlay order, while a keyframe
`z_index` can move that pose in front of or behind another track.

Layer swapping handles ordinary partial overlap. A single flat sprite cannot
place one limb in front of another player while keeping its torso behind; use
separate body/limb tracks or a brief composite contact pose for that case.

`movement` is per track. `linear` is appropriate for pucks and other objects
whose path must read continuously. `stepped` quantizes each keyframed move into
two to twelve pose-to-pose jumps (`movement_steps` defaults to three), giving
limited animation a deliberate comic-panel rhythm. `hold` keeps a pose planted
until the next keyframe. Mixing smooth puck motion, stepped skaters, held goalie
reactions, and camera movement avoids the sliding-paper-doll look.

For classic racing-anime or comic-book energy, prefer pose-to-pose movement:
hold a readable silhouette, jump to the next strong action pose, and let a
smooth puck path and camera pan carry continuity. Reserve large perspective
scale changes for foreground impact poses rather than treating every skater as
a continuously interpolated puppet.

Every background and pose asset is independently hashed in artifact lineage.
REEL performs the movement, pose timing, compositing, and final render but does
not generate the art. Transparent PNG or WebP sprites are recommended.

The shot-level `motion` treatment applies to the background plate before sprite
composition. For example, `motion: pan-right` lets the background travel across
the rink while sprite tracks retain independent screen-space movement.

As of v0.2.27, `sprite_animation.camera` can instead control the completed
composition with increasing frame-keyed center and zoom states. `linear`,
`ease-in-out`, `ease-out`, and `hold-then-burst` curves are supported. Tracks
begin at frame zero, remain inside the shot, and use zoom values from 1 through
4. The renderer crop-clamps their centers for the requested output geometry, so
16:9, 9:16, and square deliveries can be proven separately.

For a short hockey play, use separate tracks for the passer, scorer, goalie,
puck, and optionally the net response. A player usually needs only two to four
poses: glide, release, follow-through, and celebration. The puck can reuse one
asset across its entire path.
