# Pulse 03: Review status counts

## Outcome

Added aggregate review status counts to review-all reports.

## Changes

- Added `review_status_counts` to `ReviewAllReport` JSON output.
- Added review status counts to the generated review-pack index.
- Updated README and the review handoff wave record.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- review-all works --output json
git diff --check
```
