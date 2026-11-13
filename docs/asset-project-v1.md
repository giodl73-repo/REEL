# Project asset custody and cache accounting

Explicit retained-baseline tier proposals and their limits are documented in
[asset tiering](asset-tiering-v1.md). The planner is read-only, not an eviction tool.

`reel-assets` is a separate Rust crate and explicit CLI. It owns local cache
admission, immutable versions, dependency protection, local-only LFS transport,
bounded jobs and read-only cache accounting. It has no dependency on a consumer
checkout or TRACKER path. Existing `caminos.*.v1` serialized contracts and legacy
staging names remain compatibility formats; extraction does not relabel assets,
retarget current versions or change their approval scopes.

Build explicitly:

```text
cargo build -p reel-assets --release --locked --bins
reel-assets --root <project> cache-report <new-private-report.json>
```

The binaries are `reel-assets` and `reel-lfs`. The latter implements the local
Git LFS transfer protocol; uploads are refused. Opening a repository starts
neither program. Existing explicit recovery commands remain separate from the
accounting command, which never probes archive roots or copies payloads.

Authorization snapshots keep the V1 single-leaf representation through 4 MiB.
Larger snapshots use `caminos.object-index.v2`: a root of at most 4 KiB names up
to sixteen hexadecimal prefix leaves, each limited to 4 MiB. This bounds the
whole snapshot at 64 MiB while keeping bulk authorization rows in cache/LFS.
Readers verify every declared leaf and its row prefixes, then apply legacy TSV
deltas with the same conflicting-size checks. These are typed metadata leaves,
not recursive imports. A prefix that exceeds its own bound fails before
publication. Compaction preserves versions, selections, pins, leases and original
control rows; all current leaves are protected registry control objects.
Historical V1 and V2 headers remain independently readable from their retained
leaves. Adopt a V2-capable runtime before selecting a V2 root; existing jobs keep
their pinned runtime and controls until an explicit successor is qualified.

## Project profile

[Locked artifact export](artifact-export-v1.md) reopens explicitly selected roles
from a registered version into a new bounded working folder, with a read-only
plan and independent verified copies. It never changes cache residency or current
selections and never follows historical references implicitly.

Keep `asset-project.json` in the asset repository. Machine paths belong in an
ignored local storage file. For example:

```json
{
  "schema": "reel.asset-project.v1",
  "project_id": "example",
  "storage_config": ".local/storage-rust.json",
  "selections": [
    {
      "index": "catalog.json",
      "entries": "/videos",
      "record_field": "bindings",
      "format": "bindings",
      "role_prefixes": ["delivery/"]
    }
  ],
  "scan_cas": true,
  "measure_unregistered_physical": false,
  "max_inventory_objects": 500000,
  "lfs_namespaces": ["lfs/review"]
}
```

An index JSON pointer selects its item array. Null record paths are planning
slots and do not become selected movies. `artifact` format reads the current
generic artifact record and exact package lock. `bindings` format checks the
native scene snapshot, its catalogue/current lock and scoped bindings. Role
prefixes identify actual selected outputs; they do not mark every input or
review candidate as creatively approved. A record with no matching output role
fails explicitly. Profile selectors must follow the producing project's actual
record contract. Index, record and storage-file paths must stay inside the root.

Projects without this profile retain the existing explicit local storage config
location for compatibility. Accounting requires a profile. This crate currently
reads the existing version/authorization registry layout; it does not recursively
interpret arbitrary evidence files as dependencies.

## Counts and physical storage

Projects can set `require_primary_input_contracts: true` to require classified new version
registrations and selections. [Version-managed publication](asset-version-diff-v1.md) defines
primary inputs, compiled outputs, historical references, the completion receipt, and bounded
post-publication eviction. Legacy records retain their conservative compatibility behavior
until an explicit new immutable version replaces them.

For classified artifact packages, `artifact-verify` freshly hashes the complete
active primary-input/output dependency closure, including transitive inputs not
listed directly in the package. Historical references remain authorized but are
excluded from active verification; no archive lookup or promotion occurs. The
receipt names `verification_scope`, counts `verified_objects` and reports
`deferred_reference_objects` among the package's unique declared assets. Its
`fresh_hash_verified` flag applies to that named scope, not to deferred history.
Legacy packages retain declared-asset verification. Explicit `artifact-locate`
of an archived reference still requires named recovery before active lookup.

`compact-goldens` explicitly retains the exact Golden-history JSON bytes as a
registered cache asset and replaces `registry/goldens.json` with its canonical
Git LFS pointer. Projects should assign that path their normal LFS attributes
and allow it as an asset pointer in repository validation. The Rust reader
accepts existing JSON, hydrated LFS working copies and unhydrated pointers;
pointer lookup verifies registered hash/size in active tiers and never probes M.
The current index payload is protected control metadata in either working-copy
form, and a different backing asset invalidates saved tier-plan fingerprints.
Compaction does not read movies, change Golden locks or run on session entry.
Reference inspection follows the standard LFS pointer as a metadata transport,
verifying the registered body and accounting for both documents in its budgets.

Native `scene-verify` follows the same classified active-closure rule and reports
the same scope/count fields. Scene/binding snapshot validation still covers all
declared bindings, including historical references; fresh media verification
does not read those references. Explicit reference lookup stays archive-free.
`golden-verify` and isolated `golden-restore` use this scope too. A classified
Golden's movie must be declared as a compiled output. Restoring never changes
the current selection or promotes archived provenance.

### Explicit working-set roots

