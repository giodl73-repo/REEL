# Sanitized two-speaker conform fixture

This fixture is structurally representative of a poem-to-prose production
handoff without containing manuscript text, real voices, photographs, private
paths that are opened by tooling, or production binaries.

The acceptance path is:

```powershell
cargo run --quiet -- validate manifests/fixtures/two-speaker-untimed/planning.yaml
cargo run --quiet -- plan manifests/fixtures/two-speaker-untimed/planning.yaml --output json
cargo run --quiet -- conform manifests/fixtures/two-speaker-untimed/planning.yaml `
  --cues manifests/fixtures/two-speaker-untimed/cue-measurements.yaml `
  --speaker-tempo narrator=85 `
  --output-dir target/fixture-conform
cargo run --quiet -- validate target/fixture-conform/manifest.yaml
```

Expected invariant: the poet remains 2000 ms, the protected pause remains
1500 ms, the narrator becomes 4000 ms, and the complete work becomes 7500 ms.
