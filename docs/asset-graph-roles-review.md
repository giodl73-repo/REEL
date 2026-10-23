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

## Revisioned index catalog and collector projection increment

Review base: `639bbcdc5ed4ecaf5c93e525a856dfb31bca51f3`, plus new
`sidecar/file_store/catalog.rs`, `catalog/{protection,tests}.rs`, retained-reader
integration, `tests/inventory_file_store.rs` and `inventory-evidence-catalog-v1.md`.
Producer-applied review uses the three infrastructure checklists (version 1) and
Rights and Provenance. Owner: this implementation lane. No independent review,
human selection authority or live adoption claimed. Previous hashes remain
the identities of earlier reviews.

| Role | Severity | Finding/correction and actual evidence |
| --- | --- | --- |
| Registry Concurrency and Evidence Integrity | Major corrected | A replaceable head alone can reuse revisions after interruption, deletion or rollback. Immutable predecessor-bound fences, decisions and states precede atomic head replacement. Tombstones retain slots/history; equal-value updates also advance revisions. Missing/newer/pending controls fail ordinary reads. Tests reject stale writers and reset attempts, replay valid commit decisions, and consume an aborted successor without changing its predecessor root. Whole-lineage destruction/rollback still requires an external anchor; no tamper-proof guarantee. |
| Registry Concurrency and Evidence Integrity | Major corrected | Complete pending records and truncated ones need different recovery semantics. Explicit recovery validates complete pending fences/decisions; absent decisions abort and consume a revision, while complete valid commit intent replays. Partial controls remain errors untouched, not silently repaired or downgraded to abort. Recovery rechecks observed decision identity after acquiring the key; a race requires retry. One successor is processed per call, not a hidden history replay. |
| Storage Custody and Recovery | Major corrected | Catalog admission must not outlive its actual retained proof. Candidates verify whole retained indexes/source scope/receipts before admission and refresh proof on committed recovery. A real Windows sharing failure blocks head replacement after the durable decision; the writer is killed and recovery preserves its root. Missing retained evidence blocks admission/recovery. This is process/error recovery, not power-loss or independent source-custody proof. |
| Asset Graph and Version Integrity | Major corrected | An inventory's listed media cannot become live media roots merely because its index is selected. Collector facts protect current INDEX-node closures only; the observed media fixture returns OutsideCatalogScope. Encoded original evidence is separate required provenance without verified-local-custody flags. Outside scope is never zero references/removal permission. Jobs, staging, historical retained anchors and media/draft roots still require their own coverage authorities. |
| Registry Concurrency and Evidence Integrity | Major corrected | A one-pass root list can miss concurrent key changes or lost membership. Bounded membership/slot cross-check and double collection bracket full retained verification. Projection errors on missing/corrupt/pending evidence, never partial success. Explicit revalidation detects new keys, tombstones and changed revision stamps even when values repeat. This is not an execution lease against a writer after return. |
| Registry Concurrency and Evidence Integrity | Major corrected | Per-file/node bounds do not bound a large catalog projection. Charge aggregate catalog metadata before reads across both passes and separate aggregate index bytes/nodes/objects across roots. Tests exhaust key, metadata, index-byte, node and object budgets. Current reads touch bounded current/predecessor controls without replaying all historical revisions. Historical compaction is not implemented. |
| Storage Custody and Recovery | Advisory addressed | Exclusive retained ownership for read verification needlessly conflicts with independent keys using one index. Shared retained readers now coexist; only handoff publication needs exclusive root ownership. Actual child processes race one key (one committed outcome) and update independent keys (both succeed). Shared capacity locks remain short and per-key ownership never blocks another key's I/O. |
| Rights and Provenance | Advisory addressed | Catalogs initialize only in an explicitly chosen existing private cache directory with caller-admitted project/authority/instance scope and shared-ledger capacity allowance. Source bodies and large descriptor sets stay in metadata assets, not Git. Private snapshots/candidates/projections have no automatic serialization/export. Fixtures use synthetic temporary data only; no daemon, private media, live cache or external service is accessed. |

Validation: all 395 tests pass (212 unit, five prior integration, 100 model,
23 source adapter, 25 sidecar, 30 backend), plus strict all-target Clippy,
formatting, ROLES structure and diff checks. The new 17 catalog unit cases and
three process cases distinguish protocol fixtures from actual OS concurrency
and interrupted head replacement. Only Windows filesystem branches were run.

Disposition: optional inventory-index catalog and collector-facing positive read
interface implemented. Existing production collectors remain unchanged. Managed
producer/config wiring, combined staging/retained/job/media scope coverage and
shadow planner parity remain before removal integration. Platform/device
durability, external whole-lineage anchors, history compaction, exact allocation,
deadlines/cancellation and receipt/registry-format adapters remain separate work.

## Managed producer and executable shadow increment

