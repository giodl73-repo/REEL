# Pulse 08 — Verified redundant asset-copy removal

Owned slice: explicit Windows handle-based redundant hot CAS/LFS link removal
in reel-assets. Exact historical proposals, retained-copy hashing/handles,
managed-consumer protections, admission holds, immutable journals, recovery and
bounded byte/time checks are required. No cold census or last-copy retirement.

The job runner reserves before selecting source paths. Participating consumers
must adopt that admission protocol or be quiescent before real maintenance.
Legacy/unsupported metadata remains a hold; no live consumer is silently omitted.

Validation for this crate-only slice:

```text
cargo fmt --all -- --check
cargo test -p reel-assets --all-targets --locked
cargo clippy -p reel-assets --all-targets --locked -- -D warnings
git diff --check
```

The prior pulse's complete local root suite and Windows/Linux/FFmpeg CI passed;
new CI continues full repository checks. Private consumer results remain upstream.
Executed outcomes and remaining real-pilot holds are in the dated handoff.
