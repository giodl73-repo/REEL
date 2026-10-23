# Asset graph architecture: role review

Method: producer-applied checklists, not independent execution or human approval.
Initial target: `2df7f509215f43b9e2876a6a12ece801f16109d6`, architecture document
and `crates/reel-assets/tests/graph_architecture.rs`. All examples are synthetic;
no private inventory, receipt, identity or media is included in this review.
The resulting target hashes and role revisions are recorded below.

## Panel and applicability

Apply all three new infrastructure roles in `.roles/parliament/`, revision 1,
plus Rights and Provenance. Editor, Animation Director, Sound Designer and
Music Reconstruction Engineer apply to preservation of authoring/timing/source
identity. Platform and Audience applies narrowly to export-variant scope.
Story, score/arrangement and lyric adaptation have no substantive creative
change to review; no canon, text, musical, listening or viewing approval claimed.
The craft roles do not substitute for the new infrastructure review.

All corrections below are owned by the graph-engine/design maintainer.
"Corrected" means design/model correction; the proposed engine is not deployed.

## Findings by role

| Role | Severity | Finding and correction | Verification/status |
| --- | --- | --- | --- |
| Asset Graph and Version Integrity | Major | Asset-only identity could miss a timing/authoring edit with unchanged inputs. Bind native authoring/recipe state, timebases, order and output variants. | Corrected in design; `timing_only_authoring_change_creates_a_new_snapshot_without_recopying_inputs` passes. Real native serializer integration remains open. |
| Asset Graph and Version Integrity | Major | Initial fixture helper assigned scene IDs by array position, insufficient to demonstrate insertion stability. Add named-scene snapshot construction. | `inserting_a_named_scene_reuses_all_existing_scene_nodes` passes: one new leaf and root, no replaced existing scene nodes. Production indexing still pending. |
| Registry Concurrency and Evidence Integrity | Major | Caller-supplied next revisions could regress/reuse a revision and permit ABA conflicts. Require advancing revisions and preserve tombstone/incarnation semantics through recreation/compaction. | Reference model rejects reuse; `selection_revisions_cannot_roll_back_and_accept_an_aba_writer` passes. Durable multi-process recovery/recreation tests remain open. |
| Registry Concurrency and Evidence Integrity | Major | A valid source hash alone does not establish project/authority scope. Bind admission to the trusted source catalog. | `valid_hashed_evidence_from_a_foreign_project_is_rejected` passes. Full authority namespace/catalog admission implementation remains open. |
| Storage Custody and Recovery | Major | The model's volume labels could be mistaken for independent failure-domain proof. Use policy-bound physical failure domains; unknown mapping holds removal. | `different_partition_labels_do_not_prove_independent_custody` passes. Real mapping, handle identity, interrupted unlink and custody validation remain adoption gates. |
| Rights and Provenance | Major | Private provenance has no explicit public export boundary. Require private-by-default queries and a separate allowlisted shareable projection; evidence cannot install/execute adapters. | Synthetic `shareable_summary_does_not_serialize_private_claim_graph` passes. Actual adapter/export privacy and egress tests remain open. |
| Music Reconstruction Engineer | Minor | Container-byte identity and decoded-signal equivalence need separate semantics. Require normalization/tool/timebase evidence; no content-address substitution. | Design clarified; no audio processing or neutral-reassembly claim. Real-format audio adapter fixtures are future work. |
| Editor, Animation Director, Sound Designer | Minor | Preserving payload membership alone does not preserve timing, ordering, cue placement or native authoring meaning. | Native-contract boundary clarified and timing-only scenario passes. No creative or renderer-readiness approval inferred. |
| Platform and Audience | Advisory | Captioned/clean/other delivered variants may require separately scoped selection and protection. | Explicit variant identity/selection requirement added. No automatic platform or editorial acceptance. |

## Existing invariants rechecked

