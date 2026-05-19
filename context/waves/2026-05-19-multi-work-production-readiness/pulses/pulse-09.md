# Pulse 09: Corpus alternate styles

## Outcome

Added alternate-style aggregation to corpus reports so automation can inventory
planned style variants before running render-heavy checks.

## Changes

- Added top-level `alternate_styles` to `CorpusReport` JSON output.
- Added per-work `alternate_styles` to corpus work reports.
- Added alternate styles to `cargo run -- corpus` text output.
- Updated corpus regression coverage, README, and the multi-work wave record.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- corpus works
cargo run --quiet -- corpus works --output json
git diff --check
```
