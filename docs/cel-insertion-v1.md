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

## Audition a proposed split

The insertion panel shows the selected picture when the session already exports
media, plus both exact frame/sample holds and their displayed durations. In a
controls-only session it shows timing only. This uses the existing declared image
map; it does not discover, copy or admit additional media.

**Audition split** is available only for `native-recordings` sessions. After
choosing valid insertion intent, it plays the source hold from the existing
primary recording, displays its original picture before the split and the chosen
new picture after it, then pauses at the hold boundary. It does not use a compiled
movie as audio for the new proposal. The original sequence and downloaded native
request stay unchanged. Browser playback timing is approximate; native integer
clocks, compilation and delivered-frame checks remain authoritative.

Stop, navigation, form edits, draft comments/notes and panel closure cancel the
audition. Pending recording-load callbacks are invalidated, and a terminal media
`ended` event cannot silently advance the audition into the next cue. A new
explicit playback/seek resumes ordinary preview behavior. Media exports still
require their existing explicit budgets and a matching host build. Audition is
neither a render nor human visual/listening acceptance.

## Explicit native score roles

New exports add `score_palette` to the existing insertion context. Its `choices`
list contains native episode roles whose asset binding resolves exactly once
across scene/episode/season scopes, with an exact selected cache identity. The
native episode must include this scene and use matching scope IDs. At most 128
roles are admitted; role IDs are bounded to 128 UTF-8 bytes, and binding/logical
IDs to 256. Unavailable, ambiguous, unselected or out-of-budget bindings appear
in `held_roles` with reasons. Malformed palette declarations, duplicate roles and
invalid episode/scope contexts reject export before media lookup/output creation.
A missing legacy palette continues to offer silence only.

The form still requires an explicit score choice. **Silence** emits the original
native disposition. An available **Score role** emits:

```json
{"disposition":"role","role":"selected-native-role"}
```

No score binding is guessed or copied from the source cel. The request still uses
`reel.cel-sorter-insert.v1` and the native `SharedEvent.score` union, with original
source/language/project locks and no human approval. Rust rechecks the selected
role from the locked episode/scopes before creating output, independently of any
browser choice list. Unknown/unavailable roles and changed palette/binding locks
reject. Existing native `held` draft intent remains available through direct
native authoring; it is not offered as a build-ready browser choice.

Available here means a declared selected native binding, not verified cache
presence, input/output classification, lifecycle admission, rights or listening
acceptance. Register/authorize the effective primary working set and use the
existing managed-job/compiler checks before rendering. No registry lookup,
asset promotion, media read/copy, selection, new music or consent occurs during
metadata export or insertion.

Split audition continues to play the primary recording and pictures only; it
does not mix the selected score, Sonic or VFX. Native delivery compilation resolves
the chosen role into an M-bus input over the new cel's exact interval. Other
language events, role selections, recordings and prior picture revisions remain
unchanged. Final mix and musical acceptance are separate production steps.

## Explicit Sonic and VFX intent

New insertion contexts include `effects.sonic` and `effects.vfx`, each with
`choices` and unavailable `held` diagnostics. Choices come from effective events
in the selected language plus explicitly named project candidates. Unused bound
assets and effects used only by another language are not inferred as candidates.
Each kind admits at most 128 distinct binding keys, each at most 256 UTF-8 bytes.
Bindings resolve exactly once across the declared native scene/episode/season
scopes and must carry selected, valid cache identities. Declared external scopes
must match the episode context. No registry lookup or media read is performed.

The panel starts with all effects unchecked. Checking a binding emits the
existing native `SharedEvent.sonic_bindings` or `vfx_bindings` array. No second
scene format or request version is introduced. Rust independently revalidates
both shared arrays and any language overrides, including defaults hidden by an
empty override. Duplicate, unavailable, foreign, wrong-kind or stale choices
reject before source output. A binding explicitly admitted in both categories
can be chosen for either; candidate category is intent, not media inspection.
Project candidates survive ordinary edits and insertion into a new directory.

Split audition still plays only primary recordings and pictures. Sonic playback,
VFX rendering, profile authoring, primary-input classification, cache admission,
rights and human acceptance remain separate native/managed production checks.
Delivery compilation resolves Sonic into the E bus and VFX into an external layer
using its exact selected evidence identity and existing profile render mode.