Review base: `0b0fa18f7611960a03c7451d7ff515d8489d0618`, plus
`sidecar/file_store/managed.rs`, `managed/command.rs`, the bounded catalog
preflight, `cli.rs`, `tests/inventory_managed_cli.rs`, and
`inventory-evidence-managed-v1.md`. Producer-applied review uses the three
infrastructure checklists (version 1) and Rights and Provenance. Owner: this
implementation lane. No independent review, human approval or installed-runtime
adoption is claimed. Historical target hashes above remain unchanged.

| Role | Severity | Finding/correction and actual evidence |
| --- | --- | --- |
| Registry Concurrency and Evidence Integrity | Major corrected | Library components needed an explicit producer transaction boundary. Managed build now checks catalog scope/revision before source I/O, verifies the named source, reserves shared capacity, builds/seals/retains the index and performs expected-revision admission. Concurrent executable producers never force overwrite; stale revision failure precedes source access/session creation. Failed partial publication retains bounded staging and does not select or retire it. |
| Asset Graph and Version Integrity | Major corrected | Internally valid hashes/source binding do not establish observation parity with original evidence. The new read-only shadow compares requested rows, copies, ordinals and capture/scope/tier/profile/generation metadata against the verified original source, then checks unchanged catalog revision/root. A deliberately wrong but internally valid/admitted transform is rejected. This is requested adapter-observation parity, not all-row/unqueried integrity, actual media custody or planner-decision parity. |
| Storage Custody and Recovery | Major corrected | Profile path normalization must not silently lose a tier-specific free-space floor or probe unrelated archives. Only the named output root is normalized in the local config copy, with its floor preserved. Source locations must be configured named roots. Tests keep unrelated archives unavailable and prove init/recovery do not open an offline source; build fails without creating a session. Metadata directories are existing, disjoint same-filesystem children of the chosen cache. |
| Storage Custody and Recovery | Advisory addressed | Operator recovery must not imply render/job replay or automatic cleanup. Separate init/build/shadow/recover commands run in the existing binary, with no default profile, census refresh, fallback recovery or retirement. Success retains staging and reports zero media copies/no removal authority. Source bytes remain unchanged, including gzip; original source ingestion/registration/custody remain the established asset workflow. |
| Registry Concurrency and Evidence Integrity | Major corrected | Per-lookup limits do not bound a batch. Shadow enforces subject, aggregate node and aggregate byte limits, rejects duplicate/invalid/empty subjects and emits no success projection on mismatch/budget failure. Filesystem snapshots prove read-only shadow failures and current-revision recovery do not change fixture contents. Complete source preparation remains memory resident and explicitly bounded. |
| Rights and Provenance | Advisory addressed | Private profiles contain authority/key/path/hash data and must not become scorecards. New stdout schemas expose fixed schema labels and numeric/boolean fields only; tests exclude identity sentinels and non-count values. Private profiles/catalog/witnesses and stderr diagnostics remain private. Unknown profile fields and unsupported source schemas/encodings reject rather than installing a parser or executing instructions. |
| Storage Custody and Recovery | Advisory addressed | Legacy environment overrides must not redirect the new explicit producer into another cache/tool flow. An executable fixture succeeds with an empty PATH and a conflicting CAMINOS_TRANSFER_CACHE, without creating that destination. New command paths use typed config and filesystem APIs, never legacy fetch, rendering or daemon startup. |

Validation: all 409 tests pass (212 unit, five prior integration, 100 model,
23 source adapter, 25 sidecar, 30 backend, 14 managed CLI). Actual existing Rust
census/named producers feed JSON/gzip source assets through the executable.
Strict all-target Clippy, formatting, ROLES structure and diff checks pass.
Only isolated synthetic temporary data was accessed; installed aliases/bindings
and live caches were unchanged.

Disposition: optional managed producer and operator commands wired and tested;
requested lookup/observation shadow parity established. Project opt-in config,
combined staging/retained/job/media collector coverage and planner-decision
shadow comparison remain before removal adoption. Streaming source preparation,
deadline/cancellation, exact allocation, device durability and control/history
compaction remain separate work. No existing collector receives deletion authority.

## Combined collector protection and restrictive planner shadow increment

Review base: `8292cdbdac1da8a20bf4e6a67d34362a1c7181b9`, plus new
`sidecar/file_store/collector.rs`, `collector/{command,jobs,tests}.rs`,
`tests/collector_shadow_cli.rs`, native adapters in `jobs.rs`/`tiering.rs`,
catalog source/work accessors, CLI wiring and `inventory-evidence-collector-v1.md`.
Producer-applied review uses all three infrastructure checklists (version 1)
and Rights and Provenance. Owner: this implementation lane. This is not an
independent review, human approval or live adoption. Earlier review hashes and
test counts describe their original increments.

