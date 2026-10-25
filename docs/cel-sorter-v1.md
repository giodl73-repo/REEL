# Native cel sorter

Explicit `reel-cel-sorter export spec.json` creates a private static editor for an
explicit native project. The spec names `project`, new `output`, `title`, ordered
`cache_roots`, a hash/byte/cache-URI `movie` asset used for complete-recording playback,
and `max_export_copy_bytes` (at most 1 GiB) for the complete selected media set.
Only declared selected images and the movie are resolved. Every file is freshly
hashed; a corrupt earlier tier fails. Media are independent working copies,
never writable hard links to canonical objects. No archive lookup or server starts.

For metadata editing without copying a compiled movie or primary media, invoke
`reel-cel-sorter export-controls spec.json` explicitly. Its spec accepts only
`project`, new `output` and optional `title`. It emits the same native scoped
session/source locks with `media_mode: "controls-only"`, empty images and no
movie. No cache configuration, media lookup or copy is performed. The UI labels
this mode, hides previews/playback, and keeps navigation on declared recording
clocks. Editing, comments, cue fitting and draft import/export remain available.
The normal media export remains separate; controls-only checks never certify
pictures, playback, rendered output or human visual acceptance.

`project` may name a [versioned file-map descriptor](cel-sorter-project-v1.md)
instead of the legacy directory. Six native controls are required; compilation
context is optional. This opens an existing layout without rearranging it and
preserves that relative layout in the new draft. Mapped single-cue sessions/edits use version 2; explicit multi-cue projects
use version 3. Both bind the exact descriptor. Legacy folder sessions stay version 1.

Declared identities and copy sizes are checked before any media reads or output
creation. Repeated picture bindings share one exported picture per hash, and
each unique hash is verified once during resolution. Each working copy is also
hashed while copying. The budget counts actual media destinations: a movie copy
and picture copy both count even if their bytes happen to have the same hash.
Verification/copy reads are capped to declared bytes plus one; a growing source
cannot bypass the budget. Failure may leave an incomplete new export directory,
never a successful session or automatic cleanup of that directory.

Serve the resulting folder through an explicitly requested local range-capable
server. Drag cards, use up/down buttons, edit holds, substitute an already-bound
image or remove a card. Playback uses the edited holds and the unchanged song.
Fit holds to the full song before saving. Browser drafts persist locally; Save
downloads a versioned `reel.cel-sorter-edit` request, including notes and exact input
hashes. This is an edit request, not an alternative scene or render grammar.

The interface is in English. Each cel has a comment box; comments travel with
the stable cel row through reordering, hold fitting, undo, browser autosave and
draft export/import. Older drafts without comments remain supported. Cel comments
and general notes are editorial feedback, preserved in the edit request; they do
not become poem text, subtitles, approval or native scene content.

`reel-cel-sorter apply native-project-or-descriptor edit.json new-draft-directory` checks every
input hash, stable ID, picture binding, and full frame/sample coverage, then
writes native scene, alignment, bindings, graph and pointer documents. Accepted
open content and unknown fields in other copied controls survive. The source
remains unchanged. Draft alignment bytes must enter
the project's cache before native compiler/build validation. Saving never selects
a version, renders, approves or publishes. No setup/daemon/server is installed.
Alignment rebinding uses the exact cue binding and the captured intake bytes.
Unreferenced bindings with the same old hash remain unchanged. New alignment
bytes receive `selected-private-production`, never inherited approval.

Scene and scoped-binding intake now uses the shared native cel source reader
(including its 1 MiB limit for each of those controls). Native source fields that
are unsupported reject; accepted presentation content and other copied controls
remain intact. This is stricter than the original schema-name-only intake.
Alignment is checked against its native language, cue and selected take identity.
Its scoped binding must match the exact alignment intake bytes. Native graph and
pointer validation also rejects a mismatched selection or effective picture.

Preview uses the native effective picture, including a language-local override.
A no-op draft preserves the shared default and local override separately. When
substituting a picture, an existing override stays local; events without an
override update the shared binding. The native draft, graph slot and graph event
all refer to the edited effective asset. This supports overrides within the
existing single-language scope; it does not silently enable bilingual editing.
Changed selected pictures use REEL's append-only revision operation, retaining
their earlier revisions. Timing-only edits leave those slot revisions intact.
Draft graph locks use the native canonical graph-lock routine and bind event
clocks as well as asset selections. A reused slot cannot select different
assets for two edited events; such an edit holds before creating a draft.

