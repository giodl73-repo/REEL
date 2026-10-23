# Native cel sorter

Explicit `reel-cel-sorter export spec.json` creates a private static editor for an
explicit native project. The spec names `project`, new `output`, `title`, ordered
`cache_roots`, a hash/byte/cache-URI `movie` asset used for complete-recording playback,
and `max_export_copy_bytes` (at most 1 GiB) for the complete selected media set.
Only declared selected images and the movie are resolved. Every file is freshly
hashed; a corrupt earlier tier fails. Media are independent working copies,
never writable hard links to canonical objects. No archive lookup or server starts.

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
Unsupported clocks/languages/scopes fail explicitly. Insertion of new assets and
native build validation remain producer operations. Requests and browser downloads/imports are bounded to 2 MiB. The preview is locked stills;
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