- Page-local removal candidates still require whole-root and cross-root checks.
- Drafts need no Golden to receive explicit protection; review due is not deletion.
- Inventory membership, source integrity, current custody and selection stay separate.
- Contradictory observations retain separate provenance; missing/partial evidence
  remains unknown. Legacy gzip requires explicit indexed transformation first.
- Structural sharing and evidence lookup introduce no production cache operation,
  runtime switch, daemon, hydration, upload or lifetime producer lock.

## Validation and disposition

Twenty synthetic architecture scenarios pass. Strict all-target Clippy,
formatting, ROLES structural validation and diff checks pass. The previous base
also passed 195 unit and five other integration tests; no production engine code
changed in this review. The new scenarios test a serial model, not OS durability,
physical deletion, decoded audio identity or compatibility with real receipts.

Disposition: suitable to continue architecture/schema design, with the above
requirements carried forward. Production adoption remains blocked on canonical
format/budgets, real bounded adapters, transactional crash/replay and concurrent
process fixtures, independent physical custody tests, privacy/egress tests and
shadow parity. No storage action or human/creative approval follows from review.

## Result identity

The accompanying commit freezes the role files, review and target together.
SHA-256 of the reviewed corrected targets (file bytes):

- `docs/asset-graph-architecture.md`: `e67521d3f137431ed6afd7813b607fc96e3bafc27c6c93ff9ab7e1376431af86`
- `crates/reel-assets/tests/graph_architecture.rs`: `548e2edc083657e7d1102efbd84b79b9155debbfd6c6f54a8d2c11e42419e189`

Role definitions: Asset Graph and Version Integrity v1; Storage Custody and
Recovery v1; Registry Concurrency and Evidence Integrity v1. Existing craft and
provenance checklists are those carried by the initial target commit above.

## Draft-contract follow-up

The next design increment adds `asset-graph-contracts.md`; the earlier target
hashes above remain the identity of the prior review, not the new file versions.
Method remains producer-applied using the three infrastructure roles and Rights
and Provenance. This is a design/conformance increment, not adapter deployment.

| Role | Severity | Finding/correction and result |
| --- | --- | --- |
| Asset Graph and Version Integrity | Major | Domain-separated model hashes had not stored the domain prefix, making their addresses incompatible with ordinary CAS verification. Store the frame as bytes and hash exactly those bytes. Every model-node address now verifies as ordinary SHA-256; wrong-domain substitution rejects. |
| Registry Concurrency and Evidence Integrity | Major | A node-read count alone does not bound input size. Charge root and leaf bytes before decode, total query bytes, and every decoded row before emitting matching claims. Exact-byte boundary and nonmatching-row budget cases pass. |
| Registry Concurrency and Evidence Integrity | Major | The model accepted short/non-ASCII selectors before slicing a hash prefix. Validate the full lowercase SHA-256 subject first; malformed Unicode, short and uppercase selectors return errors without panics. |
| Storage Custody and Recovery | Advisory | Indexes, witness nodes and staged index-build output have storage/retention cost too. Carry explicit metadata policy, staging protection and no implicit indexing/recovery into the contract. No physical custody behavior is changed or newly proved. |
| Rights and Provenance | Advisory | Contract claims distinguish captured membership, verification at capture, completed operation and admitted current intent. Source-catalog scope and private/public projections remain explicit; a source record cannot authorize itself or install a parser. Real export/egress tests remain open. |

Twenty-five model scenarios pass, as do strict all-target Clippy, formatting,
ROLES structural validation and diff checks. Record limits in the model apply
after decoding an already byte-bounded shard; production streaming allocation,
time/decompression limits, full canonical codec conformance and legacy-reader
parity remain explicitly unimplemented. None is marked passed by this review.

New reviewed target file-byte SHA-256 values:

- `asset-graph-contracts.md`: `bdb55cc368a6d11a5b81f8eb8b17fd7fa02eeef475fc887426369849cd02f232`
- `graph_architecture.rs`: `9f4739378805d61e6e7ce0515d4e25797081bc5f9e1035e14f37f3050a9ccae4`

## Fifty-scenario follow-up

