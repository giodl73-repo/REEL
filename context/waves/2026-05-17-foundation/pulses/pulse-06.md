# Pulse 06: First original video package

## Goal

Create REEL's first original video package from a Games Design scenario and prove
it can render a manifest-fed FFmpeg draft.

## Changes

- Added `works/0001-ash-vale-last-road-before-winter/` as the first REEL work.
- Adapted BANISH's Ash Vale / Pilgrim Loss scenario evidence into a trailer
  brief, script, shotlist, production manifest, and panel review.
- Rendered the work manifest through `scripts/smoke-render.sh`, producing a
  gitignored MP4 draft under `renders/smoke/`.

## Validation

- `git grep -n "scenario:banish:ash-vale-last-road-before-winter" -- works\0001-ash-vale-last-road-before-winter\*.md works\0001-ash-vale-last-road-before-winter\manifest.yaml`
- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/smoke-render.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml'`
- `wsl -- bash -lc 'test -s /mnt/c/src/TRACKER/repos/design-labs/reel/renders/smoke/0001-ash-vale-last-road-before-winter-smoke.mp4'`
- `git status --short --ignored`
- `git diff --check`

## Status

Done.
