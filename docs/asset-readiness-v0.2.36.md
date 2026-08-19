# Asset readiness and production operations (v0.2.36)

REEL separates a valid timeline, a complete generation contract, and
materialized picture readiness. A conformed manifest no longer reports preview
or delivery readiness while its visual assets remain planned or unreviewed.

Shots may declare `visual_asset_status` as `planned-unrendered`, `candidate`,
`selected`, `approved`, or `missing`. The legacy spelling
`candidate-unreviewed` remains accepted. `selected` and `approved` mean that a
materialized picture source is available; they do not imply creative,
principal, rights, publication, or release approval.

Existing still/video manifests with a nonempty `visual_asset` remain compatible.
Animation and sprite-animation shots use their embedded materialized picture
contracts. Prompt generation requires both `render_from_prompt: true` and a
nonempty `visual_prompt`; prompt readiness never ungates FFmpeg picture
rendering.

Validation reports `timing_ready`, `generation_ready`, `asset_ready`,
`preview_ready`, `delivery_ready`, normalized `asset_status_counts`, and
`semantic_blockers`. Audio-only checks and caption export require timing
readiness. Picture rendering and delivery require timing plus materialized
assets.
