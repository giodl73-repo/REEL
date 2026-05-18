# Pulse 10: Manifest-driven export timing

## Goal

Make the FFmpeg shot-card renderer obey the manifest export contract instead of
hard-coding platform duration scales.

## Changes

- Updated `scripts/render-shot-cards.sh` to run the manifest validator before
  rendering when the validator is present.
- Read the requested platform's aspect ratio and target duration from
  `platforms` and `exports`.
- Derived the duration scale from the manifest shot total and export target.
- Kept 16:9 and 9:16 render dimensions as renderer defaults for the manifest
  aspect ratios.
- Updated the WSL setup guide and wave record.

## Validation

- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/validate-manifest.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml'`
- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/render-shot-cards.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml youtube-demo'`
- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/render-shot-cards.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml iphone-social'`
- `wsl -- bash -lc 'ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 /mnt/c/src/TRACKER/repos/design-labs/reel/renders/shot-cards/0001-ash-vale-last-road-before-winter-youtube-demo-shot-cards.mp4'`
- `wsl -- bash -lc 'ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 /mnt/c/src/TRACKER/repos/design-labs/reel/renders/shot-cards/0001-ash-vale-last-road-before-winter-iphone-social-shot-cards.mp4'`
- `git diff --check`

## Status

Done.
