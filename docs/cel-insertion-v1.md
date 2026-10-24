# Author one new native cel

```text
reel-cel-sorter insert-cel <project-or-descriptor> <insert.json> <new-source-directory>
```

This explicit producer operation splits one existing cel hold into two. The
original cel retains the prefix; the new cel takes the rest of that hold.
Recording identity, full cue duration and other cel clocks remain fixed.
The caller supplies fresh IDs, a split position and native event intent:

```json
{
  "schema": "reel.cel-sorter-insert.v1",
  "scene_id": "example-scene",
  "language": "es",
  "expected_inputs": {"scene.json": "<exact raw hash for every captured role>"},
  "after_semantic_id": "cel-a",
  "split_after_frames": 12,
  "shared_event": {
    "semantic_id": "cel-new",
    "canonical_cue_id": "cue-a",
    "picture_binding": "picture-new",
    "score": {"disposition": "silence"},
    "language_ids": ["es"]
  },
  "language_event": {
    "semantic_id": "cel-new",
    "event_id": "event-new",
    "cue_id": "es-cue-a",
    "semantic_trigger_id": "trigger-new",
    "picture_slot_id": "picture-slot-new"
  },
  "human_approval": false,
  "notes": "Explicit producer authoring; not a Golden decision"
}
```

Obtain current hashes from `reel-cel-sorter inspect`. For a descriptor, also
supply its exact raw hash as `expected_project`; omit that field for a legacy
directory. `language` is always required. Multi-cue/sample-clock projects use
`split_after_samples` instead of frames. The offset is relative to the source
cel's beginning, strictly inside its current hold. Zero, overflow, the endpoint,
mixed units and wrong-clock units reject. No automatic hold fitting occurs.

The event fragments use REEL's existing native `SharedEvent` and
`LanguageEventBinding` types. Their semantic IDs must agree. Both cue identities
must match the source cel; `language_ids` must contain exactly the edited
language. `supersedes_event_id` rejects because this is an addition. Score,
Sonic/VFX and optional trigger metadata are explicitly authored, never cloned
or inferred from source media. Source text/recordings are not rewritten.

Fresh semantic/event/slot/trigger IDs must be nonempty, at most 128 UTF-8 bytes,
without surrounding whitespace or control characters. Collisions check other
language event declarations and protected graphs. A trigger cannot overwrite
an unrelated marker. Both default and optional override pictures must already
be selected effective pictures or explicitly admitted
[candidate bindings](cel-sorter-project-v1.md#explicit-candidate-pictures).
Register/bind an additional primary image before capturing fresh source locks.

The operation adds an alignment marker, graph event and fresh selected picture
slot with one initial revision, and shortens the source graph event. Existing
slots/revisions remain intact. Changed alignment bytes are rebound as
`selected-private-production`; graph/pointer locks use the native lock routine.
Other cues, asset bindings and languages are protected, including raw separate
graph/index/alignment files. One scene node is supported, within the existing
source reader's file, clock, cue and control-byte limits.

Selected-language graph clocks must already agree with native alignments. A
stale source rejects instead of silently repairing unrelated events. If repair
is intended, apply an explicitly locked identity edit through the existing
native editor, validate it and capture new locks before insertion.

Requests are bounded to 2 MiB. Unknown fields, duplicate native fields/input lock
keys and `human_approval: true` reject. Semantic/scope checks precede output
creation. Destinations must be new; failed I/O can leave a partial directory.
Unchanged controls retain captured raw bytes. The operation request is retained
as provenance in `edit-request.json`.

Export a **fresh session** from the new source (its emitted
`cel-sorter-project.json` when mapped). Existing browser controls can then edit
the newly known row. Old edits/insertion requests fail against changed locks;
old-session browser imports still reject unknown cel IDs. Rust callers use
`cel_source::insert::apply`, the same core as the CLI.

Keep earlier owner drafts as immutable feedback/provenance. Insertion does not
rewrite their locks or silently transfer comments into a different source version.

Registration of new control bytes, primary-dependency/lease accounting, delivery
compilation, rendering and human review remain separate producer steps. No cache
or media I/O, version selection, spoken-word certification, approval or release
occurs during insertion.

## Browser insertion proposals

New exports include bounded `reel.cel-sorter-insertion-context.v1` metadata:
selected language and each source cel's native cue/canonical-cue identities. No
source score, Sonic/VFX, other-language event fragments or media are copied into
this context. Older sessions remain editable and hide the insertion controls.

Choose **Propose a new cel**, select a source cel, keep an exact number of its
frames (single-cue frame clock) or samples (multi-cue clock), and choose one
already-admitted picture. The form generates a fresh UUID-based cel ID; its
related event, marker and slot IDs are deterministic suffixes. An operator may
edit the ID; Rust remains responsible for all native collision checks. Both
resulting holds must be positive. The new cel's **Silence** score intent requires
an explicit choice; this first form does not infer or clone score/VFX intent.
More complex native event intent can still be authored directly.

**Download insertion proposal** emits the existing `reel.cel-sorter-insert.v1`
request with exact input/project/language locks and `human_approval: false`.
The browser sequence, source controls and saved review remain unchanged. The
proposal cannot be opened as an ordinary cel draft. No browser write endpoint,
media read/copy, registry change, rendering, selection or approval is added.

If holds, pictures, ordering, comments, removal or general notes differ from the
source session, proposal creation refuses. Save/apply that review first, retain
its comments as provenance, then export/reopen a fresh source. This avoids
silently proposing against an earlier clock or losing owner feedback. The native
insertion operation still rejects stale proposals after any source change.

Apply the downloaded proposal with `insert-cel`, register the new control bytes
and export a fresh session. The newly known row is then editable through the
ordinary sorter. Use an explicit matching `reel-editor-host` build: frontend
bytes have changed, so a host pinned to older exports must not serve these files.
A secure browser context is required for fresh UUID creation (the explicit
loopback host provides one). Private browser/producer adoption, human visual
and listening acceptance and production delivery remain separate checks.