| Role | Severity | Finding/correction and actual evidence |
| --- | --- | --- |
| Asset Graph and Version Integrity | Major corrected | Catalog-only protection omitted staging, retained history and other worktrees. Explicit combined scope now unions these with native selected/current/job-lease/control/tier-pin demand. A second consumer's input or lease holds the first consumer's historical demotion. Inventory-observed media remains outside scope; missing/incomplete declared coverage errors rather than returning zero demand. Explicit enrollment still cannot prove every consumer was declared. |
| Asset Graph and Version Integrity | Major corrected | Version leases alone omit direct production-job inputs. A read-only adapter reuses native request validation, includes declared inputs/CAS tools, and separately protects manifest-bound published outputs. Fixtures prove absent payload/tool paths are not opened, output roles stay separate, malformed or mismatched controls reject, and result removal invalidates a snapshot. Scratch/unpublished outputs are not inferred. |
| Storage Custody and Recovery | Major corrected | Partial staged data is demand, not a valid retained copy. Live stages receive holds without writer ownership; retained metadata requires exact full closure verification. Required original evidence remains demand without custody claims. Missing/corrupt retention rejects. Historical retained metadata remains protected after staging retirement/catalog tombstone; this does not keep all historical media hot. |
| Registry Concurrency and Evidence Integrity | Major corrected | Single-pass membership is stale under concurrent sessions. Double capture compares controls, directory membership and native fingerprints, with catalog revision revalidation; later revalidation rejects changed demand. A writer can continue while protection is captured. No global lifetime lock is added. These observations are not an execution lease and cannot fence changes after return. |
| Storage Custody and Recovery | Major corrected | Comparing a fresh independent batch after added holds could silently select extra removals. The unchanged native planner runs once; the overlay only changes protected demotion/archive proposals to holds and recomputes estimates. Fixtures preserve unrelated rows and prove no batch refill. A distinct shadow schema is rejected by existing executable-plan checks; no apply path is added. This is reference-plus-restriction, not independent planner parity. |
| Registry Concurrency and Evidence Integrity | Advisory addressed | Combined providers need aggregate budgets. File/entry/metadata/node/data/hold/catalog costs are bounded across the declared providers; fixtures exhaust aggregate budgets. Directory enumeration is bounded separately from files read. Native consumer resolution still uses its existing internal limits; not all its reads are charged to this reader. Deadline/cancellation and exact memory/physical-I/O accounting remain open. |
| Rights and Provenance | Advisory addressed | Private graph/scopes cannot appear in shareable scorecards. Snapshots have no automatic serialization; CLI stdout exposes schema/counts/booleans only. Executable fixtures exclude private sentinels and reject execution-like fields, unknown coverage and apply arguments without writes. Private diagnostics remain private. Only synthetic temporary caches were accessed; no network, live census, render, cleanup or installed binding change occurred. |

Validation: all 426 tests pass (226 unit, five prior integration, 100 model,
23 source adapter, 25 sidecar, 30 backend, 14 managed CLI and three collector CLI).
The increment adds 14 collector unit cases and three executable cases. Strict
all-target Clippy, formatting, ROLES structure and diff checks pass. Validation
ran on Windows; no device/power-loss or other-platform guarantee is claimed.

Disposition: optional positive protection union and read-only restrictive planner
comparison implemented. Before removal adoption: explicit project/all-consumer
enrollment, complete provider coverage, representative private-source performance
measurement, execution fencing and independently verified retained custody.
Snapshots never make unknown scope removable. No main merge, live reclamation,
runtime adoption or media/index payload deletion is part of this increment.

## Named custody verification preflight increment

Review base: `11d7773`, plus `collector/custody.rs`, its CLI entry and private
command-path helper, `demotion::verify_pair`, collector fixtures, README and
collector documentation. Producer-applied review uses the three infrastructure
checklists (version 1) and Rights and Provenance. Owner: this implementation
lane. No independent review, human approval, installed-runtime adoption or live
cache access is claimed.

| Role | Severity | Finding/correction and actual evidence |
| --- | --- | --- |
| Storage Custody and Recovery | Major corrected | A demand-only shadow does not verify retained bytes. The new explicit preflight hashes exact named hot/retained CAS pairs through the executor's existing streaming/handle verifier. Candidates, sizes and targets derive from native proposals; the complete named batch and two-copy logical byte budget validate before payload I/O. Missing/corrupt copies fail without copy, repair or fallback search. A cold-candidate fixture verifies the named cold root without registering it. |
| Storage Custody and Recovery | Major corrected | Two paths cannot establish two copies. Physical-identity equality always fails, including hard links across cache paths. Same-volume files require the native policy's explicit exception and are counted separately. Different volumes are not labeled independent failure domains. Real Windows temporary-file fixtures cover alias rejection and same-volume policy; no independent-device validation is claimed. |
| Asset Graph and Version Integrity | Major corrected | Successful hashes cannot override live demand or become a permanent deletion permit. Held candidates fail before payload access, source cache enrollment is mandatory, and final protection/plan/policy/request/storage checks reject changes. A private deterministic test seam introduces new staging demand or changes controls after hashing; every case rejects success and leaves the hot file. The summary explicitly denies reusable custody proof/removal authority. Complete consumer enrollment and execution fencing remain separate gates. |
| Registry Concurrency and Evidence Integrity | Advisory addressed | Verification must not serialize unrelated producers or grow a new evidence registry. The command opens only named pairs, requests no DELETE access, creates no journal/receipt, and releases pair handles before final revalidation. No global cache or render lifetime lock is added. Windows handles deny payload mutation during each pair check; earlier pairs are not leased through return. Metadata work retains existing bounds; hard cancellation during metadata traversal/blocking I/O remains open. |
| Rights and Provenance | Advisory addressed | Verification can touch private media only when explicitly invoked with named candidates. Successful stdout remains fixed-schema/counts/booleans, with zero copies/removals and no per-asset hashes, paths or identities. Executable tests compare filesystem contents before/after success and failure, use empty PATH and reject execution-like fields. This lane used synthetic temporary fixtures only; no live media, archive crawl, daemon, external service or installed binding change occurred. |

