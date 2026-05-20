# Pulse 05: Required role status work ids

## Outcome

Added role-by-status work id lists to the non-rendering review queue summary.

## Changes

- Added `required_role_status_work_ids` to `ReviewQueueReport` JSON output.
- Added role-by-status work id lists to `review-queue` text output.
- Updated README and the review queue wave record.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- review-queue works --output json
cargo run --quiet -- review-queue works
git diff --check
```
