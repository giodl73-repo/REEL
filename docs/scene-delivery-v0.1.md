# Compiled scene delivery v0.1

Scene delivery connects the cue-relative compiler to a small, reproducible FFmpeg
execution path. A consumer supplies selected pictures or precomposed motion clips,
native cue audio, music/effect excerpts, and explicit D/M/E policies. The renderer
uses `CompiledAttachment` samples directly; it does not resolve anchors again.

```console
reel scene-delivery-plan scene.yaml --asset-root local-assets --output-path plan.json
reel scene-delivery-render scene.yaml --asset-root local-assets --output-dir scene-result
reel scene-delivery-check scene.yaml --asset-root local-assets --output-dir scene-result
```

Planning reads and verifies files without rendering. Rendering requires FFmpeg and
ffprobe on PATH. The output directory must be new. Work is staged beside it and
published only after the compiled plan, input/output hashes, decoded PCM sample
counts, and video frame geometry recheck. A failed run does not replace a delivery.

## Job contract

JSON and YAML are accepted. Unknown fields are rejected. Every file reference is
`{path, sha256, bytes}`. The contract path is relative to the job directory; media
and external-layer evidence paths are relative to `--asset-root`. Absolute paths,
parent traversal, case-mismatched names and symlink escapes are rejected. Consumers hydrate exact assets
locally first. This command never downloads, synthesizes, selects, or publishes.

```yaml
schema: reel.scene-delivery.v0.1
id: scene-example
contract: {path: cues.yaml, sha256: <exact hash>, bytes: <exact bytes>}
production_manifest_sha256: <exact production manifest hash>
width: 1920
height: 1080
max_composition_samples: 480000 # consumer's ten-second rule at 48 kHz
pictures:
  - attachment_id: picture-establish
    source: {path: objects/selected.png, sha256: <hash>, bytes: <bytes>}
    kind: still
    attention: Establish who controls the entrance
    # Optional integer source-pixel crop, authored for a distinct attention target:
    # crop: {x: 100, y: 100, width: 1200, height: 675}
audio:
  - attachment_id: dialogue-first
    source: {path: objects/native.wav, sha256: <hash>, bytes: <bytes>}
    bus: D
    cue_id: cue-first
buses:
  D: {state: present, reason: Selected native performance}
  M: {state: intentional-silence, reason: Deliberate ambience-only comparison}
  E: {state: intentional-silence, reason: No physical action in this scene}
external_layers: []
```

The example omits real hashes and is not a production selection. An unavailable
required bus should be `held`, which blocks rendering. Do not label required but
missing material intentional silence to obtain a passing render. Each required
canonical cue has one complete D event when D is present; its source duration must
equal the compiled cue duration. A fully silent picture audition can declare D
intentional silence, but cannot be called an accepted performance or final episode.

Audio events may additionally set `source_start_sample`, `gain_db`,
`fade_in_samples`, and `fade_out_samples`. Only M/E may take excerpts. Sources
must already have the contract sample rate. No duration or pitch scaling occurs.
Fades use sample counts and placement uses FFmpeg's `adelay=<samples>S`, avoiding
the loss introduced by rounding to milliseconds. Short sources fail; no looping
or padding of a missing performance is inferred. Authored ambience loops and room
perspective should be prepared in the existing sonic/audio tools before intake.

## Composition and frame ownership

Picture attachments must be shot/cel targets and cover the complete scene exactly
once in supplied order. `kind: video` accepts an existing verified motion/VFX
composition; it plays at its native speed from the beginning and must supply enough
frames. Still inputs get no automatic zoom. An explicit crop changes framing;
matching adjacent source hashes and crops count as one unchanged composition even
when attachment IDs change. Each picture needs an attention statement. Machine
checks cannot establish that an almost identical crop is meaningful: human shot
review remains required.

An over-limit composition needs
`stillness_exception: {reason: ..., decision: ...}`. These fields reference a
consumer decision; REEL does not grant or authenticate that creative approval.

