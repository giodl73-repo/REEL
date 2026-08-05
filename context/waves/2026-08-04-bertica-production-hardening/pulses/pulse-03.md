# Pulse 03: Hardening and adoption

## Outcome

Closed the BERTICA production-hardening wave with cross-platform automation,
versioned release guidance, live consumer migration evidence, and an executable
handoff contract.

## Evidence

- 49 unit tests and one CLI integration test pass.
- Strict Clippy and rustfmt pass.
- Twelve current BERTICA production manifests migrate and validate as v0.2
  derivatives without modifying the private consumer repository.
- A real Windows/WSL FFmpeg proof rendered the synthetic conform fixture and
  both declared A/B variants at exactly 7500 ms.
- Linux CI performs the same conform and real animatic render path.
- The BERTICA response identifies the exact packet and commands to consume.

## Validation

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo run --quiet -- validate manifests/fixtures/two-speaker-untimed/planning.yaml --output json
git diff --check
```
