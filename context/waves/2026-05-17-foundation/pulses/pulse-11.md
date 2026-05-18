# Pulse 11: Scene-aware draft cards

## Goal

Make FFmpeg shot-card drafts easier to review as animation instructions, not just
timed text slates.

## Changes

- Added `scene_id` and `visual_prompt` to the shot-card renderer's manifest
  extraction.
- Added scene-specific background colors for quick scene recognition.
- Added a subtle top rule and lower safe-area band to improve card readability.
- Kept manifest-driven aspect ratios, duration scaling, and output filenames.

## Validation

- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/validate-manifest.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml'`
- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/render-shot-cards.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml youtube-demo'`
- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/render-shot-cards.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml iphone-social'`
- `wsl -- bash -lc 'ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 /mnt/c/src/TRACKER/repos/design-labs/reel/renders/shot-cards/0001-ash-vale-last-road-before-winter-youtube-demo-shot-cards.mp4'`
- `wsl -- bash -lc 'ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 /mnt/c/src/TRACKER/repos/design-labs/reel/renders/shot-cards/0001-ash-vale-last-road-before-winter-iphone-social-shot-cards.mp4'`
- `git diff --check`

## Status

Done.
