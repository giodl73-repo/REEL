# Wave: Multi-work production readiness

## Goal

Prove REEL's artifact and review automation over a small corpus of real work
manifests instead of a single Ash Vale sample, while preserving the existing
validation contract and keeping generated media out of git.

## Pulse table

| Pulse | Title | Status | Outcome |
|------:|---|---|---|
| 01 | Second corpus work | done | Added `0002-court-first-rally` as a lightweight COURT storyboard-animatic work so batch artifact and review reports cover more than one work. |
| 02 | Corpus summary command | done | Added `cargo run -- corpus <works-root>` with text and JSON output to validate and summarize a work corpus without rendering media. |

## Success criteria

- `cargo run -- artifact-check-all works` verifies multiple work manifests.
- `cargo run -- corpus works --output json` summarizes the multi-work corpus
  without rendering media.
- `cargo run -- review-all works --output json` reports multiple works with
  distinct work ids, titles, source manifests, artifact manifests, review packs,
  adapters, platforms, media counts, bytes, and duration totals.
- Additional corpus works remain renderer-neutral and do not require new provider
  SDKs, credentials, or binary dependencies.
- Generated renders stay under ignored `renders\` paths.
