# Capacity-admitted index handoff and staged-link retirement

Status: optional Rust library APIs over existing caches. No live collector,
current-selection catalog, runtime pin or CLI is changed. This retires index
staging names only; it does not evict media or reclaim its payload allocation.

## Shared capacity

Managed producers use `FileStore::begin_admitted(project_root, config, cache,
staging_parent, id, limits, overhead_bytes)`. The existing active-cache config
must explicitly name an existing shared `coordination_root`; worktree-local
defaults are rejected. The existing capacity ledger reserves the whole build
allowance before session creation. Its shared lock is released before hashing
or writing. Independent writers hold only their own capacity lease and session
ownership, and different worktrees see the same shared-volume reservations.

The reserved allowance is maximum staged payload bytes plus declared filesystem/
retained-link overhead plus 512 KiB for bounded controls. Overhead must cover at
least 4 KiB per maximum node. This is a defensive floor, not measured allocation;
projects must allow for actual cluster sizes, metadata and concurrent jobs.
Overflow, missing coordination, unconfigured destination and capacity denial
fail explicitly. Denial creates no session. Written staging continues to consume
actual disk space after a normal lease release.

A create-once private `capacity.json` binds the shared ledger and allowance.
`resume_admitted` reacquires that same contract. Low-level `resume` can inspect
a managed session but cannot mutate it without admission. Low-level `begin`
remains for fixtures/explicit externally admitted integrations; this is not a
security boundary against deliberate API bypass. A killed process's reservation
stays conservative until the existing explicit `admission::release_stale` check
confirms ownership is gone. Nothing expires or steals a live reservation.

## Complete verification and handoff

`sidecar::validation::validate(store, root, source, read_limits)` walks the whole
index. Here `read_limits.rows` is the total object budget. Every identity, schema,
prefix, row/copy/tier invariant, ordinal and total object/copy count is checked.
Ordinals must be unique across leaves. Missing off-path nodes, repeated nodes
and cycles fail. The private sorted descriptor set supplies a closure fingerprint,
not a new large Git registry. Ordinary queries still read only their selective path.

This is derived-index consistency/integrity checking, not an independent proof
of inclusion in the original monolithic source or authority to select current
assets. Exact root/source admission from a trusted builder and original-source
custody remain separate obligations.

After publication and sealing, `writer.handoff(retained_parent, root, source)`
verifies the complete CAS closure and exact session staging set. Unknown staging
rejects. The explicitly existing retained parent must be separate from staging,
inside the same cache and on the same filesystem. Cache/staging filesystem
identity is also checked.

Verified no-overwrite retained hard links are created under
`<retained_parent>/<header-sha256>/objects/<node-sha256>`. Only after they all
verify is a compact immutable `root.json` atomically published, binding exact
source/root, node/byte counts and closure fingerprint. These private controls
and nodes belong in cache metadata assets, not Git or shareable scorecards.

Retained roots are append-only protection anchors, not mutable current selections
or a revisioned catalog. Repeated handoff is idempotent. Interruption may leave
partial retained links; staging remains, and exact retry can finish. Corrupt
winners or partial controls fail without overwrite/repair. Ownership is per exact
retained root, never a global cache lock. Same-volume hard links are protection
against removing a name, not independent backup custody or disk-failure recovery.

## Dry run and bounded execution

`writer.plan_retirement(parent, root, source)` is read-only. It validates the whole
CAS closure, retained receipt/files and exact named session's staged set. It
rejects unknown/changed scope and creates no file, marker or directory. Existing
ownership locks coordinate inspection without changing their contents.

The private plan binds exact session, source, root and retained closure. Its
node count and logical bytes cover the full index; execution separately reports
remaining staged names. `payload_bytes_reclaimed()` is zero because retained
links keep the payload allocated. Small metadata savings are not estimated.

`writer.apply_retirement(&plan, max_links)` revalidates before the first unlink,
atomically records the exact private `retirement.json` intent, then rechecks each
retained counterpart immediately before unlinking only its verified staged name.
It never removes CAS/retained names, source evidence, controls, directories or
another session's data. `max_links` bounds each block; no recursive/age cleanup.

After a partial block or process death, explicit named recovery allows missing
staging only with a valid persisted intent bound to the sealed root. Replan and
apply the exact remainder; completed retries remove zero more links. Missing or
corrupt retained evidence stops execution and preserves remaining staging.
Incomplete controls remain errors requiring explicit reconciliation. Compact
session/control history remains; its future compaction is separate work.

## Validation and adoption

All 375 suite tests pass: 195 unit, five prior integration, 100 model, 23 source
adapter, 25 sidecar and 27 backend. Strict all-target Clippy, formatting, ROLES
structure and diff checks pass. Fixtures cover shared capacity across worktree
roots, competing reservations, scope/overflow/denial, managed recovery guards,
whole-closure counts, cross-leaf duplicate ordinals, missing off-path evidence,
strict dry-run behavior, partial handoff, unknown staging, changed retention and
cross-session plans. A real child is killed after five of 18 staged names are
removed; recovery retires the remaining 13 and the index still queries. Other
sessions remain usable. Only isolated synthetic temporary caches are accessed.

Before live adoption, wire explicit managed producers and collector protection,
implement the source/current-root catalog with revision semantics, and establish
shadow planner parity. Existing collectors are not upgraded by these APIs;
they must honor retained/staged protection. Windows power-loss namespace
durability, hostile-filesystem path-swap resistance, time/cancellation and exact
physical allocation remain unproved. Receipt/registry-format adapters remain
separate. No daemon, live-cache movement or automatic cleanup is introduced.
