# Advisory configurable asset tier plans

`reel-assets` accepts explicit project policy and a hash/byte-bound retained
`reel.asset-cache-report.v1` census. No consumer names, cache drive letters,
private hashes or media belong in REEL fixtures or documentation.

```text
reel-assets --root <project> tier-plan <policy.json> <baseline.json> <new-private-plan.json>
reel-assets --root <project> tier-plan-check <policy.json> <plan.json>
```

The first command does not recensus drives. It combines frozen per-object copy
observations with current registered objects, resolved version locks, selected
dependency closures and leases. Newly registered objects are added; baseline
unknowns that became registered are reclassified. New unregistered files still
require an explicit later census. Baseline copy observations and capacity are
not refreshed, so byte-saving estimates are advisory, not deletion eligibility.

The retained baseline may be JSON or gzip (detected by its magic bytes). Policy
identity always binds the decoded JSON SHA-256 and byte count. Decoding is
bounded by that expected length, rejects damaged gzip data, and never writes an
uncompressed temporary file. Plans also record the encoded input identity and
encoding so the exact retained asset remains traceable.

Policy schema: `reel.asset-tier-policy.v1`, with project ID, exact baseline
SHA-256/bytes; named `tiers` bound by `storage` (`active` or `archive`) and index
into ignored machine storage configuration; distinct `selected_tier`,
`history_tier`, `cold_tier`; explicit `history_versions` dependency locks;
per-hash `tier_pins`; and positive `max_objects`, `max_plan_seconds`,
`expires_after_seconds`, `max_batch_bytes`, `max_batch_objects` budgets.
Policy controls are limited to 256 KiB, census to 512 MiB/one million objects,
plan duration to one hour, expiry to one day and history roots to 4,096.

The planner rejects wrong project/baseline identity, root-binding changes,
overlapping roots, same-volume active aliases, duplicate objects/copy tiers,
invalid/missing dependency locks, cycles and unregistered graph payloads.
Archive paths are validated lexically, never opened, listed or created.
Metadata needed to resolve active versions can be read from active cache;
production payloads are not hashed or copied by planning.

Classification prioritizes registry controls, active leases, tier pins,
selected dependency closure and other current work before explicit historical
use. Retention pins do not alone mandate a hot copy. A noncurrent/deprecated
reference is not proof of actual previous use. Registered unused/unproven items
are held; unknown objects are proposed for registration and archive verification
first, not deletion. Never infer global consumer completeness from project usage.

Possible proposals are hold, keep/promote selected, verify-and-demote history,
and register-and-archive-first. No row is executable. First-batch budgets cap
historical proposals. Baseline reclaim estimates require measured distinct copy
identities and all known hot hard links accounted for; unrecognized hard links
give an unknown estimate. They do not promise actual space freed.

Plans bind baseline/policy, live version/control/registration/selection/storage
fingerprints, generation, creation/expiry and canonical plan hash. Freshness
checking rejects expired, tampered or metadata-stale plans. It verifies metadata
freshness only, not current physical bytes, protection completeness or capacity.
It never turns a proposal into removal authorization. Publishing a new private
plan is atomic and refuses an existing output; reports contain private local paths.

## Explicit implementation boundary

The planner has no apply, move, cold census/registry or permanent-retirement API.
It performs no media changes, archive probing, daemon startup or network access.
Future application must add fresh physical/hash checks, consumer coordination,
admission, durable journaling, safe path/handle operations and interruption
tests. Last-copy retirement remains separate from redundant-copy removal.
Ordinary LFS lookup and existing project accounting behavior remain unchanged.

The subsequent [explicit redundant-copy command](asset-demotion-v1.md) has its
own request, verification, concurrency and journal gates. A plan alone remains
non-executable and grants no removal authorization.

An explicit version lease protects all payloads declared by that version and its
locked dependency closure, including compiled reference assets needed by a live
job. It does not reclassify references as primary inputs or permanent selections.
Releasing the lease restores ordinary primary-input/output retention rules; an
old plan still requires generation and source revalidation before any removal.