`retention_roots` defaults to `all-current`, preserving existing protection.
Projects can explicitly choose `declared-selections` once their delivery
indexes cover every item they intend to keep hot. This mode requires
`require_primary_input_contracts: true`, complete registry coverage, nonempty
resolved selections and classified selected versions. Missing or stale records,
legacy roots and an empty delivery index fail closed. Selector role prefixes
cannot promote a historical-reference hash to an active deliverable.

Accounting, tier planning and project-aware version comparison then protect the
declared selections and their primary-input/output closure rather than every
metadata-current version. Registry current pointers remain intact for authoring
and history. Active leases, control data, independent consumer protections and
explicit tier pins still apply. This setting neither grants deletion authority
nor infers delivery from folder names; the producing project's index supplies
the exact current locks. A project can include its review shelf by selecting its
records explicitly. Delivery evidence is provider-independent: a project may
use Google Drive Goldens, another distribution destination or an explicit
delivery record as its evidence. Delivery does not imply creative approval.
Do not enable it against an incomplete index.

An artifact selector may name `version_field`, whose value is the item field
containing an explicit immutable version ID. Each selected edition must match
the artifact record's registered package/manifest lock. This supports several
published items from one grouped logical package without changing its single
metadata-current pointer. A missing explicit edition fails instead of falling
back to the latest one. Binding selectors continue to use their exact native
catalogue/current locks and do not accept this option.


The detailed private report lists each observed hash, declared/observed size,
direct version references, current/noncurrent/deprecated/pinned/leased flags and
active-tier copies. Exclusive categories are:

- selected outputs;
- inputs required by selected versions;
- other current working inputs;
- pinned or leased history;
- unprotected deprecated references;
- other retained noncurrent references;
- registry control objects; and
- unclassified objects.

Flags overlap: an old and current version can require identical bytes. Count a
hash once per logical category, rather than adding every version's file sizes.
Deprecating one version does not make shared bytes unused. Historical pins and
leases remain protective. Unclassified objects may belong to another project.

Physical accounting uses volume/file identity and allocation size, so known
hard links are counted once. It reports canonical allocation by class and tier,
independent duplicate canonical copies, known LFS links and additional links.
Windows handles request file attributes, rather than payload read access.
Volume totals/free space and configured reserve headroom are observations, not
job-admission promises; live reservations are not subtracted from those figures.
Do not sum capacity rows that identify the same physical volume.

With `scan_cas` enabled the census is bounded to canonical active-cache
`objects/sha256/<prefix>/<hash>` names. It also inspects configured LFS object
aliases for those hashes. It does not scan whole drives, delivery folders, job
scratch or orphan LFS-only namespaces. Those areas and unrelated files can
explain volume usage outside the reported objects. Without the census, scope is
registered objects only. Payloads are not freshly hashed; wrong-size copies and
missing registered bytes are reported explicitly.

Registered objects always receive physical identity/allocation measurements.
Unregistered objects use directory sizes in the normal census; their allocation
is **unknown**, not zero. Set `measure_unregistered_physical` to true for an
explicit, potentially slow physical census of those objects as well. Category
totals distinguish measured bytes from complete allocation coverage. This keeps
routine project accounting efficient without disguising legacy-cache uncertainty.

The report checks the version generation, authorization inventory and profile
again before returning. A concurrent registry/profile change rejects the result.
The live filesystem census is observational, not an atomic disk snapshot.
New report files publish without replacing existing files. Reports contain
private hashes, paths and identifiers: keep them in the consumer's cache/LFS,
never in this public tooling repository.

`safe_reclaimable_bytes` remains unknown. Global consumer coverage, independent
recovery, hard-link accounting and exact execution-time revalidation are separate
requirements. This command grants no removal, relocation, release or creative
approval. It performs no media deletion or automatic optimization.

## Validation

The extracted custody core retains its synthetic admission, corruption,
concurrency, LFS, immutable-version, scope, restoration and bounded-job tests.
Accounting adds synthetic cases for old/current sharing, protected deprecated
bytes, hard-link identities, independent copies, missing/wrong-size objects,
unknown CAS entries, archive exclusion, selector/snapshot checks and budget/path
failures. CI runs these on Windows and Linux. Consumer adoption additionally
requires real private package/LFS restoration fixtures with a pinned tool revision.

Project-specific authoring web tools and music tooling are separate ports. Their
interfaces should consume generic declarative scene, asset, arrangement and
songbook contracts; private catalogues and creative decisions remain upstream.

## Reusing a song or poem output as a source

The default input/output separation stays strict. A completed song's listening
copy or a poem's reading text can nevertheless be the exact source of another
artifact. Projects may explicitly declare this in
`registry/source-output-reuse.json` (`reel.source-output-reuse.v1`). Each entry
locks one producer version/manifest, one role and hash, and named consumer logical
IDs. Existing immutable packages and contracts are preserved.

Only `song` recording/listening roles and `poem` reading/body roles are accepted.
The registered producer package is freshly hash-checked from configured active
caches; its kind, scope, role and output identity must match the declaration.
Movie/episode outputs are not eligible. Per-version role separation and the ban
on dependencies targeting output-containing versions still apply. Removing a
grant restores the strict cross-version rejection; there is no implicit grant
from a shared hash, filename, current selection, or imported report.

Example entry (synthetic identities):

```json
{
  "schema": "reel.source-output-reuse.v1",
  "entries": [{
    "producer": {
      "logical_id": "song/example",
      "version_id": "golden-v1",
      "manifest_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    },
    "source_kind": "song",
    "role": "listening",
    "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "consumers": ["music-video/example"]
  }]
}
```

The file is bounded to 256 KiB, 64 entries and 64 named consumers per entry. Loading it
reads metadata only: it starts no render, hydration, archive recovery or deletion.
It grants no creative, likeness, voice, licensing or publication approval.