Validation: 435 tests pass (227 unit, five prior integration, 100 model,
23 source adapter, 25 sidecar, 30 backend, 14 managed CLI and 11 collector CLI).
This increment adds eight executable cases and one unit boundary case. Strict
all-target Clippy, formatting, ROLES structure and diff checks pass. Windows
filesystem branches only; no cross-platform/device durability claim.

Disposition: named demand-and-custody preflight implemented. It creates no
removal authorization or persistent custody asset. Exact unlink integration
still needs complete enrolled demand, compatible publication/removal fencing,
and fresh retained-copy guards at the physical operation. No live reclamation,
main merge or runtime replacement is part of this increment.

## Per-asset publication fence primitive increment

Review base: `f85dff0`, plus `collector/fence.rs`, `FileStore::publish_fenced`,
the physical-identity comparison helper, Windows IO feature, process fixtures
and `asset-publication-fence-v1.md`. Producer-applied review uses all three
infrastructure checklists (version 1) and Rights and Provenance. Owner: this
implementation lane. No independent review, production adoption or complete
removal integration is claimed.

| Role | Severity | Finding/correction and actual evidence |
| --- | --- | --- |
| Registry Concurrency and Evidence Integrity | Major corrected at primitive boundary | Repeated snapshots cannot exclude a publisher between the last check and unlink. The opt-in domain provides shared publisher/exclusive remover byte ranges per hash prefix, with conservative collisions. Same-asset exclusion and independent-asset progress pass across separate handles and real child processes. This establishes OS coordination only; it does not establish enrollment or make existing removals participate. |
| Registry Concurrency and Evidence Integrity | Major corrected | A file per hash would create unbounded metadata churn. One fixed 32-byte control holds out-of-file byte ranges without file growth. Hash-prefix collisions only serialize extra subjects. Sorted bounded batches reject invalid/duplicate subjects, release partial acquisition on failure, and bound retry waits to two seconds. There is no forced guard expiry or lock stealing; caller lifetime remains explicit. |
| Storage Custody and Recovery | Major corrected | Stale-owner file deletion can reset another owner's protection. Windows pins the control identity/ancestors against replacement, checks header/identity on acquisition, and rejects missing/corrupt controls without repair. Explicit reinitialization preserves existing locks. Actual killed publisher/remover processes release only their ranges; surviving guards remain effective. No persisted ownership record needs cleanup. Whole-lineage tampering and device durability remain outside this guarantee. |
| Asset Graph and Version Integrity | Major corrected at optional writer boundary | Publishing a node must establish staged protection before CAS exposure inside the same fenced section. `publish_fenced` delegates to the existing verified path after acquiring its shared node fence. Busy/foreign/oversized refusal happens before publication and leaves the writer retryable. Fixtures prove no staged/CAS mutation on contention and unrelated-node progress. Ordinary publication is still compatible and unfenced; per-call opt-in is not session-wide enrollment. |
| Storage Custody and Recovery | Blocking adoption gate retained | A fence alone cannot establish zero demand, retained custody, or an atomic removal transaction. Native jobs, retained source-evidence demand, catalogs, ordinary producers and the executor are not switched. Before adoption, persist one authoritative domain mapping, reject unfenced paths, establish overlapping lifecycle demand and define lock order with native worktree controls. Keep media verification/graph scans outside the short commit section. The current increment grants no new unlink path. |
| Rights and Provenance | Advisory addressed | The control header carries no subject hash, identity, title or media data. Guards/domains have no automatic serialization, and no new CLI/scorecard/export is added. Fixtures use synthetic temporary directories and hidden test child processes; no live metadata directory, media, cache, daemon or installed binding is touched. Non-Windows platforms fail explicitly rather than silently using a global lock. |

