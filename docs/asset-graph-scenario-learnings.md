# Lessons from 50 asset graph scenarios

Scope: executable synthetic reference model, not production storage validation.
The first 25 scenarios established sharing, root protection and bounded evidence
lookup. The second 25 extend lifecycle, merging, format/scope and failure cases.
Run `cargo test -p reel-assets --test graph_architecture --locked`.

## What changed because of the scenarios

1. A page-local removal is only a candidate. The same asset can remain in another
   unchanged page, another root or another required location. Reconcile full-root
   membership before adjusting distinct-root counts.
2. Asset-set equality is not episode equality. Timing, ordering or a move between
   scenes changes authoring identity without necessarily changing required bytes.
3. A correct snapshot hash must be a standard CAS hash of the stored frame.
   Domain separation cannot live only inside the hash function.
4. Scene/use-site IDs must survive insertion. Do not derive persistent identity
   from array position. Delete/edit and competing additions need semantic merge
   conflicts; identical changes and independent IDs can merge.
5. Protection is overlapping ownership. Releasing a draft cannot release another
   draft/Golden/job's use. Establish draft or Golden protection before releasing
   its predecessor. Zero demand alone still is not deletion authority.
6. Selection revisions cannot roll back or disappear in checkpoint compaction.
   Reverting content creates a new revision. Overflow/conflict rejects an entire
   transaction rather than admitting its other keys partially.
7. Unknown failure-domain identity is not independent custody. The earlier
   reference predicate only compared unequal strings and could accept an empty
   identity. It now requires known identities as well as distinct domains.
8. Evidence interpretation includes authority namespace and adapter configuration.
   The earlier model checked project/version only; it now rejects reuse of an
   index prepared under another authority/config and rejects unknown schemas.
9. Inventory membership is captured knowledge. Even complete absence means
   absent from that captured scope, not absent physically today or unreferenced.
10. Bounded reads must include byte/record budgets and overflow checks. Selective
    lookup need not scrub unrelated shards, but a queried missing/corrupt shard
    must produce a gap/error, never an empty success or a global health claim.

These distinguish corrections to the model from invariants that the scenarios
confirmed. No production-engine defect or fixed cache-removal bug is inferred.

## Second batch: 25 additional scenarios

The exact executable cases are in the `additional_scenarios` module.

| # | Scenario | Expected result |
| --- | --- | --- |
| 1 | Identical republication | No new nodes |
| 2 | Different insertion order into keyed bindings | Same sorted-map identity |
| 3 | Move a binding between scenes | New snapshot, unchanged input set |
| 4 | Delete a scene | Only new root; old snapshot remains readable |
| 5 | Compare differing scene topology | Explicit model gap, not false empty diff |
| 6 | Delete against unchanged peer | Merge deletion |
| 7 | Delete against same-binding edit | Conflict |
| 8 | Identical concurrent edit | Merge |
| 9 | Independent additions | Merge both |
| 10 | Competing additions at same ID | Conflict |
| 11 | Release one of two drafts | Keep other; demand ends only after last release |
| 12 | Abandon draft sharing Golden input | Golden protection remains |
| 13 | Deselect one output variant | Other output and shared inputs stay required |
| 14 | Job-to-draft transition | Protection overlaps, no modeled gap |
| 15 | Checkpoint revision round trip | Reject stale creation after restore |
| 16 | Revision overflow in multi-key change | Reject all keys atomically |
| 17 | Unknown failure domain | Hold removal |
| 18 | Empty complete inventory | Captured absence; partial source stays unknown |
| 19 | Same project, wrong authority | Reject index |
| 20 | Adapter configuration change | Require matching prepared index |
| 21 | Unknown index schema | Explicit unsupported-format error |
| 22 | Corrupt unrelated shard | Requested valid shard readable; corrupt shard's query fails |
| 23 | Missing referenced shard | Error, not successful empty lookup |
| 24 | Zero examined-record budget | No claim emitted |
| 25 | Byte-counter overflow | Error without counter wrap |

## Review and next evidence

Producer-applied review through Asset Graph and Version Integrity, Storage
Custody and Recovery, Registry Concurrency and Evidence Integrity, and Rights
and Provenance. Graph: captured absence and moved inputs do not decrement live
references blindly. Storage: missing domain evidence holds custody decisions.
Registry: configuration/namespace/schema belong in the prepared-index identity;
checkpoints preserve revisions. Provenance: bounded query results are scoped
evidence, not a global scrub, current custody or publication authority.

All 50 model scenarios, strict all-target Clippy, formatting, ROLES structure and
diff checks pass locally. The new format-policy section in the draft contracts
separates multiple external source adapters from a versioned internal contract.
Original producer/schema support and index migration need real adapter fixtures;
this model's synthetic schema is not an adopted interchange standard.

Remaining gates are unchanged: canonical wire codec, budgeted topology fallback,
streaming real-format adapter/sidecar parity, transactional crash and concurrent
process tests, physical custody/recovery, privacy/egress and performance. The
scenarios are useful design evidence, not substitutes for those implementations.
