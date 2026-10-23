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
custody and media retention remain separate producer operations. This does not
extend the current editor beyond one language and one full-recording cue.

Rust consumers can reuse `cel_source::project::Project` for explicit routing,
bounded raw snapshots, descriptor/version checks and candidate layout writing.
Native semantics are checked by the editor/shared reader, not by file routing.
The APIs grant no publication, selection, deletion or human approval authority.
