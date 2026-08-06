# Render environment diagnostics in CLI v0.2.4

REEL CLI v0.2.4 turns the hosted-CI FFmpeg probes into a consumer-facing gate.
It does not change `reel.manifest.v0.2`.

Run the doctor before a production rerender:

```text
reel render-doctor
reel render-doctor --output json
```

On Windows, REEL inspects the same FFmpeg and ffprobe reached through its WSL
adapter. On Linux, it inspects the native executables. The report records both
version lines, the transport, and individual checks for:

- `drawtext`, `subtitles`, `perspective`, `framerate`, and `xfade` filters;
- the `libx264` encoder;
- cubic interpolation support in the perspective filter.

The JSON schema is `reel.render-environment.v0.1`. A missing executable returns
the underlying invocation error. A missing capability prints the complete
report, exits unsuccessfully, and names every failed check. Real
`animatic-render` operations enforce the same capability set before FFmpeg
receives the render graph, so an incompatible build cannot create a partial
video or artifact report. Explicit legacy renders do not require the smooth-only
perspective checks, preserving deterministic reproduction on older builds.

For BERTICA, retain the JSON doctor report beside the first v0.2.4 rerender as
environment evidence. Existing manifests, timings, captions, audio choices,
private paths, and approval states do not need migration or modification.

CLI v0.2.5 additionally fingerprints this report and embeds the same evidence
inside every real animatic artifact report. See `render-lineage-v0.2.5.md`.
