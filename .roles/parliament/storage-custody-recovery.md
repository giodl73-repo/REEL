---
name: Storage Custody and Recovery
slug: storage-custody-recovery
tier: parliament
applies_to: [storage, placement, capacity, custody, recovery, demotion]
version: 1
---

# Storage Custody and Recovery

## Key question

Can this operation free the intended physical space while preserving required
working copies and a verified recovery path, including after interruption?

## Verify

- Placement derives from exact Golden, authorized-draft, job and pin roots
  under configurable policies. Shared assets can require multiple hot locations.
- Distinguish logical bytes, aliases, physical allocation and failure domains.
  Different drive letters, paths or partitions alone do not prove independence.
- Admission accounts for bounded copies, scratch, reserves and concurrent jobs;
  metadata indexes/evidence also consume storage and need retention policy.
- Copy publication is atomic and independently verified before source removal.
  Corruption fails explicitly; absence, stale observations and permissions are
  distinct. Prior proof reuse has explicit policy and current-identity checks.
- Fresh plans revalidate roots, leases, controls, source/retained identities and
  complete managed scope at execution. No last-copy or unknown-coverage deletion.
- Fixtures cover disk full, missing archive, corrupt target, hard links,
  cancellation, custody publication, partial unlink, receipt replay and recovery.
- Startup and review do not hydrate, render, repair, crawl archives, start a
  daemon or resume cleanup. Named recovery and movement remain explicit.

## Blocking findings

Unverified recovery; same-failure-domain backup presented as independent;
stale-plan removal; unexplained space accounting; unbounded admission;
automatic expiry treated as deletion permission; or unsafe interrupted recovery.

## Tension and review record

Balance active capacity against recoverability and unnecessary copies. Report
severity, exact evidence, owner, correction and verification. Distinguish model
assertions from real physical-I/O fixtures. This checklist authorizes no move,
unlink, publication or policy exception.
