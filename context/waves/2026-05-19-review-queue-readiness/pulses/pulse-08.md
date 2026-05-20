# Pulse 08: Review status required roles

## Outcome

Added status-to-required-role lists to the non-rendering review queue summary.

## Changes

- Added `review_status_required_roles` to `ReviewQueueReport` JSON output.
- Added `status_roles` to `review-queue` text output.
- Updated README and the review queue wave record.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- review-queue works --output json
cargo run --quiet -- review-queue works
git diff --check
```
