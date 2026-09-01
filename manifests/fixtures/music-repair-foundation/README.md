# Synthetic music-repair foundation fixture

This fixture contains 62 bytes interpreted as mono unsigned 8-bit raw PCM. It
is deliberately tiny and synthetic: no consumer audio, lyric, title, identity,
or creative judgment is present.

It proves the v0.2.26 foundation boundaries:

```powershell
cargo run --quiet -- music-source-validate manifests/fixtures/music-repair-foundation/source.yaml --output json
cargo run --quiet -- music-neutral-plan manifests/fixtures/music-repair-foundation/source.yaml --output-path target/music-neutral-plan.json --output json
cargo run --quiet -- music-neutral-check target/music-neutral-plan.json manifests/fixtures/music-repair-foundation/source.yaml manifests/fixtures/music-repair-foundation/source.u8 --output json
cargo run --quiet -- music-repair-plan manifests/fixtures/music-repair-foundation/repair.yaml --output json
```

The repair plan marks samples 16–32 as changed and locks every sample outside
that half-open range. It validates planning only; it does not render the cut or
claim that the synthetic bytes sound musical.
