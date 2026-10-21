# Pulse 09 — Bounded version indexes and legacy protections

Explicit immutable control-baseline compaction avoids repeated large Git control
blobs. Compatibility reads retain flat State and version-record shards, add the
existing version-index-v2 shape, protect metadata roots, and use bounded typed
E/D-only lookups. No history rewrite, limit increase or media retirement.

Explicit read-only legacy consumers can export current/lease/metadata/catalogue
protections without configuring or editing their checkout. Default copy
verification can report live admissions without blocking jobs; apply still holds.

Validation:

```text
cargo fmt --all -- --check
cargo test -p reel-assets --all-targets --locked
cargo clippy -p reel-assets --all-targets --locked -- -D warnings
git diff --check
```

Private production compaction/protection evidence remains upstream. No private
values are public fixtures. See the dated handoff for actual outcome and scope.
