# Wave: Review queue readiness

## Goal

Make REEL review handoff metadata available without rendering media, so humans
and agents can inspect outstanding review queues before running expensive
artifact or review-pack generation.

## Pulse table

| Pulse | Title | Status | Outcome |
|------:|---|---|---|
| 01 | Non-rendering review queue | done | Added `review-queue` text and JSON output for manifest-owned review statuses and required roles across a works root. |

## Success criteria

- `cargo run -- review-queue works --output json` exposes review statuses,
  required roles, status counts, role counts, and role-by-status workload without
  rendering media.
- Review queue metadata remains manifest-owned and renderer-neutral.
- `review-all` remains the render-heavy artifact and review-pack routing command.
