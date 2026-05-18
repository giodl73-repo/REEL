# Pulse 04: Renderer research

## Goal

Choose the first renderer direction for REEL animated scenario videos without
locking the repo into a single provider or heavyweight editor too early.

## Changes

- Added `docs/research/2026-05-17-renderer-paths.md` with local and external
  findings.
- Recommended WSL2 Ubuntu as the execution environment for video tooling.
- Recommended FFmpeg as the baseline assembler/encoder and Remotion as the first
  prototype boundary for programmatic animation.
- Deferred Blender and cinematic AI until a specific style/package needs them.

## Validation

- `git grep -n "REEL-R04" -- docs\research\2026-05-17-renderer-paths.md`
- `git grep -n "Adopt now" -- docs\research\2026-05-17-renderer-paths.md`
- `git diff --check`

## Status

Done.