Validation: all 445 tests pass (227 unit, five prior integration, 100 model,
23 source adapter, 25 sidecar, 30 backend, 14 managed CLI, 11 collector CLI and
10 publication-fence test entries). The latter includes nine substantive cases
and one subprocess harness, with two actual killed processes. Strict all-target
Clippy, formatting, ROLES structure and diff checks pass. Windows only; no Unix,
independent-device or power-loss claims.

Disposition: optional OS fence primitive and bounded index-node writer API
implemented. Removal integration remains gated on complete producer enrollment,
overlapping demand and a bounded final check/unlink protocol. Existing collector,
preflight, executor, runtime pins and live caches remain unchanged.

## Persisted node-writer enrollment increment

Review base: `f891195`, plus versioned session controls/recovery in
`file_store.rs`, admitted construction, private fence bindings, collector
binding validation, `managed::build_fenced`, the optional managed profile field
and their fixtures/docs. Producer-applied review uses the three infrastructure
checklists (version 1) and Rights and Provenance. Owner: this implementation
lane. No independent review, installed adoption or complete producer enrollment
is claimed; earlier review counts/hashes remain historical.

| Role | Severity | Finding/correction and actual evidence |
| --- | --- | --- |
| Registry Concurrency and Evidence Integrity | Major corrected for enrolled sessions | Per-call fencing can disappear when ordinary publication or recovery is used. New version 2 session intent requires a persisted canonical domain/schema/physical-identity binding. Ordinary publication automatically takes its shared node fence; named recovery reopens that exact control. Fixtures show ordinary and resumed calls refuse an exclusively fenced subject, unrelated nodes proceed, and retry succeeds after release without poisoning. |
| Registry Concurrency and Evidence Integrity | Major corrected | Equal header bytes do not identify the old coordination domain. Recovery rejects missing controls and replacement physical files with identical schema bytes. Version 2 without its binding and version 1 with a binding reject; per-call domain substitution also rejects. Bound metadata is checked inside the same cache filesystem and outside staging. Old readers reject the new protocol instead of resuming it unfenced. Arbitrary rewriting/whole-lineage rollback still requires external authority. |
| Storage Custody and Recovery | Major corrected | Persisted fencing must not bypass shared capacity admission. The admitted constructor validates domain scope before reservation, then uses the existing reservation/session contract; ordinary named recovery still requires admitted reacquisition before mutation. The new managed build path uses that admitted constructor and preserves sealed/retained/catalog sequencing. No reservation becomes a lifetime subject/global lock. |
| Asset Graph and Version Integrity | Major corrected | A collector cannot treat an unsupported or missing enrolled control as no demand. It parses both supported session protocols, meters/stamps fence control bytes and verifies physical identity. A staged node remains held with a valid binding; missing/recreated controls make capture and revalidation fail. No partial successful scope or zero-reference result is emitted. |
| Registry Concurrency and Evidence Integrity | Major corrected at managed producer boundary | Optional profile enrollment must affect actual node publication, not just receipts. Executable managed builds create enrolled sessions; ordinary Store publication fences every generated node. An exclusive root-node fence causes build failure before seal/admission, leaves its prior catalog revision intact and requires a new explicit attempt after release. Missing, foreign and overlapping profile domains fail without filesystem writes. |
| Storage Custody and Recovery | Blocking adoption gate retained | This is session enrollment, not authoritative cache-wide enrollment. A fresh profile can omit the optional domain and create a legacy session. Native jobs, original-source read demand, retained source receipts/catalog lifecycles and final unlink are not enrolled by this change. Removal remains gated on complete producer policy, overlapping demand and consistent short commit lock order. No new unlink API or reusable custody permit is added. |
| Rights and Provenance | Advisory addressed | The physical identity/path binding stays inside private compact session controls and has no shareable projection. Managed success stays counts/booleans; executable privacy and read-only parity fixtures pass. Profiles only open existing domains and never initialize/repair them. Only synthetic temporary fixtures were used; no media, live cache, installed runtime or external service was accessed. |

Validation: all 453 tests pass (228 unit, five prior integration, 100 model,
23 source adapter, 25 sidecar, 30 backend, 17 managed CLI, 11 collector CLI and
14 fence test entries, including the existing subprocess harness). Added four
session/fence cases, one collector case and three managed executable cases.
Strict all-target Clippy, formatting, ROLES structure and diff checks pass.
Windows only; no device durability, Unix or hostile whole-lineage guarantee.

Disposition: enrolled index-node sessions now enforce their persisted domain
through ordinary publication, admitted construction and named recovery; managed
profiles opt in explicitly. Full producer enrollment and removal integration
remain open. Live caches, main and installed runtime bindings are unchanged.

## Source-demand publication and retained overlap increment

Review base: `2fa584e`, plus version 3 session intent/construction/recovery,
admitted source construction, managed build ordering, collector source holds,
handoff/retirement identity checks, fixtures and updated contracts. Review is
producer-applied using the three infrastructure checklists (version 1) and
Rights and Provenance. Owner: this implementation lane. No independent review,
live adoption or full removal protocol is claimed.

