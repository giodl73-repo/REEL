# Asset graph and evidence contracts: draft 0

This refines the [architecture](asset-graph-architecture.md) after its role review.
It proposes wire and adapter contracts, not a deployed API. The executable model
checks selected invariants, not complete codec conformance or real-format parsing.
Existing scene, registry, receipt and cache formats remain authoritative today.

## 1. Content identity and node envelope

Every node uses the existing CAS rule: SHA-256 of exactly the bytes stored.
Domain separation MUST be included in those bytes, not secretly prepended only
while hashing. Otherwise a node could not pass ordinary cache verification.

Proposed graph frame:

```text
UTF-8 bytes "reel.graph-node/0" + NUL + canonical JSON envelope
```

The envelope has exactly `kind`, `payload`, `schema`; schema is
`reel.graph-node.v0`. Node kinds are `artifact-root`, `key-branch`,
`binding-leaf`, `evidence-index` and `evidence-leaf`. A parser must reject wrong
framing/kind/schema, unknown required interpretation and duplicate keys. Existing
raw inventory/receipt assets keep their original bytes and content addresses;
an adapter indexes them without reframing or rewriting them.

Canonical envelope JSON: UTF-8, no BOM/insignificant whitespace/trailing newline;
object keys recursively sorted by UTF-8 bytes; arrays preserve order; no duplicate
keys; standard JSON escapes with one specified encoder; booleans/null and
nonnegative integer metadata only. Metadata integers are limited to 2^53-1 for
interoperability. The exact string-escape rules and canonical codec must be
implemented/tested before this wire version is frozen. Authoring JSON is an
opaque byte-hashed reference, so its native float/time/text semantics are not
redefined by the envelope. Do not normalize creative strings to obtain equality.

All object references are `{sha256, bytes}` with 64 lowercase ASCII hex digits
and an exact encoded size. Child-node references additionally declare `kind`.
Storage locations are resolved separately; drive paths are not identities.

The reference model uses other synthetic domain strings, not this proposed
wire prefix, but now stores its domain frame and verifies the ordinary CAS hash.
It is not a canonical JSON validator or an interoperable envelope implementation.

## 2. Node payloads and semantic identity

| Kind | Required meaning |
| --- | --- |
| Artifact root | Logical artifact/version IDs, native authoring descriptor, binding-index descriptor, exact dependency version locks, declared retained outputs and historical references |
| Key branch | Partition version and sorted unique stable-key child descriptors; optional bounded range/count summaries explicitly marked derived |
| Binding leaf | Sorted stable use-site IDs with exact primary asset descriptors and native authoring anchors; no compiled output admitted as primary |
| Evidence index | Admitted source descriptor identity, adapter/config identities, shard partition and bounded shard descriptors |
| Evidence leaf | Original record locators/digests and typed claims, each bound to source, subject and scope |

A use-site ID distinguishes two uses of the same cel/cue. It is not its ordinal
or the payload hash. Reordering, a cue offset or a caption change updates native
authoring identity even if the set of physical inputs stays identical.
Native authoring remains editable using existing contracts; these nodes provide
versioned storage and indexing, not a competing scene language.

Output variants have stable IDs and typed roles. Selection/draft transactions
state which variants are retained; adapters do not infer audience acceptance.
Container-byte and normalized decoded-audio identities remain separate records.
Only byte-identical encoded objects share a physical CAS object automatically.

Branch splitting uses stable IDs, with a versioned partition algorithm. Do not
commit a partition/fanout algorithm until measured against realistic workloads.
Changed topology must use a bounded traversal or return an explicit limit/gap;
the current changed-page model deliberately rejects topology mismatch.

## 3. Source catalog and adapter interface

An admitted source catalog entry contains:

```text
source_id: stable ID within project + authority namespace
encoded: {sha256, bytes}
encoding: identity | gzip
decoded: {sha256, bytes} when transformed/compressed
source_schema: explicit supported schema
adapter: {id, version, implementation_sha256, config_sha256}
scope: {project_id, authority_namespace, subject/location coverage}
capture: {started_at, completed_at, clock/provenance status}
verification: declared evidence method, never an inferred approval
```

The trusted caller supplies the expected catalog revision and entry. A record
cannot admit itself by asserting a project, digest or verification level. Unknown
adapter/schema/config versions fail explicitly. Adapter selection is from an
installed allowlist; evidence never downloads or executes a parser/plugin.

Read-only interface, independent of local paths:

```text
describe(admitted_source) -> supported schema/scope/capture/verification
lookup(admitted_source, exact_subject, query_scope, budgets) -> query result
enumerate(admitted_source, query_scope, cursor, budgets) -> bounded query page
```

The resolver receives an exact descriptor, not permission to crawl a directory.
Offline source absence returns `SourceUnavailable`; named recovery is a separate
explicit caller operation. No automatic indexing, hydration, cache maintenance,
network access, rendering or movement is part of lookup.

Bind cursors to source descriptor, adapter/config, query scope and last stable
key. Reject reuse with another source/interpretation. A refreshed inventory is
a new source version; do not mutate an index under an old descriptor.

