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
| 03 | Corpus manifest paths | done | Added top-level manifest path aggregation to corpus text and JSON reports so automation can identify exactly which work manifests were summarized. |
| 04 | Corpus source ids | done | Added top-level source id aggregation to corpus text and JSON reports so automation can inventory exact source scenario identifiers. |
| 05 | Corpus regression test | done | Added a focused regression test for the multi-work corpus summary totals, manifest paths, work ids, source ids, formats, styles, and duration totals. |
| 06 | Corpus manifest versions | done | Added manifest-version aggregation to corpus text and JSON reports so automation can verify schema uniformity before render-heavy checks. |

## Success criteria

- `cargo run -- artifact-check-all works` verifies multiple work manifests.
- `cargo run -- corpus works --output json` summarizes the multi-work corpus
  without rendering media, including exact manifest paths, manifest versions, and
  source ids.
- `cargo test --quiet` locks the expected corpus summary totals and identifier
  sets for the current multi-work fixture.
- `cargo run -- review-all works --output json` reports multiple works with
  distinct work ids, titles, source manifests, artifact manifests, review packs,
  adapters, platforms, media counts, bytes, and duration totals.
- Additional corpus works remain renderer-neutral and do not require new provider
  SDKs, credentials, or binary dependencies.
- Generated renders stay under ignored `renders\` paths.
