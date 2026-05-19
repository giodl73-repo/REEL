# Pulse 05: Corpus regression test

## Outcome

Added a focused regression test for the non-rendering corpus summary surface.

## Changes

- Added `summarizes_multi_work_corpus_without_rendering`.
- Locked expected multi-work corpus totals for manifests, work ids, titles,
  source repos, source ids, formats, styles, platforms, scenes, shots, exports,
  and manifest timing totals.
- Updated the multi-work wave record to call out the new test coverage.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- corpus works
cargo run --quiet -- corpus works --output json
git diff --check
```