| Role | Severity | Finding/correction and actual evidence |
| --- | --- | --- |
| Registry Concurrency and Evidence Integrity | Major corrected for enrolled builds | A source can acquire demand only after it has already been read. Enrolled managed builds now reserve capacity and publish version 3 source intent before opening original evidence. The source hash's shared fence covers only compact session publication, then releases. A fenced-source refusal precedes session/source access; a missing source leaves durable demand and no automatic retry. Defaults remain compatible. |
| Storage Custody and Recovery | Major corrected | Early source intent must not silently lose its admission contract after interruption. Admitted version 3 intent persists a required-capacity marker; missing capacity controls make recovery/collection fail. A fixture removes the capacity record after a failed source read and proves ordinary recovery cannot reinterpret it as an unadmitted writer. Low-level source constructors still require external volume admission. No process-kill-at-capacity-publication guarantee is claimed. |
| Asset Graph and Version Integrity | Major corrected | Handoff could otherwise substitute source evidence or drop demand between stage/retained lifecycles. The exact canonical source descriptor persists through named recovery and must match handoff and retirement planning. A real native-report fixture retains RequiredSource before any node, after verified handoff, and after full staging retirement; observed media remains OutsideDeclaredScope. Session demand is deliberately not released or expired. |
| Registry Concurrency and Evidence Integrity | Major corrected | Dropping a source range lock must not drop its published demand. A real child is terminated after source-intent publication; control bytes survive unchanged, ownership is released and explicit recovery can continue. Missing source demand in version 3, source-bearing old protocol combinations and malformed descriptors reject rather than producing an empty scope. Existing publisher/remover process fixtures remain passing. |
| Storage Custody and Recovery | Blocking adoption gate retained | The source intent/domain is in the output cache and can refer to evidence in another configured cache. This does not enroll all consumers or source-cache writers and is not physical custody/tier affinity. Cache-wide policy, cross-domain reconciliation/order, native jobs, explicit demand release and final bounded check/unlink remain required. Existing removal does not automatically honor this intent. No new media/CAS unlink or reusable custody permit is added. |
| Rights and Provenance | Advisory addressed | Only the existing bounded canonical source descriptor enters compact private session controls. No inventory body, observed-media list or new public identity-bearing receipt is written. Successful CLI summaries remain count/boolean-only. Source demand is provenance/intent, not selected media or independent custody. Synthetic fixtures only; no live cache, private media, Drive, daemon, network processing or runtime update occurred. |

Validation: all 459 tests pass (229 unit, five prior integration, 100 model,
23 source adapter, 25 sidecar, 30 backend, 19 managed CLI, 11 collector CLI and
17 fence entries including the subprocess harness). This increment adds three
source/fence cases, two managed executable cases and one collector lifecycle
case. Three fence-suite child processes now test killed publishers, removers
and source-intent writers. Strict all-target Clippy, formatting, ROLES structure
and diff checks pass. Windows fixtures only; no power-loss/device or Unix claim.

Disposition: opt-in index builds establish durable original-evidence demand
before preparation, bind exact retained handoff and preserve demand through
failure/recovery/staging retirement. Complete producer enrollment, explicit
demand release and guarded physical removal remain separate work. Main, live
caches and installed runtime bindings are unchanged.

## Retained native-job enrollment increment

Review base: `a9d83a1`, plus `jobs/publication.rs`, runner/discard/intake changes,
collector job-control validation, CLI count projection, native-runner/executable
fixtures and `asset-fenced-jobs-v1.md`. Producer-applied review uses all three
infrastructure checklists (version 1) and Rights and Provenance. Owner: this
implementation lane. No independent review, live adoption or complete job-family
enrollment is claimed. Historical counts/hashes above retain their original scope.

