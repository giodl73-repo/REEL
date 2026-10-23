# Asset graph snapshots and evidence adapters

Status: proposed architecture, with an executable reference model in
`crates/reel-assets/tests/graph_architecture.rs`. This is not an implemented
storage format, registry migration, deletion API or replacement for existing
custody checks. All examples are synthetic. No production cache changes follow.

## Boundary and authority

Develop a project-neutral Rust graph/lifecycle library within REEL first.
REEL rendering and authoring are clients; projects supply policy and adapters.
Extract the library when its contracts are stable, not as a prerequisite.
Git owns schemas, policies, source and compact release/checkpoint pointers.
Manifests, inventories, receipts, registry checkpoints and indexes are immutable
cache/LFS assets. No media or evidence upload is implicit.

Separate four authorities:

1. Artifact manifests describe exact versions and semantic dependencies.
2. Root transactions authorize selections, drafts, jobs and pins.
3. Evidence records describe observations and completed operations.
4. Placement policy derives demands; guarded execution changes physical copies.

Neither an inventory nor a receipt selects a Golden, authorizes a draft or
grants creative approval. An inventory's age does not establish present absence.

## Complete snapshots with structural sharing

Use immutable, content-addressed nodes, not reconstruction through prior versions.
An episode root links stable scene IDs to scene manifests. Large binding maps
may use bounded persistent index nodes; start with natural scene boundaries.
Small 1,000-entry maps can remain one node if measurements favor simplicity.
These are storage/index envelopes around existing declarative artifact contracts,
not a second scene grammar. Native `scene.json` remains the authoring contract;
adapt or index its references without silently changing its meaning.

For a synthetic 1,000-binding episode split over ten scenes, replace ten cels
and three cues in two scenes. Write thirteen new payloads, two scene manifests,
and one episode root. Eight scene hashes remain identical. Both versions resolve
directly without the other version, a transaction log or a delta replay.
If all ten scenes change, all ten manifests change; sharing is not magic.

```text
episode v1 -> root A -> scene 00 old, scene 01 old, scenes 02..09 unchanged
episode v2 -> root B -> scene 00 new, scene 01 new, scenes 02..09 unchanged
```

Index large collections by stable logical IDs, not ordinal array positions.
Choose a bounded persistent radix tree or equivalent stable keyed partitioning;
avoid pages where inserting one item shifts every later page. Node splitting
may rewrite a bounded ancestor path. Measure metadata bytes, node reads and
write amplification before adopting a specific fanout. Scene ordering is an
explicit authored property, separate from identity and index partitioning.

Proposed wire contract: versioned node kind, canonical encoding, sorted unique
keys, explicit child hashes/byte lengths/types, bounded fanout and total traversal
budget. Hash a domain separator plus canonical bytes. Define UTF-8 identifier
and numeric rules; reject ambiguous encodings, duplicate keys and unknown kinds.
Do not normalize submitted creative text to deduplicate it. The reference model
uses sorted JSON maps and domain separators only; it is not this wire protocol.
An LFS pointer is a locator, not validation of the node or its transitive closure.
Missing/corrupt nodes produce an incomplete graph, never an empty subtree.

## Two graphs, not one reference count

The structural Merkle DAG records node sharing and enables subtree comparison.
The semantic requirement graph records why an asset must remain available.
Structural child edges, inventory membership and provenance references do not
all become hot-cache requirements.

For version v, let I(v) be the exact transitive primary inputs, O(v) its declared
retained delivery/review outputs, and H(v) historical/provenance references.
Dependency edges resolve immutable version locks; build dependencies may consume
primary inputs, never compiled deliveries. Generated cels/cues/bounded VFX can
be declared primary inputs: role is semantic, not simply "generated or not".
Reject compiled-output/primary-input conflicts rather than relabeling an output.

```text
R = selected deliveries union authorized drafts union live jobs union pins
Need(r) = I(version(r)) union O(version(r))
Users(h) = { r in R | h in Need(r) }
Demand(h, location) = any r in Users(h) requires that working location
```

Counts are derived from distinct root identities, not duplicate graph paths.
Store/query explanations behind counts. An unchanged subtree can reuse a
validated closure summary keyed by its hash and interpretation/schema version.
Differencing roots skips equal subtree hashes, but a changed root's removal
candidates must still be reconciled against every other applicable root.
An asset dropped from one changed page may still occur in an unchanged page of
the same root. Page-local differences are candidates, not exact reference-count
decrements. Deduplicate and reconcile against the complete new root before
updating its asset membership.
Cached counts alone never authorize removal. Control nodes needed to interpret
live roots have their own active-metadata retention requirement, even though
historical payload references need not stay hot. Avoid counting every old receipt
as a permanent hot-media root.

