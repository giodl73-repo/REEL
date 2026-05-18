# Pulse 05: WSL FFmpeg smoke render

## Goal

Prove REEL can turn a scenario-video manifest into a generated, gitignored MP4
inside WSL2 Ubuntu using FFmpeg.

## Changes

- Added `docs/setup/wsl-ffmpeg.md` with the Linux-first setup and smoke-render
  commands.
- Added `scripts/smoke-render.sh`, a small Bash renderer that reads the starter
  scenario manifest and creates a title-card MP4 under `renders/smoke/`.
- Kept generated media out of git via the existing `renders/` ignore rule.

## Validation

- `wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/smoke-render.sh'`
- `wsl -- bash -lc 'test -s /mnt/c/src/TRACKER/repos/design-labs/reel/renders/smoke/0001-example-scenario-video-smoke.mp4'`
- `git status --short --ignored`
- `git diff --check`

## Status

Done.
