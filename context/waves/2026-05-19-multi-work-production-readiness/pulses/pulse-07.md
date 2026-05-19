# Pulse 07: Corpus platform names

## Outcome

Added platform-name aggregation to corpus reports so automation can inventory
target platform identifiers before running render-heavy checks.

## Changes

- Added top-level `platform_names` to `CorpusReport` JSON output.
- Added per-work `platform_names` to corpus work reports.
- Added platform names to `cargo run -- corpus` text output.
- Updated corpus regression coverage, README, and the multi-work wave record.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- corpus works
cargo run --quiet -- corpus works --output json
git diff --check
```
