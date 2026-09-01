---
skill: roles-check
topic: music-repair-candidate-c8
date: 2026-09-01
roles_used: 4
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL role review: governed repair candidates C8

## Artifact identification

- Type: Rust governance validator, CLI, generated synthetic fixtures, tests,
  and documentation.
- Domain: exact audio candidates, acoustic evidence, listening, candidate
  selection/rejection, immutable lineage, and private review.

## Role selection

- Music Reconstruction Engineer: exact source/repair/EDL/candidate/evidence
  reconstruction and tamper resistance.
- Sound Designer: seam, duration, tail, listening, and audible-quality limits.
- Editor: motivated candidate comparison and explicit selection/rejection.
- Rights and Provenance Steward: exact authority, private lineage, human gate
  separation, and release boundaries.

## Music Reconstruction Engineer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Recursive validation reaches from candidate bytes back through evidence, EDL, repair intent, model, analysis, and source. | P3 | recursive chain | Preserve full revalidation rather than trusting stored pass flags. |
| 2 | The evidence repair must be byte-, contract-, and identity-equal to the C7 repair. | P3 | repair equality | Retain all three comparisons when repair schemas evolve. |
| 3 | Candidate mutation invalidates both its direct binding and recomputed evidence. | P3 | candidate identity | Keep candidate identity based on decoded PCM for future container formats. |
| 4 | Generated temporary fixtures avoid platform-specific checked-in EDL paths. | P3 | portability | Add a portable EDL projection only as a separately versioned contract. |

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Outside-region, boundary, right-tail, and duration checks are recomputed rather than copied. | P3 | technical evidence | Continue expanding evidence profiles without converting thresholds into taste. |
| 2 | Listening is mandatory for final selection or rejection and cannot be inferred from metrics. | P3 | listening | Bind each real listening record to the exact candidate hash. |
| 3 | Failed listening remains distinct from technical failure. | P3 | state model | Preserve separate reasons in future comparison ledgers. |
| 4 | Passing seams do not claim the repair sounds musically right. | P3 | authority boundary | Keep audible preference with actual reviewers. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | A passing candidate can remain pending instead of being automatically promoted. | P3 | selection | Maintain explicit non-selection as a valid state. |
| 2 | Selection and rejection use separate decisions from the listening decision. | P3 | decision separation | Record exact artifact versions and reasons in the producing project. |
| 3 | A technically failed candidate cannot be labeled selected. | P3 | failed candidates | Preserve rejected candidates and reasons for later learning. |
| 4 | C7 intent remains the explanation for why the candidate exists. | P3 | editorial lineage | Do not move musical rationale into renderer logs. |

## Rights and Provenance Steward

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Candidate, evidence, repair, and intent identities are all immutable and recursively checked. | P3 | lineage | Recheck at every consuming-project handoff. |
| 2 | Technical pass, listening, selection, review, delivery, and release remain separate concepts. | P3 | gate separation | Add delivery and release only through independent contracts. |
| 3 | Reports remain non-shareable and contain no candidate path in their output. | P3 | privacy | Use a separately approved redacted receipt for external exchange. |
| 4 | Validation performs no rendering, network access, upload, or listening. | P3 | execution boundary | Keep external adapters explicit and locally governed. |

## Synthesis

Roles reviewed: 4
P1 blockers: 0 | P2 issues: 0 | P3 notes: 16

Verdict: APPROVED-WITH-CONDITIONS

Top finding: C8 proves the exact candidate and technical evidence before
accepting independent listening and selection records, while preserving failed
candidates as explicit rejections.

Cross-role consensus: technical success, audible judgment, selection, delivery,
and release must remain separate gates bound to the exact candidate version.

## Amend

1. Record actual listening and selection decisions in the consuming project's
   ledger; simulated review cannot supply either decision.
2. Preserve candidate failure reasons and exact hashes in later comparison
   receipts rather than deleting unsuccessful attempts.
3. Add any shareable or delivery receipt as a separately reviewed redacted
   projection, never by relabeling this private report.

These are simulated role findings, not actual human opinions or approvals.
