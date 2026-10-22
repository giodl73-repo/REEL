# Compare versions before cache eviction

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
reel-assets --root <project> tier-demote <new-request.json> <absolute-owned-verification-journal>
reel-assets --root <project> tier-demote <new-request.json> <absolute-owned-apply-journal> --apply
```

The scope uses the existing `reel.asset-demotion-request.v1` contract: explicit policy,
saved plan, managed consumer roots, budgets and optional source controls. An empty scope
hash set means all eligible dropped hashes; a nonempty set narrows the comparison further.
Request generation rejects stale plans and empty intersections, respects batch limits,
and preserves the existing consumer and retention settings. Actual eviction rechecks
other consumers and backup custody; preparing a request grants no deletion authority.
This path handles previously used registered assets. Unregistered cold candidates keep
their separate register-and-archive-first workflow.
