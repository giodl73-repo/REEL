# Deterministic browser compositions v0.3.22

REEL can render a local HTML, SVG, CSS, and JavaScript composition through a
Chromium-compatible browser while keeping one DOM alive for the complete
sequence. This is intended for score highlighting, lyric state, diagrams, and
other visuals whose browser layout must remain stable between frames.

## Manifest

```yaml
schema: reel.browser-composition.v0.1
source:
  path: score.html
  sha256: <64 lowercase hex characters>
assets:
  - path: score.css
    sha256: <sha256>
  - path: score.js
    sha256: <sha256>
width: 1920
height: 1080
fps: 50
frame_count: 304
dynamic_regions:
  - { x: 100, y: 140, width: 1720, height: 720 }
audio:
  path: master.wav
  sha256: <sha256>
```

Every path is relative to `--asset-root`, must not contain `..`, and is
hash-verified before use. The source and declared assets are copied to an
isolated staging root. Text assets containing remote HTTP(S), absolute file
URLs, or HTML base URLs are rejected. Chromium also blocks HTTP(S) and
WebSocket requests, and every frame verifies that the document has not
navigated away from the staged entry point.

## Frame API

REEL installs the clock before page scripts execute. For each requested frame,
it calls `window.__reelSetFrame(index)`, updates `data-reel-frame` and
`data-reel-time` on the document element, and dispatches:

```js
window.addEventListener("reelframe", ({ detail }) => {
  // detail.frame, detail.time, detail.fps
  document.querySelector(".active")?.classList.remove("active");
  document.querySelector(`[data-frame='${detail.frame}']`)?.classList.add("active");
});
```

`Date.now()` and `requestAnimationFrame()` observe the frame clock rather than
wall time. Compositions should use the `reelframe` event as their authoritative
state transition and avoid nondeterministic APIs.

If `dynamic_regions` are declared, REEL masks those rectangles and requires all
remaining RGBA pixels to have one stable hash. This catches unintended
background or score re-rasterization while allowing the declared highlights to
change.

## Render and verify

```powershell
reel browser-composition-render composition.yaml `
  --asset-root package `
  --browser "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe" `
  --clean-picture --output score.mp4 --format json

reel browser-composition-check score.browser-artifacts.json `
  --manifest composition.yaml --asset-root package --video score.mp4 `
  --output json
```

The report binds the manifest, source, assets, optional master audio, Chromium
product version, dimensions, FPS, exact frame count, every captured PNG hash,
index-bound sequence hashes, stable-background hashes, encoded output hash,
duration, and audio-stream count. Verification rejects changed inputs, reordered
or missing frame lineage, non-CFR output, an encoded frame-count mismatch, and
any duration drift.

`--clean-picture` sets `data-reel-clean-picture=true` and hides elements marked
`.reel-disclosure` or `data-reel-disclosure`. It does not imply rights, review,
creative selection, or publication approval.

## Limitations

- Chromium and installed font versions can change raster bytes. Pin the browser
  build in production evidence and package web fonts as declared local assets.
- The composition is trusted local code. Static reference checks and browser
  network blocking are defense in depth, not a sandbox for hostile JavaScript.
- CSS transitions, timers, random values, media playback, WebGL, and other
  autonomous browser behavior should not drive production state. Use the
  `reelframe` event.
- The first implementation captures PNG frames before FFmpeg encoding; long or
  high-resolution sequences require corresponding temporary disk capacity.
