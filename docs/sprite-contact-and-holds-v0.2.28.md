# Sprite contact and intentional holds — v0.2.28

v0.2.28 adds two generic limited-animation mechanics discovered while testing
short action choreography: a child sprite may follow a moving performer, and a
moving shot may explicitly permit stationary transitions during named held
phrases.

## Parent-relative tracks

```yaml
sprite_animation:
  background: backgrounds/stage.png
  timing_fps: 24
  sprites:
    - id: performer
      anchor_x: 0.5
      anchor_y: 0.9
      keyframes:
        - { frame: 0, asset: poses/receive.png, x: 0.40, y: 0.60, width: 0.30 }
        - { frame: 16, asset: poses/turn.png, x: 0.55, y: 0.50, width: 0.26 }
    - id: handheld-prop
      parent: performer
      position_space: parent-width
      z_index: 30
      keyframes:
        - { frame: 0, asset: props/token.png, x: 0.34, y: 0.08, width: 0.01 }
        - { frame: 16, asset: props/token.png, x: 0.29, y: 0.04, width: 0.01 }
```

For `parent-width` tracks, `x` and `y` are offsets from the parent's normalized
keyframed position rather than canvas coordinates. Both offsets are measured in
the parent's displayed width, so the child follows parent travel and scale.
Pose-specific offsets remain explicit because the contact point may move when
the parent swaps art. Child width remains a fraction of canvas width.

Parents must exist, cannot be the child itself, and must use canvas position
space. Nested attachments are deliberately rejected in this version so render
order and geometry stay deterministic. A child must also share its parent's
movement mode and step count and include every parent keyframe. These checks
ensure that pose changes cannot occur without a corresponding child contact
state. Between those shared states, REEL interpolates the resolved child
positions using the common movement cadence.

## Intentional holds

```yaml
sprite_animation:
  intentional_holds:
    - { start_frame: 0, end_frame: 8, reason: readable anticipation }
    - { start_frame: 12, end_frame: 15, reason: conceal the direction change }
```

A hold span permits near-stationary transitions after `start_frame` through
`end_frame`. Spans must be ordered, non-overlapping, remain inside the shot, and
include a reason. Together they may cover at most half of the shot's
transitions, preventing a frozen shot from exempting itself wholesale. They do
not force the entire composite to freeze; secondary movement may continue.

`motion-check` retains the measured total stationary fraction and additionally
reports declared hold transitions, permitted stationary transitions, and the
unexpected stationary fraction outside declared spans. Moving-shot approval is
based on the unexpected fraction. This keeps the existing ten-percent safety
threshold without rewarding constant drift or penalizing authored anticipation.
