# Wave: Review handoff readiness

## Goal

Make REEL review packs easier for humans and agents to hand off by surfacing the
review status, required review roles, and role-specific review focus before any
render-heavy artifact sections.

## Pulse table

| Pulse | Title | Status | Outcome |
|------:|---|---|---|
| 01 | Review role summary | done | Added a review summary section to generated review packs with status, required roles, and role-specific review focus. |
| 02 | Review-all handoff metadata | done | Added aggregate review statuses and required roles to review-all Markdown and JSON output. |
| 03 | Review status counts | done | Added aggregate review status counts to review-all Markdown and JSON output so automation can distinguish reviewed and not-reviewed totals. |
| 04 | Required role counts | done | Added aggregate required-role counts to review-all Markdown and JSON output so automation can see review workload by role. |
| 05 | Required role status counts | done | Added aggregate required-role counts split by review status so automation can see outstanding workload by role. |
| 06 | Review status work ids | done | Added aggregate work id lists split by review status so automation can identify outstanding review packs directly. |
| 07 | Review status work titles | done | Added aggregate work title lists split by review status so handoff reports are readable without opening each pack. |

## Success criteria

- `cargo run -- review-pack <manifest>` generates a handoff pack with review
  status and required-role guidance.
- `cargo run -- review-all works --output json` exposes review statuses and
  required roles plus status counts, status work ids/titles, role counts, and
  role-by-status counts across the corpus without opening every review pack.
- Review metadata remains manifest-owned and renderer-neutral.
- The FFmpeg baseline remains the only implemented deterministic renderer.