The [scenario lessons](asset-graph-scenario-learnings.md) record the next 25 cases
and their producer-applied review through the same four technical/provenance
checklists. New model corrections hold unknown failure-domain identity and bind
prepared indexes to authority namespace, configuration and explicit schema.
The format-policy contract now distinguishes supported external source formats
from the proposed internal index contract. All 50 model scenarios and the local
Clippy/format/role/diff checks pass; production adoption gates remain open.

Corrected target file-byte SHA-256 values for this increment:

- `asset-graph-contracts.md`: `e6e4eb63e782cf9a6a15dfa06671140897f433dea1ff1b95f8f12fd5f93d64c2`
- `graph_architecture.rs`: `964f46477e60ab6a7716dbb9825c86586dae9a368fee3c8a07def66f867c1da5`

## Hundred-scenario follow-up

Fifty further cases live in `tests/support/graph_scenarios_100.rs`; the scenario
lessons enumerate cases 51-100. Review remains producer-applied using the same
technical/provenance checklists. Graph: fast diff success is not closure integrity
validation; repeated DAG paths still cost traversal. Storage: prior-proof reuse
requires explicit policy and current identity evidence; model assertions are not
real custody checks. Registry: reject corrupt publication winners without
overwrite; validate decoded subjects and shard routing before emitting claims.
Provenance: keep separate location/verification observations and reject malformed
or partial-success records; multiplicity does not create new retention roots.

All 100 model cases, strict all-target Clippy, formatting, ROLES structural and
diff checks pass locally. Serial interleavings/checkpoint rejection do not prove
multi-process durability or crash recovery. No production format or runtime is
adopted by this review, and all previously named production gates remain open.

Corrected target file-byte SHA-256 values for this increment:

- `asset-graph-contracts.md`: `67fa26d63589ffc50ae673a135621dfb5aafb33523c97db5045a07c6939143d5`
- `graph_architecture.rs`: `6db78b3c4a48455c3e55c7e8c1d3aa577d048a25d9e310a9ab78287496fc6486`
- `support/graph_scenarios_100.rs`: `e8e2a8cd1875109a2c9bd634c57d856453ae7a30836e23d853e029079be99cb1`

## First real-format adapter

Target: new opt-in `inventory_evidence` library module, integration tests and
`inventory-evidence-adapter-v1.md`. Method: producer-applied review using the
three infrastructure roles and Rights and Provenance; no independent or human
approval claimed. No private source data, current cache inventory or media was
used. Native producers run only in an isolated synthetic filesystem fixture.

| Role | Finding and result |
| --- | --- |
| Asset Graph and Version Integrity | Captured registration/selection-looking diagnostics must not become current graph intent. Adapter exposes captured membership and source locators only. The producer's dependency `coverage_complete` is separately named and never physical-census completeness. |
| Storage Custody and Recovery | Major corrected during implementation: unregistered objects have no declared-size authority, so native size-match flags can stay true while observed copies differ. Apply descriptor consistency checks only to registered objects; native producer fixture covers both cases. No inventory observation is upgraded to verified custody. |
| Registry Concurrency and Evidence Integrity | Encoded/decoded identities and declared-length-plus-one limits verify complete sources. Seeded record bounds reject excess objects before decoding the next one; cumulative copy bounds stop within one bounded row. Native census/named producer output, copy metadata, tier mapping and original locators have parity fixtures. No persistent publication/concurrency claim. |
| Rights and Provenance | Private source, witness and observation types have no automatic serialization. Only a count-only summary is serializable. Reported paths are data, never opened. No implicit network, adapter installation, command or archive resolution exists; a provided reader remains the caller's explicit responsibility. |

Validation: 195 unit tests, five existing integration tests, 100 model scenarios
and 23 adapter integration tests pass locally. The producer-parity test was
additionally rechecked after strengthening assertions for the unregistered row
and tier/link metadata. Strict all-target Clippy, formatting, ROLES structural
and diff checks pass.

