# Combined collector protection and planner shadow

The optional Rust collector combines positive demand from explicitly declared
cache metadata and project consumers. It does not discover every project,
authorize removals, copy payloads, start jobs, or change installed runtime bindings.

## Operator boundary

Run `reel-assets --root <project> collector-shadow <private-job.json>`.
The job is private, limited to 64 KiB, and rejects unknown fields:

```json
{
  "schema": "reel.collector-shadow-job.v1",
  "policy": "policy.json",
  "census": "baseline.json",
  "protection": {
    "schema": "reel.collector-protection-scope.v1",
    "caches": [{
      "cache": "C:/existing-cache",
      "staging_parent": "C:/existing-cache/index-staging",
      "retained_parent": "C:/existing-cache/index-retained",
      "catalogs": [{
        "directory": "C:/existing-cache/index-catalog",
        "project_id": "example",
        "authority_namespace": "example-private",
        "catalog_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
      }],
      "job_parent": "C:/existing-cache/production-jobs"
    }],
    "consumers": [{
      "root": "C:/example-project",
      "policy": "C:/example-project/policy.json"
    }],
    "limits": {}
  }
}
```

Cache and metadata directories must already exist. Metadata parents must be
disjoint, checked children of the selected cache on the same filesystem.
`job_parent` is optional. Consumer roots/policies and cache scope paths are
explicit absolute paths; the outer job's policy/census can be root-relative.
No directories are initialized by this command. The existing native planner's
configuration semantics, including its transfer-cache environment handling,
remain in effect. This differs from the explicit managed-index producer profile.

Successful stdout contains only a fixed schema plus counts and booleans:
`reel.collector-shadow-summary.v1`, object/protection/held/unchanged counts,
`reference_parity`, `dry_run: true`, `removal_authorized: false`,
`payload_bytes_copied: 0`, and `declared_scope_only: true`.
Profiles, identities, paths, hashes, reason sets and diagnostics remain private.
There is no `--apply` or implicit cleanup.

## Protection semantics

The union includes native selected inputs/outputs, working and pinned state,
version job leases, registry controls and tier pins from every declared consumer;
staged index nodes; complete retained index roots, including historical roots;
current catalog index closures; and required encoded source evidence.
Inventory-observed media does not become selected media merely by appearing in
an index. Historical media alone does not become hot demand.

Optional native production-job workspaces contribute declared inputs and CAS
tools from validated `job.json`, plus separately classified published outputs
from a matching `result.json`. Compiled outputs never become build inputs.
Installed tools and scratch/output payloads are not opened or recursively scanned.
Unpublished outputs cannot be inferred. A workspace continues to protect its
recorded demand until its ordinary explicit lifecycle retires it; no PID or
expiry guess grants removal permission.

Partial staged nodes count as intent, not verified backups. Retained closures
must verify exact node identities, counts, bytes and receipts. Required source
identity is demand, not proof of an independent retained copy. Invalid, missing,
partial, unsupported or contradictory required controls fail capture; they never
produce an empty successful scope. Unknown hashes return `OutsideDeclaredScope`,
not zero references or deletion permission.

Capture compares two passes and catalog revisions. Revalidation detects changed
controls, directory membership, native consumer state and catalog revisions.
Readers acquire no staging-writer ownership or global cache lifetime lock.
This is a scoped observation, not a lease against changes after return. Declared
consumers can omit another worktree, so even an internally complete capture
cannot establish global completeness. Holds conservatively apply across declared
caches by hash; this is not a precise placement-affinity optimizer.

## Planner comparison

The collector runs the existing native tier planner once, then restricts its
result: held demotion/archive proposals become holds, with no replacement batch
candidate. Other rows remain unchanged; estimated totals are recalculated.
This is a restrictive overlay of the reference planner, not an independently
implemented second planner. `reference_parity: false` means additional holds.

The private library result uses `reel.asset-collector-shadow-plan.v1`, explicitly
declares non-global scope/no removal authority, and cannot pass the existing
executable-plan schema check. The CLI exports only the count summary. Neither
result supplies custody proof or permission to remove a file.

## Bounds and validation

Default limits are 32 scopes, 4,096 sessions/retained roots per namespace,
100,000 read files and separately 100,000 enumerated entries, 64 MiB aggregate
metadata, 256 KiB per index node, 1 GiB aggregate index data, 100,000 graph nodes,
one million objects/holds and 10,000 catalog keys. Limits apply per collection
pass; captures/revalidation perform multiple passes. Catalog costs accumulate
with staging/retained work. File/byte limits are checked before reading payloads;
directory names also consume metadata allowance. Native job controls have a
1 MiB individual bound. Bounds are not exact RSS, physical I/O or time promises.
Native consumer resolution retains its existing engine limits: its top-level
controls are metered here, but every internal registry read is not charged to
this collector's reader budget. Deadlines/cancellation remain future work.

Fourteen collector unit cases and three executable cases cover shared consumers,
job inputs/outputs, corrupt/missing evidence, budgets, live staging without writer
ownership, retained history, catalog changes, no batch refill, privacy and
read-only failures. Fixtures use synthetic temporary caches on Windows.

Live adoption still requires explicit project/all-consumer enrollment, complete
provider coverage, measurement on representative private evidence, and an
execution integration that fences changing demand and independently verifies
retained custody before unlinking. This increment performs no live cache census,
movement, reclamation, rendering or installed-runtime update.
