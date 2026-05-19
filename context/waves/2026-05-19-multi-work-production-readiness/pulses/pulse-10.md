# Pulse 10: Corpus audience metadata

## Outcome

Added audience metadata aggregation to corpus reports so automation can inventory
reviewer targeting before render-heavy checks.

## Changes

- Added top-level `audience_primaries`, `audience_contexts`, and
  `audience_desired_effects` to `CorpusReport` JSON output.
- Added per-work `audience_primary`, `audience_context`, and
  `audience_desired_effect` to corpus work reports.
- Added audience metadata to `cargo run -- corpus` text output.
- Updated corpus regression coverage, README, and the multi-work wave record.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- corpus works
cargo run --quiet -- corpus works --output json
git diff --check
```
