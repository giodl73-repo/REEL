# Render-environment lineage in CLI v0.2.5

REEL CLI v0.2.5 binds the successful v0.2.4 render preflight to the artifact it
authorized. It does not change `reel.manifest.v0.2` or require manifest
migration.

Every real `animatic-render` report now contains a `render_environment` object
with:

- schema and `native` or `wsl` transport;
- exact FFmpeg and ffprobe version lines;
- the seven filter, encoder, and interpolation capability checks;
- the aggregate pass state;
- a deterministic SHA-256 fingerprint over the schema, transport, executable
  identities, capability IDs, and availability results.

Dry-run reports keep `render_environment: null` because no executable was
probed. For real v0.2.5+ reports, `animatic-check` requires the environment
object, verifies the complete ordered capability set, recomputes its
fingerprint, and confirms its FFmpeg identity matches the motion backend. Older
artifact reports remain readable under their original rules. Legacy artifacts
may record the default smooth-only perspective checks as unavailable; all core
filters, encoder support, executable identity, and fingerprint checks remain
mandatory.

For BERTICA, the render and verification commands remain:

```text
reel animatic-render manifest.yaml --asset-root C:/src/bertica --audio master.wav --captions captions.srt --output candidate-v025.mp4 --format json
reel animatic-check candidate-v025.artifacts.json --output json
```

Retain the video and sibling artifact report together. The separate
`render-doctor --output json` report remains useful as a pre-production machine
audit, but the artifact now proves which successful environment snapshot was
used for that render.
