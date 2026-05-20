# Pulse 08: Review status review packs

## Outcome

Added aggregate review-pack path lists split by review status to review-all
reports.

## Changes

- Added `review_status_review_packs` to `ReviewAllReport` JSON output.
- Added review-status review-pack path lists to the generated review-pack index.
- Updated README and the review handoff wave record.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- review-all works --output json
git diff --check
```
