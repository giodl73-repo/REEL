# Compare versions before cache eviction

`version-manage` is the explicit post-publication hook for a producer or CI job:

```text
reel-assets --root <project> version-manage <logical-id> <old-version> <new-version> <consumer-scope.json> <new-owned-directory>
reel-assets --root <project> version-manage <logical-id> <old-version> <new-version> <consumer-scope.json> <different-new-owned-directory> --apply
```

It verifies changed/new payloads, observes only the changed block's named E/D-equivalent
cache paths, creates a fresh hash-bound plan, prepares missing retained copies, verifies
custody, evicts redundant hot aliases and writes `management.json`. It never walks the
whole cache or starts a daemon. The selected version must already be current and declare
`supersedes` for the exact prior version. For an initial version, pass its ID as both
old and new; all its active payloads are verified without inventing a prior version.

Default mode prepares a dry run and returns `managed: false`. The job/PR completion
gate requires `managed: true` from explicit apply. Keep the receipt and comparison as
private cache/LFS assets. A successful renderer process alone does not satisfy this gate.
This hook does not register/select a new version, grant creative approval, or silently
attach itself to existing render recipes; producers must call it after publication.

If another managed consumer still needs a superseded cel, it remains hot and is reported
as referenced. Zero-reference superseded objects must be absent from hot CAS/LFS aliases
and have freshly verified retained copies. If a bounded batch leaves unreferenced hot
objects behind, the completion check fails; process another batch before completing.
Repeating an already completed transition verifies retention and removes nothing twice.

Historical retention defaults to the configured history tier. An explicit
`retained_tier` naming the configured cold tier uses named archival custody instead.
Set `history_fallback_to_archive: true` in the consumer scope to authorize fallback
when history-cache admission is unavailable or below reserve. Corruption does not
trigger fallback. Archive access is limited to named hashes; no archive inventory scan
occurs. Apply/copy publication uses guarded Windows handles in this release. Active
reservations for unrelated jobs do not block a classified transition; exact references,
source aliases, pinned handles and apply-time control checks still protect shared data.
Unrelated registrations can be reconciled without pausing producers. This exception is
limited to classified exact transitions: profile, storage bindings, policy and expiry stay
frozen; all target references are refreshed before removal. Ordinary tier plans retain their
strict stale-state behavior. A newly acquired reference holds the affected transition.

## Primary inputs, outputs and historical references

A version's optional `dependency_contract` classifies its payload hashes into disjoint
`primary_inputs`, `outputs` and `references` sets. Every payload other than the manifest
must appear in exactly one set. Cels, cues, audio sources and VFX artifacts are primary
inputs. Compiled episodes/videos are outputs, even when an imported binding called one
an audio/caption source. An output hash cannot be redeclared as a primary input.

Version dependency edges may target only explicitly classified primary-input versions,
which contain no compiled outputs. References retain provenance, approvals and history;
they do not expand the active build closure. A product's own selected outputs remain
protected as outputs, independently of its primary-input dependencies.

Set `require_primary_input_contracts: true` in `asset-project.json` to reject unclassified
new registrations and selections. Legacy immutable records remain readable and conservatively
protected; they must be migrated explicitly, not silently reinterpreted. Managed replacement
requires a classified new version regardless of this compatibility setting.

The report includes a separate `primary_input_diff`, so five changed cels can be counted
separately from changed manifests and compiled delivery files. `active_reference_count`
counts protecting roots in the invoking project; the executor additionally reconciles all
declared managed consumers. History pins preserve recovery without requiring hot placement.

## Lower-level commands

Run `reel-assets --root <project> version-diff <old-logical-id> <old-version> <new-logical-id> <new-version> <new-private-report.json>`.
The command reads registered immutable versions and their complete dependency closures.
It reports added, retained and dropped content hashes, including manifests, with registered logical byte sizes.
Renamed roles or identical content reused under another name do not inflate the difference.

Dropped means absent from the new version's closure; it does not mark a version deprecated,
select a new version, or authorize deletion. The report includes immutable manifest hashes and registry generation.
It must remain a private asset when retained, rather than a large Git report.

Candidates require the new version to be current. Current versions, leases and registry control
metadata protect shared hashes. Each dropped row lists protection reasons and historical retention
pins separately. A history pin preserves recovery; the configured tier policy determines placement
and enforces explicit tier pins. Global census coverage is reported without asserting that a partial
census authorizes removal; the executor still requires explicit managed-consumer coverage.
Logical candidate bytes are not an estimate
of physical free space: files may already be absent from the hot tier or have other hard links.

Use the candidate hashes to scope an existing fresh tier plan and review. Apply only through
`tier-demote`, with the complete managed-consumer scope, current protections, configured tier pins,
independent retained-copy verification and physical identity checks. The comparison itself does not
scan caches, probe an archive, copy media, start a daemon or remove files. A registry generation in
the report is descriptive; it does not replace the executor's apply-time revalidation.

To prepare the exact intersection with a fresh historical tier plan, use:

```text
reel-assets --root <project> version-eviction-request <old-logical-id> <old-version> <new-logical-id> <new-version> <consumer-scope.json> <new-request.json>
reel-assets --root <project> version-backup <new-request.json> <new-private-backup.json> --apply
reel-assets --root <project> tier-demote <new-request.json> <absolute-owned-verification-journal>
reel-assets --root <project> tier-demote <new-request.json> <absolute-owned-apply-journal> --apply
reel-assets --root <project> version-management-check <new-request.json> <execution-result.json> <new-private-receipt.json>
```

The scope uses the existing `reel.asset-demotion-request.v1` contract: explicit policy,
saved plan, managed consumer roots, budgets and optional source controls. An empty scope
hash set means all eligible dropped hashes; a nonempty set narrows the comparison further.
Request generation rejects stale plans and empty intersections, respects batch limits,
and preserves the existing consumer and retention settings. Actual eviction rechecks
other consumers and backup custody; preparing a request grants no deletion authority.
This path handles previously used registered assets. Unregistered cold candidates keep
their separate register-and-archive-first workflow.

`version-plan <old-id> <old-version> <new-id> <new-version> <scope.json> <new-owned-directory>`
refreshes only the changed hashes into a separate scoped observation. It preserves the
full inventory and does not misrepresent the scoped observation as a full cache census.
All owned directories must be new children of the invoking project's existing `.local`.
`unselect <logical-id> <expected-version>` withdraws an obsolete current root with an exact
version check. It preserves immutable versions, pins and leases and removes no cache bytes.

The planner rejects same-volume tiers by default. The explicit `allow_same_volume_copies`
policy option supports synthetic fixtures/logical tiers; actual removal separately requires
the same explicit request permission. Production redundancy should use independent volumes.
