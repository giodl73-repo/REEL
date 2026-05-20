# Pulse 02: Required role work ids

## Outcome

Added required-role work id lists to the non-rendering review queue summary.

## Changes

- Added `required_role_work_ids` to `ReviewQueueReport` JSON output.
- Added required-role work id lists to `review-queue` text output.
- Updated README and the review queue wave record.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- review-queue works --output json
git diff --check
```
