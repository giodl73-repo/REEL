# Rendered caption-layout evidence in CLI v0.2.10

`caption-layout` creates local visual evidence from a successfully rendered and
verified v0.2.9+ animatic artifact:

```powershell
reel caption-layout scene.artifacts.json --output-dir scene-caption-layout --output json
```

The command runs `animatic-check` before creating output. It then binds
`layout.json` to the artifact, verified video, caption lineage, and caption
presentation hashes. For every SRT cue it records the renderer-declared caption
region, any active speaker-badge region, font sizes, margins, colors/background,
frame containment, badge/caption intersection, and occupied screen percentage.

The packet also contains `first.png`, `middle.png`, `last.png`, and
`contact-sheet.png`. Frames are sampled at the midpoint of the first, middle,
and last caption cues and normalized into three deterministic 480x270 panels.
The report contains only cue indexes, timing, geometry, hashes, and relative
packet filenames; it does not copy caption text, speaker identity, or local
paths.

The packet is private review evidence by default because burned-in frames may
show manuscript captions or audience-facing labels. It is never added to the
privacy-safe animatic receipt. REEL reports declared presentation regions, not
OCR-derived glyph bounds, and makes no claim about translation accuracy,
phone/television legibility, human accessibility approval, or device behavior.

Output publication is atomic. An existing nonempty directory is refused, a
failed artifact verification publishes nothing, and all image hashes are
recorded in `layout.json`. Existing `reel.manifest.v0.2` files require no
migration.
