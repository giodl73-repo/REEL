# Compile a native scene from Tig

`reel-tig-scene compile-native` captures the explicit committed source profile
and resolves the scene's current `authored.binding_ids` map. Each value is exactly
`{"id":"opaque-asset-id"}`. The parent file need not change when a leaf changes.
The parent definition and all primary definitions are explicitly admitted and
pinned by the existing source profile. The native request distinguishes the scene
definition from permitted primary definitions; scene/control/delivery definitions
must not be admitted as primary types. Definition admission is trusted project
policy, not execution of every JSON Schema or Tig semantic rule.

```text
captured Git commit
  scene.tig --binding_ids--> cel.tig / cue.tig / alignment.tig
      |                         |
      |                  selected bytes + current status
      |                         |
      +---- selected scene -----+---- fresh scoped bindings
      +---- named controls -----+---- measured alignment clocks
                                         |
                                current assembly projection
                                         |
                                 existing REEL compiler
                                         |
                                ID + hash + size in job
                                         |
                            existing Tig guarded input provider
```

The source profile still supplies the repository, full commit, namespace scopes,
exact definition pins/selectors and existing cache roots. The compile request is
small native REEL policy. For example:

```json
{
  "schema": "reel.tig-native-scene.experimental-1",
  "scene_id": "scenes/dawn",
  "scene_definition": "types/native-scene",
  "primary_definitions": ["types/picture", "types/take", "types/alignment"],
  "language": "es",
  "controls": {
    "catalog": "catalog.json",
    "episode": "episode.json",
    "policy": "policy.json",
    "profile": "profile.json"
  },
  "delivery_id": "dawn-es-private",
  "delivery_title": "Private scene build"
}
```

```powershell
reel-tig-scene --profile .local/tig.json compile-native .local/dawn-es.json .local/build-dawn-es
reel-tig-scene --profile .local/tig.json plan .local/build-dawn-es/delivery/job.json
reel-tig-scene --profile .local/tig.json render .local/build-dawn-es/delivery/job.json renders/new-dawn-es
```

The selected parent object is native `scene.json`, not rendered video. Its
`authored.scene_id` must match that scene. Control names select exact named streams
on the same parent revision; they are never executed as scripts or interpreted as
paths. Retained `bindings.json`, old assembly graphs and old alignment-path lists
are not current dependency authorities. The adapter generates scene bindings from
current leaves, empty enclosing scopes, the requested language's alignment paths
and a current assembly projection. Version history stays in the Tig records/Git;
this derived assembly is a current snapshot, not an append-only history import.

Native scene validation still resolves the entire declared language matrix;
this version does not infer partially completed variants. Each declared leaf must
have an admitted selected/named payload and one of native REEL's eligible
selection states. A draft leaf refuses before reading cached bytes. Replacing a
take without its matching alignment refuses; historical captured views retain
their own payloads and status. `Source::resolve_asset(id)` is the metadata-only
library operation for deriving an exact `AssetRef`; it does not read media or
grant production authority.

The new output directory contains bounded private metadata only. Original scene
bytes, including holds and source text, are preserved. Controls are at most 4 MiB
each, with 16 MiB aggregate metadata and 4,096 bindings. The 64 KiB request is read
with a bound and rejects duplicate/unknown fields. Profile `asset_paths` is derived
as `asset-id`. Original control pins and the captured parent record/checkpoint are
recorded in `native-source.json`; this is private provenance, not a public receipt.
Optional presentation receipts use an explicit `controls.template_receipt` stream;
the compiler still compares it to the fresh selected receipt binding. Sonic
placement evidence uses protected exact input pins during compilation. Rendering
subsequently prepares the whole job independently.

The adapter creates no media cache, copies no primary media, runs no transfers,
and changes no runtime binding. Compilation is not production approval or asset
registration. Errors may leave a partial new metadata directory for diagnosis;
it is not a completed build. `complete_semantic_coverage` remains false. Full
schema/variant rules, source/job recovery, custody/eviction authority, family
migration, rendered parity and explicit production cutover remain open.

## Declared inputs and archived source metadata

`reel::tig_scene::requirements(&mut source, &request)` derives a metadata-only
Production query from the existing request and source admission. The scene uses
its selected document and exact named control slots; primary definitions retain
their explicit selected/named payload selectors. The original `binding_ids` map
is the reference authority. Extra admitted types never default to primary.

The result separates primary payloads, scene/control payloads and exact bound
instance/definition/schema objects. It covers the entire declared scene, including
all declared languages; it does not infer minimal per-language consumption.
Missing required controls, malformed references and duplicate primary definitions
refuse. Limits are 4,097 nodes, 4,096 edges, depth two and 16 MiB returned metadata.
The native compiler still checks leaf eligibility before byte IO and validates
opaque scene/alignment/control meaning through the ordinary REEL compiler.

