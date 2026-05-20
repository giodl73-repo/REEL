# Pulse 01: Non-rendering review queue

## Outcome

Added a non-rendering review queue summary command.

## Changes

- Added `review-queue` text and JSON output.
- Added `ReviewQueueReport` and per-work review queue reports.
- Documented the command and started the review queue readiness wave.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- review-queue works --output json
git diff --check
```
