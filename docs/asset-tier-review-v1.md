# Private inventory block review

For repeatable project entry, keep a small `reel.asset-manager-project.v1`
declaration in the consumer repository and run:

```text
reel-assets --root <project> manager-review asset-manager.json <new-review-name>
```

The declaration contains a relative `policy` file, an exact registered inventory
asset (`sha256`, `bytes`, `cache_uri`), `active_branches`, explicit
`inactive_branches` / `legacy_branches`, and `max_verify_seconds`. It contains no
machine cache/worktree paths. Git resolves actual registered worktrees; unknown
branches and detached/bare consumers hold. Absent declarations create no worktree.
The inventory resolves only through configured active caches, with fresh hash
verification and no copy/archive fallback. Outputs go to a new explicitly owned
`.local/asset-reviews/review-<name>` directory; existing reviews never overwrite.
There is no setup, ingestion, session-entry maintenance or server side effect.

The generated scope starts with an empty hash set, so it cannot accidentally be
applied as an existing selection. Optional `source_controls` freeze relative
project/policy file hashes. Review and request preparation check them again;
Windows application also guards the exact regular files against replacement and
writers before every unlink. A changed declaration requires a new review.

The existing Rust asset manager produces an offline, self-contained HTML view
from an explicit saved tier plan and consumer-scope request:

```text
reel-assets --root <project> tier-review <consumer-scope-request.json> <new-private.html>
reel-assets --root <project> tier-request <consumer-scope-request.json> <saved-block-choice.json> <new-request.json>
reel-assets --root <project> tier-demote <new-request.json> <absolute-owned-journal>
```

The scope uses the existing `reel.asset-demotion-request.v1` contract: policy,
plan/hash, all managed consumer roots, explicit inactive/legacy roots and time
budget. Its initial hash set is not a browser choice. Native preparation replaces
that set only with the exact members of the owner's selected blocks, under the
policy's item/byte limits. Verification bytes equal twice selected logical bytes.
Creating a request never applies it. Existing explicit `--apply` gates remain.

The review checks plan integrity/expiry/current metadata and independently
exports all declared consumer protections. Blocks partition hashes exactly once
by classification, one version-reference label, observed locations, proposal and
protection state, then split at 256 items / 1 GiB or smaller policy budgets.
Shared content is not exclusive to the displayed reference. Rendering is capped
at 10,000 blocks / 16 MiB; the browser paginates 60 rows and filters locally.

The view shows classification totals, per-tier logical copy bars, observed free
capacity, inventory generation and census timestamps when available. New reports
record census start/completion; older inventories display unknown acquisition
time. Planning time is distinct from census time. New unregistered files require
a new inventory; registration deltas do not refresh physical observations.
Unknown physical allocation remains unknown. Archive tiers remain unobserved;
the view performs no archive scan and never infers a verified backup from size.

Only unprotected `previously-used` / `verify-and-demote` rows with observed source
and history copies can enter a removal request. Other classes remain visible but
held. Promotions, archive-first custody and last-copy retirement are separate
capabilities; no misleading executable action is offered for them.

Browser choices use strict `reel.asset-tier-decision.v1`: review hash, plan hash,
`remove-redundant-hot-copy` action and exact selected block IDs. Native preparation
rebuilds the review and checks all identities/protections again. Expired plans,
changed metadata or scope, another consumer's new selection, unknown/held blocks,
empty decisions and excess budgets fail before publishing a request. Registering
review evidence itself may stale the plan; regenerate explicitly rather than
silently accepting old choices. Outputs are new-only and atomically published.

The browser starts with nothing selected. It saves a local JSON decision; it
cannot call Rust, remove files or run commands. No server, daemon, CDN, telemetry,
remote font or network API is used. CSP disables connections, project labels are
escaped inside JSON and written as text, and private content stays upstream.
The HTML and raw reports are retained assets, not ordinary Git documents.

Fixtures cover a real native decision-to-verification round trip without
removal, held/forged blocks, stale/expired choices, cross-consumer protection
changes, script-element escape prevention, private networkless rendering,
portable project recreation, undeclared worktree holds and frozen source controls.

## Explicit existing cold-copy removal

`tier-demote` accepts optional `retained_tier` naming the policy cold tier.
Omission retains the history-only contract. Cold mode accepts only planned
`unregistered-cold-candidate` / `register-and-archive-first` objects. It probes
only the exact configured archive CAS paths, requires an existing independent
copy, and freshly verifies both copies with the same handle/alias protections.
It never enumerates the archive or creates payload copies. Missing or corrupt
cold content holds; registered content in any modern consumer also holds.

Default verification does not register cold custody or remove source links.
Explicit apply publishes an immutable `reel.cold-custody.v1` receipt in the named
archive's `snapshots/reel-assets-cold-v1/<prefix>/<hash>.json` before any source
unlink. Receipt paths reject reparse ancestors; receipt handles exclude writers
and replacement through removal. A conflicting receipt or failed publication
keeps the source. The cold register preserves hash, bytes, exact recovery path,
unknown provenance and retention state separately from active selections.
It is not a permanent retention pin. All-consumer, stale-plan, last-copy and durable-journal safeguards remain.
Cold mode excludes every hash registered by any modern consumer, so unrelated
job reservations do not globally block it. Under held metadata handles, late
consumer changes trigger exact-object protection/registration refresh. A newly
registered or selected input holds; unrelated new assets can proceed. The short
metadata transaction lock remains; no reservation is removed or job stopped.
History mode retains its existing job coordination gate. Browser block
choices remain history-only; cold requests are explicit native operator batches.
