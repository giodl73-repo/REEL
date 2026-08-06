# Production manifest v0.2

`reel.manifest.v0.2` is an additive production contract for planning before
voice timing, deterministic voice conform, input provenance, private-reference
protection, explicit derivatives, and long-still review.

REEL CLI v0.2.5 does not revise this contract. Smooth subpixel motion,
ease-in/out, cadence analysis, legacy reproduction, artifact verification, and
render-environment diagnostics and lineage are CLI and artifact-report behavior;
existing `reel.manifest.v0.2` files need no migration. See
`smooth-motion-v0.2.2.md`, `animatic-verification-v0.2.3.md`,
`render-environment-v0.2.4.md`, and `render-lineage-v0.2.5.md`.

The supported profiles are `animatic`, `voice-audition`, and
`production-package`. Migration recognizes legacy work IDs containing
`voice-audition`; it does not force audio review artifacts into the animatic
profile.

## Timing lifecycle

- `untimed`: ordered scenes and shots exist, but timing is not invented.
- `guide`: provisional timing may be previewed but is not delivery-ready.
- `conformed`: cue measurements have deterministically produced a consistent
  timeline.
- `locked`: a reviewed conformed timeline that cannot be reconformed directly.

`validate` and `plan` work at every stage. Render, preview, caption-export, and
artifact commands reject `untimed` manifests with `timing not conformed`.
Delivery is allowed only for `conformed` or `locked` manifests.

All v0.2 seconds are normalized to integer milliseconds during validation and
transformation. Serialized seconds therefore have millisecond precision.

## Speakers, cues, and protected pauses

`speakers` contain stable identity, language, pronunciation, performance,
approval-reference, and human/guide/synthetic asset-kind metadata.
`narration_cues` contain stable cue and speaker IDs, exact text or an upstream
reference, source ranges, shot associations, and pause policy.

`protected_pauses` identify a cue boundary and exact millisecond duration. A
speaker-specific tempo transform cannot change a protected pause.

## Conform packet

```powershell
cargo run -- conform planning.yaml `
  --cues cue-measurements.yaml `
  --speaker-tempo narrator=85 `
  --output-dir production/conformed/scene-v2
```

The output directory is published atomically and contains:

- `manifest.yaml` — validated conformed derivative consumed by renderers.
- `captions.srt` — cue-derived caption timing.
- `lineage.json` — input, cue/audio hashes, tempo parameters, output hashes,
  and tool version.
- `conform-report.json` — machine-readable packet summary.

The command refuses to overwrite a non-empty packet and emits no partial final
packet when conform fails.

Conformed cues carry their own start and duration, so captions can be reproduced
without the original measurement file:

```powershell
cargo run -- caption-export conformed/manifest.yaml --output captions.srt
```

## Source provenance

`source_ranges` assign IDs to contiguous upstream ranges. Cues and shots use
`source_refs`, allowing discontiguous selections. `omissions` identify omitted
ranges and require one of these bridges: `silence`, `title-card`,
`archival-image`, or `approved-adaptation`.

```powershell
cargo run -- source-coverage manifest.yaml --output json
```

The report identifies invented, unattributed, invalid, and uncovered material.

## Private continuity and provider egress

Continuity `entities` carry age-at-scene, approved textual observations,
confidence, provenance, confirmation status, and local reference metadata.
Every reference declares `provider_transfer` as `forbidden`,
`approval_required`, or `approved`.

```powershell
cargo run -- provider-package manifest.yaml `
  --output provider-package.json --format json
```

The package never serializes local paths. It lists all requested assets, prompt
text, observation text, and outbound text-field declarations. Requested assets
without both `approved` policy and an approval reference block execution.

## Variants and actual findings

`lineage` records parent, root work, transformation reason, changed dimensions,
review-candidate status, principal-approved status, and creation time. Human
findings remain separate entries in `review.principal_findings`.

```powershell
cargo run -- review-select production --output json
```

The report selects the latest declared candidate by deterministic path order;
it never converts role output or filename recency into approval.

## Long-still controls

`quality_controls` can require maximum low-motion holds, focal points, protected
regions, no-lip-sync mode, and declared narration/effects A/B outputs. Shots can
record normalized focal points and protected regions, depth layers, screen
position, and eye line.

```powershell
cargo run -- quality-check manifest.yaml --output json
```

## Asset-backed animatic rendering

```powershell
cargo run -- animatic-render conformed/manifest.yaml `
  --asset-root C:/src/consumer-repo `
  --audio production/audio/master.wav `
  --narration-only-audio production/audio/narration.wav `
  --effects-music-audio production/audio/effects-music.wav `
  --captions conformed/captions.srt `
  --output production/video/private-review.mp4
```

The FFmpeg adapter scales and crops before bounded pan/zoom motion, burns
captions and a disclosure, records input hashes and command parameters, and
writes a sibling `*.artifacts.json` report. When `quality_controls.ab_outputs`
requests `narration-only` or `effects-music`, the matching audio arguments are
required and named A/B videos are rendered from the identical visual timeline.
All requested A/B inputs are checked before any output is written.
`--dry-run` builds and records every command without invoking FFmpeg.

For a platform whose manifest declares `sound_optional: true`, the renderer can
produce a video with no audio stream:

```powershell
cargo run -- animatic-render conformed/vertical.yaml `
  --asset-root production/frames `
  --silent `
  --captions conformed/captions.srt `
  --width 720 --height 1280 `
  --output production/video/vertical-sound-off.mp4
```

Silent output is explicit in the artifact report and cannot be combined with
manifest-requested audio A/B outputs. Portrait renders also receive larger
caption styling and platform-safe lower margins. The sanitized acceptance proof
is in `manifests/fixtures/vertical-sound-off/`.

## Migration

```powershell
cargo run -- migrate legacy.yaml --output migrated-v0.2.yaml --normalize-timing
```

Migration always writes a new file, preserves unknown YAML fields, aliases the
legacy `schema` key, normalizes accumulated shot timing to milliseconds, and
lifts legacy shot narration into review-required single-speaker cues.
