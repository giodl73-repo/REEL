# Pulse 12: Contact sheet review exports

## Goal

Create a fast visual review artifact for draft cuts so reviewers can inspect shot
rhythm without opening the full MP4.

## Changes

- Added `scripts/render-contact-sheet.sh`.
- Reused the manifest validator before contact-sheet generation.
- Derived the work id, platform target duration, and shot count from the
  manifest.
- Created a gitignored PNG contact sheet from the platform's shot-card MP4,
  rendering the MP4 first when needed.
- Documented the contact-sheet command in the WSL FFmpeg guide.

## Validation

- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/validate-manifest.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml'`
- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/render-contact-sheet.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml youtube-demo'`
- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/render-contact-sheet.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml iphone-social'`
- `wsl -- bash -lc 'test -s /mnt/c/src/TRACKER/repos/design-labs/reel/renders/contact-sheets/0001-ash-vale-last-road-before-winter-youtube-demo-contact-sheet.png'`
- `wsl -- bash -lc 'test -s /mnt/c/src/TRACKER/repos/design-labs/reel/renders/contact-sheets/0001-ash-vale-last-road-before-winter-iphone-social-contact-sheet.png'`
- `git diff --check`

## Status

Done.
