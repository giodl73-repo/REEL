# Pulse 04: Required role counts

## Outcome

Added aggregate required-role counts to review-all reports.

## Changes

- Added `required_role_counts` to `ReviewAllReport` JSON output.
- Added required-role counts to the generated review-pack index.
- Updated README and the review handoff wave record.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- review-all works --output json
git diff --check
```