Unknown coverage is not zero. Relevant consumers, roles, source controls and
generation must be known before a zero-demand removal decision. Historical
versions remain recoverable independently of current working-set placement.

## Root transactions and concurrent writers

Writers publish immutable blobs/nodes independently, verifying existing winners.
Then atomically admit a change set with expected revisions for the mutable keys
it changes: artifact selection, draft authorization or job protection.
Different keys can advance concurrently. Competing updates to the same key
require an explicit merge/rebase, even when both writers started from one root.
Two edits in the same scene/page can still be merged by stable binding ID;
page hash inequality is not itself a semantic conflict. Same-binding edits
remain conflicts unless equivalent. No silent last-writer-wins selection.

A logical registry is a checkpoint plus admitted transactions; checkpoints and
sharded indexes are cache assets. Periodic compaction preserves logical state.
An episode snapshot is self-contained and never requires this log for playback.
A derived local database can accelerate reverse lookups and transaction
admission, but remains rebuildable from authoritative checkpoints/transactions.
Small publication/admission transactions may serialize; no lock spans hashing,
copying, rendering or the lifetime of a producer. Crash-safe commit/WAL behavior,
replay idempotence, multi-process publication and checkpoint discovery remain
implementation work, not claims proved by the reference model.

Hold job protection while publishing a new version and establishing its draft
or delivery root. Release old protection only after the successor is admitted.
Uncommitted uploads need bounded staging protection so reconciliation cannot
race publication. Root changes are all-or-nothing, with explicit expected values.

## Large inventories and receipts as evidence assets

Reuse them through adapters. Do not embed their rows in manifests, duplicate
them into Git, or convert every observation into a permanent retention root.
An evidence source descriptor binds content hash, encoded bytes, schema,
adapter/version, project/scope, capture interval, coverage and verification level.
For compressed sources bind both encoded and expanded interpretation, with
decompression/record budgets. A digest supplies integrity relative to a trusted
descriptor, not authenticity, current custody or permission.

An adapter returns a typed claim and a provenance reference:

```text
source descriptor -> authenticated index/shard -> exact source record
claim = subject hash/bytes + predicate + location/physical identity + time
        + verification method + source record locator
```

Use an adapter boundary with operations equivalent to `describe`,
`lookup(subject, budget)` and bounded `enumerate(scope, cursor, budget)`.
Lookup returns claims plus explicit completeness/unknown information; adapters
perform no movement, deletion, repair, upload or automatic whole-cache census.
Scope and expected source digest come from the caller's admitted source catalog,
not from an untrusted record being read. Verification failures are explicit.
Keep source-specific fields behind adapters and preserve original evidence.

For legacy monolithic JSON/gzip, first access may require a full bounded
streaming pass. Do not promise random access into ordinary gzip. Explicitly
build a content-addressed, sharded sidecar index once, recording source hash,
adapter version, record key/locator and record or shard digest. Later lookups
read the required authenticated shard. A record ordinal in a monolith is
provenance, not a cryptographic inclusion proof; proving it requires reading
the source or trusting a previously verified transformation/index checkpoint.
Rebuild the sidecar when the source or adapter interpretation changes.
Keep the source catalog and required active index/control nodes available under
an explicit metadata policy. Large historical evidence bodies may be archived.
An unavailable body returns an evidence gap or requires explicit named recovery;
an adapter must not hydrate it, scan an archive drive or infer an empty source.

For native sharded sources, verify the root-to-shard hash path and shard bytes.
If a source supports range reads, authenticate ranges before accepting claims.
Large receipts can likewise separate a small operation header from sharded
per-object records; the header binds the shard root and scope. Never replace
an execution receipt with an unauthenticated summary count.

Existing adapter targets, not implementations promised by this proposal:

| Source | Useful claims | Cannot establish alone |
| --- | --- | --- |
| `reel.asset-cache-report.v1` | observed locations, sizes, physical-copy accounting, capture scope | payload integrity, present absence, selection or removal permission |
| Existing object/version registry checkpoints | authorization membership and immutable version locks | current physical custody or creative approval |
| Demotion/copy receipts and prior verified backup indexes | named completed actions and verification method at capture | unchanged custody today or a new removal authorization |
| Project delivery/draft adapters | explicit admitted working-set roots and evidence links | rights/approval beyond their actual recorded scope |

