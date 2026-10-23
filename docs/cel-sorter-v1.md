# Native cel sorter

Explicit `reel-cel-sorter export spec.json` creates a private static editor for an
explicit native project. The spec names `project`, new `output`, `title`, ordered
`cache_roots`, a hash/byte/cache-URI `movie` asset used for complete-song playback,
and `max_export_copy_bytes` (at most 1 GiB) for the complete selected media set.
Only declared selected images and the movie are resolved. Every file is freshly
hashed; a corrupt earlier tier fails. Media are independent working copies,
never writable hard links to canonical objects. No archive lookup or server starts.

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
downloads one `reel.cel-sorter-edit.v1` request, including notes and exact input
hashes. This is an edit request, not an alternative scene or render grammar.

The interface is in English. Each cel has a comment box; comments travel with
the stable cel row through reordering, hold fitting, undo, browser autosave and
draft export/import. Older drafts without comments remain supported. Cel comments
and general notes are editorial feedback, preserved in the edit request; they do
not become poem text, subtitles, approval or native scene content.

`reel-cel-sorter apply native-project edit.json new-draft-directory` checks every
input hash, stable ID, picture binding, and full frame/sample coverage, then
writes native scene, alignment, bindings, graph and pointer documents. Unknown
fields survive. The source remains unchanged. Draft alignment bytes must enter
the project's cache before native compiler/build validation. Saving never selects
a version, renders, approves or publishes. No setup/daemon/server is installed.
Alignment rebinding uses the original intake hash rather than reopening its
source path midway through draft application.

The first bounded port supports one language and one full recording cue, an
integral frame/sample clock, existing selected cels and ordinary native picture
slots in one scene node, with no unrelated graph events or alignment markers.
Unsupported clocks/languages/scopes fail explicitly. Insertion of new assets and
native build validation remain producer operations. The preview is locked stills;
it does not simulate rendered motion/VFX. Browser drafts with a temporary duration
mismatch are retained locally but need fitting before export. The editor is private project output; no
production pictures, titles or paths belong in REEL source or synthetic tests.
