# Pulse 01: Review role summary

## Outcome

Added review status and role guidance to generated review packs.

## Changes

- Added a `Review summary` section before adapter and FFmpeg artifact sections.
- Listed manifest-owned review status and required roles.
- Added role-specific review focus for story, animation, edit, sound, and
  platform/audience reviewers.
- Added regression coverage for the generated Markdown section.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- review-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml
git diff --check
```
