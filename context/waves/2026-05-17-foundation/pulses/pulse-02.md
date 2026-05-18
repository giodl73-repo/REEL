# Pulse 02: Animation style grammar and roles review

## Goal

Make REEL explicitly capable of planning animated movies in multiple styles,
especially for Games Design scenarios, and add a repo-local review panel before
renderer work begins.

## Changes

- Added a starting animation style catalog covering animatic, motion graphics,
  cutout 2D, illustrated 2D, pixel art, isometric game, 3D previs, and
  cinematic AI styles.
- Documented the Games Design scenario-to-video flow.
- Added `.roles` panel files for story, animation direction, editing, sound, and
  platform/audience fit.
- Recorded the foundation plan review in `docs/reviews/foundation-plan-review.md`.

## Validation

- `git grep -n "animation" -- README.md PRODUCT_PLAN.md docs\reviews\foundation-plan-review.md`
- `git grep -n "Role:" -- .roles\*.md`
- `git diff --check`

## Status

Done.
