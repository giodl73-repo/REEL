---
name: Registry Concurrency and Evidence Integrity
slug: registry-evidence-concurrency
tier: parliament
applies_to: [registries, concurrency, inventories, receipts, adapters, indexes]
version: 1
---

# Registry Concurrency and Evidence Integrity

## Key question

Can independent producers publish without losing intent, and can every derived
claim be traced to correctly scoped evidence without trusting a stale index?

## Verify

- Immutable publication precedes atomic root admission with expected revisions.
  Independent keys proceed; same-key conflicts reconcile explicitly. Revisions
  cannot be reused after rollback, deletion/recreation or checkpoint compaction.
- Transactions reject duplicate keys and partial application. Job/staging
  protection overlaps publication; no lifetime render/copy/global-cache lock.
- Checkpoints and admitted changes recover authoritative state. Query indexes
  and refcounts are rebuildable; compaction preserves selections and protection.
- Inventories/receipts remain immutable evidence assets behind versioned,
  bounded adapters. Trusted source catalogs bind project/authority scope,
  schema, source hash, coverage, capture time and verification method.
- Membership, observation, verified custody and intent are separate predicates.
  Missing records and contradictory claims remain explicit; summaries alone
  cannot authorize removal. Sidecars bind the original source/interpretation.
- Byte, record, decompression, traversal and time budgets apply to adversarial
  sources. Data cannot install adapters, execute commands or grant egress.
- Private diagnostics/provenance and allowlisted shareable reports are separate.
  Apply Rights and Provenance review to private-source/exchange changes.
- Test interleavings, duplicate publication, stale/ABA conflicts, crash/replay,
  index loss, compaction, scope mismatch and corrupt/missing evidence.

## Blocking findings

Lost updates; revision reuse; hidden global lifetime locks; unrecoverable
checkpoint discovery; untrusted source self-admission; fabricated certainty;
private graph leakage; or unbounded adapter side effects.

## Tension and review record

Balance explainable durable authority against cheap derived queries and short
coordination. Report severity, target/evidence, owner, correction and actual
verification. Tests of a serial reference model do not prove multi-process
durability. This role grants no human approval, storage action or delegation.