Disposition: read-only library implementation completed; no CLI or existing
planner integration, cache movement, source rewrite or runtime adoption.
Preparation is memory-resident with input/record ceilings, not an exact RSS
bound or constant-memory/persistent shard backend. Deadline/cancellation, catalog
admission, receipt/registry adapters, durable sidecars and shadow-planner parity
remain open. These limits are in the operator-facing adapter documentation.

Reviewed target file-byte SHA-256:

- `src/inventory_evidence.rs`: `524e457b4226bdfddb30bf957c9767030dee48a40f99cb93105ead84448274e6`
- `tests/inventory_evidence.rs`: `79a32b9831260c220b8452ef57612a1bb4678784eaa8de5b9a67c93cc43588d6`
- `inventory-evidence-adapter-v1.md`: `7d0a4c5559eb53f5f392df86bbdbdfdfe3d48d79831b020133e25f3a0d516129`

## Persistent inventory-sidecar increment

Review base: `983df887369e0b256ad265d9d61d591354f9e8f2`, plus
`src/inventory_evidence.rs`, new `src/inventory_evidence/sidecar.rs`,
`tests/inventory_sidecar.rs` and `inventory-evidence-sidecar-v1.md`. Historical
review hashes above remain unchanged. Producer-applied checklists: Asset Graph
and Version Integrity, Storage Custody and Recovery, Registry Concurrency and
Evidence Integrity (all version 1), and Rights and Provenance. No independent
review, human approval or production adoption is claimed. Owner for corrections
and remaining adoption work: this implementation lane.

| Role | Severity | Finding, correction and evidence |
| --- | --- | --- |
| Asset Graph and Version Integrity | Major corrected | Valid byte hashes alone do not establish subject routing or evidence semantics. Queries validate every row in the visited leaf, original ordinals, tier references, registered-size consistency and advancing radix prefixes. Validly hashed malformed rows, wrong-leaf rows and malformed branches reject. Off-path integrity and global cross-leaf counts are explicitly not re-proved by a selective lookup. |
| Registry Concurrency and Evidence Integrity | Major corrected | Binding source hashes is not an inclusion proof or source admission. Queries require exact caller-admitted root and expected source; trusted builder provenance remains required. Encoded/decoded identities, encoding, schema, project, authority and interpretation changes reject. Old/new snapshots remain independently readable; neither source nor catalog is rewritten. |
| Storage Custody and Recovery | Major corrected | Store success can lie or leave partial output. Verify returned address and exact readback; publish children before header. Fixtures reject false identities, corrupted readback and corrupt existing winners; injected publication failures return no root and leave no header. Unrooted staged chunks still require caller-owned protection, capacity accounting and retirement. No cleanup permission follows from failure. |
| Registry Concurrency and Evidence Integrity | Major corrected | Record-only limits allow large nodes; node-only limits allow large traversals. Enforce per-node/total bytes and node counts before open, declared-length-plus-one reads, leaf rows and fixed maximum depth. Build preflights variable-length fields and bounds leaf attempts. Real fixture covers full 64-digit shared prefixes and byte-driven splits. Allocation/RSS and time/cancellation are not claimed bounded by these counters. |
| Rights and Provenance | Advisory addressed | Sidecars contain private source hashes, paths and observations. Wire DTOs are private; public evidence types remain without automatic serialization and no new public report/export is added. Documentation requires private metadata-asset storage, never raw evidence in Git/scorecards. Synthetic temp-directory fixtures introduce no private source/egress dependency. |

Validation: all 345 tests pass (195 unit, five prior integration, 100 model,
23 source-adapter and 22 sidecar). The reopened 1,000-record fixture reads three
chunks and at most 128 rows without original preparation/source bytes. Actual
native census and named reports preserve recorded sizes, tier/copy metadata,
capture/scope distinctions and original record locators through persistence.
Strict all-target Clippy, formatting, ROLES structure and diff checks pass.