Compilation adds `declared_inputs` to the existing private `native-source.json`,
including checkpoint, interpretation/binding hashes and the three object sets.
`tig_source_pin_sha256` binds the imported runtime source manifest. These hashes
and source identities are private provenance, not a sanitized public receipt,
consumed-property recipe, human approval, retained-custody proof or eviction grant.
Delivered-output custody remains a separate obligation from production inputs.

The Rust API also supports explicit metadata-provider ownership:

- `Source::archive_metadata(store, tree_limits)` returns a candidate archive root.
- `Source::from_archive(ArchiveProfile, store, tree_limits)` restores it without
  a Git executable or repository path. The profile schema is
  `reel.tig-archive-input-profile.experimental-1`; it pins the archive Object and
  expected checkpoint, native limits, exact definition admissions and existing
  byte-source profile. A wrong checkpoint or unavailable archive refuses.

The supplied store implements Tig's existing `NodeStore` port. The caller owns
provider creation, explicit finish/reopen/readback, root protection and job/source
admission. These methods initialize no store or cache and install no runtime.
The existing CLI continues to accept the committed Git source profile; archived
source loading is a Rust API in this version. Payloads and scene/control bytes
remain in the separately enrolled existing caches and are freshly verified when
read. Archive existence alone does not establish their availability or custody.

Synthetic tests physically move Git/native working metadata aside before exact
native recompilation. The explicit Windows FFmpeg case also prepares archived
inputs, drops the source, renders/checks and compares original pixels and PCM.
Actual job/claim recovery, source/job completeness, schema/variant execution,
consumed-property invalidation, full family parity and production cutover stay open.

## Frozen declared scene jobs

Optional Rust APIs use the same request-derived Production policy:

```text
freeze_context(source, request, metadata_store, tree_limits) -> candidate context
              |
     caller finishes provider, closes and reopens it
              |
restore_context(admitted_archive_profile, request, selection, store, limits)
              -> exact source + verified context (no byte/cache reads)
              |
     caller owner-journal prepare/adopt and job/resource admission
              |
compile(restored_source, request, new_private_job_directory)
```

The private descriptor binds the exact request, native scene/input adapter source
bytes and vendored Tig source manifest. A different request, adapter, selection,
admission, archive or checkpoint refuses. This experimental replay requires the
matching adapter binary; no upgrade conversion or latest-source fallback occurs.
The restored Source binds that descriptor for native compilation/refreezing;
changing the request afterward refuses before byte reads or job-directory writes.
Lower-level Source readers remain explicitly caller-managed, not a capability
sandbox limited to this context's object set.
The returned context has exact root and primary/control/interpretation pins;
the returned source uses its verified archived snapshot without Git. Neither API
creates a provider/journal, starts a job, adopts an owner claim or reads payloads.
CLI source profiles remain unchanged. Caller-provided stores and graph/archive
limits bound individual stages, not total memory or a capacity reservation.

An isolated durable fixture finishes and reads back two cel revisions before
journal adoption, advances main, retires only its predecessor, reopens the journal
and metadata store, then compiles the old job byte-identically after Git/working
metadata are unavailable. The new main compilation uses the replacement cel;
the independent old job keeps its original root. Negative cases refuse changed
requests/tool descriptors, admissions, missing metadata and budget failures.

These are declared production contexts, not consumed-property render recipes,
full runtime/tool closures, human approvals, global job/source enrollment or media
custody. Physical execution guards remain separate. Canonical output dependencies
remain forbidden; optional derived-output cache reuse needs its own validated
recipe and execution leases. The owner journal still reports incomplete global
claims, and no context or job completion grants cache-removal authority.

## Declared scene recipe

`reel::tig_scene::recipe(&mut source, &request, &producer_object)` returns Tig's
immutable recipe Plan without reading payloads, rendering or opening a provider.
It uses the same request-derived Production graph as requirements and contexts;
there is no separately authored input list. It consumes the scene's authored
`scene_id` and `binding_ids`, qualified primary payloads, selected scene content,
and exact named controls. Primary authored scalar fields are explicitly excluded:
the current compiler reads their payloads and selection eligibility, not scalar
production parameters. Changing compiler consumption requires updating this rule.

The operation binds the exact private context descriptor as parameters, including
language, delivery request and adapter/vendor pins. The caller supplies an explicit
producer package/runtime Object. This method does not inspect that package or
prove that it names the executing REEL binary, FFmpeg, fonts or shared libraries.
It is a declared whole-scene recipe, not complete runtime closure or minimal
language/variant consumption. Different language requests conservatively produce
different recipes even while their declared whole-scene primary sets coincide.

