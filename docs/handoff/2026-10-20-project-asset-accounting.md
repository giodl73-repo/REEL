# Project asset accounting

This slice extracts the shared Rust custody core into `crates/reel-assets`,
preserving legacy serialized contracts and adding a versioned asset-project
profile plus explicit read-only cache reporting. Consumer commands can delegate
to the library without copying its implementation into asset worktrees.

Review method: producer-applied rights/provenance and operator checklists, with
synthetic executable tests. No independent or human creative approval is claimed.
Only implementation, documentation and synthetic cases belong in REEL. Real
consumer catalogues, reports, profile paths and production evidence remain private.

Validation commands:

```text
cargo fmt --all -- --check
cargo test -p reel-assets --all-targets --locked
cargo clippy -p reel-assets --all-targets --locked -- -D warnings
cargo build -p reel-assets --release --locked --bins
git diff --check
```

The report is not a deletion plan. Cross-project usage, independent recovery and
full-volume attribution remain open. Project authoring interfaces and music
crates follow as separate bounded ports after the consumer adopts this core.