Disposition: bounded persistent derived-query library implemented over an
explicit store interface, with a real temporary-filesystem fixture backend.
No global lock, daemon, configured cache path, planner integration, default
backend or runtime change. Backend process races/power-loss durability, catalog
admission, staging lifecycle, deadline/cancellation, receipt/registry adapters,
large real-source benchmarks and shadow planner parity remain adoption gates.
The fixture's no-clobber writes/syncs do not establish production crash durability.

## Existing-cache file backend increment

Review base: `d77a53cce981182ebd2c448676d51f34fecba98f`, plus new
`src/inventory_evidence/sidecar/file_store.rs`, `tests/inventory_file_store.rs`,
`inventory-evidence-file-store-v1.md` and associated module/documentation links.
Producer-applied review uses all three infrastructure checklists (version 1)
and Rights and Provenance; owner is this implementation lane. No independent
review or human approval is implied. Earlier review hashes remain historical.

| Role | Severity | Finding/correction and evidence |
| --- | --- | --- |
| Asset Graph and Version Integrity | Major corrected | Reopening a writer verifies its whole staged set and would defeat bounded ordinary queries. A separate ReadOnlyStore opens exact named CAS chunks without session recovery or ownership. A 1,000-record query during live writer ownership reads three logical chunks; an unrelated broken staging session has no effect. Source/root admission remains external. |
| Storage Custody and Recovery | Major corrected | An interrupted writer can leave published children before header admission. Each complete node has a verified protected staged hard link before its CAS name is published. Actual process termination retains staging; exact named recovery/retry succeeds. Partial staged files and corrupt controls reject without deletion or repair. Missing CAS names fail ordinary reads, even while staged bytes remain intact. Same-volume hard links are explicitly not independent custody. |
| Registry Concurrency and Evidence Integrity | Major corrected | A global lock would block independent producers. Ownership is per explicit staging session only; separate OS processes concurrently publish identical CAS names without overwrite. Same-session takeover rejects while live and succeeds after process termination. Corrupt existing winners are preserved/rejected. This is actual process concurrency and kill testing, not a serial model or power-loss proof. |
| Storage Custody and Recovery | Major remaining adoption gate | Node/count/logical-byte staging bounds are enforced and recovered from create-once private controls. They do not reserve shared-volume capacity or account filesystem overhead across sessions. Shared capacity admission, collector-aware protection, catalog handoff and bounded explicit retirement must be integrated before live use. Drop/age/seal never deletes staging. |
| Registry Concurrency and Evidence Integrity | Advisory addressed | Completion seals are create-once, bounded and idempotent for one root. Sealing blocks new writes but is not catalog admission, closure verification, a CAS revision or release authority. Incomplete seals remain a recovery error. No source/catalog updates or collector adoption are hidden in this backend. |
| Rights and Provenance | Advisory addressed | Absolute paths and session controls remain private local metadata assets. Paths are explicit, traversal/reparse checks reject Windows junctions, and no inventory data supplies storage roots or executes tools. Fixture child processes operate only on synthetic temporary directories; no project cache, private source, network or shareable evidence export is accessed. Trusted ownership is required; TOCTOU-resistant hostile-filesystem sandboxing is not claimed. |

Validation: all 361 suite tests pass, including 16 backend cases. Strict
all-target Clippy, formatting, ROLES structure and diff checks pass. Windows
directory-junction rejection is exercised; Unix symlink and namespace-sync
branches are not exercised by this Windows run. File data is synced, but no
portable Windows directory namespace durability barrier is implemented.

Disposition: optional existing-layout file backend and independent exact reader
implemented and tested. No live cache/runtime/CLI/planner wiring or new cache
root. Production adoption remains blocked on shared capacity admission, trusted
catalog/root revisions, collector-aware staging handoff/retirement, shadow
planner parity and platform/device crash validation. Time/cancellation and
receipt/registry adapters remain separate work.

## Capacity admission and verified staging handoff increment

