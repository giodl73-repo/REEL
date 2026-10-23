# Explicit cel project paths

The editor's `project` argument accepts an existing legacy project directory or
an explicit `reel.cel-sorter-project.v1` JSON descriptor. A directory continues
to require the original thirteen filenames. No descriptor is discovered or
installed automatically. Use the descriptor file when native controls already
live in different folders, or when optional compilation context is unnecessary.

```json
{
  "schema": "reel.cel-sorter-project.v1",
  "root": "../episode",
  "files": {
    "scene.json": "authoring/scene.json",
    "scene-bindings.json": "bindings/pictures.json",
    "alignment.json": "timing/recording.json",
    "selected-graph.json": "assembly/graph.json",
    "selected-pointer.json": "assembly/current.json",
    "profile.json": "render/profile.json"
  }
}
```

`root` is explicit, either absolute or relative to the descriptor's directory.
Each file path is relative to that resolved root. The six roles above are
required; these roles are also accepted as optional copied context:
`catalog.json`, `episode.json`, `policy.json`, `season-bindings.json`,
`episode-bindings.json`, `alignment-paths.json` and `compile.json`.
The role names identify existing native controls; they do not replace any
native scene, alignment, binding or graph grammar. Unknown roles/fields,
duplicate roles, missing core roles and aliased paths reject.

Paths use portable `/` separators. Absolute paths, traversal, device names,
alternate streams, reserved draft-metadata paths and reparse/symlink controls
reject. The descriptor is bounded to 64 KiB, paths to 512 UTF-8 bytes, and input
count to thirteen. Raw scene/bindings controls are bounded to 1 MiB each; other
controls to 2 MiB. Only declared files are read. No directory, asset, cache,
archive or transitive-reference discovery occurs.

Export a normal editor spec with `project` naming this descriptor. The session
uses `reel.cel-sorter-session.v2` and binds both the exact raw descriptor SHA-256
(`expected_project`) and per-role input hashes. Browser downloads use
`reel.cel-sorter-edit.v2`; apply it with:

```text
reel-cel-sorter apply project.json edit.json new-draft-directory
```

A changed descriptor rejects the old edit even when its controls still have
identical bytes. Layout/whitespace changes therefore require a fresh session.
This lock binds content and configuration, not physical custody: identical
portable copies can share the same descriptor identity. A version 1 edit cannot
downgrade this check. Legacy directory sessions/edits stay version 1, including
their existing browser autosave key; mapped projects use a distinct key.

The new draft preserves each supplied relative file location, adds its source
edit request and emits `cel-sorter-project.json` with `root: "."`. Reopen that
descriptor explicitly. It creates a new candidate directory, refuses existing
destinations and checks encoded output control limits before creating it.
Failed I/O can leave an incomplete candidate for explicit reconciliation.

Embedded native references, including absolute references, remain unchanged.
Preserving a valid relative layout avoids a converter, but does not certify a
self-contained compiler project. Only supplied context is copied. Cache/LFS
registration, full native compilation, selected-version advancement, physical
custody and media retention remain separate producer operations. The editor remains single-language; the explicit v2 descriptor below enables
multiple cues.

Rust consumers can reuse `cel_source::project::Project` for explicit routing,
bounded raw snapshots, descriptor/version checks and candidate layout writing.
Native semantics are checked by the editor/shared reader, not by file routing.
The APIs grant no publication, selection, deletion or human approval authority.

## Native multi-cue projects

Use `reel.cel-sorter-project.v2` with the same explicit root and file map.
Replace the singular `alignment.json` role with `alignment-paths.json` (required).
The index is the existing native cue-to-filename object, for example:

```json
{"first-cue":"first.json","second-cue":"second.json"}
```

These paths are relative to the index's parent directory, matching the native
alignment-path contract. No wildcard scanning or transitive discovery occurs.
The index has 1–128 unique cue IDs (at most 256 UTF-8 bytes each), is bounded to
128 KiB, and must exactly cover the native scene's cues. Index routes obey the
same portable-path rules; aliases against any static control or another cue
reject. Total captured controls are bounded to 32 MiB. The index is read once
when opening a `Project`, and its exact buffer is retained with those routes.
Reopen explicitly to observe index edits; this snapshot is no custody lease or
protection against a hostile filesystem namespace.

Per-cue alignment locks appear as `alignment:<cue_id>` data keys in
`expected_inputs`; these keys are never interpreted as paths. Each alignment's
raw SHA-256 and byte count must match the exact binding named by its cue.
Export emits `reel.cel-sorter-session.v3`; browser requests use
`reel.cel-sorter-edit.v3` and cannot downgrade to frame-based v1/v2 edits.
Rows contain `cue_id`, integer `samples` and `sample_rate` instead of `frames`.
Session `cues` carry each original sample budget, rate and concatenated preview
offset; `total_seconds` replaces the singular session sample clock.

Rust consumers can reuse `cel_source::sequence::Sequence` to validate native
cue clocks, obtain editor rows and plan cue-local edits without media I/O.
It uses the native measured-span compiler, preserves unrelated markers, and
rejects insertion, duplicate IDs, changed cue/rate, cue reordering, missing cues
and any per-cue timing drift. It validates semantic metadata; callers must also
check exact raw file locks as the CLI does. These checks do not constitute a
full project compiler/render, cache registration, visual review or approval.

Session `delivery_clock` contains `native_sample_rates`, the optional
`profile_sample_rate`, and a rate-only state: `not-declared`, `matches-profile`,
`profile-rate-mismatch` or `mixed-native-rates`. Mixed-rate editing does not
extend the delivery compiler's single-rate contract. A matching observation is
not a successful compiler/build/media check. The editor displays mismatch
notices without blocking draft authoring or changing source recordings.
