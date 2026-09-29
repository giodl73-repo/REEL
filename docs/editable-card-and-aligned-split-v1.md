# Editable card and aligned performance tools

`reel-editable-card` renders a cue-free chapter card from a selected `reel.editable-text-template.v1`, exact source text, and a `reel-scene-template` compiled ASS layer. The render manifest supplies identity, exact file hashes/bytes, a background color, 24 fps and 44.1 kHz. The template owns typography, fades, geometry and fixed duration. The tool creates a lossless FFV1/PCM24 `master.mkv`, a browser `review.mp4`, and a `reel.presentation-master-receipt.v1` with exact hashes and clocks. `check` verifies the input closure, output hashes, 96-frame/four-second geometry for a four-second template, 176,400 stereo samples, and full decodes. Reusing a clean chapter template needs no fabricated narration cue or episode-specific renderer.

```text
reel-editable-card build <manifest.json> --root <authoring-root> --output-dir <new-dir>
reel-editable-card check <manifest.json> --root <authoring-root> --output-dir <rendered-dir>
```

`reel-aligned-split` preserves cue identities when a single continuous PCM16 mono 44.1 kHz performance is preferable to separately generated lines. Its manifest pins one source WAV, the provider's exact character alignment response, the source text, ordered cue IDs and optional bounded per-cue gain. It verifies that the aligned characters reconstruct the exact text, derives split samples solely from newline end markers, emits contiguous cue WAVs and records source sample ranges and hashes. With zero gains, concatenating the output PCM must reproduce the source PCM byte-for-byte. Gains that would clip are rejected.

```text
reel-aligned-split build <manifest.json> --root <authoring-root> --output-dir <new-dir>
```

Both tools produce technical media and receipts. They do not approve a performance, title, likeness, score, or publication.

## Title over the next scene's first picture

For a chapter title that belongs on the opening cel rather than on a separate card, the presentation scene declares:

```json
"placement": {
  "mode": "overlay-on-target-scene-start",
  "target_scene_id": "next-scene-id",
  "target_picture_slot_id": "next-scene-first-picture-slot"
}
```

`reel-scene-title-overlay` consumes the presentation scene, target scene, selected template, compiled ASS layer and rendered target scene through exact hash/byte references. Its manifest supplies the language but no layout coordinates or timing. The template owns four seconds, placement and fades. The tool rejects a target slot that is not the first picture event in that language, then renders a new scene review with the title over that picture. Its checker verifies full decode, identical video frame count and bit-identical copied audio packets. This lets the episode use the resulting scene as one input without inserting a separate card or changing narration clocks.

```text
reel-scene-title-overlay build <manifest.json> --root <authoring-root> --output-dir <new-dir>
reel-scene-title-overlay check <manifest.json> --root <authoring-root> --output-dir <rendered-dir>
```
