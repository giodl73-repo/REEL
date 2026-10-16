# Source-bound semantic labels

The `semantic-label` editable text template displays a short label over a
narrated scene, for example a year at a time jump. Use the existing
`reel-scene-template compile` command and scoped source/asset bindings.

The template supplies `title_x`, `title_y`, `title_size`, font, color and a
bounded `fixed_duration_seconds` (1–30 seconds). The label is bottom anchored
at that position. The scene supplies one source-bound line whose `text`
equals its localized `title`, a native `cue_id` and `semantic_trigger_id`.
An optional `audio_cue_id` resolves a source cue in a combined performance.
These are the same line/marker fields used by poem invocations; a label has
no panel, stanza, chapter number, byline or post-poem title phase.

The compiler sums the ordered native cue clocks, then resolves the marker
in the selected language's take. No authored timestamp is accepted. ES/EN
may enter at different times. The display ends at the template duration or
scene end, whichever is earlier; it never extends or replaces the narration
clock. Missing markers, an out-of-scope cue, a marker outside the cue, mixed
sample rates and empty visible intervals fail compilation.

The resulting ASS is an editable presentation layer. Keep it separate from
clean reusable picture assets and bind its compile receipt and exact bytes
to the subsequent native scene build. Compilation is not proof that the
final movie consumed the layer. A chronology claim needs its own approved
source evidence; the compiler does not infer calendar years from prose.

Chapter-opening dates may simply be part of the approved localized chapter
title. Use a semantic label for a later time change without creating another
chapter entrance or separate runtime unit.
