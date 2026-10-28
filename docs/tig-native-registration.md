# Register native source as Tig candidates

For family-neutral current/prior package preservation, see the explicit
[versioned mode](#versioned-package-records). Native scene conversion remains
the default behavior below.

Build with `cargo build --locked --features tig --bin reel-tig-register`, then:

```text
reel-tig-register private-profile.json new-candidate-directory
```

The command reads one exact retained artifact package and its named native
scene/bindings through Tig's existing ordered byte provider. It exports
`scene.tig` and `inputs/<binding-key>.tig`. IDs are copied from native source;
they are never interpreted as filesystem paths. Selected byte pins and selection
states remain unchanged, including unselected candidates. Native text, holds,
timing and fractional values stay in the original pinned scene document.
Primary media are neither opened nor copied. Registration does not run a render,
change Git, publish to a cache, approve media or adopt a runtime.

The private profile uses `reel.tig-native-registration.experimental-1`:

| Field | Meaning |
| --- | --- |
| `byte_source` | Existing Tig byte-provider profile: explicit ordered places, layouts, protection and verification limits |
| `package` | Exact SHA-256/size Object of a retained `caminos.artifact-package.v1` source package |
| `scene_asset`, `bindings_asset` | Exact keys in that package's assets map |
| `scene_definition`, `primary_definition` | Each has `definition` and `authored_schema` FileRefs with explicit absolute local paths and expected hashes/sizes |
| `controls` | Explicit Tig stream descriptors on the scene record; external admission, not automatically inferred from the source package |
| `metadata` | Optional private provenance/rights data copied into records; `native_registration` is reserved for checked source package/binding identity |
| `primary_annotations` | Optional map from binding key to project authored fields, such as `media_kind`; cannot override source-derived fields |
| `filenames` | Optional map from binding key to a portable one-component `.tig` filename |
| `max_output_bytes` | Explicit aggregate candidate-record allowance, positive and at most 16 MiB |

Scene authored fields are `scene_id`, `episode_id`, `source_package`, `binding_ids`
and `public_release:false`. Primary authored fields are `scene_id`, `binding_key`,
`source_package` and `public_release:false`, plus explicit annotations. Supply
definitions that declare these fields. For example, a project definition requiring
`media_kind` needs an annotation for each input; the tool does not guess media
roles from filenames. Exact definition/schema binding runs before export, but
the JSON Schema engine and full native semantic validation are separate checks.
Definition/schema pins must describe the exact bytes stored in Git. Normalize
line endings before computing them or use files exported from the committed
view. After export, capture and bind the complete committed Tig snapshot before
adoption: Git clean filters or newline normalization can otherwise change a
support blob without changing the pin embedded in a candidate record.

The profile is at most 64 KiB. Each package/native document is at most 4 MiB,
with 16 MiB total metadata read; definition/schema documents and each generated
record are at most 64 KiB. Binding maps have at most 4,096 unique keys. Existing
Tig envelope/node limits also apply. Duplicate asset IDs, conflicting sizes,
wrong native scopes, unknown annotations and case-insensitive filename collisions
refuse. Unsafe or reserved filenames require explicit portable names.

The complete candidate set is prepared and bound before the output directory
is created. The destination must not exist; files use create-new semantics.
An IO failure may leave a partial candidate directory for explicit inspection;
the command grants no cleanup authority. Use a trusted caller-owned parent
directory, outside ordinary Git until the candidates are reviewed.

Successful stdout contains bounded counts and incomplete-coverage flags. It
contains no private source text, IDs, hashes or machine paths. Errors/profile
files are private diagnostics. New records do not prove primary-byte availability,
production freshness, creative approval, complete reference closure or custody.
Across scenes, reconcile shared IDs into one canonical asset file and validate
the complete target snapshot before Git proposal/adoption. This command does not
perform a project-wide merge or infer historical production claims.

## Versioned package records

```text
reel-tig-register --versioned PRIVATE_PROFILE NEW_CANDIDATE.tig
```

This mode converts a bounded `caminos.artifact-record.v1` source record without
depending on its media family. Its private profile is:

| Field | Meaning |
| --- | --- |
| `schema` | `reel.tig-versioned-registration.experimental-1` |
| `source_record` | Exact SHA-256/size Object of the complete original control record, already retained in an enrolled provider |
| `byte_source` | Explicit existing ordered providers, protection and limits |
| `definition` | `definition` and `authored_schema` FileRefs, each with absolute path, SHA-256 and size |

The definition must declare authored fields `artifact_id`, `kind`, `current_version`
and optional `public_release`. The importer copies their original values, uses the original
artifact ID as its logical ID, and selects the exact current package Object. Every
source display version becomes a `version/<original-version-id>` stream. The
immutable lock's version ID is preserved in that stream's provenance as
`version_lock_id`; it may differ from the display label. The complete original
lock remains in the pinned source record. Logical ID and manifest hash must
still match. Absent release declarations stay absent and confer no authority.
A separate
`source-record` evidence stream preserves the original control record, including
rights, editions and unknown source fields; these expanding details are not
duplicated into ordinary Git. Status is explicitly unclassified for production.

Source, profile, definitions and output use the strict bounded JSON intake;
each is at most 64 KiB and existing Tig node/depth/safe-integer limits apply.
There must be 1–255 unique versions. Current version, immutable lock identities,
manifest hashes, cache URIs and sizes must agree. Slot limits also apply. Refuse
unsupported/oversized records rather than truncating history. Packages and their
payloads remain opaque and are never opened by this operation.

The output is one new absolute file with an existing trusted parent. All
preparation/binding completes before a temporary file is created; publication
uses no-overwrite semantics and handled failures remove the owned temporary.
Stdout contains only counts/booleans; refusal diagnostics omit raw paths/hashes.
This is candidate preservation, not complete authoring conversion, native package
schema validation, production freshness, approval, payload availability, custody
or eviction authority. Before adoption, bind the complete committed target view
and verify required source/package availability through explicit providers.

## Independent assets from retained packages

```text
reel-tig-register --package-assets PRIVATE_PROFILE NEW_CANDIDATE_DIRECTORY
```

This mode reads the exact original versioned control record and each retained
package manifest, then creates a parent plus independent native assets. It does
not open the mapped payloads. It reuses the versioned mode's lock/URI validation;
display versions and immutable lock versions may differ. Original source,
selected package and all prior packages remain pinned on the parent.

The strict profile uses `reel.tig-package-assets-registration.experimental-1`:

| Field | Meaning |
| --- | --- |
| `source_record`, `byte_source` | Exact original record and enrolled existing providers |
| `parent_definition`, `parent_filename` | Exact definition/schema FileRefs and portable root `.tig` name |
| `parent_authored` | Optional declared extra fields; original identity/kind/current/release fields and generated `asset_ids` cannot be overridden |
| `entries` | Map of stable project binding names to independent asset declarations |
| `max_output_bytes` | Positive aggregate candidate budget, at most 16 MiB |

Each entry has `id`, portable `filename`, `definition`, `authored`, and `assets`.
Definitions use the same pinned FileRef pair as other registration modes. The
parent definition must admit `asset_ids`; the tool generates its ID references.
An English text entry can declare `authored.translated_from:{"id":"..."}` under
an appropriate project definition. This is a desired dependency, not proof that
an imported translation is current or approved.

`assets` maps **every display version** to its exact package asset role, or null
for an explicit caller-declared absence. For example, an independently maintained
Spanish text might map `v1` to `submitted-spanish` and `v2` to `corrected-spanish`.
No fallback or filename/language inference occurs. Current must be present.
Each available version becomes a leaf `version/<display-version>` stream with
exact object, source package, source role and original lock version provenance.
Original packages remain available even for a deliberately absent leaf version.

The parent is written at the candidate root and leaves under `inputs/`. There
are at most 128 entries and 255 source versions. Profile/source/definition/schema
and generated records retain the 64 KiB intake bounds; each package is at most
4 MiB with at most 16 MiB total metadata read. Every candidate binds before the
new output directory is created. Case-folded filename collisions, duplicate IDs,
parent/leaf identity collisions, unsupported names, missing mappings/roles,
foreign packages/locks and exceeded budgets refuse rather than truncate history.

All production states remain unclassified. Unknown source rights/holds/evidence
stay in their original pinned records/packages, not expanded into Git or inferred
from labels. An explicitly mapped subset is not complete family/graph conversion.
The caller must reconcile shared asset IDs and assign production/evidence/output
semantics through trusted definitions/adapters. Full authored-schema and graph
validation, historical byte readback and consumer qualification remain required
before adoption. No registration mode installs runtimes or grants eviction.