Keep contradictory observations as separate claims with their sources/times.
Do not let a newer size-only inventory silently overwrite an earlier verified
copy receipt, or let that receipt prove current existence. Derived views can
show the conflict and request an exact named recheck. Deduplicate identical
evidence references, not independent physical copies. Missing receipt, partial
coverage and outside-scope absence remain unknown. Evidence source/adapter
changes invalidate affected derived results and executable plans.

## Placement and verified demotion

Working locations are configurable affinities, not fixed E/F drive names in the
engine. A shared primary input can require two hot locations simultaneously.
Authorized drafts have an owner, exact root, declared retained outputs, budget
and review policy. Becoming overdue requests review, not deletion. Promotion
establishes delivery protection before draft release. Abandonment is explicit.
Automatic draft expiry/budget enforcement is not implemented by this design.

Historical placement requires project evidence of actual use; a filename, old
timestamp or abandoned draft does not by itself establish that eligibility.
Archive policy can place other retained material on a named cold tier.

```text
RemoveCopy(h, L) requires:
  complete applicable consumer knowledge and no demand/pin/job for L
  fresh relevant root/policy/source revisions and physical-copy identities
  verified independent retained custody meeting the project's recovery policy
  a fresh explicit plan, bounded request, dry run and authorized apply
```

Independent custody means another failure domain, not another hard link or
alias on the same physical volume. Count physical allocation groups separately
from logical bytes and directory entries. Revalidate immediately before unlink;
coordinate with new admissions using bounded per-object/metadata protocols,
not a lifetime cache lock. A reference change cancels/replans affected removal.
Crash recovery must distinguish published custody, removal intent, unlink and
completed receipt. Never delete the last verified copy. Losing an index cache
can slow a query; it must not manufacture deletion authority.

## Scenario suite and limits

Run `cargo test -p reel-assets --test graph_architecture --locked`.
The pure Rust reference model uses synthetic metadata and no filesystem caches.
It exercises structural sharing, complete snapshots, root demand and bounded
evidence lookup. It supplements, not replaces, existing real custody/concurrency
fixtures. No performance benchmark, Windows unlink guarantee, real-format
adapter compatibility or production migration is established by these tests.

Required scenarios: 13 of 1,000 bindings changed; unchanged subtree reuse;
stable-ID insertion; corrupted/missing nodes; shared episode/music input;
draft without Golden; overdue draft; safe promotion; independent-key admission;
same-key conflict; stale removal plan; independent custody; reference-only
history; bounded evidence shard lookup; tampered shard; contradictory claims;
partial inventory absence; and evidence-version invalidation.
The suite also tests disjoint edits inside one chunk, duplicate transaction-key
rejection and a local removal candidate retained in an unchanged subtree.
The changed-page comparison model covers matching scene topology; topology
changes deliberately return an error and need a budgeted production fallback.

## Implementation sequence and review

1. Review this algebra and scenarios against representative project records.
2. Specify canonical nodes, source descriptors, typed claims, limits and adapters.
3. Implement read-only legacy adapters and exact-source sidecar indexing; compare
   membership/claims with original readers using public synthetic fixtures.
4. Implement persistent nodes, transactional root publication and rebuildable
   indexes; test crash recovery, concurrent processes and adversarial inputs.
5. Shadow existing accounting/diff/planning without granting removal authority.
6. Adopt one project family only after parity, recovery, byte/node-read benchmarks
   and current physical custody tests pass. Preserve existing runtime/job pins.

Producer-applied provenance checklist: all examples are synthetic; source
integrity, observation, selection and human authority remain distinct. No private
paths, inventory records, media hashes or consent claims are imported here.
Open design decisions: node fanout/splitting thresholds, canonical wire encoding,
evidence trust/catalog admission, transactional backend and platform recovery
protocol. This proposal intentionally does not resolve them by documentation alone.

Local validation: 14 model scenarios, the existing 195 unit tests and five other
integration tests pass; strict all-target Clippy, formatting, ROLES structure and
diff checks pass. Review caught two important boundaries: a page-local removal
may still be used elsewhere in the same root, and monolithic compressed evidence
needs explicit indexing before bounded random lookup. These are reflected above.
