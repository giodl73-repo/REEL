---
skill: roles-check
topic: music-repair-link-c7
date: 2026-09-01
roles_used: 4
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL role review: model-bound repair intent C7

## Artifact identification

- Type: Rust governance validator, CLI, synthetic fixture, tests, and docs.
- Domain: governed musical intent, sample-exact repairs, candidate evidence,
  human listening and selection, immutable source lineage, and private review.

## Role selection

- Music Reconstruction Engineer: source identity, model evidence, repair
  operation census, locks, envelopes, and candidate evidence.
- Sound Designer: audible seam/tail evidence and the listening boundary.
- Editor: motivated changes, intent completeness, and selection separation.
- Rights and Provenance Steward: authority, immutable decisions, privacy, and
  the distinction among technical pass, selection, and release.

## Music Reconstruction Engineer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Recursive draft and repair validation prevents a stale musical model from authorizing a different source recording. | P3 | exact bindings | Preserve the three-way source manifest, source contract, and decoded-PCM equality check. |
| 2 | Every mutating operation is linked exactly once, while keep and lock remain non-creative boundaries. | P3 | intent census | Retain exact census behavior when additional repair operations become executable. |
| 3 | Model refs express musical purpose but cannot enlarge the sample envelope. | P3 | asymmetric authority | Keep sample ranges authoritative in the repair contract. |

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Boundary continuity and right-tail identity are explicit candidate checks. | P3 | candidate gate | Continue producing measured evidence through the existing EDL pipeline. |
| 2 | Human listening remains mandatory and cannot be satisfied by technical validation. | P3 | listening | Record the exact candidate/version heard. |
| 3 | Exact outside-region identity protects ambience and performance outside the repair. | P3 | acoustic boundary | Do not relax this for generative adapters. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Each change has an objective, rationale, model targets, and immutable decision. | P3 | repair intent | Keep the actual editorial decision outside simulated review. |
| 2 | Complete operation coverage prevents an incidental technical edit from escaping explanation. | P3 | operation census | Maintain one-intent-only linkage for each mutation. |
| 3 | Candidate selection is a distinct gate after listening. | P3 | selection | Preserve rejected candidates and reasons in later selection receipts. |

## Rights and Provenance Steward

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Draft and repair are bound by byte and canonical identities with recursive provenance. | P3 | lineage | Revalidate all bindings in consuming projects. |
| 2 | A technical pass, listening review, and human selection are explicitly separate. | P3 | gate separation | Add delivery and release only as later independent decisions. |
| 3 | Reports remain non-shareable because source and decision identities are retained. | P3 | privacy | Create a separately approved redacted projection if exchange is needed. |

## Synthesis

Roles reviewed: 4
P1 blockers: 0 | P2 issues: 0 | P3 notes: 12

Verdict: APPROVED-WITH-CONDITIONS

Top finding: C7 makes repair intent accountable to a governed musical model
without weakening the sample-exact repair boundary or turning evidence into
creative approval.

Cross-role consensus: technical evidence, listening, candidate selection, and
release must remain separate gates tied to exact artifact versions.

## Amend

1. Record actual repair and candidate-selection decisions in the consuming
   project's decision ledger; this simulated review is not approval.
2. Bind later listening evidence to the exact candidate bytes and intent.
3. Preserve failed/rejected candidates and their reasons when candidate
   generation adapters are added.

These are simulated role findings, not actual human opinions or approvals.
