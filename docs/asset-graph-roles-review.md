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
