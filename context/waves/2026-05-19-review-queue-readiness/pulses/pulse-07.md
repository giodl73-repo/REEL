# Pulse 07: Required role status manifests

## Outcome

Added role-by-status manifest path lists to the non-rendering review queue
summary.

## Changes

- Added `required_role_status_manifests` to `ReviewQueueReport` JSON output.
- Added role-by-status manifest path lists to `review-queue` text output.
- Updated README and the review queue wave record.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- review-queue works --output json
cargo run --quiet -- review-queue works
git diff --check
```
