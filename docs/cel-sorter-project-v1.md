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

## Explicit candidate pictures

Descriptors v1–v3 may declare `"picture_candidates": ["new-picture"]`.
Each entry names an existing native `scene-bindings.json` asset key, authored
and registered by the producer as a primary picture input. This is a bounded
picture palette, not a file picker or another asset registry. At most 128 unique
nonempty keys (256 UTF-8 bytes each) are allowed. Undeclared bindings do not
become choices. Cue takes and alignment bindings cannot be picture candidates.
The native identity, byte count, cache URI and selected state must be valid;
the declaration grants no picture, rights or publication approval.

Export a fresh session after changing either bindings or this list. The raw
descriptor and binding locks protect these choices; editing an old request's
palette cannot extend its native scope. Older binaries reject the new field
rather than silently ignoring it. Omitting it preserves existing behavior.

Both media and controls-only sessions expose `picture_choices`. Media export
freshly verifies and copies these explicitly listed pictures in addition to
the effective cel pictures, within the same complete copy budget and tier order.
It does not scan for alternatives or fall through a corrupt earlier tier.
Controls-only export lists choices without resolving/copying any media.

The existing image selector can substitute a candidate while keeping stable
cel IDs, comments and cue clocks. Native apply uses the descriptor's allowlist,
appends a picture revision, and retains previous revisions and protected lanes.
The candidate descriptor retains the palette for explicit reopening. No media
is ingested, version selected, render started or cache object removed. The
producer must register the resulting primary dependency and manage its draft
lease before execution; this interface does not prove registry/custody coverage.
New cel identities still require new native events/alignments/graph controls
and a fresh session; listing a picture does not authorize event insertion.

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

## Scoped language intake

`reel.cel-sorter-project.v3` edits one explicitly named language in a complete
ready V2 scene. Keep the other lanes in the source; do not strip or convert them.
Use the five native core roles and explicit routes to every language's existing
native alignment index:

```json
{
  "schema": "reel.cel-sorter-project.v3",
  "root": "../episode",
  "language": "es",
  "files": {
    "scene.json": "authoring/scene.json",
    "scene-bindings.json": "bindings/pictures.json",
    "selected-graph.json": "assembly/graph.json",
    "selected-pointer.json": "assembly/current.json",
    "profile.json": "render/profile.json"
  },
  "alignment_paths": {
    "es": "timing/es/paths.json",
    "en": "timing/en/paths.json"
  }
}
```

Each native index still maps its own cue IDs to filenames relative to that
index's parent. Its key set must equal that language's cues. All scene languages
must have an explicit index; no directory crawling or implicit translation occurs.
The singular `alignment.json` and `alignment-paths.json` file roles are forbidden
in v3, avoiding two competing routes. Up to 16 language indexes, 128 total cues,
1000 edited-lane cels, the existing per-control limits and 32 MiB aggregate raw
control budget apply. Every index retains the 128 KiB limit. Duplicate language
keys, routes and aliases reject before candidate output.

Cue IDs may repeat in different languages. Captured roles are
`alignment-paths:<language>` for indexes and `alignment:` followed by the compact
JSON pair `[language,cue_id]` for alignments. These are unambiguous data keys,
never filesystem paths. Existing v2 single-language alignment roles stay intact.

Export uses session v4, including `language`; edit v4 binds that language, the
exact descriptor and all source control hashes, including protected languages.
Missing/wrong scope, changed protected controls and v1/v2/v3 request downgrades
reject. The interface names the edited language and keeps other lanes intact.
Legacy protocols and autosave keys remain supported.

Scoped edits retain shared defaults and use native local picture overrides.
Reordering changes only local event order. Removing a row narrows the shared
semantic event's language applicability when another language still uses it;
only exclusively removed events disappear. Picture slots used by another lane
cannot be retargeted implicitly. Such substitutions require explicit slot
separation; removal from the edited lane still retains a protected shared slot.

Before writing, the engine checks protected materialized lanes, graph events,
selected slot revisions, active closure and asset bindings. Exact captured bytes
are reused for unchanged controls, preserving alignment formatting and hashes.
Raw native scene/binding/graph/pointer/alignment buffers also pass their native
typed parsers before Value normalization can hide duplicate typed fields. The
new candidate reopens through the same full-scene validation; source files are
unchanged and existing output directories still refuse.

Rust callers use `Sequence::for_project` for scoped intake and the shared
`editor::load/apply` API for source checks and application. `Project` can preserve
captured bytes through `write_draft_from_snapshot`; changing a raw buffer without
its recorded hash/size refuses. None of these APIs grants publication authority.
The complete scene/graph hash changes with an edit, so old build receipts remain
bound to their original snapshots even when the other lane is semantically
unchanged. Scoped drafts do not select a new Golden or reclaim media.

## Separate language graph files

When each language already has its own selected native graph and pointer, keep
those files separate. Project v3 accepts an optional `protected_graphs` map:

```json
"protected_graphs": {
  "en": {
    "graph": "assembly/en-graph.json",
    "pointer": "assembly/en-current.json"
  }
}
```

The five core roles still name the edited language's graph and pointer. This
map must cover every other scene language exactly once, exclude the edited
language, and contain 1–15 entries. Without it, the existing combined-graph
contract applies. Partial mixtures of combined and separate graphs reject.
Each graph must contain exactly its language's native cels in one scene node;
each pointer must select that graph correctly. No graph fields are merged.

Graph/pointer routes obey the same bounded, root-relative path and alias rules
as other controls. Duplicate language keys, unknown route fields, wrong-domain
events, missing languages and stale protected-file hashes refuse. Slot names
belong to their graph: identical labels in two separate files are independent,
while references sharing a slot inside one graph retain the existing protection.

Captured roles are `protected-graph:<language>` and
`protected-pointer:<language>`. Their original raw bytes and every other lane's
alignment/index controls remain unchanged in the candidate. The emitted project
descriptor retains the explicit map. Session/edit v4 already locks the complete
descriptor and all controls; no alternate request grammar is introduced.
