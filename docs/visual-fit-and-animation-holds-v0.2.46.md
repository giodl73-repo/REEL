# Visual Fit and Animation Holds v0.2.46

REEL v0.2.46 adds a provider-neutral composition choice for owner-rendered
visuals and makes authored animation-frame holds visible to motion review.

## Visual fit

Shots accept `visual_fit: cover | contain`.

- `cover` remains the default and preserves all existing manifests and render
  commands.
- `contain` scales the complete source inside the delivery frame, centers it,
  and fills unused canvas with black.

`contain` applies to video and limited-animation shots. A still shot may use it
only with `motion: hold` or `motion: hold-dark` and without `camera_track`.
Sprite-animation composition remains `cover` because its normalized sprite and
camera geometry is authored against the filled canvas.

REEL owns only the fit operation and records the selected mode in each shot's
artifact lineage. Source semantics, labels, colors, map or chart geometry,
claims, and publication review remain with the visual's owner.

## Animation motion review

Limited-animation shots now report the `animation-frames` treatment. Each
frame's `hold_frames` interval is an intentional stationary range; boundaries
between authored frames remain expected visual changes. `motion-check` permits
stationary transitions only inside those declared ranges and continues to
reject unexpected stationary transitions elsewhere.

This removes the prior workaround of treating an authored frame sequence as a
continuous `push`, which caused valid held-frame animation to fail motion
review.
