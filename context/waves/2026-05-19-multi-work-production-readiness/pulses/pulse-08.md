# Pulse 08: Corpus source paths

## Outcome

Added source path and source commit aggregation to corpus reports so automation
can inventory upstream scenario files and revisions before render-heavy checks.

## Changes

- Added top-level `source_paths` and `source_commits` to `CorpusReport` JSON
  output.
- Added per-work `source_path` and `source_commit` fields to corpus work reports.
- Added source paths and commits to `cargo run -- corpus` text output.
- Updated corpus regression coverage, README, and the multi-work wave record.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- corpus works
cargo run --quiet -- corpus works --output json
git diff --check
```
