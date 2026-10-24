# Exact-text underlay review

Base: main `24a53eb`. Producer-applied Lyrics/Vocal Adaptation, Music
Reconstruction, Score/Arrangement, Editor and Rights/Provenance checklists;
no independent review or human musical/text approval is implied.

| Role / finding | Resolution and evidence |
| --- | --- |
| Lyrics/Vocal: text identity and underlay authority must remain distinct | The caller explicitly names the native selected lyric file. Its path and raw hash must match the model; requests lock the model, layer ID and lyric hash. Text stays read-only, including spelling, accents and original line endings. Candidate lyric-layer authorities are unchanged; model review resets and adds the lyric/vocal role requirement. Reasons remain in a private request, not in source lyrics. |
| Reconstruction: browser indices are not native byte offsets | Selection mapping preserves original UTF-8 offsets through normalized textarea line endings and surrogate pairs. Native application rejects partial characters, empty/overlapping text, wrong identities and incomplete note coverage. Synthetic fixtures cover accents, emoji, BOM, CRLF and stale files. |
| Score/Arrangement: source order must hold inside a melisma too | Shared native underlay validation now checks every note, including those within the first group. A regression rejects a reversed first melisma through the pure validator and the full native model validator. Correct phrase grouping round-trips through native score exports; notes and their provenance remain unchanged. |
| Editor: repair must be possible without manufacturing a completed source | Typed, bounded incomplete underlay can open as an invalid working draft. Application/save require complete valid coverage and a reason for changes. Add/remove/reorder groups, native note ranges, undo/reset, scoped autosave and draft import/export stay explicit. One shared note list and 100-span pages bound UI controls. |
| Provenance: a technical pass cannot authorize a performance or publish text | The new candidate remains unreviewed and retains the exact original source and lyric declarations. CLI summaries contain counts/state only, with lyric-byte verification separated from full project/authority validation. No recordings, analysis sidecars or other lyric layers are resolved by this workflow. |
| Maintainability: two editors must share source publication and native semantics | Both workflows reuse bounded model I/O, exact-source checks and create-new candidate publication. The underlay editor calls the native validation helper; it introduces no alternate model, lyric or notation grammar. Existing note session/edit v1 is unchanged and covered by its prior regression suite. |

Actual fixture paths: `crates/reel-music/tests/underlay_editor.rs`,
`crates/reel-music/tests/underlay_editor_ui.test.cjs` and the root
`tests/music_note_editor_roundtrip.rs`. The synthetic native exporter check
preserves all original controls, lyric and tiny PCM bytes. Full private reports
and any retained requests/models are project assets, not public source material.

Open adoption findings remain with the producing project/operator: a frozen
private native-model fixture and source custody, actual browser selection/layout,
musical/lyric/engraving review, retained version registration and explicit runtime
adoption. The bounded registered song-tool test report describes a private fixture
but has no structured native-model reference; it does not establish this editor's
private adoption. Prior browser inventory and in-app/Chrome entry points were
unavailable. No server, renderer job, cache movement, producer edit, runtime
retargeting or Golden selection follows from this review.
