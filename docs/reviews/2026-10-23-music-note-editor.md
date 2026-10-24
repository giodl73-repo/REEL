# Native note editor review

Base: main `17f85c7`. Producer-applied Music Reconstruction, Score/Arrangement,
Lyrics/Vocal Adaptation, Sound Design, Editor and Rights/Provenance checklists.
This is one implementation review, not independent or human musical approval.

| Role / finding | Resolution and evidence |
| --- | --- |
| Reconstruction: note proposals need stable source identity and native clocks | Requests bind exact original model bytes and every part/note identity. Rust shares the native note-layout validator; integer clocks, source order and same-voice overlap checks hold before output. Stale/foreign/duplicate/missing edits refuse. The source model remains unchanged. |
| Score/Arrangement: another score grammar or arbitrary model replacement would hide changes | The request carries only pitch/start/duration/reason for existing notes and a new model ID. Candidate output is the existing native model schema. Parts, instrumentation, velocity, tempo/meter, form, harmony, hooks and expressive declarations stay fixed. Full native validation remains an explicit next operation and can reject a proposal. |
| Lyrics/Vocal: pitch/timing editing must not silently author new words or syllabification | Lyric files are not read or modified. Existing layer identities, byte hashes, authorities and underlay remain unchanged. This slice does not claim underlay editing, pronunciation, prosody or listening acceptance. |
| Sound/Editor: a diagram or guide is not a recording or reviewed performance | The UI displays declared note pitches/ticks; it plays no audio and performs no transcription. The native integration fixture checks a synthetic MIDI/MusicXML/SVG/guide packet after proposal application. Musical identity, sound and notation readability still need actual review. |
| Provenance: changing an approved note must not manufacture a human correction | Changed notes become explicitly unreviewed inferences with retained existing declared observations; old note correction references are cleared. Missing observation references refuse. Candidate authority/review statuses reset and old decision refs clear. Untouched notes retain their native provenance. No authority is inferred from UI use. |
| Privacy: editing should not trigger dependency hydration or external transfer | Intake/export/application read only the explicit bounded model/request; they resolve no dependencies, media, cache or archive and start no service. Success summaries contain counts/state flags. Private sessions/requests/candidates remain project assets when retained. Public fixtures are synthetic. |
| Operator: drafts need recoverable edits and bounded controls | Static handlers implement model-bound autosave, import/export, undo/reset, per-part views and 100-row pages. Only valid drafts replace autosave. Literal text labels and a restrictive CSP avoid interpreting imported names as markup. Candidate output uses create-new atomic file publication beside the original, preserving native relative references. |

Evidence is exercised by `crates/reel-music/tests/note_editor.rs`,
`crates/reel-music/tests/note_editor_ui.test.cjs` and the root
`tests/music_note_editor_roundtrip.rs`. The handler tests execute exported script
logic against a synthetic DOM; they are not browser visual acceptance. The root
fixture copies five explicit tiny synthetic source/control files, then invokes
the existing full native validator and score packet renderer/checker. It checks
that every original byte remains unchanged.

Open advisory findings, owned by the adopting project/operator: browser layout,
private-song/model compatibility, dependency custody, musical/engraving review,
runtime adoption and retained asset/version registration. The browser inventory
was empty and the supported in-app entry point returned unavailable. No server
was started to work around it. The existing private song-tool record establishes
tool-source/runtime evidence, not an available native song model fixture.

This increment does not close the cel editor's browser/render gate, add lyric
underlay or arrangement authoring, adopt a runtime, select a Golden, update a
producer worktree or move/remove cache objects. No critical/major finding is
waived by a structural pass; unsupported scopes remain explicit follow-up work.
