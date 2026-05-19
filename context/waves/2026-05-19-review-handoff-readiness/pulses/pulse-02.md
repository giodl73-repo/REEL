# Pulse 02: Review-all handoff metadata

## Outcome

Added review handoff metadata to aggregate review-all reports.

## Changes

- Added top-level `review_statuses` and `required_roles` to `ReviewAllReport`.
- Added per-work `review_status` and `required_roles` to review-all JSON output.
- Added review statuses and required roles to the generated review-pack index.
- Updated README and the review handoff wave record.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- review-all works --output json
git diff --check
```
