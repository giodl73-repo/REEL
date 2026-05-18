# Pulse 08: Shot-card export variants

## Goal

Generate platform-specific draft exports from the same REEL manifest so the Ash
Vale package has both a 16:9 review cut and a 9:16 phone/social cut.

## Changes

- Updated `scripts/render-shot-cards.sh` to accept a platform argument:
  `youtube-demo` or `iphone-social`.
- Kept `youtube-demo` at 1280x720 and the full manifest shot durations.
- Added `iphone-social` at 720x1280 with shot durations scaled to the 45-second
  manifest phone target.
- Updated WSL setup docs with both render commands.

## Validation

- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/render-shot-cards.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml youtube-demo'`
- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/render-shot-cards.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml iphone-social'`
- `wsl -- bash -lc 'test -s /mnt/c/src/TRACKER/repos/design-labs/reel/renders/shot-cards/0001-ash-vale-last-road-before-winter-youtube-demo-shot-cards.mp4'`
- `wsl -- bash -lc 'test -s /mnt/c/src/TRACKER/repos/design-labs/reel/renders/shot-cards/0001-ash-vale-last-road-before-winter-iphone-social-shot-cards.mp4'`
- `git status --short --ignored`
- `git diff --check`

## Status

Done.
