# Asset readiness v0.2.24

REEL CLI v0.2.24 separates a valid, conformed timeline from a renderable
production package. This closes a BERTICA acceptance gap where a 70-shot
S1E01 plan reported `preview_ready: true` and `delivery_ready: true` while most
visual assets were still unrendered.

## Shot contract

A shot may declare:

```yaml
visual_asset: production/visual/shot-001.png
visual_asset_status: candidate
render_from_prompt: false
```

`visual_asset_status` accepts:

- `planned-unrendered`
- `candidate`
- `selected`
- `approved`
- `missing`

The legacy spelling `candidate-unreviewed` is accepted as `candidate`.
`selected` and `approved` both mean that media is available for rendering; they
do not imply principal, editorial, publication, or release approval.

Existing manifests without `visual_asset_status` remain compatible. A nonempty
`visual_asset` is treated as selected. A missing asset is not inferred from a
nonempty prompt. Prompt-driven rendering must opt in with
`render_from_prompt: true`, and the shot must provide a nonempty
`visual_prompt`.

## Validation report

`reel validate` now reports:

- `timing_ready`: schema and timeline permit timed operations;
- `generation_ready`: every shot already has selected media or has explicitly
  opted into a nonempty prompt-generation contract;
- `asset_ready`: every shot has materialized selected/approved media;
- `preview_ready`: timing and assets are both ready;
- `delivery_ready`: delivery timing and assets are both ready;
- `asset_status_counts`: normalized shot counts by readiness state; and
- `semantic_blockers`: production blockers distinct from schema errors.

`asset_ready` requires materialized selected/approved media. Prompt-only plans
can therefore be generation-ready while remaining asset-, preview-, and
delivery-blocked. This prevents the FFmpeg animatic adapter from accepting a
plan that still lacks files.

Audio-only checks and caption export require timing readiness, not visual asset
readiness. Picture preview, animatic rendering, artifact packaging, and delivery
remain gated until both timing and assets are ready.
