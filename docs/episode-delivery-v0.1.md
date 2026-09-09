# Episode consumption and controlled scene review

These tools connect verified scene deliveries to the actual lossless episode and
to bounded creative comparisons. They do not select scenes or render a new episode.

```console
reel episode-delivery-check episode.json --asset-root hydrated --output-path episode-check.json
reel scene-review-render comparison.json --asset-root hydrated --output-dir new-review
reel scene-review-check comparison.json --asset-root hydrated --output-dir new-review
```

## Episode contract

JSON or YAML, strict schema `reel.episode-delivery.v0.1`:

```yaml
schema: reel.episode-delivery.v0.1
id: private-episode
scenes:
  - id: scene-01
    job: {path: scene-01/job.json, sha256: <hash>, bytes: 123}
    asset_root: scene-01/assets
    receipt: {path: scene-01/delivery/receipt.json, sha256: <hash>, bytes: 456}
master: {path: episode/master.mkv, sha256: <hash>, bytes: 789}
layers: []
boundary_decisions: []
```

Replace placeholders with real identities. All references, including scene job
and receipt paths, are relative to `--asset-root`; a scene's contract remains
relative to its job as in scene delivery. Exact case, local-root confinement,
SHA-256 and byte counts are checked. Every scene is recompiled and rechecked.
Scene IDs must match their compiled jobs and be unique within the episode.
Model bookends as scene deliveries too. This contract describes a continuous
ordered concatenation, not overlapping scene transitions or a lossy final MP4.

The master must contain exactly FFV1/yuv444p picture and stereo PCM24 audio at the
scene dimensions, frame rate and sample rate. Decoding uses no audio resampling,
time stretching or frame-rate conversion. Bounded-memory hashing compares every
frame and PCM sample, in order, with the scene picture and mix. Extra, short,
reordered or substituted content fails, even with updated file hashes. Each
scene master must also match its picture/mix, and its mix must recombine D/M/E
within three PCM24 least-significant steps of quantization tolerance.

The report maps scene source/receipt hashes to exact episode frame/sample offsets.
Master timestamps must start at zero and follow the continuous frame/sample
timeline within 2 ms of container rounding. More than one frame of cumulative
scene audio/picture rounding fails; reconform explicitly upstream. Neither drift
nor a late audio stream is fixed by changing an expected file hash.

The full lossless decode is intentionally an integration check, not something to
repeat on every planning edit. Existing changed-only graphs should schedule it
when the final conform or its selected inputs change.

## External layers

Each scene external attachment needs exactly one episode `layers` record:

```yaml
- scene_id: scene-01
  attachment_id: light-effect
  disposition: baked-picture
  picture_attachment_id: selected-motion
  evidence: {path: scene-01/assets/effect-receipt.json, sha256: <hash>, bytes: 123}
  reason: Checked effect precomposition consumed through selected-motion
```

`baked-picture` must identify a video picture in that scene and preserve the exact
external evidence hash. `excluded-from-clean-master` requires a reason and null
`picture_attachment_id`; use it for separately delivered titles/captions or an
explicitly excluded audition layer. Missing, duplicate and unknown layers fail.

This proves the scene's declared precomposition and final picture binding. It
does **not** inspect an effect's semantics, authenticate its upstream checker, or
verify separately delivered subtitle/title streams. Run the existing effect,
animatic and caption validators and inspect onset/sustain/release. An exclusion
remains visible in the report; it is not an approval to omit required material.

## Cut-boundary findings

The checker examines neighboring scene edges and reports:

| Code | Measured trigger |
| --- | --- |
| audio-step | Last/first mix sample changes by more than 0.1 full scale in either channel |
| ambience-level-step | E-bus RMS differs by more than 12 dB over adjacent 100 ms windows |
| music-tail-risk | Last M-bus sample exceeds 0.01 full scale before a cut |
| black-at-cut | At least 98% of either boundary frame has grayscale value at or below 16 |

These are heuristics. E includes foreground effects, dark images can be authored,
and a music cut may be deliberate. Equal-level ambience resets, quiet shortened
tails, internal black frames and other perceptual faults still need listening
and picture review. Do not describe a clean report as comprehensive quality proof.

An unresolved finding returns `content_verified: true`, `passed: false`, and a
nonzero CLI exit after writing the report. A decision may acknowledge one exact
finding with `left_scene`, `right_scene`, their exact `left_receipt_sha256` and
`right_receipt_sha256`, `code`, `owner`, `reason`, and an evidence
FileRef. Decisions must match current findings; stale, duplicate or blanket
exceptions fail. Decision evidence is hash-checked, not authenticated as human
approval. REEL never smooths a cut or changes a mix automatically.

## Controlled scene comparison

```yaml
schema: reel.scene-review.v0.1
baseline:
  id: scene-01
  job: {path: before/job.json, sha256: <hash>, bytes: 123}
  asset_root: before/assets
  receipt: {path: before/delivery/receipt.json, sha256: <hash>, bytes: 456}
candidate:
  id: scene-01
  job: {path: after/job.json, sha256: <hash>, bytes: 123}
  asset_root: after/assets
  receipt: {path: after/delivery/receipt.json, sha256: <hash>, bytes: 456}
```

Both deliveries must have identical decoded D audio, native duration, frames and
geometry. A performance change is a separate comparison and fails this contract.
For each side the tool encodes three review videos directly from the lossless
picture: dialogue only, dialogue plus E (no score), and full mix. The HTML page
switches variants at the same playhead. Labels remain outside the picture.
Supply clean/effects-on scene variants for a VFX comparison; the tool cannot
remove effects from an already flattened picture.

The package retains no-score and E+M PCM, existing `audio-check` measurements for
both full mixes (including D versus E+M margin), child receipts, and hashes for
all 15 outputs. Audio-quality violations are preserved for review, not hidden or
turned into permission to normalize. Silence-heavy auditions may intentionally
fail a delivery loudness profile. Every video is decoded before the staged
directory is atomically renamed. Existing output directories are never replaced.
`scene-review-check` revalidates source receipts, identical dialogue and retained
output hashes. It does not claim browser inspection or human listening.

The existing general comparison composer remains the adapter for animatic
receipts/slates. Scene review consumes native scene-delivery receipts without
inventing fake animatic evidence and reuses the existing audio quality gate.

## Validation

`cargo test --test scene_delivery real_episode_consumption_boundaries_and_controlled_review -- --ignored --exact`

The sanitized two-scene canary verifies exact offsets, a detected black edge,
explicit boundary dispositions, wrong scene order, a missing effects mix,
controlled review outputs, tampering, overwrite refusal and changed-dialogue
rejection. CI runs it on Windows and Linux. No consumer media is in this repository.
