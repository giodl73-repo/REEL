---
name: Asset Graph and Version Integrity
slug: asset-graph-integrity
tier: parliament
applies_to: [architecture, manifests, dependencies, versions, references]
version: 1
---

# Asset Graph and Version Integrity

## Key question

Does every exact version reconstruct its authored meaning, and can we explain
which live roots still require each asset without replaying an accidental history?

## Verify

- Canonical typed nodes bind complete native authoring and recipe state, not
  just payload lists. Ordering, timing, variants and interpretation versions
  affect identity. Exact bytes and decoded/semantic equivalence stay distinct.
- Stable logical IDs survive insertion/reordering. Structural sharing and
  bounded node updates do not introduce a second authoring grammar.
- Primary inputs, retained outputs, provenance and structural links have
  explicit semantics. Compiled outputs cannot become hidden build dependencies.
- Root membership, graph reachability and physical-copy counts remain distinct.
  Local differences are reconciled against the whole root and other consumers.
- Drafts, jobs, selections and pins have exact locks and explainable protection;
  unknown coverage or missing nodes never becomes a zero-reference result.
- Canonicalization, schema changes, corrupt nodes, traversal limits, transitive
  dependencies, shared references and historical reconstruction have fixtures.

## Blocking findings

Ambiguous/floating locks; incomplete snapshot identity; unsafe decrement of
shared references; role laundering; unbounded traversal; or a missing/corrupt
subtree interpreted as empty. Design-model success is not production readiness.

## Tension and review record

Balance natural authoring boundaries against index complexity and measured
write amplification. Report severity, exact target/evidence, responsible owner,
correction and verification. State producer-applied versus independent review.
This checklist grants no selection, creative approval or removal authority.
