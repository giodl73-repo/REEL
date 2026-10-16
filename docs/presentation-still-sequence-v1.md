# Editable still presentation sequence

The template owns a square-pixel output canvas. Each source image is fitted
using its displayed aspect ratio (including any source pixel aspect), rounded
to integer dimensions, then padded to the canvas. The renderer assigns SAR1
to this newly fitted canvas; it never patches an existing master. Portrait
images therefore retain their shape without leaking resize compensation into
the finished video's pixel aspect. Native picture and audio clocks are unchanged.

`reel-presentation-still-sequence` builds a bounded, language-local still sequence from exact hash-bound pictures and a separate ASS layer. A reusable `reel.presentation-still-template.v1` sets width, height, frame rate, audio rate, number of pictures and frames per picture. The render manifest supplies only selected picture references, the editable layer and optional audio. Each reference includes a root-relative path, SHA-256, byte count and matching `cache://sha256/` URI.

```text
reel-presentation-still-sequence build manifest.json --root <input-root> --output-dir <new-dir>
reel-presentation-still-sequence check manifest.json --root <input-root> --output-dir <rendered-dir>
```

The builder writes `master.mkv` as FFV1/yuv444p with stereo PCM24, a browser `review.mp4`, a copy of the editable ASS layer and a hash-bound receipt. It pads short optional audio to the exact template duration; without audio it uses digital silence. Build and check verify exact frames, audio sample count and complete decodes. This is a technical presentation renderer, not an asset selection, creative approval or publication decision. `reel-presentation-adopt` can bind a selected output as an episode presentation master after its scoped selection and hydration checks.

The synthetic regression test uses two stills and checks 48 frames, 96,000 stereo samples, complete output closure and layer-tamper rejection.

## Variable cuts on a continuous source clock (V2)

Use `reel.presentation-still-template.v2` with `total_frames` instead of
`frames_per_picture`. A `reel.presentation-still-sequence.v2` manifest supplies
the same ordered `pictures` and a hash-bound `picture_timing` file. Audio is
required for V2. Picture timing can be shared by language manifests while their
editable ASS layers differ.

The `reel.presentation-picture-timing.v1` file supplies a hash-bound `clock`
reference and ordered `spans`, one per picture:

```json
{"schema":"reel.presentation-picture-timing.v1","clock":{"path":"clock.json","sha256":"<hash>","bytes":123,"cache_uri":"cache://sha256/<hash>"},"spans":[{"start_anchor":"intro","end_anchor":"P001"},{"start_anchor":"P001","end_anchor":"outro"}]}
```

The `reel.presentation-source-clock.v1` file declares `source_sha256`, native
`sample_rate`, `total_samples`, a hash-bound `evidence` file and `anchors` with
unique `id` and integer `sample`. Anchors may identify performed words, phrase
occurrences or measured musical boundaries. The producer owns their meaning and
review status; hashing evidence does not establish musical alignment or human
approval. Do not transfer clocks from a different recording merely because the
song title matches.

Spans must close the complete source clock, beginning at sample zero, sharing
adjacent boundaries and ending at `total_samples`. The compiler rounds each
shared boundary once to the nearest picture frame. A collapsed span, missing or
duplicate anchor, gap, wrong audio hash or mismatched native duration/rate/stereo
channels fails before an output directory is created. The source is consumed
once continuously; each picture's exact frame count is retained in the receipt.
All timing/evidence references are reverified by `check`.

V1 uniform sequences remain supported. V2 preserves the existing bounded
120-second/120-picture policy. PCM24 is the delivery normalization; a float32
source remains retained unchanged as an input and is not claimed byte-identical
to PCM24. Optional explicit gain/fades still apply when selected; omit
`audio_treatment` to preserve the normalized source samples. The FFmpeg V2
fixture checks an actual uneven picture cut and complete PCM24 source/output
equality, plus transitive evidence tamper rejection. Selection and
`reel-presentation-adopt` remain required for episode conform consumption.
