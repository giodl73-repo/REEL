# Pulse 04: Executable voice performance

## Outcome

Implemented REEL v0.2.15 cue-span performance direction after a consumer proof
showed that stored prose direction was not necessarily executed by a synthesis
engine. The additive sidecar binds exact narration substrings to controlled
actions and expressive dimensions. Compilation and receipts distinguish native,
deterministic and advisory-only behavior; human listening remains mandatory.

The role review is recorded at
`signals/roles/check/reel-voice-performance-v0215-roles-check-2026-08-13.md`.
It approved the planning interface with the explicit condition that a later
render-result receipt bind output audio hash and duration; this pulse does not
claim that plan compilation rendered audio.

## Validation

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo run --quiet -- voice-performance-plan manifests/fixtures/voice-performance/manifest.yaml manifests/fixtures/voice-performance/performance.yaml --engine chatterbox --engine-version 0.1.7 --seed 1947 --output-dir target/voice-performance-fixture --output json
cargo run --quiet -- voice-performance-plan-check target/voice-performance-fixture manifests/fixtures/voice-performance/manifest.yaml manifests/fixtures/voice-performance/performance.yaml --output json
git diff --check
```
