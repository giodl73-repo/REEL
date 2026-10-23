# Lessons from 100 asset graph scenarios

Scope: executable synthetic reference model, not production storage validation.
The first 25 scenarios established sharing, root protection and bounded evidence
lookup. The second 25 extend lifecycle, merging, format/scope and failure cases.
The third batch adds 50 multi-root, merge/interleaving, evidence and boundary
scenarios in `tests/support/graph_scenarios_100.rs`.
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

## Third batch: 50 more scenarios

| # | Scenario | Result |
| --- | --- | --- |
| 51 | Twenty roots share one asset | Twenty explainable users, one content identity |
| 52 | Repeated use sites in one root | One root membership |
| 53 | Nested shared DAG leaf | Asset membership deduplicates |
| 54 | Deep graph over traversal budget | Error, not empty closure |
| 55 | Unchanged subtree is corrupt | Diff can succeed; full closure validation fails |
| 56 | Repeated paths to one leaf | Traversal cost still charged |
| 57 | Corrupt existing publication winner | Reject without overwrite |
| 58 | Independent stores, same snapshot | Same content address |
| 59 | Scene order changes | New authoring identity, same inputs |
| 60 | Distinct Unicode identifiers | Preserve distinct IDs |
| 61 | Three independent writers in six orders | Same admitted revisions |
| 62 | Two same-key writers in either order | First wins; stale second fails |
| 63 | Disjoint merges across multiple versions | Commutative result |
| 64 | One unchanged merge side | Preserve peer's changes |
| 65 | Rename via delete/add versus edit | Explicit conflict |
| 66 | Duplicate transaction keys with identical values | Reject |
| 67 | Empty transaction | No revision changes |
| 68 | Repeated checkpoint serialization | Read-only deterministic result |
| 69 | Tampered checkpoint under old hash | Reject |
| 70 | Publish nodes without admitting root | No implicit selection |
| 71 | One root, two hot affinities | One logical owner at both locations |
| 72 | Episode moves; music still uses original tier | Preserve music's requirement |
| 73 | Explicit old-version pin without Golden | Protect pinned inputs/output |
| 74 | Known compiled output labeled primary | Reject semantic role conflict |
| 75 | Generated primary cue produced by a job | Do not confuse generation with compiled-delivery role |
| 76 | History reference also needed by a current root | Current primary demand wins |
| 77 | Retained receipt for abandoned draft | No implicit draft authorization |
| 78 | Working root lacks affinity | Admission/coverage gap |
| 79 | Release one of two required locations | Other requirement remains |
| 80 | Zero users and unverified archive observation | Removal remains held |
| 81 | Same subject observed at two archive locations | Preserve both observations |
| 82 | Conflicting sizes | Diagnostic conflict, not custody proof |
| 83 | Mixed verified/unverified copies | Verification stays per-copy |
| 84 | Malformed subject inside hashed shard | Reject |
| 85 | Valid subject routed to wrong shard | Reject, not false absence |
| 86 | Repeated evidence records | Do not inflate root count |
| 87 | Future-dated prior proof | Reuse rejected |
| 88 | Optional proof-age limit | Honor exact boundary |
| 89 | Explicit append-only prior-proof policy | Old proof permitted only with authorization/recheck |
| 90 | Changed physical identity | Prior proof unusable |
| 91 | Zero total-byte budget | Reject root read |
| 92 | Empty captured scope | One header read sufficient |
| 93 | Oversized index header | Charge before decoding it |
| 94 | Invalid later record after a valid row | No partial success emitted |
| 95 | Repeated reads on one operation meter | Charge each read |
| 96 | Rejected resource charge | No partially advanced counters |
| 97 | Duplicate fields in otherwise complete hashed JSON | Duplicate-field parse rejection |
| 98 | Unknown scope-affecting index field | Reject |
| 99 | Truncated JSON with a correct hash | Format rejection |
| 100 | Refresh evidence | Old and new sources both resolve independently |

### New corrections and implications

The model's original duplicate-publish path reused any existing bytes under a
matching key without checking them. It now rejects a corrupt winner without
overwriting it. A valid shard hash also did not establish that its rows belonged
to the indexed partition: the adapter model now validates subject syntax,
partition membership and required location fields before returning any claims.
Those are corrected model gaps, not claims about production-engine defects.

The corrupt-unchanged-subtree scenario separates candidate discovery from
integrity validation. A successful fast diff cannot bless untouched data. Shared
closure summaries will need explicit validation/trust context in production.
Repeated DAG paths also show that deduplicated asset counts do not bound parser
work; traversal limits still matter until validated closure caching exists.

Further scenarios introduce explicit design-only root-affinity/role validators
and prior-proof reuse predicates. They are not connected to the production
engine and cannot perform custody checks. All I/O, time, failure-domain and
authorization inputs in those predicates are synthetic assertions. Serial
permutations explore transaction semantics; they do not test OS processes or WAL
durability. Checkpoint corruption tests prove rejection, not crash recovery.

Producer-applied technical/provenance review: carry publication winner validation
and semantic shard validation into implementation requirements; preserve separate
root intent, evidence multiplicity and per-copy verification. The 100 model cases,
strict all-target Clippy, formatting, ROLES structure and diff checks pass locally.
All production adoption gates listed above remain open.
