# Provider economics evidence (v0.2.39)

REEL v0.2.39 reconciles sanitized cost and latency evidence supplied by an
owner-controlled adapter. REEL does not call a provider, choose a provider,
maintain a price catalog, reserve funds, authorize spending, reconcile an
invoice, or establish payment truth.

## Report contract

`provider-economics-report` consumes
`reel.provider-economics-input.v0.1` and writes a no-clobber, path-free
`reel.provider-economics-report.v0.1`:

```powershell
cargo run -- provider-economics-report economics-input.json `
  --output-path economics-report.json --output json
```

The strict input contains:

- one report, intent, and production-manifest identity;
- a complete sequence-one provider-attempt chain, with every local receipt
  pinned by SHA-256;
- quote, reservation, and realized-charge evidence for every attempt;
- an optional capture observation for a completed captured artifact;
- one owner-authored budget policy.

Every reported amount uses an integer `amount_micros`, equal to one millionth
of its declared denomination. A denomination is either a three-letter uppercase
currency code or a bounded provider-credit code. Every reported value requires
an evidence SHA-256. `pending` and `unavailable` values contain no amount and
are never replaced with a quote, reservation, catalog estimate, or zero.

All reported amounts must use the policy denomination. This prevents currency
conversion, provider-credit conversion, and mixed-unit totals from being
invented inside REEL.

## Independent dimensions

The report preserves three cost axes:

- `quote`: an estimate observed before or around submission;
- `reservation`: an amount reserved or bounded by the owner/provider system;
- `realized_charge`: a provider-reported charge after execution.

It does not model invoiced or paid amounts. A quote is not a reservation, a
reservation is not a realized charge, and none of them grants spending
authority.

REEL derives latency only from canonical provider-attempt observations:

- queue: `submitted -> running`, when `running` was observed;
- execution: `running -> completed|failed`, when both were observed;
- terminal-to-capture: terminal observation to the supplied capture time;
- total observed: submission to capture when capture is present, otherwise
  submission to the latest lifecycle observation.

Missing intermediate observations stay absent. REEL does not divide total time
into estimated queue and execution components.

## Retry and budget evaluation

The complete chain is validated before reconciliation. Initial, retry, retake,
remix, and extension counts remain separate. Their observed costs are totaled,
but one operation kind is never relabeled as another.

The owner policy can independently limit:

- total quote;
- total reservation;
- total realized charge;
- retry count;
- total observed latency.

Each configured axis produces:

- `pass` when complete evidence is at or below its inclusive limit;
- `warn` when required cost evidence is pending or unavailable and the known
  partial total has not exceeded its limit;
- `block` when the known observed total exceeds its limit, even if additional
  values remain pending or unavailable;
- `not-evaluated` when the owner supplied no check for that axis.

The overall result is `block` if any axis blocks, otherwise `warn` if any axis
warns, otherwise `pass`. These are technical policy findings only. The report
always states that REEL did not execute a provider or grant spending authority,
and that human authority remains required.

## Portable-output boundary

Portable output contains hashes, normalized provider identifiers, exact
amounts, lifecycle-derived durations, operation counts, and policy findings.
It contains no credentials, prompts, raw provider payloads, private URLs,
private errors, local paths, invoice claims, payment claims, creative
selection, rights approval, publication approval, or release approval.