One global frame partition avoids cumulative per-shot rounding: starts and interior
joins use floor of the compiled sample position on the delivery frame grid; the
final end uses ceiling. Each frame belongs to exactly one picture. A span too short
to receive a frame fails rather than silently disappearing. This partition differs
from the compiler's overlapping floor/ceiling interval *coverage* representation;
the sample timing remains unchanged and the delivery plan exposes both boundaries.

## Existing effects, captions, titles and camera tools

This path does not invent a second effect renderer or caption engine. Render
existing governed camera/VFX work through the established REEL adapters, bind the
result as a `video` picture, and preserve its source receipt. For any compiled
beat/camera/effect/title/caption attachment owned by another layer, list an
`external_layers` item with `attachment_id`, `reason` and exact evidence FileRef.
The reason must say whether it is baked into a named clean motion input, delivered
as a separate track, or excluded from this particular audition. Primary picture
and audio attachments cannot be omitted this way.

**External-layer intake verifies evidence bytes, not that the effect/title/caption
was actually rendered.** The receipt lists these attachments explicitly; consumers
must validate those layers with the existing effect/animatic/caption checks and
verify their inclusion in the final conform. A clean-picture master must consume
clean assets; disclosures, review labels and captions belong in separate delivery
layers. Do not feed a flattened review overlay back as clean picture.

## Outputs and checks

- `picture.mkv`: FFV1 picture intermediate, yuv444p; no newly added labels.
- `D.wav`, `M.wav`, `E.wav`: full-scene stereo PCM24 roles, including explicit
  zero buses for intentional silence.
- `mix.wav`: explicit D+M+E sum, without automatic normalization or ducking.
- `master.mkv`: FFV1 plus PCM24, stream-copy binding of the intermediates.
- `review.mp4`: one H.264 CRF18 / 192 kb/s stereo AAC encode from the master.
- `receipt.json`: source/compiled identities, integer timing, FFmpeg version,
  exact output hashes/bytes, external-layer limitations and no publication grant.

Float buses and their sum are checked for overload before PCM quantization; a
clipping mix fails and must receive explicit gain/mix changes. This does not replace
`audio-check` loudness, true-peak, stem-margin or listening review. Decoded PCM
content length is verified separately from frame duration and AAC codec padding.
The `check` command rejects changed jobs, production/contract/media/evidence hashes,
tampered outputs, missing role files, wrong samples and wrong picture geometry.

FFV1 is lossless relative to the rendered pixels, not to an upstream lossy source.
Supply high-quality scene inputs. This is a scene tool: the consumer's episode
assembly must concatenate selected scene masters and encode the delivery once,
instead of concatenating and repeatedly transcoding review MP4s.

## Incremental production and adoption

Use existing `changed-only-plan`, `changed-only-result-receipt`, and
`changed-only-state-advance` commands. One node per scene includes the job,
contract, production manifest, every selected input and layer receipt; the recipe
also binds the exact REEL executable/build and FFmpeg version. The final episode
node depends on ordered scene outputs plus separately selected bookends. Changes
to a cue rebuild that scene and its dependent conform, not unrelated scenes.
The compiler and this renderer run from native cue data; marker evidence must be
refreshed for changed performances. A changed hash alone does not realign words.

Adopt in this order: synthetic canary; one real scene with identical selected
inputs; parity check and role review; one controlled native recast; selective
rebuild proof; then whole-episode migration. Report audio buses and external layers
that were intentionally absent from a prior master, and require explicit choices
before restoring them. Current-master selection remains a separate consumer record.

## Synthetic verification

```console
cargo test --test scene_delivery
cargo test --test scene_delivery real_delivery_preserves_sample_offset_stems_frames_and_rejects_tamper -- --ignored --exact
```

The generated local fixture uses color images, silent dialogue and a short signal.
It tests an off-millisecond 123-sample event, a native cue boundary at sample 48,001,
unchanged-composition detection, bus omission, exact D/M/E sum, native recast,
overwrite refusal and tamper rejection. No consumer media or literary text is used.
