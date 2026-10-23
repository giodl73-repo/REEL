# Existing-cache inventory index storage

Status: opt-in library implementation in
`inventory_evidence::sidecar::file_store`. No production configuration, shared
runtime, CLI, collector or planner is changed. This backend uses the existing
`objects/sha256/<first-two-hex>/<sha256>` layout. It creates no cache root and
does not choose a drive or discover project paths.

## Writer and query boundaries

`FileStore::begin(cache, staging_parent, session_id, limits)` requires explicit
absolute existing directories. The staging parent must be inside the selected
cache on the same filesystem. Session IDs are bounded single components.
Symbolic links/reparse points, including Windows directory junctions, are
rejected along checked paths. Directories must be caller-owned trusted storage;
these checks are not an adversarial filesystem sandbox against path swaps.

The writer creates its own session directory and holds exclusive ownership of
that session only. Other sessions and exact read-only queries continue. There
is no shared cache lock, lifetime render reservation or background process.

`ReadOnlyStore::new(cache, node_bytes)` opens only exact requested CAS names,
verifies regular-file type, length and content identity, and never publishes.
Use it with `sidecar::lookup` and the caller-admitted exact header/source.
It does not resume a writer, read staging directories or enumerate the cache.
The backend verifies a bounded chunk before returning a reader; the sidecar
also checks the bytes it consumes. Query counters count logical node reads,
not physical I/O or this verification overhead.

## Staging protection and publication

Each session contains:

```text
<explicit existing staging parent>/<session id>/
  owner.lock       per-session ownership only
  session.json     create-once private scope and budgets
  objects/<sha256> protected complete or interrupted staged node
  root.json        optional create-once completion marker
```

New node bytes are written to a private staged file, synced and verified before
an atomic no-overwrite hard link publishes their CAS name. Existing CAS nodes
are verified and hard-linked into staging rather than copied again. A corrupt
existing winner is preserved and rejected. New concurrent duplicate writers
can briefly create separate identical staged inodes; the backend does not
silently replace either one to deduplicate them. There is no large media copy.

The protected staged hard link retains bytes even if a CAS alias disappears.
It is the same volume/inode, not independent backup custody. Reads fail on a
missing CAS name; they do not silently recover from staging. Exact publication
retry after explicit session recovery can reestablish the missing CAS alias.
Dropping or killing a writer does not remove staged data or CAS names.

After `sidecar::publish` returns the header, `writer.seal(&header)` records
completion and prevents further writes. A repeated identical seal is accepted;
another root rejects. A seal is not catalog admission, closure verification,
selection, a root handoff or removal permission. All staged hard links remain.
There is deliberately no automatic staging retirement or age-based expiry.

## Bounds and interruption

Default per-session limits are 256 KiB per node, 1 GiB total logical staged
bytes and 100,000 unique nodes. Ceilings are 4 MiB per node, 4 GiB staged and
one million nodes. Duplicate nodes in one session are charged once. Limits
reject before opening a new staged node; existing exact duplicates remain
usable within a full session. Controls are bounded to 64 KiB each.

These are logical per-session limits, not filesystem allocation measurements
or a shared-volume free-space reservation. Callers must arrange shared-volume
capacity admission, including session/file overhead and concurrent producers,
before production use. A hard link is counted as a protected logical object,
not another independent physical copy. No shared-volume reserve is invented by
this API.

A failed publication poisons that writer; it cannot continue writing or seal.
`FileStore::resume` is an explicit operation that takes the named session's
ownership, validates scope/controls, enumerates only that session's staged nodes,
checks budgets and verifies all their identities. It never enumerates cache
payloads or archives. Recovery is bounded by the persisted limits but may read
the whole staged index. Ordinary queries use `ReadOnlyStore` to avoid that cost.

An incomplete staged file or control fails recovery and remains untouched for
explicit reconciliation. A complete staged node interrupted before CAS
publication can be published by exact retry. Resume does not rewrite files,
repair partial nodes or raise budgets. Session controls are create-once through
this API under trusted filesystem ownership, not signed tamper-proof records.

File data is flushed. Unix directory namespace barriers are also issued;
portable Windows directory flush is not implemented. Process-kill recovery
tests are not power-loss, device-failure or remote-filesystem durability proof.
Unsupported hard-link/flush operations fail; there is no fallback copy or
overwrite strategy.

## Verification and adoption

`tests/inventory_file_store.rs` uses isolated temporary caches only. It tests
index publication/sealing/reopening, exact read-only lookup during writer
ownership, separate concurrent writer processes, process termination after
publication and reacquisition of that session, partial-file recovery rejection,
complete-stage exact retry, corrupt existing winners, missing CAS aliases,
budget exhaustion/recovery, scope mismatch and Windows junction rejection.

All 16 backend suite tests pass locally, including two concurrent writer child
processes and a third writer killed after publication. The complete reel-assets
suite passes 361 tests: 195 unit, five prior integration, 100 model, 23 source
adapter, 22 sidecar and 16 backend. Strict all-target Clippy, formatting, ROLES
structure and diff checks pass. This is a Windows local-filesystem validation;
the Unix-specific symlink/directory-sync paths are not validated by this run.

Before live adoption, integrate shared capacity admission and an exact source/
root catalog with revisions. Collectors must understand staged protection and
preserve it through root admission; release only after an independently verified
handoff with bounded explicit retirement/recovery. Existing collectors are not
updated by this increment. Automatic cleanup would be unsafe until that contract
is implemented. Shadow planner parity, time/cancellation, physical accounting
and platform/device crash validation remain separate work.

Private session controls and index chunks belong in metadata-asset storage,
not Git or shareable scorecards. No raw project data is included in these tests
or documentation, and no serialization/export of private observations is added.
