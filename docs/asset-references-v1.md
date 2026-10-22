# Asset schema and reference inspection

`reel-assets` can export its inspection contracts, inspect an explicit metadata
closure, answer direct incoming/outgoing-reference questions and render a private
offline browser view. The engine is Rust. It uses the existing project registry
and configured active caches. Opening the view starts no service.

```powershell
reel-assets --root . schema-catalogue .local/schema-catalogue.json
reel-assets --root . reference-inspect operations/asset-inspection.json .local/references.json
reel-assets --root . reference-query .local/references.json 'cache://sha256/<full-sha256>'
reel-assets --root . reference-review .local/references.json .local/references.html
```

Output paths must be new. `reference-check` takes the same arguments as
`reference-inspect`, writes the report, and exits unsuccessfully if there are any
findings, including unsupported schemas or uninspected fields. Inspection exits
successfully when it has produced an honest report; that does not mean its
findings passed. Budget overruns or a changing metadata snapshot abort publication.

The repository-relative profile has no machine paths:

```json
{
  "schema": "reel.asset-inspection-project.v1",
  "project_id": "example-film",
  "roots": ["episodes/pilot/episode.json", "episodes/pilot/artifact.json"],
  "documents": {
    "pilot": "episodes/pilot/episode.json",
    "pilot-scene-01": "episodes/pilot/scenes/01/scene.json",
    "series-policy": "policies/scene-policy.json",
    "episode-master": "templates/catalog.json"
  }
}
```

`documents` explicitly resolves native logical identifiers to repository
documents. The target must have the expected schema and declare that identifier.
There is no directory search or guessed sibling-file lookup. Missing bindings
remain unresolved findings. A template identifier binds to its native template
catalogue. Native scene V2 and episode structures are parsed with `reel-assembly`;
the inspector does not introduce another authoring grammar.

## Resolve an authored scene's selected bindings

An optional `scene_contexts` array names the native resolver's seven metadata
inputs. It invokes the existing `reel-assembly::scene_authoring::resolve_scene`;
there is no parallel implementation of scene semantics.

```json
{
  "id": "pilot-scene-01",
  "scene": "episodes/pilot/scenes/01/scene.json",
  "episode": "episodes/pilot/episode.json",
  "templates": "templates/catalog.json",
  "policy": "policies/scene-policy.json",
  "season_bindings": "seasons/one/bindings.json",
  "episode_bindings": "episodes/pilot/bindings.json",
  "scene_bindings": "episodes/pilot/scenes/01/scene-bindings.json"
}
```

Each field and each `documents` value accepts a repository-relative string or a
strict cache descriptor containing exactly `sha256`, `bytes` and `cache_uri`.
Cached metadata must already be registered at that exact byte count. It is read
only by its declared digest, checked freshly in active-cache order and included
in the same metadata budgets and snapshot rechecks. No repository copy or media
hydration is needed. Contexts have unique, bounded IDs and a hard limit of 64.

Each context records its document hashes, native scene/language fingerprints,
template-definition declarations and selected binding map. Native scope, cue
coverage, template use, policy, selection-state and ambiguous/missing-slot
failures hold that context. Only the resolver's selected inputs get
`requires-asset` edges. Additional entries in binding files keep `bound-asset`
edges; they are not promoted to required inputs. A symbolic resolution can pass
while selected bytes are absent: that context is `inputs-held`, with separate
`native_resolution_valid` and `input_presence_valid` flags.

This is native symbolic selected-binding resolution. It does not decode media,
verify source-text/alignment or template-definition payload contents, validate
rendered timing, produce a build certificate or supply human approval. Existing
standalone coverage findings remain visible; a successful context does not make
an incomplete whole-episode graph pass `reference-check`. Explicit scene policy
overrides and presentation-template references are also extracted.

## Opt in to selected presentation source-text checks

Set `verify_presentation_sources: true` on a scene context to open only its
native resolver-selected `source_text_bindings` for each language. Default false
preserves symbolic-only inspection. There is no arbitrary JSON discovery.
Selected source-text metadata passes the same authorization, first-tier failure,
hash/size, document-count, byte-budget and final snapshot checks as other named
metadata. Corruption never silently falls back to another active tier.

