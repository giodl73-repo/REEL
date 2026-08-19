---
skill: faces-development-loop
topic: reel-provider-economics-v0.2.39
date: 2026-08-19
roles_used: 5
p1_count: 0
verdict: APPROVED
---

# REEL v0.2.39 provider economics roles check

## Frame

**Thesis:** REEL can replace a consumer-maintained provider cost/retry ledger
with portable evidence without becoming a billing system or spending
authority.

**Smallest falsifiable slice:** reconcile quote, reservation, realized charge,
observed latency, and operation counts over one complete provider-attempt chain;
then evaluate one owner-authored policy.

**Deletion target:** BERTICA can retire its parallel S1E01 attempt-cost and
latency worksheet while retaining provider calls, pricing policy, invoices,
payments, creative judgment, rights, publication, and release.

The thesis fails if REEL must ingest raw provider payloads, infer missing actual
charges, convert denominations, choose an output/provider, or treat a budget
finding as permission to spend.

## Audit and analogues

The v0.2.37 attempt receipt already provides immutable attempt identity,
operation kind, complete parent lineage, and canonical lifecycle observations.
The v0.2.36 voice and music ledgers demonstrate that technical evidence and
human selection must remain separate. The v0.2.38 approval contract demonstrates
that authority belongs in an explicit owner-issued artifact rather than an
operational report.

External primary-source comparisons support a deliberately narrower contract:

- Runway prices generations in provider credits and reports realized router
  cost in response metadata, so credits must not be silently converted into
  currency:
  <https://docs.dev.runwayml.com/guides/pricing/>.
- Google Cloud budgets distinguish planned amounts from actual or forecasted
  cost thresholds and describe budgets as alerts/monitoring:
  <https://cloud.google.com/billing/docs/how-to/budgets>.
- AWS Budgets distinguishes actual from forecasted spend, supports multiple
  cost bases, and warns that billing data is delayed:
  <https://docs.aws.amazon.com/cost-management/latest/userguide/budgets-managing-costs.html>.

REEL therefore records observations and owner policy, not a universal price
catalog, invoice, payment state, forecast, or automated billing action.

## Role review

### Animation Director

- **P2:** Cheap or fast output must not become the selected style or candidate.
  Keep economics separate from promotion and approval.
- **P2:** Retry, retake, remix, and extension have different visual intent.
  Count them separately even when their charges share a total.
- **P3:** Queue and execution evidence may help feasibility planning, but absent
  `running` observations must remain absent rather than estimated.

### Editor

- **P2:** Aggregation must validate the complete attempt chain before totaling
  costs; otherwise omitted failed attempts create a false economical history.
- **P2:** The latest, cheapest, or fastest attempt must never supersede an
  editorial alternate.
- **P3:** Inclusive budget boundaries should pass deterministically; only
  observed totals above the owner limit block.

### Platform and Audience

- **P2:** Quote, reservation, realized charge, invoice, and payment are
  independent states. V1 ends at provider-reported realized charge.
- **P2:** Currency and provider credits are not interchangeable. Reject
  denomination mismatch rather than converting.
- **P2:** Missing realized charges make a configured actual-cost check
  indeterminate (`warn`), never successful, unless the known partial total has
  already exceeded the limit and therefore blocks.

### Story Director

- **P2:** Cost evidence needs the attempt and manifest hashes, not prompt prose
  or source-story content.
- **P2:** Technical budget conformance cannot approve a creative result or
  alter story scope.
- **P3:** Provider identifiers may remain normalized and portable, while job
  IDs remain hash-only.

### Sound Designer

- **P2:** The contract must stay media-generic because voice, music, effects,
  and picture providers use different monetary and credit units.
- **P2:** Low charge or short runtime says nothing about performance, timing,
  mix, or listening approval.
- **P3:** Intentional silence and no-score require no provider attempt and
  therefore must not be treated as missing economics evidence.

## Security and simplicity lens

- Strict schemas reject raw provider fields and unknown secret-bearing data.
- Local receipt files are hash-pinned and outputs contain no paths.
- Integer micro-units avoid floating-point accounting drift.
- Reported values require evidence hashes; pending and unavailable values
  cannot carry amounts.
- One report command reuses the existing attempt chain instead of creating a
  provider adapter, price engine, invoice model, or workflow database.
- Reports are no-clobber and always deny provider execution, spending
  authority, creative approval, rights approval, publication, and release.

## Verdict

**APPROVED.** No P1 blockers remain. V1 is bounded to immutable reconciliation
and deterministic owner-policy findings. Invoice/payment truth, price catalogs,
currency conversion, provider execution, automatic provider selection, and
spending authorization remain outside REEL.
