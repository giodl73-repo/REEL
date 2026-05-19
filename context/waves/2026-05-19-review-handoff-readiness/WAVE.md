# Wave: Review handoff readiness

## Goal

Make REEL review packs easier for humans and agents to hand off by surfacing the
review status, required review roles, and role-specific review focus before any
render-heavy artifact sections.

## Pulse table

| Pulse | Title | Status | Outcome |
|------:|---|---|---|
| 01 | Review role summary | done | Added a review summary section to generated review packs with status, required roles, and role-specific review focus. |

## Success criteria

- `cargo run -- review-pack <manifest>` generates a handoff pack with review
  status and required-role guidance.
- Review metadata remains manifest-owned and renderer-neutral.
- The FFmpeg baseline remains the only implemented deterministic renderer.
