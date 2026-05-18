# Pulse 03: Production manifest sketch

## Goal

Define the first renderer-neutral production manifest contract so a Games Design
scenario can become a style-specific video package before renderer selection.

## Changes

- Added `manifests/README.md` with the required production-manifest sections.
- Added `manifests/templates/scenario-video.yaml` as the first reusable manifest
  template.
- Updated README, PRODUCT_PLAN, CLAUDE instructions, and the foundation wave to
  make manifests part of the core REEL workflow.
- Kept renderer/provider-specific details optional until a research pulse selects
  the first implementation path.

## Validation

- `git grep -n "source_scenario" -- CLAUDE.md manifests\README.md manifests\templates\scenario-video.yaml`
- `git grep -n "renderer" -- README.md PRODUCT_PLAN.md manifests\README.md context\waves\2026-05-17-foundation\WAVE.md`
- `git diff --check`

## Status

Done.