The bounded port supports one language, existing selected cels and ordinary
native picture slots in one scene node, with no unrelated graph events. Legacy
and project-v1 editing use one full recording cue and an integral frame/sample
clock. [Project v2](cel-sorter-project-v1.md) supports up to 128 native cues with
independent, sample-exact clocks, including mixed sample rates and off-frame cuts.
Native trigger IDs locate markers; shared semantic IDs identify editor rows.
Unrelated alignment markers survive. Reorder, fit and removal operate within
each cue; a cue cannot disappear or change its take, sample rate or duration.
Graph event seconds remain local to their cue; preview concatenates the cue
clocks in source order. The supplied movie must match that recording order.
Alternatively, `export` accepts `"media_mode": "native-recordings"` with no
`movie` field. It copies the exact selected take from each native cue binding
alongside the pictures, using the same explicit roots, fresh verification and
copy budget. Repeated takes share one hash-named WAV destination; a picture and
recording with the same hash still count as two destinations. No movie, audio
transcode, render, cache discovery or selection change is needed.

This mode previews browser-compatible WAV primary recordings in cue order.
The native audio controls show one cue; the sequence slider and cel navigation
seek across cues using their original sample clocks. Playback advances at cue
end, with a possible loading pause: this is an authoring preview, not gapless
delivery, mix, Sonic/VFX or final-render proof. Mixed sample rates remain local
to each cue. A decoder failure or duration mismatch is reported without altering
the source clock. Playback blocked by browser policy requires pressing Play.
Sessions retain the existing edit schema and source locks; `recordings` is
preview transport metadata and never becomes an input in an edit request.
Controls-only and compiled-movie exports remain available. Use a matching new
host binary for exports containing this frontend revision.
Unsupported clocks/languages/scopes fail explicitly. Explicit
[candidate pictures](cel-sorter-project-v1.md#explicit-candidate-pictures) allow
substitution using newly bound primary pictures. Registry admission, new cel
identities have an explicit [native insertion operation](cel-insertion-v1.md);
registry admission and native build validation remain producer operations.
Requests and browser downloads/imports are bounded to 2 MiB. The preview is locked stills;
it does not simulate rendered motion/VFX. Browser drafts with a temporary duration
mismatch are retained locally but need fitting before export. The editor is private project output; no
production pictures, titles or paths belong in REEL source or synthetic tests.

The same candidate-application core is available to Rust callers through
`cel_source::editor::{load, apply}`. The CLI uses these functions; producers can
validate and write native candidates without a subprocess or another authoring
grammar. `load` checks native semantics and exact raw alignment identities;
`apply` additionally checks the request's input/descriptor locks and writes a
new directory. These APIs resolve no media and grant no selection authority.

Each exported session now includes a rate-only `delivery_clock` observation.
The interface keeps a notice visible for mixed native rates or a profile-rate
mismatch while allowing draft editing. The delivery compiler currently requires
all cue rates to match its profile; editing preserves the originals and never
resamples them. `matches-profile` only describes rates, not build readiness.
Synthetic integration tests apply the native candidate and invoke the actual
`scene_delivery_compile::compile_to_dir`: same-rate off-frame edits compile,
and mixed rates fail before creating delivery output. Real media, rendering,
private compiler acceptance and visual review remain separate producer checks.

Explicit project v3 now opens complete bilingual/multilingual native scenes for
one scoped language edit; session/edit v4 binds that choice. Other lane controls,
alignment bytes, selected graph records and referenced slots remain protected.
See [scoped project intake](cel-sorter-project-v1.md#scoped-language-intake).
This extends the earlier single-language-source limit; simultaneous linked
editing and automatic slot forks remain outside this editor. A synthetic
scoped edit compiles both languages through their separate native indexes.
Private episode/browser/render acceptance and runtime adoption remain separate.

Project v3 also supports explicit separate per-language graph/pointer files via
[`protected_graphs`](cel-sorter-project-v1.md#separate-language-graph-files).
Each graph keeps its native domain; protected files are copied byte for byte.
No source-language stripping or graph merging is required.

For metadata-only intake and a locked identity edit template, use:

```text
reel-cel-sorter inspect project.json new-private-report.json
```

The create-new report uses `reel.cel-sorter-inspection.v1` and contains
`edit_request`, cue `clock` metadata and `media_resolved: false`. It is bounded
to 2 MiB and contains private identifiers and input hashes: keep it with project
evidence, outside public tooling source. Standard output contains only counts
and scope flags. This command reads declared native controls, resolves no media
and starts no server. It neither exports a browser session nor certifies a
compiler, render, selection or approval. Existing reports are never overwritten.
