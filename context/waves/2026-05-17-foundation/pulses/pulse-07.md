# Pulse 07: FFmpeg shot-card draft

## Goal

Move beyond one title-card smoke renders by generating a timed shot-card MP4 from
the production manifest's actual shot list.

## Changes

- Added `scripts/render-shot-cards.sh`, a WSL/FFmpeg renderer that parses shot
  ids, durations, camera notes, actions, narration, and captions from a REEL YAML
  manifest.
- Updated the WSL FFmpeg setup guide with the shot-card render command.
- Updated the foundation wave to mark shot-card rendering as part of the initial
  REEL capability.

## Validation

- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/render-shot-cards.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml'`
- `wsl -- bash -lc 'test -s /mnt/c/src/TRACKER/repos/design-labs/reel/renders/shot-cards/0001-ash-vale-last-road-before-winter-shot-cards.mp4'`
- `wsl -- bash -lc 'ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 /mnt/c/src/TRACKER/repos/design-labs/reel/renders/shot-cards/0001-ash-vale-last-road-before-winter-shot-cards.mp4'`
- `git status --short --ignored`
- `git diff --check`

## Status

Done.