| Role | Severity | Finding/correction and actual evidence |
| --- | --- | --- |
| Registry Concurrency and Evidence Integrity | Major corrected for opt-in retained jobs | Native input bytes were resolved before workspace demand existed. New retained-job enrollment publishes synced manifest-bound controls under shared input/CAS-tool fences before source resolution. Fences release before reads/execution, leaving durable intent. An input-fenced fixture with missing payload fails on contention before creating scratch; missing-input and killed-executable fixtures leave collectible demand with no subject lock remaining. |
| Asset Graph and Version Integrity | Major corrected | New canonical outputs need demand before publication without becoming hidden build inputs. Verified local outputs now acquire shared hashes only to publish compact output intent, then release before CAS verification/copy. Collector reasons separate input from output; output intent remains a hold without a result. Mismatched result roles/hashes/sizes, corrupt controls, wrong manifests and replaced domains reject collection. A fenced-output fixture leaves only local unpublished bytes and requires a fresh explicit job attempt. |
| Storage Custody and Recovery | Major corrected | Enrollment metadata cannot be an unbudgeted staging tail. Enrolled jobs require budget for exact manifest bytes plus three 64 KiB controls, bounded subjects/output roles, and no-overwrite synced controls/results. Fences never span execution or media copy/hashing. Input and output intent are demand only; unchanged native retained-copy verification and explicit registration remain required before discard. |
| Registry Concurrency and Evidence Integrity | Major corrected | Intake parsed a second mutable file read after hashing different bytes. Runner/collector decoding now uses the exact bounded intake snapshot, with BOM support; discard also rechecks snapshot size/hash against the registered manifest. This preserves valid native job behavior while rejecting a changed manifest. Existing native execution/timeout/default retirement tests pass. |
| Storage Custody and Recovery | Major corrected at discard validation boundary | Explicit cleanup must not bypass damaged enrollment. Discard validates the same bound domain and output controls before applying its existing checks. A fixture corrupts the control and observes preserved scratch, explicitly restores the same physical control, then verifies successful discard with zero CAS removals and retained output bytes intact. No new automatic cleanup or canonical unlink is added. |
| Storage Custody and Recovery | Blocking adoption gate retained | Auto-retirement diagnostics/salvage/evidence are not yet enrolled. The new path therefore requires explicit retain_workspace rather than changing the default or pretending full coverage. This is bounded opt-in retention on success/failure and needs explicit verified discard. Complete consumer/cache policy, cross-domain routing/order and final guarded removal remain open; the old runner and installed runtime are not switched. |
| Rights and Provenance | Advisory addressed | New CLI success is a fixed schema and typed numeric/boolean allowlist; private native results, roles, paths, hashes and authority stay in controls/API results/stderr. Bad counter types reject projection. Actual executable tests exclude private sentinels. The killed fixture command also proves descendant termination. Only synthetic temporary Rust tools/assets were used, with no production cache/media, external service, daemon, installed binding or main change. |

Validation: 469 tests pass (236 unit, five prior integration, 100 model,
23 source adapter, 25 sidecar, 30 backend, 19 managed CLI, 11 collector CLI,
17 fence entries and three new fenced-job CLI cases). Seven native-job unit
cases and three executable cases are new. After the final discard refinement,
all 18 native lifecycle tests pass again. Strict all-target Clippy, formatting,
ROLES structure and diff checks pass. Windows only; no Unix, independent-device,
power-loss or hostile-program sandbox claim.

Disposition: opt-in retained native jobs now publish typed input/output demand
around short fences and preserve it through interruption, with validated explicit
discard. Auto-retiring jobs, complete producer policy and final guarded physical
removal remain separate work. Live caches and installed bindings are unchanged.

## Automatic native-job retirement enrollment increment

Review base: `5e8f444`, plus `jobs/{publication,lifecycle}.rs`, runner/discard
integration, collector retirement-intent interpretation, native/executable
fixtures and updated job/fence contracts. Producer-applied review uses all three
infrastructure checklists (version 1) and Rights and Provenance. Owner: this
implementation lane. No independent review, live adoption or canonical-cache
removal integration is claimed; earlier reviews remain historical.

| Role | Severity | Finding/correction and actual evidence |
| --- | --- | --- |
| Registry Concurrency and Evidence Integrity | Major corrected | Retained-only job enrollment left automatic salvage/diagnostics outside the protocol. Automatic jobs now require enrollment version 2, paired with the manifest's retention choice. Bounded outputs/request/evidence descriptors enter one immutable retirement intent under short shared artifact fences before their retirement CAS publication. Hashing/copy/registration stay outside these fences. Default unenrolled execution remains compatible. |
| Asset Graph and Version Integrity | Major corrected | Diagnostic evidence must not become compiled input demand or imply selection. Collector interpretation protects retirement outputs separately from request/evidence controls. It validates job/manifest identity, exact request hash/bytes, declared output roles/bounds and evidence size; malformed controls reject. These are positive demands, never custody or human approval. A pre-unlink fixture verifies both reason classes and refuses corrupted intent. |
| Storage Custody and Recovery | Major corrected | Metadata allowance must cover retirement before cleanup is enabled. Automatic admission now includes bounded request/evidence copies, registration and retirement control. Retained artifacts must be registered and freshly verified before scratch removal or explicit discard. Missing canonical evidence holds cleanup even if intent is valid. Large diagnostic bodies stay compressed CAS assets; no large Git registry is introduced. |
| Storage Custody and Recovery | Major corrected with actual Windows alias fixture | Verification followed by an unguarded scratch unlink can lose its canonical counterpart. Enrolled cleanup/discard now keep canonical file and ancestor handles through unlink. A direct fixture proves an eviction DELETE handle/canonical removal is refused while the owned temporary hard-link directory can be removed. This tested operation needs no weaker sharing fallback. Hash-range fences still do not span media hashing/copying. Unsupported sharing/custody conditions fail. |
| Registry Concurrency and Evidence Integrity | Major corrected at recovery boundary | A failed successful-job retirement must not re-enter the failure path and rewrite prepared records. Enrolled retirement is attempted once, then requires explicit reconciliation. A real writer is killed after registration at the pre-unlink boundary; durable demand remains, recorded admission blocks discard until explicit stale-owner reconciliation, then named verified discard completes without tool replay or CAS deletion. A separate injected fault/corrupt-control/missing-custody fixture proves the same hold behavior. |
| Storage Custody and Recovery | Blocking adoption gate retained | This closes opt-in job retirement publication, not global producer policy or canonical eviction. Unenrolled clients, cross-domain policy/lock order and the final managed-cache removal transaction remain gates. The process fixture stops before unlink; partial publication or partial filesystem deletion can require manual reconciliation and is not advertised as automatically replayable. No last-copy/cache-tier/M deletion is added. |
| Rights and Provenance | Advisory addressed | Native candidate registration preserves private provenance and grants no creative selection. Count-only success now reports retained versus retired workspace without paths, hashes, roles or evidence bodies; failed jobs retain private diagnostics. Synthetic Windows executable cases prove automatic success/failure salvage and no remaining scratch. No live media/cache, Drive, external service, daemon, installed binding or main change occurred. |