Annotation-only and selection-status changes preserve recipe identity while exact
source provenance changes. Compilation still enforces current selection eligibility:
a matching recipe cannot authorize a held input. Changing a cel, cue, control,
consumed scene field, producer or request invalidates the declared recipe.
An archived/frozen source can reconstruct the original recipe with Git unavailable;
a changed request on a restored context refuses before payload IO.

No result is selected, delivered or physically protected by a matching recipe.
Connecting an actual completed render, verified output bytes and retained tool
closure to Tig's produced claim remains a separate integration step. Historical
approved masters without a native claim remain selected history, not fabricated
producer evidence. Existing cache/job admission and independent custody checks
remain required; no metadata-only recipe comparison grants removal authority.

## Checked local render candidates

The optional Rust `tig_scene_production::render` takes a verified frozen context,
its exact Source/request, the externally admitted producer Object, and new private
job/output directories. Before creating directories it checks exact context
checkpoint, binding, admission and parameters. It compiles its own native job,
prepares guarded exact inputs, renders through the ordinary scene-delivery engine
and independently decodes/checks the result. It returns a sealed in-process
`RenderedCandidate`, not a deserializable success receipt or a selected asset.

The caller must first finish/reopen/read back and durably protect/adopt the context
and admit job/storage resources. This API does not establish complete global job
claims or capacity reservations. Existing path-protection requirements still
apply; no global render lock is added. Compile/render errors may leave diagnostic
job metadata or completed outputs without a candidate claim.

`candidate.claim(&[OutputBinding { output_name, subject }])` explicitly maps checked
render outputs to separately admitted Tig BoundSubjects. It verifies the exact
private receipt pin and freshly hashes mapped output files before creating the
unsigned claim. Empty/duplicate mappings, foreign subjects, unknown outputs,
changed receipts or changed payloads refuse. The claim pins original context,
recipe and outputs without hashing its own containing .tig record. Callers retain
the claim, private receipt and bytes through their existing storage/publication
system; the API writes no .tig record, cache, selection or delivery update.

This closes a scoped local execution/result-binding seam. The recipe remains
declared whole-scene consumption, and the producer Object remains external
admission: complete executable/decoder/library/font closure is not verified here.
It therefore does not establish full renderer-freshness acceptance, approval,
delivery currency or independent retained custody. Input guards end with rendering;
candidate claim creation alone neither retains outputs nor authorizes eviction.

## Original native claim replay

`tig_scene::restore_claim(original_archive_profile, request, producer, claim,
store, limits)` returns the original verified Source and Tig's ReplayedClaim.
Operation/request/producer mismatch refuses before any metadata-store read.
The API derives native profiles and consumed fields through the same helpers as
recipe planning, then delegates to upstream original-context/recipe replay. It
checks exact external archive/checkpoint, native scene root and descriptor, and
binds the restored source to the original request. A missing store never falls
back to Git, another source revision or a latest payload selection.

The caller supplies the original admitted request/profile and an exact parsed
asset-side Claim; replay does not treat a claim's own declaration as authority.
It reads metadata only and writes no store/cache, starts no producer, and does
not verify output payloads. A verified original claim can then be compared with
an independently planned desired recipe and separately bound current output
subject through Tig's correspondence API. Annotation-only desired source changes
can preserve a claimed recipe match; consumed changes are stale. Neither state
proves approval, completed delivery, availability, full tool closure or custody.
Historical selected outputs with no native claim remain Unproven in this new
correspondence contract and retain their existing selection/history obligations.

## Checked candidate byte publication

With `tig-publication`, `RenderedCandidate::publish(bindings, publication,
backend)` takes explicit admitted output mappings, a caller-owned operation/owner
and acknowledgement level, and a Tig `PublicationBackend`. It rechecks the native
receipt and all selected local output bytes, builds the exact unsigned claim and
preflights every object against backend capabilities before provider writes.

```text
checked render candidate + admitted output bindings
                       |
          revalidate local selected bytes
                       |
          preflight every publication request
                       |
       selected payloads -> native receipt -> claim
                       |
      PublishedCandidate (all acknowledgements)
                       |
      explicit project registration/adoption later
```

The result exposes the exact claim, native receipt pin and individual byte
acknowledgements. It is not deserializable and has no public constructor.
Per-item operation keys bind the caller operation and item role/name independently
of content. Exact retries keep those keys; changing an item under the same
operation conflicts rather than hiding the change behind a new content-derived
operation. Mappings are explicit and unselected render files are not published.
Subjects sharing one output name share its publication request while retaining
their separate logical claim bindings.

