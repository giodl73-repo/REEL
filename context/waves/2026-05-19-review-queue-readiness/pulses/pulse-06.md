# Pulse 06: Required role status work titles

## Outcome

Added role-by-status work title lists to the non-rendering review queue summary.

## Changes

- Added `required_role_status_work_titles` to `ReviewQueueReport` JSON output.
- Added role-by-status work title lists to `review-queue` text output.
- Updated README and the review queue wave record.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- review-queue works --output json
cargo run --quiet -- review-queue works
git diff --check
```
