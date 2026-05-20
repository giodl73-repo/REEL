# Pulse 07: Review status work titles

## Outcome

Added aggregate work title lists split by review status to review-all reports.

## Changes

- Added `review_status_work_titles` to `ReviewAllReport` JSON output.
- Added review-status work title lists to the generated review-pack index.
- Updated README and the review handoff wave record.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- review-all works --output json
git diff --check
```