The inspector reuses the shared native `scene_invocation`,
`scene_presentation_scope`, `verify_source_text` and
`verify_native_line_ownership` functions. The rendering CLI uses the same scene
projection helpers. Checks cover exact authored versus selected wording, titles,
chapter/byline fields, stanza structure, language, ordered declared source scope,
V2 display-text hashes and native performance ownership. An explicit authored
language text state must equal the selected source-text state. The source document
hash and scopes must also belong to one revision of the bound authority record.

Context details report `presentation_source_valid` and per-language checks.
A failed requested check is `source-held` even when symbolic native resolution
succeeds. Other languages retain their individual results. Report details contain
hashes/counts/states, not the selected source's wording, titles or bylines.
A passed check leaves `project-draft-review-held` translations as drafts.

This verifies consistency between authored presentation and selected source-text
JSON, plus declared cue ownership and authority-document identity. It does not
extract or compare the original manuscript content, decode spoken/sung words,
validate alignment timing or template-definition contents, certify complete
presentation performance coverage/rendering, or infer consent/human approval.
The source-authority and delivery unverified-semantics findings remain. General
standalone source-text schema traversal is not enabled by this option.

## Source authority and delivery reference records

`reel.source-authority-record.v1` is a strict, project-neutral declaration:

```json
{
  "schema": "reel.source-authority-record.v1",
  "authority_id": "original-text",
  "revisions": [{
    "revision_id": "original",
    "document": {"sha256": "<sha256>", "bytes": 123, "cache_uri": "cache://sha256/<sha256>"},
    "scope_ids": ["paragraph-001"],
    "evidence": []
  }]
}
```

Bind a scene's `source_authority_id` through `documents`. The target must declare
that authority ID and this exact schema. The inspector checks that the scene's
source, canonical-cue and presentation scope IDs are declared by the record.
Scope IDs must be uniquely assigned across its revisions. This checks declared
coverage, not correspondence between source text and cues. At most 128 revisions,
10,000 total scope IDs and 128 evidence objects per revision are allowed.

`reel.delivery-record.v1` declares an exact historical delivery subject:

```json
{
  "schema": "reel.delivery-record.v1",
  "delivery_id": "pilot-private-copy",
  "subject": {"logical_id": "episode/pilot", "version_id": "golden-1", "manifest_sha256": "<sha256>"},
  "audience": "private",
  "outputs": [{
    "role": "movie", "language": "es",
    "asset": {"sha256": "<sha256>", "bytes": 456, "cache_uri": "cache://sha256/<sha256>"},
    "evidence": []
  }]
}
```

The subject lock must match the immutable registry. Each output must be retained
in that exact version's payload set. Output role/language pairs must be unique;
labels describe the declaration, not a verified package-role or language match.
At most 128 outputs and 128 evidence objects per output are allowed. Audience is
`private` or `public`; neither value grants release permission. There is no
remote destination or credential field and inspection never contacts a service.

Both contracts reject unknown fields. Descriptors contain exactly SHA-256,
positive byte count and matching cache URI. Referenced source, output and
opaque evidence payloads get ordinary registration/size checks, not content hash
verification. Evidence JSON is never parsed merely because it looks supported.
Empty evidence arrays produce explicit findings. Every record also retains an
unverified-semantics finding: neither a nonempty evidence list nor a successful
structural check establishes authorship, rights, consent, approval, remote
readback, playback, current delivery availability or public-release authority.

These compact records can project historical evidence without rewriting it.
An asset repository owns its declarations and private payloads; REEL contains
only the generic types, inspection rules, schema export and synthetic fixtures.

## Relationship meanings

