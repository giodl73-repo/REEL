# Episode verification and scene review handoff

Consumer-facing commands and schemas are documented in
`docs/episode-delivery-v0.1.md`. Adopt this increment after the existing compiled
scene-delivery parity proof. Pin the merged commit, not package version alone.

First return: run the synthetic episode/review canary, then verify a bounded
ordered group of real scene deliveries and create an identical-dialogue A/B
review. Record the source and master hashes, per-scene offsets, boundary findings
and decisions, external-layer dispositions, audio-quality reports and exact
review outputs. Use the existing changed-only graph and cache handoff contract.

`episode-delivery-check` does not produce or select a final episode. It verifies
an independently conformed FFV1/PCM clean master. Bookends must be modeled as scene
deliveries. Separate subtitle/title tracks and semantic VFX correctness still
require their own evidence. No private media or new performance operation is
included or authorized by this handoff.

The review package is a comparison artifact, not a new Golden. Preserve unresolved
audio measurements and listening/visual findings. A valid receipt is not approval.

Validation commands:

```console
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --test scene_delivery -- --include-ignored
```

CI runs the existing scene canary and new episode/review canary on both Windows
and Linux, alongside the established broader FFmpeg verification job.