Validation: all 477 tests pass (242 unit, five prior integration, 100 model,
23 source adapter, 25 sidecar, 30 backend, 19 managed CLI, 11 collector CLI,
17 fence entries and five fenced-job executable cases). Five substantive native
cases plus one subprocess harness and two executable cases are added. Strict
all-target Clippy, formatting, ROLES structure and diff checks pass. One existing
process-tree fixture exposed parallel startup timing sensitivity; its local
allowance is now five seconds instead of two, retaining mandatory timeout,
both PID records and descendant-exit assertions. Production deadlines are unchanged.
Windows-only validation; no Unix, device/power-loss or hostile-filesystem claim.

Disposition: opt-in automatic jobs now enroll salvage and diagnostic artifacts,
register/verify custody and retire only owned scratch, with explicit recovery
after a pre-unlink interruption. Complete producer policy and canonical cache
removal remain separate work. Live caches and installed runtimes are unchanged.

## Producer-family cache requirement increment

Review base: `d3056ac`, plus `file_store/publication_policy.rs`, FileStore
creation/recovery/publication, native-job intake, synthetic fixtures and related
contracts. Producer-applied review uses all three infrastructure checklists
(version 1) and Rights and Provenance. Owner: this implementation lane. No
independent approval or production activation is claimed.

| Role | Severity | Finding/correction and actual evidence |
| --- | --- | --- |
| Registry Concurrency and Evidence Integrity | Major corrected within declared producer families | Optional enrollment allowed fresh legacy sessions and jobs to bypass fencing. An immutable, bounded, create-new cache requirement binds the exact domain identity. Updated FileStore begin/resume/publish and native-job intake reject missing enrollment or conflicting domains. Per-call fencing cannot upgrade a legacy session. Fixtures verify refusal before session creation/CAS publication or job admission. |
| Registry Concurrency and Evidence Integrity | Major corrected | Policy replacement could silently change coordination domains. Accepted writers pin the requirement file, ancestors and bound domain, without lifetime asset range locks. Actual Windows tests refuse control writes/removal while independent enrolled writers publish and unrelated removal ranges remain available. Malformed, oversized, misplaced, unsupported and recreated-domain controls reject. |
| Asset Graph and Version Integrity | Blocking adoption gate retained | A producer-family requirement does not certify every consumer, route demand between caches or supply source demand to node-only sessions. Jobs check all configured active caches and hold on distinct required domains. No policy field grants coverage, creative selection, semantic input status or removal authority. |
| Storage Custody and Recovery | Blocking activation gate retained | Installation requires quiescent affected producers; it neither detects/drains old processes nor atomically activates policy across caches. No replacement, uninstall, automatic repair/migration, canonical unlink or last-copy operation is added. Old binaries, direct CAS writers and other publisher families remain outside enforcement. Partial controls hold work. |
| Rights and Provenance | Advisory addressed | The requirement is a private compact local control containing paths/identity, with no new shareable receipt or successful stdout. Tests use synthetic temporary caches only; no live cache, Drive, daemon, installed runtime binding or main change occurred. |

Validation: all 481 tests pass (244 unit, five prior integration, 100 model,
23 source adapter, 25 sidecar, 30 backend, 19 managed CLI, 11 collector CLI,
19 fence entries and five fenced-job CLI cases). Two Windows integration cases
and two native-job cases are added. Strict all-target Clippy, formatting, ROLES
structure and diff checks pass. No Unix, device/power-loss, hostile filesystem or
all-consumer coverage guarantee is inferred from these fixtures.

Disposition: updated FileStore/native-job APIs can enforce a declared cache
requirement. Safe activation, other producers, cross-domain demand routing and
the final guarded canonical eviction transaction remain integration gates.
