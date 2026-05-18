# Pulse 13: Review pack renderer

## Goal

Make a single command produce all draft review artifacts for a manifest.

## Changes

- Added `scripts/render-review-pack.sh`.
- Validated the manifest before rendering.
- Read platform names from the manifest instead of hard-coding them in the
  review-pack script.
- Rendered each platform shot-card MP4 and contact-sheet PNG.
- Wrote a gitignored Markdown report under `renders/review-packs/` with each
  platform's MP4, duration, and contact-sheet path.
- Documented the review-pack command in the WSL FFmpeg guide.

## Validation

- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/render-review-pack.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml'`
- `wsl -- bash -lc 'test -s /mnt/c/src/TRACKER/repos/design-labs/reel/renders/review-packs/0001-ash-vale-last-road-before-winter-review-pack.md'`
- `git diff --check`

## Status

Done.
