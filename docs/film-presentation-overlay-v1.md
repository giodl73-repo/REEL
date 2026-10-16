# Presentation overlays on an existing film

`reel-film-presentation-overlay` adds chapter-master layers to hash-bound scene
placements in an existing film. It copies the complete original audio stream.
This path is useful when a delivered film's sound cannot safely be decomposed or
cut into scene WAVs. It does not repair dialogue, remove old title cards, replace
pictures, or rebuild an episode's ordered presentation units.

```text
reel-film-presentation-overlay build manifest.json --root <asset-root> --output-dir <new-directory>
reel-film-presentation-overlay check manifest.json --root <asset-root> --output-dir <existing-directory>
```

## Inputs

The `reel.film-presentation-overlay.v1` manifest contains `language`, `film`,
`timeline`, and an `overlays` array. Each file reference has `path`, `sha256`, and
`bytes`. An overlay binds `presentation_scene`, `target_scene`, `template`,
`template_receipt`, `layer`, and `font`. Paths resolve relative to `--root`.

The timeline is `reel.source-film-timeline.v1`:

```json
{
  "schema": "reel.source-film-timeline.v1",
  "source_film_sha256": "<exact source film SHA-256>",
  "frame_count": 240,
  "fps_numerator": 24,
  "fps_denominator": 1,
  "units": [
    {
      "scene_id": "scene-two",
      "first_picture_slot_id": "picture-first",
      "start_frame": 24,
      "frame_count": 120
    }
  ]
}
```

Units may cover only the scenes receiving overlays. They must be ordered,
nonoverlapping, unique and inside the film. Scene placement is asserted source
evidence supplied by the producer; this tool does not identify story scenes from
pixels. The target scene's first language event and the presentation's
`overlay-on-target-scene-start` placement must agree with the timeline slot.

The chapter template must declare four seconds. Its editable-layer compile
receipt must bind the exact template definition, ASS bytes, presentation scene,
language and duration. Every ASS dialogue must cover local 0–4 seconds. The tool
derives the full-film time from the source frame PTS, including a nonzero origin;
the author does not type movie offsets into the layer. It rejects frame boundaries
that ASS centisecond precision cannot express. Ordinary narrative pacing policies
do not control these four-second presentation layers.

## Closure and limits

The source must have one video and one audio stream and a continuous picture
clock at the declared rate. FFmpeg re-encodes video with source timestamps and
copies audio. Build and check compare every decoded video frame PTS and every
audio packet's payload hash, PTS, DTS, duration, size and side data. They also
compare the audio codec, profile, sample rate, channel count/layout, decoder
extradata hash and stream side data. Timestamp values are compared as normalized
rationals. Audio timestamp overlaps in the original are preserved, not guessed
away. Both commands fully decode the result and reject a closure mismatch.

The MP4 movie timescale is the least common multiple of the normalized source
video and audio timestamp denominators. This prevents the default millisecond
edit-list grid from rounding a preserved video origin. A grid beyond FFmpeg's
signed 32-bit movie-timescale limit is rejected before rendering. Two unit
fixtures cover exact mixed timestamp grids and that range rejection.

Movie hashing streams through a fixed buffer. Complete ffprobe frame/packet
metadata remains in memory; memory use grows with film duration. Production runs
must demonstrate acceptable resource use on their real sources.

The output includes `review.mp4`, `receipt.json`, shifted editable ASS files,
pinned font copies, `fonts.conf`, and `render.log`. A Fontconfig configuration
restricts Fontconfig-based builds to supplied fonts. Other providers, including
Windows DirectWrite, may resolve host fonts; the actual provider is recorded.
Font bytes are pinned, but exact glyph-source selection is not certified.
`font_selection_verified` and `presentation_pixels_verified` remain false.
Production checks must inspect the actual title pixels and font treatment before
selection. A receipt is technical custody evidence, never human approval.

Five executed integration challenges cover actual nonzero source PTS, exact
AAC/decoded PCM preservation, visible first/last title frames, two independent
chapter placements, stale input/wrong slot rejection, invalid master timing,
receipt tampering, and audio timestamp changes despite refreshed output hashes.
