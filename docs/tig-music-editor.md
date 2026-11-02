# Native music control authoring from Tig

`reel-tig-music-editor` uses the existing explicit committed-source profile and
byte providers. It does not initialize a cache, discover a project, start a
server, copy recordings, publish objects or run Git writes. Build with:

```text
cargo build --locked --features tig --bin reel-tig-music-editor
```

```text
reel-tig-music-editor --profile PRIVATE_SOURCE export MODEL_ASSET_ID NEW_EDITOR_DIR
reel-tig-music-editor --profile PRIVATE_SOURCE propose SOURCE_LOCK EDIT_JSON NEW_PROPOSAL_DIR
```

The profile is `reel.tig-input-profile.experimental-1`, with an exact Git commit,
record/definition scopes, exact admitted definition pins and existing providers.
The model's admission must select its current payload. Named-stream admissions
refuse. The model is at most 2 MiB and must pass the existing native note-editor
layout contract (at most 64 parts and 1,000 notes). This authoring entry does not
interpret a project selection status as production eligibility. Existing render
eligibility checks remain unchanged.

To propose an edit, the exact model definition must explicitly declare
`semantic_rules.profile: "reel.music-note-authoring.v1"`. Retained package-member
definitions remain readable but cannot silently become editable model contracts.
Project migration must admit an appropriate model definition separately; the
editor does not promote an imported hold or change a definition itself.

The model is captured through Tig's verified byte reader, with its requested
protection kept live across the operation. Its complete bounded bytes are hashed
again after reading, before any output is created. The editor receives captured
bytes, never a cache path. `prevent-mutation` may be requested where supported;
cooperative immutable readers remain explicitly cooperative, not a promise that
another writer is excluded. A corrupt snapshot or unavailable requested protection
refuses. Existing path-based render protections are unchanged. No audio,
referenced analysis or other model dependency is opened.

## Edit and lock

Export writes the existing reusable note editor's session, HTML and JavaScript,
plus a private `source-lock.json`. The lock binds the actual asset record,
definition and selected model object. The ordinary editor's downloaded edit
also locks the exact model bytes. No new project authoring script is involved.

Propose reopens the captured current source. A changed target record, definition
or model refuses; an unrelated asset revision may coexist with the edit. The
existing note-editor rules still require every original note exactly once,
valid clocks/voices, and a rationale and existing evidence for changed notes.
The candidate has a new internal model identity, unreviewed authority/review
and no inherited decision references. The logical Tig asset ID stays stable.

Both destination arguments must be new absolute directories with existing
ordinary ancestors, outside all configured byte-provider roots and Git internals.
They are private bounded workspaces; cache publication is a separate operation.
Existing output directories refuse. An I/O failure may leave an incomplete new
workspace, which is not an adopted source or retained result. The proposal's
receipt is written after its other files.

The proposal includes `model.json`, `record.tig`, the original source lock and
edit, original record/definition/schema documents, and a private receipt. The
record preserves its identity, definition, authored fields, historical streams
and metadata, replaces only its selected object, and clears current review
status to `draft-unreviewed`. Explicit lineage retains the predecessor's exact
record, subject and status. A conflicting reserved lineage field refuses.

## Dependency context and adoption

The provider-neutral byte API preserves the model's dependency names and fields;
it does not resolve or reinterpret them against the proposal directory. The
existing local-file editor continues to require writes beside its source to
preserve ordinary relative paths. The Tig entry instead produces a locked
unreviewed artifact whose consumers must retain the original logical dependency
context. Running a standalone musical validator on relocated relative paths is
not equivalent to that context. Full dependency/musical validation stays `not-run`.

The new object pins are not publication acknowledgments. Use admitted existing
publication/job retention and the normal exact-base Tig/Git proposal workflow
before adopting a candidate. No byte obligation is released, no Git reference
changes, and the old asset remains selected until that separate adoption. These
commands do not establish render currency, full musical validation, creative
approval, delivery, backup custody or eviction authority.

Source locks, editor sessions, candidate files and receipts contain private
project metadata and stay in private workspaces/assets. Stdout reports only the
operation, count, and explicit non-approval/non-publication flags. Full
music-production dependency binding and managed music-job adoption remain
separate integrations.

## Exact-text lyric underlay

```text
reel-tig-music-editor --profile PRIVATE_SOURCE export-underlay MODEL_ASSET_ID NEW_EDITOR_DIR
reel-tig-music-editor --profile PRIVATE_SOURCE propose-underlay SOURCE_LOCK EDIT_JSON NEW_PROPOSAL_DIR
```

These commands additionally consume one exact selected lyric text, at most
128 KiB of UTF-8. The model definition must declare
`semantic_rules.binding_profile: "reel.music-model-input-bindings.v1"` and
`semantic_rules.underlay_profile: "reel.music-underlay-authoring.v1"`.
Its `authored.binding_ids["lyric:<selected-layer-id>"].id` names an admitted Tig
asset, whose definition declares `semantic_rules.profile:
"reel.music-lyric-source.v1"`. The lyric record's authored `language` and
`lyric_kind` must match that selected native layer, and its selected bytes must
match the hash observed by the model. Missing, ambiguous, stale or incompatible
bindings refuse before creating output. No relative lyric path is opened.

The private lock captures both records, definitions and selected objects.
An unrelated commit is allowed; a changed model, lyric annotation, definition,
selection or binding requires a fresh session. Both byte-provider guards stay
live throughout the operation. The original underlay editor's exact UTF-8 byte
offset, source-order, note coverage and rationale checks apply. Proposals preserve
all lyric bytes and notes, clear review/authority, and require the lyric/vocal
adaptation editor role. The selected lyric's exact lock is retained in private
lineage; no text is printed to stdout. Counts describe resulting spans.

These checks are underlay-specific, not complete source/analysis/audio validation
or listening approval. The local-file editor retains its original selected-path
and same-directory output requirements. Tig output remains a separate unreviewed,
unpublished candidate; the current project selection is unchanged.
