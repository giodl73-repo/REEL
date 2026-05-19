# Pulse 02: Corpus summary command

## Outcome

Added a fast non-rendering corpus summary command for multi-work production
readiness.

## Changes

- Added `cargo run -- corpus <works-root>` to validate every work manifest under
  a root and summarize the corpus without rendering media.
- Added `--output json` for machine-readable corpus inventories.
- Corpus reports include work ids, titles, source repos, formats, styles,
  platform counts, scene counts, shot counts, export counts, and manifest timing
  totals.
- Documented the command in README, product validation, and the multi-work wave.

## Validation

```powershell
cargo fmt
cargo test --quiet
cargo run --quiet -- corpus works
cargo run --quiet -- corpus works --output json
cargo run --quiet -- artifact-check-all works
cargo run --quiet -- review-all works --output json
git diff --check
```
