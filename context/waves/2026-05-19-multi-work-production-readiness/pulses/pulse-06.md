# Pulse 06: Corpus manifest versions

## Outcome

Added manifest-version aggregation to corpus reports so automation can verify
work-manifest schema uniformity before running render-heavy artifact checks.

## Changes

- Added top-level `manifest_versions` to `CorpusReport` JSON output.
- Added per-work `manifest_version` to corpus work reports.
- Added manifest versions to `cargo run -- corpus` text output.
- Updated corpus regression coverage, README, and the multi-work wave record.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- corpus works
cargo run --quiet -- corpus works --output json
git diff --check
```
