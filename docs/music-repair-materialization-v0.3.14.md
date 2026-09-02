# Deterministic music-repair materialization v0.3.14

REEL v0.3.14 completes executable coverage for the existing
`reel.music-repair.v0.1` operation vocabulary without changing the legacy
cut-only EDL or its expected hashes. The new commands are:

```powershell
reel music-repair-materialize repair.yaml `
  --output-pcm candidate.raw `
  --receipt candidate.receipt.json `
  --output json

reel music-repair-materialize-check repair.yaml `
  candidate.raw candidate.receipt.json `
  --output json
```

Both outputs are no-clobber writes. The checker recompiles the repair from its
current source and assets, then rejects source, manifest, policy, output, or
receipt tampering.

## Operation semantics

- `keep` and `lock` retain source frames byte-for-byte.
- `cut` removes its source range.
- `insert` places its hash-pinned asset immediately before the destination and
  retains the destination.
- `replace` substitutes the destination with its hash-pinned asset.
- `repeat` places a copy of the equal-length source immediately before the
  destination and retains the destination.
- `move` removes the source and places it immediately before the destination,
  retaining the destination and total duration.
- `crossfade` treats the equal halves of its even-length range as outgoing and
  incoming material and overlaps them with `linear` or `equal-power` weights.
- `preserve-tail` mixes the equal-length source tail over the destination with
  a deterministic linear decay.
- `match-gain` measures the range with deterministic K-weighted, absolute- and
  relative-gated EBU R128 logic and applies the gain required by
  `target_millilufs`.
- `match-eq` applies ordered peaking biquads. Inline integer bands use mHz,
  milli-Q, and millidB; `profile_sha256` must equal their canonical hash.
- `extend-bars` repeats its one-bar range the declared number of additional
  times and requires an explicit sample-domain beat grid.

All coordinate ranges are half-open source-frame ranges. Structural operations
are resolved together against the immutable source, not against results of
earlier operations, so manifest order cannot introduce hidden coordinate drift.

## Evidence

The path-free `reel.music-repair-render-receipt.v0.2` binds the tool version,
repair manifest and canonical contract, source contract and decoded PCM,
output PCM, format, sample geometry, and operation counts. Every unaffected
span records matching source/output hashes and coordinates.

When a beat grid is declared, every changed boundary is checked with its
declared sample tolerance. Bar extension additionally requires an exact
declared bar length. Every output seam records:

- boundary delta;
- ambience-window RMS/loudness delta;
- reverb-tail correlation;
- phase/window correlation;
- spectral distance; and
- clipping sample count.

Continuity failure remains explicit evidence on a retained candidate. It does
not mutate the source, select another operation, call a generative system, or
claim listening approval. The original FFmpeg cut renderer remains available
for legacy manifests and its established exact-hash behavior is unchanged.
