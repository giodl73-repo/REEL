# Native note workshop

The first score-authoring slice edits pitch and tick timing for existing notes in
`reel.music-model.v0.1`. It uses that native model directly, with a small locked
edit-request transport like the cel editor. It introduces no song/scene wrapper
or replacement notation format. Lyrics, underlay, tempo, meter, form, harmony,
instrumentation, velocity, voice assignment and stable note IDs stay fixed.

```text
cargo build -p reel-music --bin reel-music-editor --locked
reel-music-editor export model.yaml new-editor-directory
reel-music-editor apply model.yaml music-note-edit.json new-proposal.json
```

The output model must be a new file beside the source model. This preserves its
native relative source/analysis/lyric references without copying or rewriting
dependencies. `new-proposal.json` is ordinary native model JSON, also readable by
the existing YAML-compatible model loader. Existing outputs refuse. Candidate
files publish from an exclusive temporary file; source bytes remain
unchanged. The export directory is create-new; an I/O failure may leave an
incomplete export for explicit reconciliation.

Export reads only the named model, checks its typed grammar and native note
layout, and writes `index.html`, `editor.js` and `session.json`. It reads no source
recording, analysis manifest, lyric file, cache or archive and starts no service.
Open through an explicitly requested local static server. No server is included
or automatically launched. Browser visual acceptance remains separate.

The interface shows a declared-note pitch/timing view, part selection and up to
100 editable note rows per page. Each changed note needs a reason. Undo, reset,
model-bound browser autosave and draft import/export keep that reason with the
note. Only valid drafts replace the last browser autosave. Fix invalid values
before saving or reloading. The editor does not play audio or engrave a score.
Its labels use literal text, including imported names and note IDs.

## Locks and bounded transformations

`reel.music-note-session.v1` supplies an identity request using
`reel.music-note-edit.v1`. A request contains only:

- the exact original `expected_model_sha256` and a new `candidate_model_id`;
- every original note exactly once, identified by `part_id` and `note_id`;
- integer `midi_note`, `start_tick`, `duration_ticks`, and a `rationale` string.

Unknown fields, missing/duplicate/foreign notes and changed source bytes refuse.
Existing source order is retained; timing cannot cross another note in native
canonical order. Same-voice overlaps refuse rather than being quantized or
silently repaired. The candidate reuses the native note-layout checker.

Input, request, session and model output are each bounded to 2 MiB. The editor
supports at most 64 parts and 1000 notes, MIDI voices 1–16, pitches 0–127, positive
durations and clocks no larger than JavaScript's safe integer limit. IDs are at
most 256 UTF-8 bytes; reasons at most 4096. Direct input reparse/symlink files
refuse. File/hash checks are optimistic content locks, not a filesystem lease.

Changed notes retain their existing declared observation references, use
`inferred` provenance, discard an old note correction decision and explicitly
name the unreviewed proposal in their rationale. An edit with no existing
observation references refuses. The tool cannot create or verify evidence.
Unchanged notes retain their original provenance. New candidates reset model
authority/review status to `not-reviewed` and clear their decision references;
the original authority identity and required roles remain declarations, not new
candidate approval. Lyric-layer authority and exact text hashes stay unchanged.

## Native validation and adoption

Editor intake/application deliberately reports `native_validation: "not-run"`
and `media_resolved: false`. It is not complete source, analysis, lyric, hook,
expressive-timing or authority validation. The existing explicit
`music-model-validate` and `music-score-export-plan/render/check` operations must
validate the proposed native model and its actual dependencies before adoption.
They may read source media; they are not invoked by the note editor.

The synthetic integration fixture applies a pitch/release change, validates the
candidate through the full native model validator, then produces and checks the
existing MIDI/MusicXML/rehearsal-guide/SVG packet. All original fixture controls,
lyrics and tiny synthetic PCM bytes stay unchanged. That proves a bounded
structural round trip, not musical correctness, listening, readable engraving,
private-project compatibility, creative approval or selection.

Sessions, requests and model candidates contain private identities/hashes and
reasons. Keep them as private project assets when retained; public REEL fixtures
are synthetic only. Retain the edit request with the exact original model for
lineage; project asset/version registration and selection remain separate. The
CLI success summary contains only counts and validation-state flags. It performs
no upload, rendering, runtime installation, cache registration or reclamation.

Next slices: explicit source-bound lyric-underlay editing, phrase/arrangement
decisions and musical/engraving review. Browser and private-song acceptance are
still open; this source increment does not declare runtime adoption.
