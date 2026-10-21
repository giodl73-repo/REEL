# Verified redundant hot-copy removal

This explicit Windows command supplements the read-only tier planner:

```text
reel-assets --root <project> tier-demote <request.json> <absolute-owned-journal>
reel-assets --root <project> tier-demote <request.json> <absolute-owned-journal> --apply
```

Default mode verifies and journals without removing payload links. Apply is
explicit, names a bounded exact-hash set from a fresh saved historical proposal,
and removes only redundant hot CAS/configured LFS links. It never moves a last
copy, copies new payloads, accesses archive roots or permanently retires history.
There is no path-based deletion fallback on unsupported platforms.

Compatible read-only older consumers, protection exports and compacted control
indexes are described in [version index v2](asset-version-index-v2.md). Default
verification reports admission presence; actual apply still holds on admissions.

The strict `reel.asset-demotion-request.v1` record contains `policy` and `plan`
paths, exact `plan_sha256`, absolute `consumer_roots` including the invoking
project, `consumer_scope: all-managed-cache-consumers`, `sha256s`, positive
`max_verify_bytes` and `max_seconds` (at most one hour). Hash verification budgets
cover both source and retained copy. Batch/object limits also come from tier
policy/storage admission. Optional `allow_same_volume_copies` defaults false;
enabling it permits distinct physical copies on one volume and never claims
independent backup. Synthetic isolated fixtures use that option deliberately.

## Protection and concurrency

All declared consumers must have a supported asset project profile and matching
active-cache/shared-coordination bindings. Selected closures, other current work,
leases and registry control objects protect a hash in any consumer. Missing or
unsupported consumer metadata holds the operation, not just that consumer.
The caller must enumerate all managed active consumers. In a Git worktree, local
worktree registration is reconciled and rechecked before unlink: each registered
worktree must be active or explicitly listed in `inactive_consumer_roots` as
completed/quiescent. A newly created undeclared worktree holds maintenance.
This is not global discovery of unrelated repositories. Legacy consumers require a compatible protection export
or scoped reader before inclusion can succeed. Do not silently omit a live lane.

Apply refuses any live or unreconciled admission entries. The shared job runner
now reserves before resolving sources, avoiding source paths captured before a
maintenance admission boundary. Before production rollout, participating readers
must use that protocol or be quiescent; old binaries do not acquire new behavior
from a Git merge. Existing jobs are never stopped or their reservations cleared.

Hashing happens outside the short coordination lock. Immediately before each
object's removals, the command rechecks admissions, plan expiry, policy/request
and consumer metadata tokens. On Windows it holds those control files against
write/replace while removing the exact opened source links. Unknown hard links,
independent LFS copies, readonly copies, unexpected types, reparse ancestors,
corrupt/missing destinations and changed alias counts all hold/fail closed.

The verified retained file remains open denying write/delete until removal and
journaling finish. Source bytes are freshly hashed with a time bound and held
against writes. Windows deletion uses the opened handle, not a later name lookup;
a replaced path cannot redirect that deletion. See Microsoft's
[SetFileInformationByHandle contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfileinformationbyhandle)
and [POSIX disposition semantics](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddk/ns-ntddk-_file_disposition_information_ex).

## Journals and verification

Journals must be explicit absolute directories inside the invoking repository's
`.local` namespace, outside caches, with normal non-reparse ancestors. A journal
lock excludes concurrent applies. Request identity, verified retention proof,
per-link intent and completion are immutable, durably written before/after
unlink. Publication refuses differing existing records. Interrupted temporary
files are not authoritative.

An explicit repeat of the same request/journal freshly verifies the retained
copy and current protections. It observes missing links without deleting them
twice; interrupted intents get separate recovery records. Changed/expired plans
stop, requiring a fresh request rather than relaxing checks. Earlier receipts
remain intact. Unexpected observation errors never count as absence.

Receipts distinguish link counts, estimated removed allocation, observed volume
free-space delta and its nonexclusive attribution. Other activity and open
handles may affect when space becomes available. Retain journals/results in the
consumer's private cache/LFS; only compact control facts belong in ordinary Git.

Windows fixtures exercise actual handle deletion, CAS plus LFS aliases,
idempotence, interruption after a durable intent/unlink, stale controls, late
admission, denied writer/replacement access and readonly protection. Linux verifies
read-only behavior and explicitly rejects destructive apply.