This is an ordered batch, not an atomic transaction or retained lease over every
object. Failure may leave valid immutable partial objects; an exact retry can
reuse them. No source file is deleted or repaired, no cache/status/Git selection
is inferred, and the candidate still requires its local files for revalidation.
Caller-owned original context, parameters, runtime/code and job/custody claims
must remain available separately. Independent custody and current protection
checks are still required before retiring any source. For the concrete local
provider's capabilities and limits, see [existing-cache publication](tig-publication.md).

## Explicit protected native jobs

With `tig-publication`, `tig_scene_production::jobs::JobPlan::from_context` binds
the native request/source context and held `BoundPrograms` through the same recipe
checks as rendering. Runtime pins come from that held declaration's host, programs
and explicit dependencies. Opaque component labels are hashed for metadata keys;
the original private runtime manifest retains their meaning without host paths.

```text
JobPlan + held runtime + original source context
                     |
        freeze -> provider finish/reopen -> readback
                     |
        protect: scoped job fence + owner prepare/adopt
                     |
             native render -> checked candidate
                     |
           publish payloads/receipt/claim
                     |
       freeze_outputs -> finish/reopen -> readback_outputs
                     |
             adopt_outputs (exact owned handoff)
                     |
       claimed output root + prior input root retained
```

The explicit coordination directory must already exist and be shared by managed
clients of the journal. The fence derives its namespace from the journal's stable
authority identity and source ID. It covers one job; unrelated jobs and journal
authorities can progress. Capabilities also bind that authority, owner, exact
source revision and selection. The same source/owner/revision/root labels in
another journal cannot substitute for a protected job.

`Readback::protect` issues a job capability after explicit journal prepare/adopt.
`resume` accepts an exact current planned job without adding transitions. Each
render/publication/readback checks current ownership before and after work;
foreign or stale capabilities refuse. `adopt_outputs` uses exact idempotent
commands, preserves the input predecessor and allows only an exact completed
handoff retry. Dropping a capability unlocks its scoped fence and leaves durable
obligations intact. It does not expire a job or remove its files.

Use one admitted authoritative journal and trusted minimum checkpoints; copying
a database does not create independent authority. Metadata ports/providers and
caller authority remain trusted contracts. These APIs do not admit complete job
capacity, workspace/scratch growth or byte availability, infer undeclared runtime
dependencies, authenticate unsigned claims, establish independent retained custody
or authorize cleanup/selection/delivery. Ambiguous persisted transitions keep
their requirements and need exact owner recovery; there is no timeout reset.
Native managed CLI adoption, global source/job coverage and storage convergence
remain separate integration work.

## Declared runtime files and explicit programs

`media_runtime::BoundPrograms::bind(&Bindings)` verifies the actual running host
executable, explicit FFmpeg/FFprobe files and up to 32 named extra dependencies.
Private bindings carry absolute local paths and expected byte identities. Held
Windows handles deny file mutation/replacement and parent-directory rebinding
while allowing independent file additions and writes. Unsupported mandatory
protection refuses; there is no cooperative downgrade or global cache/render lock.

The binding generates a portable private manifest containing only logical names,
hashes and sizes. `manifest_bytes()` and `identity()` let callers retain that
manifest as an ordinary asset. Bounds are 1 GiB per component, 4 GiB total, 64
path components and 64 KiB manifest bytes. `ByteIdentity::observe_file` is a bounded
read-only enrollment aid; observation alone is not the held capability. Bind
always rechecks exact bytes under effective guards.
Native executables may use opaque extensionless cache names; implicit shell batch
programs refuse. This needs no new runtime cache or production file copy.

Use `tig_scene_production::render_with_programs` to bind that manifest Object as
the producer and borrow its absolute programs throughout rendering and independent
checks. The existing `InputFiles::media_program` port carries this choice through
scene/audio rendering, overlays, PCM rail checks and Motioncraft evidence. Explicit
provider refusals do not fall back to PATH. A binding may be reused across jobs
while its read protection is retained; no process-global tool selection changes.
Existing local-root callers keep their ambient program behavior, and the earlier
`render` API still takes externally admitted producer identity. Neither legacy
path establishes the new verified runtime-file capability.

The manifest explicitly reports `complete_environment: false`: this verifies the
declared host/tool/dependency files and does not discover every OS library, font,
CPU setting, environment dependency or software license/authority. Callers retain
the manifest and code through actual job/resource/custody admission. This adds
declared execution-file identity, not full renderer-freshness or production cutover
acceptance, delivery approval or an output retention lease.
