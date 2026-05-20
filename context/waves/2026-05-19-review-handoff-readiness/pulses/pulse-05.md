# Pulse 05: Required role status counts

## Outcome

Added aggregate required-role counts split by review status to review-all
reports.

## Changes

- Added `required_role_status_counts` to `ReviewAllReport` JSON output.
- Added required-role status counts to the generated review-pack index.
- Updated README and the review handoff wave record.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- review-all works --output json
git diff --check
```
