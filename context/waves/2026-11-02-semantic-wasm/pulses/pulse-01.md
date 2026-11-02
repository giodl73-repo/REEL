# Pulse 1: preserve and qualify the portable core

Current public code and prior PITFALL doctrine are reconciled at fe08024.
`git diff --exit-code e4d22e0 -- Cargo.toml Cargo.lock src crates vendor` passes.
Run pinned Rust 1.95.0 formatting, locked reel-assembly all-target tests, strict
Clippy and wasm32 check. Native functions/types stay unchanged. Later pulses own
browser implementation and release qualification; this entry is not live proof.
