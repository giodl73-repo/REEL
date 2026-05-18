# Renderer paths research

## Decision supported

Select REEL's first practical renderer path for animated Games Design scenario
videos while keeping the production manifest renderer-neutral.

## Research question

Which renderer/tooling path should REEL adopt first for making animated movies in
multiple styles from scenario manifests?

## Local evidence

### REEL-R04-01: WSL2 Ubuntu is available

- Source: local command `wsl --status; wsl --list --verbose`.
- Observed: default distribution is Ubuntu, default version is WSL2, and the
  Ubuntu distro exists.
- Implication: REEL should run video tooling in Linux/WSL rather than fight
  Windows path, shell, and codec issues.
- Confidence: high.

### REEL-R04-02: REEL already separates format, style, and renderer

- Source: `README.md`, lines 67-92; `styles/README.md`, lines 1-31;
  `manifests/README.md`, lines 69-74.
- Observed: REEL requires format + style before rendering, and the manifest keeps
  renderer assumptions separate from required production fields.
- Implication: renderer adoption can start with one implementation path while
  preserving later Blender, browser, or AI-video paths.
- Confidence: high.

### REEL-R04-03: Games Design scenario truth remains upstream

- Source: `manifests/README.md`, lines 59-67;
  `docs/reviews/foundation-plan-review.md`, lines 12-23.
- Observed: source scenarios own canon; REEL adapts tone, order, and
  presentation without silently changing scenario facts.
- Implication: renderer tooling should consume REEL manifests, not reach into
  game repos and mutate scenario data.
- Confidence: high.

## External evidence

### REEL-R04-04: FFmpeg is the baseline media assembler and encoder

- Source: FFmpeg documentation, <https://ffmpeg.org/ffmpeg.html>.
- Observed: FFmpeg is described as a universal media converter that reads many
  inputs, filters/transcodes them, and writes many output formats. The docs show
  examples for format conversion, bitrate selection, and frame-rate control.
- Implication: FFmpeg is the right baseline for assembling stills/audio, creating
  animatics, transcoding outputs, and producing iPhone/social/YouTube exports.
- Confidence: high.

### REEL-R04-05: Remotion fits programmatic motion graphics and manifest inputs

- Source: Remotion `renderMedia()` documentation,
  <https://www.remotion.dev/docs/renderer/render-media>.
- Observed: `renderMedia()` renders video/audio programmatically; it accepts a
  composition with `width`, `height`, `fps`, and `durationInFrames`, supports
  `inputProps`, codec choice, output location, frame ranges, concurrency,
  metadata, audio codec, audio bitrate, and video bitrate.
- Implication: Remotion is a strong first prototype boundary for manifest-driven
  motion graphics, storyboard animatics, captions, social cuts, and simple game
  scenario animations.
- Confidence: high.

### REEL-R04-06: Blender is viable for headless 3D and previs, but heavier

- Source: Blender command-line render documentation,
  <https://docs.blender.org/manual/en/latest/advanced/command_line/render.html>.
- Observed: Blender can render from the command line without a graphical display,
  including full animations with `blender -b file.blend -a`; command argument
  order matters, and `-f` or `-a` should be last.
- Implication: Blender should be a deferred style-specific path for `3d-previs`
  and more spatial cinematics, not the first default renderer.
- Confidence: high.

## Recommendations

## Adopt now

1. Use WSL2 Ubuntu as the REEL video tooling environment.
   - Owner: REEL.
   - Validation: `wsl --status; wsl --list --verbose`.
   - Non-goal: do not build first renderer scripts around Windows-only shell
     behavior.
2. Use FFmpeg as the baseline assembler/encoder.
   - Owner: REEL.
   - Validation: future setup pulse should run `ffmpeg -version` inside WSL and
     create a short stills/audio smoke render.
   - Non-goal: FFmpeg does not replace shot/style authoring.

## Prototype behind a compatibility boundary

1. Use Remotion as the first programmatic animation prototype.
   - Owner: REEL.
   - Validation: future setup pulse should render a manifest-fed composition to
     MP4 in WSL.
   - Boundary: keep REEL manifests renderer-neutral; map manifest fields into
     Remotion `inputProps` in an adapter.
2. Keep browser capture as an implementation detail under the Remotion/browser
   adapter, not as a separate first-class renderer path yet.
   - Owner: REEL.
   - Validation: only after the Remotion prototype proves that a browser-based
     pass is insufficient or needs direct capture.

## Reject or defer

1. Defer Blender until `3d-previs` or spatial cinematics are required.
   - Owner: REEL.
   - Validation: later Blender pulse should render a tiny headless animation in
     WSL.
2. Defer cinematic AI provider integration until a provider-specific research
   pass compares continuity controls, licensing, cost, and automation.
   - Owner: REEL.
   - Validation: provider research with cited docs and a non-committed prototype.

## Next implementation pulse

Add a WSL setup guide and smoke-render target:

1. Install/check FFmpeg in Ubuntu.
2. Add a tiny renderer workspace or script that reads
   `manifests/templates/scenario-video.yaml`.
3. Produce a generated, gitignored smoke MP4 from still text/cards and audio or
   silence.
4. Keep generated media under `renders/` or `exports/`, which are already
   gitignored.
