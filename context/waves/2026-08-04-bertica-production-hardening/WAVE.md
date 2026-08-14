# Wave: BERTICA-driven production hardening

## Goal

Turn repeated BERTICA animatic production costs into portable REEL v0.2
contracts and deterministic CLI transformations without copying private assets
or creating a consumer runtime dependency on REEL internals.

## Pulses

| Pulse | Title | Status | Outcome |
|---:|---|---|---|
| 01 | Production contract and conform | complete | Add untimed planning, speaker cues, protected pauses, atomic conform, source coverage, privacy egress, lineage, quality checks, migration, and sanitized acceptance fixtures. |
| 02 | Asset-backed animatic adapter | complete | Add deterministic FFmpeg still-animatic rendering and provenance-rich artifact reports. |
| 03 | Hardening and adoption | complete | Added CI/release/install guidance, completed 12/12 read-only BERTICA migration validation, rendered a 7.500-second Windows/WSL FFmpeg proof with A/B variants, and documented the consumer handoff. |
| 04 | Executable voice performance | complete | Added exact-span performance direction, Chatterbox/generic compilation, executed-versus-advisory disclosure, atomic path-free receipts, re-verification, sanitized fixtures and v0.2.15 CLI tests. |
| 05 | Scoped cadence evidence | complete | Separated emotion scope, register, contour, terminal boundary, relative pitch and joins; added explicit IndexTTS 2.5 capability disclosure plus path-free, hash-bound prosody evidence that detects a requested fall rendered as a rise. |

## Invariants

- REEL never uploads or copies BERTICA private photographs, voice, manuscript
  text, or binary renders.
- Transformations create derivatives and do not overwrite planning sources.
- Human approval is referenced, never inferred.
- Child implementation commits precede TRACKER pointer updates.

## Validation

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo run --quiet -- validate manifests/fixtures/two-speaker-untimed/planning.yaml --output json
git diff --check
```
