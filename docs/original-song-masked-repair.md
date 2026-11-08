# Original-song masked materialization

The `plan`, `materialize` and `check` commands of `reel-song-repair` supply the deterministic materialization and recheck portion
of same-source lyric repair. It accepts already retained raw patch candidates;
it never executes a provider, installs a runtime, retries, selects or publishes.
It does not finish the managed provider operation or qualify a repaired verse.
The separate [original-source provider entry](original-song-repair-provider.md)
adds explicit bounded execution without changing these pure commands.

```text
reel-song-repair plan ADMISSION RECIPE
reel-song-repair materialize ADMISSION RECIPE CANDIDATES NEW_ABSOLUTE_DIRECTORY
reel-song-repair check ADMISSION RECIPE CANDIDATES DIRECTORY
```

Every call reopens the [original-source admission](mapped-original-song-source.md)
and its exact original package/version, request, generation evidence, canonical
text and recording. A serialized source context cannot substitute for those
inputs. Source read handles remain held during materialization and checking.
The modern generated-guide validator is unchanged.

The bounded `reel.original-song-repair.v1` recipe repeats those six source fields
(`source_version`, `original_package`, `original_request`, `original_generation`,
`canonical_text`, `recording`), the actual `sample_rate`, `channels` and
`source_frames`, and one to three ordered nonoverlapping `patches`.
Each patch has integer `start_frame`, exclusive `end_frame`, `fade_frames` and
one-based inclusive `source_lines: [first, last]`. Lines are the existing native
nonblank canonical line sequence; their original wording survives exactly.
An optional `context: {"start_frame": ..., "end_frame": ...}` declares the
conditioning interval in original audio sample frames. It must contain the
entire replacement mask and stay inside the original recording. Omission keeps
the existing five-second padding on each side, clipped to the source; retained
legacy recipes and their request geometry remain unchanged. An explicit context
can include the complete song when the supplied canonical lines require it.
Total conditioning PCM across patches remains bounded at 512 MiB; the separate
512-MiB raw-candidate file allowance is unchanged.
Each mask is at most 30 seconds. Fades stay inside the mask, last at most one
second per edge and leave a nonempty core. No insertion or resampling is admitted.

The `reel.original-song-repair-candidates.v1` roster hash/length-binds the recipe
and contains exactly one `patches` entry per recipe patch in the same order.
Each entry is a regular file `path` and exact `object` pin. Absolute named files
remain supported; ordinary relative paths resolve inside the roster's view.
Traversal, relative escapes and reparse leaves refuse. Relative rosters reopen
after a retained view moves without relying on a retired staging directory. These are
raw audio candidates for each declared context (or the default padded context).
Candidates must have that exact native length,
rate and channels, finite samples and an absolute peak at most 1.001. Total raw
candidate bytes are bounded at 512 MiB. No raw candidate is treated as proof of
its generation settings, actual attempt count or performed words.

Materialization copies the original interleaved float samples and replaces only
the declared mask. Linear edge gains span zero to one over `fade_frames`
positions (a one-frame fade preserves its sole edge sample); a zero-frame fade
copies the candidate directly. Original samples outside masks must retain every
bit, including signed zero. A separate traversal checks that invariant. Native
clock and sample count remain unchanged, and a float32 WAVE reopen checks every
written sample bit. This materializer requires float32 WAVE sources and raw
candidates, including supported extensible float containers. Integer sources
refuse rather than silently changing format. The original container, format,
metadata chunks and every byte outside the masked sample data are preserved.

Output consists of `recording.wav` and a private `repair-receipt.json` in a new
directory. The receipt retains source, recipe and raw-candidate pins, exact
geometry and supplied canonical words. It keeps provider execution, performed
words, mapping authority and human approval unverified, and listening, beat
alignment and acoustic continuity pending. `check` reconstructs expected samples
from the original inputs and raw candidates, then verifies exact output and
receipt bytes. Editing both the output and its claimed receipt pin cannot pass.
No file is overwritten or deleted on failure; partial outputs remain for recovery.

Use the shared bounded managed-job workflow for provider execution, immutable
runtime/tool inputs, one actual attempt per mask, retention of raw candidates,
independent acoustic/word diagnostics and owned rollback. This core does not
replace those gates. A byte-lock pass can coexist with a failed musical result.

Producer-applied review: reconstruction checks integer clocks and protected
bits; editing/sound requires listening to every changed boundary; lyric review
distinguishes supplied canonical lines from actual performance; provenance keeps
the original request and release/voice restrictions separate from new candidates.
