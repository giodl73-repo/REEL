# Pulse 04: Corpus source ids

## Outcome

Added top-level source id aggregation to corpus reports so automation can
identify exact source scenario identifiers without walking every per-work row.

## Changes

- Added `source_ids` to `CorpusReport` JSON output.
- Added `source_ids` to the text summary line for `cargo run -- corpus`.
- Updated README and the multi-work wave record to describe the source-id
  inventory.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- corpus works
cargo run --quiet -- corpus works --output json
git diff --check
```
