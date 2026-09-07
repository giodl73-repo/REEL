# Cue-relative assembly contract v0.1

`reel cue-relative-compile` recompiles editorial timing after a native cue is
replaced. It is a strict sidecar to `reel.manifest.v0.2`, not a second media
manifest or renderer. Targets bind existing narration cue, shot, beat marker,
camera track, effect pass, and audio event IDs. Cel, caption, and editable title
IDs remain adapter-owned IDs attached to those production objects.

Anchors may reference a cue start/end, an explicit rational progress point, or
the start/end of a word or phrase marker, plus a signed sample offset. Spans can
start and end in different cues. `start_boundary` and `end_boundary` name shared
editorial joins; compilation fails unless every lane bearing that name resolves
to the same integer sample.

The compiler constructs the cue clock only from ordered native
`duration_samples`. It never time-scales audio and never proportionally remaps
old absolute timestamps. Word/phrase markers must be re-supplied by alignment
for a new performance. An explicitly authored `cue-progress` anchor is resolved
as a rational of the new cue duration with declared `floor`, `ceiling`, or
`nearest-half-up` rounding.

Output intervals are half-open. Starts use
`floor(sample * fps_numerator / (sample_rate * fps_denominator))`; ends use the
corresponding ceiling. Samples remain authoritative when two semantic boundaries
fall within the same video frame.

Example:

```console
cargo run -- cue-relative-compile tests/fixtures/cue-relative/recast.yaml \
  --output-path target/recast-conform.json
```

The paired `old.yaml` / `recast.yaml` fixture changes one actor's native cue
from 96,000 to 144,000 samples. Its cel span, aligned spill VFX and sonic,
caption, and following title all recompile from semantic anchors. The spill
marker moves by its new alignment rather than by the cue's overall duration
ratio.

## Adapter boundary

Render adapters consume `CompiledAttachment` and write the resulting integer
spans into existing REEL artifacts: shot/conform timing, exposure sheets,
camera tracks, effect-pass visibility, audio events, and caption/title sidecars.
The v0.1 compiler intentionally does not rewrite a production manifest or infer
word alignment. A BERTICA importer therefore needs only to translate its cue
timeline and forced-alignment words/phrases into `CueClock`, then map its stable
cel/title IDs to the matching REEL shot IDs.
