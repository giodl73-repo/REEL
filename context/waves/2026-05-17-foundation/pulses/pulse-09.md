# Pulse 09: Manifest validation gate

## Goal

Catch manifest timing and export mistakes before a renderer spends time producing
review cuts.

## Changes

- Added `scripts/validate-manifest.sh` as a WSL-friendly Bash validator.
- Checked required top-level manifest fields.
- Checked scene and shot duration totals, contiguous shot start offsets, and
  shot-to-scene references.
- Checked that `exports` match `platforms` by id, aspect ratio, duration, and
  filename presence.
- Documented the validation command in the manifest contract and WSL FFmpeg
  setup guide.

## Validation

- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/validate-manifest.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml'`
- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/render-shot-cards.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml youtube-demo'`
- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/render-shot-cards.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml iphone-social'`
- `git diff --check`

## Status

Done.
