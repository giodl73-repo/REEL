# Pulse 14: Batch review-pack index

## Goal

Make REEL able to refresh review artifacts for every work manifest with one
command.

## Changes

- Added `scripts/render-all-review-packs.sh`.
- Discovered `works/*/manifest.yaml` files in stable sorted order.
- Rendered each manifest through `scripts/render-review-pack.sh`.
- Wrote a gitignored `renders/review-packs/INDEX.md` linking manifests to their
  generated review-pack reports.
- Documented the batch command in the WSL FFmpeg guide.

## Validation

- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/render-all-review-packs.sh'`
- `wsl -- bash -lc 'test -s /mnt/c/src/TRACKER/repos/design-labs/reel/renders/review-packs/INDEX.md'`
- `wsl -- bash -lc 'grep -q "0001-ash-vale-last-road-before-winter" /mnt/c/src/TRACKER/repos/design-labs/reel/renders/review-packs/INDEX.md'`
- `git diff --check`

## Status

Done.
