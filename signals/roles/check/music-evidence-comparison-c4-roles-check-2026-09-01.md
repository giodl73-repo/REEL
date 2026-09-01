---
skill: roles-check
topic: music-evidence-comparison-c4
date: 2026-09-01
roles_used: 4
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL role review: music evidence comparison C4

## Artifact identification

- Type: Rust contract validator, CLI review queue, sanitized fixtures, tests,
  and documentation.
- Domain: competing decomposition/transcription evidence, correction intake,
  candidate selection, private lineage, and review governance.

## Role selection

- Music Reconstruction Engineer: candidate comparability and measurement scope.
- Sound Designer: listening evidence, stems, bleed, and mixture-quality limits.
- Editor: disagreement visibility, correction ordering, and explicit selection.
- Rights and Provenance Steward: upstream identity, decision evidence, and
  private-report boundaries.

## Music Reconstruction Engineer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Exact intake byte and canonical hashes prevent comparisons from surviving changed upstream evidence. | P3 | intake binding | Preserve both identities in later model-promotion contracts. |
| 2 | Same-purpose admission prevents unlike artifact classes from masquerading as alternatives. | P3 | comparison set | Add narrower semantic profiles only when real sanitized operator outputs justify them. |
| 3 | Measurements are externally supplied and bounded but are not recomputed. | P3 | candidate assessment | Bind future measurement receipts through `evidence_sha256`; do not treat declarations as calculations. |

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Bleed and mixture consistency remain optional because they do not apply to every artifact purpose. | P3 | typed metrics | Require them in a later stem-specific comparison profile. |
| 2 | No numeric aggregation claims one stem or sonification sounds better. | P3 | selection boundary | Retain human audition findings as separate evidence. |
| 3 | Selection is rejected when the selected artifact has an open correction. | P3 | correction gate | Later candidate audition packets should preserve this gate. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Unresolved candidate selection appears as an explicit queue item instead of manifest-order preference. | P3 | queue | Keep queue order deterministic and semantically neutral. |
| 2 | Findings preserve disagreement and inconclusive states rather than averaging them away. | P3 | findings | Promote only reviewed facts into a corrected model. |
| 3 | Corrections identify an artifact and target, allowing competing evidence to remain immutable. | P3 | corrections | Later correction derivatives should cite both request and resolution decisions. |

## Rights and Provenance Steward

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Full upstream validation makes stale or substituted intake evidence fail. | P3 | intake binding | Retain producer/model/license provenance from the intake rather than duplicating it. |
| 2 | Selection and correction closure require immutable decision references. | P3 | decisions | Human decision artifacts must remain separately stored and version-bound. |
| 3 | The report retains private hashes and reasons and is correctly non-shareable. | P3 | report | Add a redacted projection only when a concrete approved sharing workflow exists. |

## Synthesis

Roles reviewed: 4
P1 blockers: 0 | P2 issues: 0 | P3 notes: 12

Verdict: APPROVED-WITH-CONDITIONS

Top finding: C4 correctly governs comparison and selection without pretending
to measure or understand each tool's semantic output.

Cross-role consensus: this is the right precondition for real operator use;
semantic adapters, acoustic calculations, listening judgments, corrections,
and approval remain separate downstream evidence.

## Amend

1. Require purpose-specific measurements only when a real sanitized workflow
   establishes which measures are reproducible and useful.
2. Preserve separately authored human findings and exact decision artifacts in
   the consuming project; simulated role review is not approval.
3. Add corrected-model promotion only after typed timebase-aware semantic
   adapters exist for the actual formats operators provide.

These are simulated role findings, not actual human opinions or approvals.
