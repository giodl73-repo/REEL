# Managed inventory-index producer and read-only shadow commands

Status: opt-in Rust library and commands in the existing `reel-assets` binary.
No installed runtime, project binding, cache tier policy or existing producer is
switched automatically. The commands transform a named existing inventory asset;
they do not generate a census, render, hydrate media, start a daemon or clean caches.

## Explicit commands

```text
reel-assets --root <project> inventory-index-init <private-profile.json>
reel-assets --root <project> inventory-index-build <private-profile.json>
reel-assets --root <project> inventory-index-shadow <private-profile.json> <exact-subjects.json>
reel-assets --root <project> inventory-index-recover <private-profile.json>
```

`init` explicitly initializes a new catalog and key, or opens the exact existing
catalog and creates/reads the key without resetting its revision. The metadata
directories must already exist; no cache root is created. Missing manifests in
nonempty catalogs remain errors. Initialization and catalog recovery do not probe
the source asset, so an offline named source does not silently trigger recovery,
archive fallback, source copying or a new census.

`build` checks catalog scope and expected revision before opening the source.
It verifies/prepares the exact encoded source asset, acquires the shared capacity
allowance, publishes the index, seals staging, hands off verified retained links,
refreshes candidate proof and admits the expected catalog revision. Source bytes
are unchanged. Staging stays protected after success; retirement is a separate
explicit API/workflow. No source inventory or media is copied into Git.

`shadow` compares explicit exact-subject queries against the verified original
source and currently selected index. It checks listed/captured-nonmembership
results, every preserved row/copy field, original source ordinal, tiers, scope,
capture interval, profile identity and registry generation. It verifies that the
catalog revision/root stayed unchanged. Duplicates, invalid/empty subjects,
over-budget requests and mismatches fail without a success report. The command
is read-only, including on failure. The build profile's expected revision is a
build CAS precondition; shadow observes and reports the current selected revision.

This establishes parity for the requested adapter observations, not all original
diagnostic fields, unqueried subtrees, current physical media custody or planner
decisions. Captured nonmembership remains distinct from current absence. A
validly hashed but wrong derived observation fails shadow even when its source
binding and internal closure are consistent. That distinction matters because
source binding alone is not an independent source-inclusion proof.

`recover` explicitly invokes one catalog recovery step; it does not resume a
producer, start a daemon, replay a render or retire a session. A failed build may
leave bounded protected staging/retained nodes or an uncertain catalog outcome.
Inspect/recover the exact key rather than forcing a revision or overwriting a
session. A new attempt needs an explicitly chosen session ID and current expected
revision. There is no automatic retry, rollback, orphan sweep or age-based expiry.

## Private declarative profile

Schema: `reel.inventory-index-job.v1`. Unknown fields and unsupported source
schemas/encodings fail. Profiles and storage config are each limited to 64 KiB;
exact-subject files are limited to 4 MiB with additional count/work bounds.
Relative profile, storage-config and query paths resolve beneath `--root` without
parent traversal. Cache and metadata directory paths are explicit absolute paths.

The output cache must match a configured active root; the named source cache
must match a configured active/archive root. Only the named output root is
canonicalized in the local config copy, preserving its tier-specific free-space
floor. Unrelated roots are never probed. The source's named cache is opened only
for build/shadow; the object path is the existing `objects/sha256/<prefix>/<hash>`.
It is not a new resolver mode and does not change the project's LFS tier order.
Reported tier paths inside the source are data, never input-file locations.

Staging, retention and catalog directories must be separate existing children of
the selected cache on the same filesystem. The shared coordination directory
must already exist in storage config. Source schema, project, namespace and exact
encoded/decoded descriptors are externally admitted controls, not guessed from
filenames or read from a self-authorizing source. Identity JSON and explicit gzip
use the existing native `reel.asset-cache-report.v1` producer shapes.

Illustrative private profile; all names/hashes are placeholders, not actual assets:

```json
{
  "schema": "reel.inventory-index-job.v1",
  "storage_config": ".local/storage-rust.json",
  "cache": "C:/existing-cache",
  "staging_parent": "C:/existing-cache/index-staging",
  "retained_parent": "C:/existing-cache/index-retained",
  "catalog_directory": "C:/existing-cache/index-catalog",
  "catalog_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "key": "cache-inventory/current",
  "expected_revision": 0,
  "session_id": "inventory-001",
  "source": {
    "schema": "reel.asset-cache-report.v1",
    "project_id": "example",
    "authority_namespace": "example-private",
    "adapter_version": 1,
    "encoding": "identity",
    "cache": "C:/existing-source-cache",
    "encoded": {"sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "bytes": 4096},
    "decoded": {"sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "bytes": 4096}
  },
  "budgets": {
    "encoded_bytes": 67108864,
    "decoded_bytes": 134217728,
    "objects": 100000,
    "copies": 200000,
    "node_bytes": 262144,
    "index_bytes": 67108864,
    "index_nodes": 4096,
    "leaf_rows": 128,
    "shadow_subjects": 4096,
    "shadow_bytes": 268435456,
    "shadow_nodes": 16384
  }
}
```

Budget fields default to the existing adapter/builder limits; the example chooses
a smaller explicit build allowance. Optional `staging_overhead_bytes` defaults
to 8 KiB per maximum index node. The shared reservation additionally includes
payload allowance and bounded controls. This is an allowance, not actual written
bytes or exact filesystem allocation; choose adequate overhead for the filesystem.
Shadow byte/node costs are aggregate over all subjects, including repeated chunks.
The original admitted source asset must already exist; source ingestion, LFS
registration and original-source retention remain the existing asset workflow.

## Privacy, validation and adoption

Successful stdout is a count/boolean-only projection plus a fixed schema name.
No root/source hashes, paths, key, project or authority identity are serialized.
Profiles, catalog state, source evidence and stderr diagnostics remain private.
Builds explicitly report zero media payload copies and no removal authorization.

Fourteen executable integration cases use actual native census/named producers
and synthetic temporary caches. They cover JSON/gzip, source preservation,
read-only parity/failure, stale CAS preconditions, malformed profiles, partial
publication, offline/unrelated archive handling, tier-floor preservation,
concurrent producers, privacy and a simulated faulty trusted transformation.
Explicit profiles ignore legacy transfer-cache environment overrides; commands
also run with no external executable search path, exercising the no-tool/no-daemon boundary.
Filesystem snapshots verify shadow/recovery do not alter fixture files.

All 409 suite tests pass locally: 212 unit, five prior integration, 100 model,
23 source-adapter, 25 sidecar, 30 backend and 14 managed executable cases.
Strict all-target Clippy, formatting, ROLES structure and diff checks pass.
Validation ran on Windows; this is not device/power-loss durability proof.

Next adoption work is explicit project configuration and shadow comparison of
planner decisions after combining staging/retained/job/media protection coverage.
These commands do not connect a removal executor or replace the shared installed
binary. Complete source ingestion streaming, deadline/cancellation, device-level
durability and history compaction remain separate work.
