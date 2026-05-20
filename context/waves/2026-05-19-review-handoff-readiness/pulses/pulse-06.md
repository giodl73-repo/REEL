# Pulse 06: Review status work ids

## Outcome

Added aggregate work id lists split by review status to review-all reports.

## Changes

- Added `review_status_work_ids` to `ReviewAllReport` JSON output.
- Added review-status work id lists to the generated review-pack index.
- Updated README and the review handoff wave record.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- review-all works --output json
git diff --check
```
