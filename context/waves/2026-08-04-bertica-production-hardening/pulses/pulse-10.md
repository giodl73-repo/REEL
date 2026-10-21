# Pulse 10 — Inventory block review and owner choices

Add private offline HTML review to the existing Rust asset manager, typed block
choices and exact bounded native verification-request preparation. Preserve all
existing apply gates; do not add browser execution, a server or a cleanup script.
New inventory timestamps distinguish acquisition from plan reconciliation.

Validation: `cargo fmt --all -- --check`,
`cargo test -p reel-assets --all-targets --locked`,
`cargo clippy -p reel-assets --all-targets --locked -- -D warnings`, roles and
`git diff --check`. Local Windows: 87 unit + 1 real CLI tests pass. Private real
inventory and browser round-trip evidence stay upstream. Promotions, cold custody
and last-copy retirement remain separate work, never enabled by this pulse.
