# Per-asset publication fence

This opt-in Rust library primitive coordinates a short publication/removal
critical section for one asset or a bounded batch. It does not authorize
removal, prove custody, enroll producers, or change the existing executor.
It addresses the race between a final demand check and concurrent publication;
snapshot revalidation alone cannot make that boundary atomic.

## Control and API

`collector::fence::Domain::initialize(existing_directory)` explicitly creates
one `asset-publication-fence.v1.lock` control containing a fixed 32-byte schema
header. It creates no directories, cache or per-asset file. Existing controls
are checked without overwrite; missing/partial/corrupt controls are not repaired.
`Domain::open` only opens an existing control. The caller must choose a trusted
local metadata directory, separate from enumerated staging/retained/catalog
namespaces. No CLI, daemon, default discovery or installed configuration is added.

`Domain::acquire(subjects, Mode::Publish | Mode::Remove, wait)` returns an owned
guard. Publish uses shared Windows byte-range locks; Remove uses exclusive locks.
Publishers of the same asset can overlap, while removal excludes participating
publishers/removers of that asset. Other assets proceed independently, except
conservative prefix collisions. The domain itself holds no subject lock.

Each validated lowercase SHA-256 maps its first 64 bits, masked to 62 bits,
to a one-byte range starting at `4096 + prefix`. Locks beyond EOF do not extend
or populate the file. The mapping is part of this control schema. Collisions
add contention; they never drop protection. Batches reject duplicates, empty or
invalid subjects and more than 512 subjects, deduplicate collided offsets, and
acquire offsets in sorted order. A failed acquisition releases earlier ranges.

Acquisition is nonblocking by default; explicit retry waits are limited to two
seconds for the whole batch. There is no lock stealing, expiry or stale-owner
reset. Dropping the guard or process termination releases its OS locks. The wait
bound is not a maximum guard lifetime or a hard cancellation guarantee for
blocking filesystem calls. Callers must keep critical sections short; never
hold these guards over renders, media copies/hashing or whole graph traversal.

The control's file identity and ancestor directories are pinned against Windows
replacement while opened. Acquisition checks identity/header again. Trusted
metadata ownership is still required; this is not protection from an adversary
with arbitrary writes or whole-lineage rollback. Different directories or
different schema mappings are different domains and cannot coordinate.

Only Windows is implemented. Other platforms fail explicitly, with no global
file-lock fallback. `Win32_System_IO` is an additional feature of the existing
windows-sys dependency; no new crate, service or runtime executable is added.

## Index publication opt-in

`FileStore::publish_fenced(&domain, bytes)` requires an initialized domain inside
that store's cache and obeys the existing node-size limit. It acquires shared
protection for the node before staged/CAS publication, then uses the existing
verified publication path. The bounded node write/verification happens inside
this short guard. Busy-fence refusal occurs before publication and does not
poison the writer, so an explicit retry can succeed later. Unrelated nodes can
publish while another node is exclusively fenced.

Ordinary `publish`, managed producer commands, native jobs, retained receipts,
catalog updates and the removal executor retain their existing behavior. This
explicit per-call API does not persist session-wide fence enrollment and cannot
prove every producer used it. No code may treat the primitive alone as complete
coverage or permission to unlink a CAS name.

## Remaining integration contract

Before guarded removal can adopt this mechanism:

1. Persist authoritative domain/schema enrollment for every participating cache
   and producer; compatibility checks must reject unfenced publication paths.
2. Establish overlapping demand before source reads or root publication,
   including native jobs, retained source-evidence demand and catalog lifecycles.
3. Acquire removal protection before the final bounded demand check and exact
   unlink, while preserving the existing native worktree/selection controls and
   independently verified retained-copy handles.
4. Define one acquisition order across domains/worktree controls. A timeout
   holds the operation; it never relaxes custody or demand rules. Expensive
   graph/physical verification must precede that short commit boundary.

These are integration gates, not guarantees provided by this primitive. The
current collector/preflight remains advisory, and existing cache removals do not
automatically honor new fences. No production runtime adoption occurs here.

## Validation

Nine synthetic Windows integration cases plus a subprocess harness cover shared/exclusive behavior,
unrelated-asset progress, partial-batch rollback, bounded waiting, conservative
collisions, high offsets without file growth, invalid/missing/corrupt controls,
control replacement prevention, idempotent initialization, killed publisher and
remover recovery, and retryable index publication with cache/node bounds.
Two real child processes are terminated while holding ranges; surviving owners
keep their protection and released subjects can be reacquired. Tests access
only temporary fixture directories, with no live cache, media or runtime change.
