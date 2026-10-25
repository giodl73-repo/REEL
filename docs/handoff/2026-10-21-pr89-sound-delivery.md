# Native audio and delivery recovery

Third bounded port from PR #89: explicit mono-to-stereo duplication propagated
through authoring and compilation, independently measurable scene audio mixing,
source-bound Sonic placement/channel intent, signed PCM range checks, optional
lossless H264/FLAC episode masters, semantic conformation endpoints and terminal
effect-frame preservation. Source commits: `ced41d3`, `494cd1c`, `0f75096`,
`271cec2`, `0127476`, `32b90c3`, `395aa1d`, `7d1cd0f`.

Preserves newer main's VFX render-mode bindings, language-specific effect
overrides, global picture-frame partition, compositor lossless encoding and
adjacent-overlay end-frame regression. Optional audio mapping is explicit;
the engine does not silently reinterpret a selected source channel layout.
Episode encoding selection leaves complete decoded picture/audio verification,
timestamp checks and review failures in force.

Producer-applied sound/editor/animation/provenance review: technical channel
mapping is not a new performance selection. An explicitly bound offset needs
its own source evidence. Correct decoded content does not waive a failed
editorial boundary finding. Tests use synthetic sources; the historical full
bilingual episode and normal-speed review gates remain project work.

Together with the first two ports this recovers the unique generic code from
PR #89. Its older separate composition-encoding option is superseded by main's
unified existing API. Historical notes and the private-poem regression remain
preserved at `archive/pr-89-motioncraft-phase-2` without being engine defaults.