Review base: `f87e7b42e135e11d9c4e558d4626db113ea047aa`, plus
`sidecar/validation.rs`, `sidecar/file_store/{capacity,retirement}.rs`, their
parent modules, `tests/inventory_{file_store,sidecar}.rs`, and
`inventory-evidence-handoff-v1.md`. Producer-applied review uses all three
infrastructure checklists (version 1) and Rights and Provenance. Owner: this
implementation lane. No independent review, human approval or production
adoption is claimed; earlier target hashes remain historical.

| Role | Severity | Finding/correction and evidence |
| --- | --- | --- |
| Storage Custody and Recovery | Major corrected | Per-session logical limits do not coordinate shared free space. Managed begin reserves payload, declared filesystem/link overhead and bounded controls in the existing shared ledger before creating staging. Shared coordination is explicit; defaults reject. Two worktree roots share the ledger, existing live reservations reduce capacity, and denial leaves no staging. Normal mutation after recovery requires reacquisition in the persisted ledger scope. A 4 KiB/node overhead floor is not measured allocation. |
| Asset Graph and Version Integrity | Major corrected | A short lookup cannot establish a whole index suitable for handoff. Explicit full validation checks every node, total counts and globally unique original ordinals. Cross-leaf duplicate ordinals and missing off-path nodes reject even when a selective query succeeds. Ordinary queries retain bounded paths. This remains derived-index consistency, not source inclusion or current-root authority. |
| Registry Concurrency and Evidence Integrity | Major corrected | Releasing staging on a completion seal alone leaves no verified admitted protection. Handoff now creates append-only exact-root retained links and atomically publishes a compact source/root/closure receipt only after verification. Existing corrupt winners reject without overwrite; interrupted handoff can retry. Ownership is per session or exact retained root; the shared capacity lock is released before node I/O. Retained anchors are not a mutable selection catalog or CAS revision protocol. |
| Storage Custody and Recovery | Major corrected | A retirement dry run must not create controls or treat unknown staged files as unrelated noise. Plan creation enumerates/validates only the exact session against its complete closure and creates nothing. Execution revalidates the plan, records exact intent and verifies retained counterparts before unlinking only staged names. Tests cover missing/corrupt retention, cross-session/stale scope, injected unknown staging and capacity bypass rejection. |
| Registry Concurrency and Evidence Integrity | Major corrected | Process death during unlink needs explicit replay semantics. Persisted retirement intent binds sealed root/session/retained closure before any unlink. A real child is killed after five of 18 removals; named recovery plans and retires exactly the remaining 13, preserves queryability and leaves other sessions usable. Completed replay removes zero more links. Partial/corrupt controls remain explicit errors, not silent repair. |
| Storage Custody and Recovery | Advisory addressed | Hard-link retirement does not free retained payload allocation or establish independent custody. Plans report zero payload reclamation, separate logical bytes from remaining names, and leave CAS/retained aliases and compact controls untouched. Same-filesystem identity is checked before staging/handoff. No recursive deletion, retained-root deletion, media eviction or last-copy operation is added. |
| Rights and Provenance | Advisory addressed | Plans, closure descriptors and source/authority receipts remain private without automatic export. Retained/session controls are compact private cache metadata; large descriptor sets are not added to Git. Tests use synthetic temporary caches only. Source trust, original-source custody and human selection stay external; no daemon, network or private-media processing is triggered. |

Validation: all 375 tests pass (195 unit, five prior integration, 100 model,
23 source adapter, 25 sidecar, 27 backend), plus strict all-target Clippy,
formatting, ROLES structure and diff checks. Four actual child processes exercise
two concurrent publishers, post-publication termination and partial-retirement
termination. Only the Windows filesystem branches are exercised locally.

Disposition: optional shared-capacity writer APIs and verified bounded index
staging retirement implemented. Live adoption still needs explicit managed
producer/collector wiring, source/current-root catalog revisions, shadow planner
parity and platform/device durability validation. Existing collectors do not
automatically honor new anchors. Time/cancellation, exact physical allocation,
receipt/registry adapters and compact-control retention remain separate work.