| Relationship | Meaning |
|---|---|
| `requires-version` | Exact immutable dependency in the native version registry. |
| `retains-asset` | Payload custody for a version; does not establish creative selection. |
| `current-selection` / `historical-version` | The record's pointers; registry current state is separately reported and disagreements are findings. |
| `manifest` | A record's exact cache-addressed package document. |
| `declares-asset` | A package role points to bytes; specialized role meaning is not inferred. |
| `inventory-member` | An inventory or input part observes bytes; not a required build input. |
| `inventory-part` / `inventory-snapshot` | An inventory's declared component or a record's current inventory. |
| `describes-version` | The Golden/version an inventory or package describes. |
| `declared-dependency` | A package's declaration, checked against the native registry. |
| `record-link` | An explicit repository record reference. |
| `requires-scene` / `requires-policy` / `requires-template` | Native episode references, resolved through the explicit profile. |
| `episode-context` | A scene's contextual backlink; not a dependency cycle. |
| `bound-asset` | An asset descriptor in a native scoped binding document. |
| `source-authority` | A scene's explicitly bound authority record; scope coverage is declaration-only. |
| `source-document` / `authority-evidence` | Exact source revision and opaque provenance/decision evidence. |
| `delivers-version` / `delivers-asset` | Historical subject lock and retained output; neither is a build dependency. |
| `delivery-evidence` | Opaque receipt evidence; no destination access or fresh remote verification. |
| `selected-source-text` | Explicit native selected source-text metadata opened by an opted-in context. |
| `authoring-document` | One explicitly supplied metadata document for a native scene context. |
| `requires-asset` | An exact selected input returned by the native scene resolver; media presence remains size-only. |

Only `requires-version` edges participate in immutable dependency-cycle checks.
Queries show direct relationships, not a complete transitive rebuild analysis.
Shared inventory membership must never be used as evidence that an episode
consumed an asset or that it is safe to remove that asset.

## Contract coverage

The schema catalogue exports JSON Schema generated from the Rust inspection
types, including profile/report and compatibility reference projections for
`caminos.artifact-record.v1`, `caminos.artifact-package.v1`,
`caminos.episode-input-inventory.v1` and `caminos.episode-input-index.v1`.
These legacy schema names remain compatibility identifiers: their shapes and
engine are reusable and contain no project names, storage paths or creative
data. They do not require a CAMINOS checkout.

Projection schemas preserve additional fields. Their keys are reported as
uninspected; their contents are never searched for hashes. Inventory row probe
results, old hash readbacks, source-path labels and unknown ancestral references
are evidence, not freshly verified dependencies. JSON payloads remain opaque
unless a supported contract explicitly declares them as metadata documents.
The special `inventory` role is opened only for an `episode-input-inventory`
package; arbitrary reports using that role are not traversed.

Native episode, scene, scoped binding, policy and template catalogue structures
use the existing Rust parsers. Exported JSON Schemas are not yet available for
those native structures; the catalogue reports `null` and states extraction
coverage. Explicit scene contexts perform native symbolic selected-binding
resolution. Whole-episode presentation, rendered motion, cue/source evidence
and complete build/delivery validation remain separate. Rights,
approval interpretation, delivery mirror verification, generic family indexes and other schema families
remain explicit coverage gaps. No inspection pass supplies human approval,
voice consent or a build certificate. JSON Schemas describe structure; Rust
also enforces identifiers, exact locks, budgets and cross-document invariants.

## Bounds, freshness and privacy

Defaults are 1,000 documents, 20,000 nodes, 100,000 edges, depth 64, 16 MiB per
document and 64 MiB of metadata reads. The `limits` object can lower or raise
these within explicit hard caps. Existing registry authorization is read through
the bounded native registry APIs, independently of traversal budgets.

Named metadata bytes are freshly hashed and registered descriptors checked.
Media is never opened: active-cache existence/regular-file/size checks are
reported as `present-size-only`. Unsafe, wrong-size, corrupt or unreadable first
active copies hold inspection of that target; they do not silently fall through
to a lower tier. There is no archive access, hydration or cache census. Explicit
metadata paths reject traversal and symlink/reparse ancestors. Before return,
opened documents, storage controls, version state and object authorization are
rechecked. Concurrent producers need no global lock; a changed snapshot retries.
Media presence can subsequently change and is never an execution authorization.

The saved report has a reproducible SHA-256 integrity digest, checked by query
and HTML generation. This is an integrity check, not a signature or freshness
certificate. The browser uses literal text, embedded data and a network-denying
content policy. Search, typed-edge filters, bounded node pagination and clickable
incoming/outgoing links work without Node, a daemon or a web server. Node is used
only by development interaction tests. Browser layout acceptance is separate.

Keep the compact project profile in Git. Retained graph reports, schema exports,
HTML and review evidence belong in the producing project's private cache/LFS.
Do not commit real inventories or producer metadata to public REEL. The graph
never authorizes removal or changes creative/version selections.
