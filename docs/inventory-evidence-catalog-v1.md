# Revisioned inventory-index catalog and collector facts

Status: optional Rust library in `sidecar::file_store::catalog`. It selects exact
inventory evidence indexes, not episode/media Goldens. It does not run an
existing collector, evict data, alter project/runtime bindings or start a daemon.

## Scope, admission and layout

The caller supplies an existing empty private metadata directory inside its selected
cache and an expected `Scope` with project, authority namespace and a unique
64-hex catalog instance ID. Reuse that ID only for the same lineage. This is
externally admitted configuration, not authority discovered in source files.
`Catalog::initialize` is explicit; `open` verifies expected scope. Neither
recreates a missing catalog or searches Git/cache/archive directories.

All mutations require a `MutationContext` with the existing storage config,
project root and filesystem overhead allowance. The explicit shared coordination
directory must match initialization. Each operation reserves up to eight bounded
64 KiB controls plus declared overhead (at least 64 KiB), using the existing
capacity ledger. Its shared transaction lock is released before root hashing.
Keys have independent ownership; no global catalog/render/cache lifetime lock.

`Candidate::verify` requires the exact caller-admitted root/source and verifies
the whole retained index and compact protection receipt. `admit` refreshes that
verification and checks project, namespace, schema, adapter and cache scope before
performing a revision compare-and-swap. Source binding and derived consistency
remain distinct from independent source inclusion, original-source custody or
human creative authority. Retained verification now allows shared readers;
handoff publication alone needs exclusive retained-root ownership.

Private storage contains a compact manifest, immutable membership tickets and
one slot directory per hashed logical key. Each slot has a small replaceable
head plus immutable numbered fence, decision and state controls. Original
inventory bodies and index chunks are referenced, not copied into records.
These controls belong in cache metadata storage, never a large Git registry or
shareable scorecard. Private snapshot/plan/source types have no automatic export.

## Revisions and recovery

`create_key` establishes known revision zero with no selected root. `admit` and
`tombstone` require the caller's exact expected revision and consume a new one.
Even repeated equal selections advance the revision. Tombstones keep the slot,
membership and revision history, preventing delete/recreate ABA. Existing-key
creation never resets it. Different keys proceed independently; same-key
concurrent writers conflict rather than silently reconciling.

For each successor, the writer persists an immutable fence bound to the exact
predecessor state, then an immutable commit decision, then the new immutable
state. Only afterward does it atomically replace the head with the existing
platform replacement helper. Reads validate the head's state and immutable
decision/predecessor references in bounded work without replaying all history.
A newer fence or its pending record makes the key unavailable for ordinary
projection until explicitly recovered; missing/corrupt heads do not mean empty.

`recover(context, key)` handles one successor explicitly. A complete valid
decision, including complete pending decision bytes, is replayed after refreshing
retained protection. If no decision exists, an immutable abort consumes that
revision and preserves the predecessor root. A complete pending fence can be
published and aborted this way. Truncated/ambiguous controls fail untouched for
explicit reconciliation; they are never silently replaced, discarded or turned
into an abort. If a decision changes during recovery, the caller must retry.

The successor fence prevents reuse after a lost or rolled-back head. A missing
head or interrupted initial key creation remains an error requiring scoped
reconciliation; no highest-file guessing or automatic startup repair. If several
known revisions follow a rolled-back head, recovery advances one successor per
call and ordinary read still errors while a later successor remains. There is
no history deletion/compaction API. Complete destruction/rollback of the whole
lineage cannot be detected without an external durable anchor; this is not
tamper-proof storage or a claim of power-loss durability.

## Collector-facing positive facts

`catalog::protection::collect` verifies catalog membership/slot coverage, reads
current revisions, verifies every current retained index, then repeats the
membership/revision read. Changed, missing, corrupt, pending or over-budget
evidence returns an error; no partial projection is emitted. Membership tickets
and slots cross-check, so losing one family does not silently drop a key.

The private `ProtectionSnapshot` classifies exact hashes as `ProtectedMetadata`
or `OutsideCatalogScope`. The protected set is the selected index-node closure,
not the media hashes listed in inventory rows. Original encoded source blobs are
reported separately as required provenance, without inventing local verified
custody. Different retained aliases/paths are not independent backups.

Absence never becomes an unprotected/zero-reference/removal result. Historical
retained anchors, staging, other projects/catalogs, jobs, media Golden/draft
roots and external pins are outside this projection. A production collector must
combine these other authorities and scope proofs separately. Tombstoning a key
changes only its catalog reference; it does not remove retained roots or payloads.

`snapshot.revalidate(catalog)` repeats full verification and compares exact
membership, revisions and descriptors. A new key or changed/tombstoned root
invalidates the old snapshot, even if its final binding looks like an older one.
This is not a deletion lease or a fence against a writer acting after return;
no cleanup executor is connected by this increment.

Projection defaults are 10,000 keys, 64 MiB catalog metadata, 256 KiB per index
node, 1 GiB total index bytes, 100,000 visited nodes and one million observed
objects. Ceilings are 100,000 keys, 256 MiB metadata, 4 MiB per node, 4 GiB index
bytes, one million nodes and one million objects. Catalog metadata is charged
before each bounded read across both passes; index work is charged across roots.
Shared chunks deduplicate the positive result, not the cost of repeated visits.
Ordinary per-key reads have a 12-control byte budget; each control is at most
64 KiB. These are logical byte/work bounds, not exact RSS, physical-I/O or time
guarantees. Verification can reread bytes; cancellation/deadlines remain future work.

## Tests and remaining adoption

Catalog fixtures exercise revision conflicts, tombstones/recreation, independent
keys, rollback/missing-head rejection, committed/aborted recovery, complete and
partial pending controls, changed source/scope/retention, positive-only media
classification, stale projection rejection, missing membership and budget failure.
Two real child processes race on one key; two update separate keys successfully.
A Windows sharing failure blocks head replacement after the immutable decision;
the writer is killed and explicit recovery preserves its committed root. This
is process/error recovery, not simulated power failure. Only synthetic temporary
caches are accessed.

All 395 suite tests pass locally: 212 unit (including 17 catalog cases), five
prior integration, 100 model, 23 source-adapter, 25 sidecar and 30 backend cases
(including three catalog process cases). Strict all-target Clippy, formatting,
ROLES structure and diff checks pass. Windows branches are exercised here;
Unix-specific filesystem/durability behavior is not established by this run.

Before live adoption, wire managed producers, configure explicit catalog scope,
combine staging/retained/job/media protection into collector coverage, and run
shadow comparison against existing plans. Platform/device durability, checkpoint
compaction, exact allocation and receipt/registry-format adapters remain separate.
Existing production collectors remain unchanged and receive no removal authority.
