# Pulse 03: Corpus manifest paths

## Outcome

Added top-level manifest path aggregation to corpus reports so automation can
identify the exact work manifests covered by a non-rendering corpus summary.

## Changes

- Added `manifests` to `CorpusReport` JSON output.
- Added manifest path output to the text summary line for `cargo run -- corpus`.
- Updated README and the multi-work wave record to describe the manifest-path
  inventory.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- corpus works
cargo run --quiet -- corpus works --output json
git diff --check
```
