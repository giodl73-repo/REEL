# Wave: Review queue readiness

## Goal

Make REEL review handoff metadata available without rendering media, so humans
and agents can inspect outstanding review queues before running expensive
artifact or review-pack generation.

## Pulse table

| Pulse | Title | Status | Outcome |
|------:|---|---|---|
| 01 | Non-rendering review queue | done | Added `review-queue` text and JSON output for manifest-owned review statuses and required roles across a works root. |
| 02 | Required role work ids | done | Added required-role work id lists to `review-queue` so reviewers can see which works need each role without rendering media. |
| 03 | Required role work titles | done | Added required-role work title lists to `review-queue` so reviewer assignments are readable without rendering media. |
| 04 | Required role manifests | done | Added required-role manifest path lists to `review-queue` so reviewers can jump directly to source manifests without rendering media. |
| 05 | Required role status work ids | done | Added role-by-status work id lists to `review-queue` so reviewers can see outstanding assignments by role without rendering media. |
| 06 | Required role status work titles | done | Added role-by-status work title lists to `review-queue` so outstanding assignments are readable by role without rendering media. |

## Success criteria

- `cargo run -- review-queue works --output json` exposes review statuses,
  required roles, status counts, role counts, role manifests/work ids/titles, and
  role-by-status workload plus work id/title lists without rendering media.
- Review queue metadata remains manifest-owned and renderer-neutral.
- `review-all` remains the render-heavy artifact and review-pack routing command.
