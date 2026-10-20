# Project asset custody and cache accounting

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

## Project profile

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
