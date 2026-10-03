# Editable still presentation sequence

`reel-presentation-still-sequence` builds a bounded, language-local still sequence from exact hash-bound pictures and a separate ASS layer. A reusable `reel.presentation-still-template.v1` sets width, height, frame rate, audio rate, number of pictures and frames per picture. The render manifest supplies only selected picture references, the editable layer and optional audio. Each reference includes a root-relative path, SHA-256, byte count and matching `cache://sha256/` URI.

```text
reel-presentation-still-sequence build manifest.json --root <input-root> --output-dir <new-dir>
reel-presentation-still-sequence check manifest.json --root <input-root> --output-dir <rendered-dir>
```

The builder writes `master.mkv` as FFV1/yuv444p with stereo PCM24, a browser `review.mp4`, a copy of the editable ASS layer and a hash-bound receipt. It pads short optional audio to the exact template duration; without audio it uses digital silence. Build and check verify exact frames, audio sample count and complete decodes. This is a technical presentation renderer, not an asset selection, creative approval or publication decision. `reel-presentation-adopt` can bind a selected output as an episode presentation master after its scoped selection and hydration checks.

The synthetic regression test uses two stills and checks 48 frames, 96,000 stereo samples, complete output closure and layer-tamper rejection.