## 4. Typed claims and provenance

Each claim contains the exact subject descriptor, predicate, applicable location,
capture interval/method and witness reference. A witness names source descriptor,
index/shard descriptor, original record locator/digest and adapter interpretation.
Locators can be native record IDs, JSON pointers or ordinals in a frozen source;
an ordinal is not itself an inclusion proof.

| Predicate | Establishes at capture | Does not establish |
| --- | --- | --- |
| Registry membership | Exact subject authorized by the specified registry snapshot | physical presence or current selection |
| Observed copy metadata | Named location/size/physical identity fields actually observed | payload SHA verification or present-day custody |
| Verified copy at capture | Exact verification performed, target and method with its witness | unchanged custody today or independence by path alone |
| Completed operation | Named completed action and per-object outcome | permission to repeat or delete another location |

Current placement intent is admitted separately as a root transaction. A project
adapter can propose that transaction from recorded delivery/draft authority, but
cannot silently turn an evidence claim into a selected root.

Retain conflicting claims with provenance; don't overwrite the earlier claim
because a later inventory is newer. A validated receipt can be authentic evidence
of a past operation while insufficient to prove today's target copy unchanged.
Required present-state checks and permitted prior-proof reuse remain explicit
custody policy. Hash integrity is never authenticity or removal permission.

Query result coverage distinguishes `present`, `absent-from-captured-scope`,
`unknown` and `partial`. Even complete captured membership absence is not proof
of current physical absence or zero live users. Coverage is scoped, not a global
boolean blessing every project/location. Missing or wrong-size/corrupt evidence
must not return a successful empty result. Diagnostics can expose partial claims
privately, but incomplete results cannot support a removal decision.

Private claims and witnesses are not a shareable report format. Construct public
technical summaries from allowlisted fields; do not serialize and then redact
the graph. No private paths, titles, hashes or transitive witness links leak into
the public summary. Actual export/egress enforcement remains an adoption gate.

## 5. Legacy source preparation and bounded access

First adapters target existing `reel.asset-cache-report.v1`, compact registry
checkpoints and exact existing copy/demotion receipts. Establish a fixture for
each supported producer/schema variant; a generic "receipt" label is not enough.
In particular, size-only inventory flags must not become per-copy verification.

Explicit preparation reads and hashes a bounded original source, validates its
schema/scope, and produces immutable sharded indexes with a transformation
receipt binding input, adapter/config and output index descriptors. Preserve
the original source and any unknown fields there. Reject duplicate subject keys
unless that source schema explicitly represents distinct observation records.

For gzip, verify encoded and decoded identities, consume all members/trailer,
and reject excess decoded bytes or malformed/trailing input. A source that
exceeds budgets fails without admitting a partial index. Partial build nodes can
remain protected staging objects until explicitly retired. Publish the new index
only after validation; a crash leaves the previous catalog entry usable.

An ordinary monolithic gzip source requires a bounded initial pass. Later queries
authenticate the index and necessary shards. Indexed extraction is a trusted,
versioned transformation of the source, not magic random access into gzip or
independent cryptographic proof that a locator described its source correctly.
Retain transformation evidence and parity tests against the original reader.

## 6. Limits and result semantics

All operation budgets are explicit and charged before results are emitted.
Byte/node budgets apply before node decode. Record budgets count every decoded
record examined, not just matching/returned records. Streaming production parsers
must bound allocation and reject excess records while decoding. The current
model's record limit is post-decode within an already byte-bounded node.

Provisional settings to benchmark, not installed policy:

| Operation | Suggested initial bounds |
| --- | --- |
| Indexed lookup | 256 KiB/node, 8 MiB total encoded node bytes, 64 node reads, 4,096 examined records, explicit deadline |
| Explicit source preparation | 256 MiB encoded, 512 MiB decoded, 1,000,000 records, explicit deadline |
| Node publication | bounded keys/edges, bounded identifier bytes, max depth; exact thresholds await benchmarks |

Limits include index headers, witnesses and duplicate lookup work, not only the
matching leaf. Counter overflow fails. A budget error never becomes "not found".
Cancellation/time/decompression budgets need production fixtures; counting two
reads in the model does not prove either latency or an upper bound on all memory.

## 7. State transitions and remaining implementation gates

Admit a root only after its immutable nodes and required protection exist.
Transactions bind expected per-key revisions and advance non-reusable revisions;
tombstones/incarnations survive removal/recreation and checkpoint compaction.
Independent producers do not need a shared lifetime lock. Release predecessor
protection only after successor admission. New node/claim formats cannot make
an existing dry-run plan executable without a fresh admission/revalidation.

Reapply the three infrastructure roles plus Rights and Provenance before freezing
these contracts. Required gates remain: complete canonical codec; real bounded
source adapters/sidecars with reader parity; transactional crash/replay and
multi-process tests; scope/privacy/egress checks; physical custody verification;
benchmarks; shadow accounting. No production removal authority is added here.
