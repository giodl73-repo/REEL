# Independent review-decision records in CLI v0.2.13

`review-record` creates a strict private `reel.review-decision.v0.1` JSON file
from an exact local video, artifact report, animatic receipt, or comparison
receipt plus a strict finding YAML:

```yaml
schema: reel.review-finding.v0.1
record_id: 2026-08-06-reviewer-a
reviewer_key: reviewer-a
target_kind: comparison-receipt
kind: selection
selected_option: B
reason: Private human reasoning retained only in this local record.
timestamp: '2026-08-06T15:00:00Z'
scope: S1E01
authority: advisory
cites: []
claims:
  authenticated: false
  signed: false
  consent: false
  approval: false
```

```powershell
reel review-record review.comparison.receipt.json finding-a.yaml `
  --output reviews/finding-a.json --format json
```

The target hash is computed rather than accepted on trust. Output records bind
the source-finding hash, exact target hash/kind, reviewer key, selection or
objection, private reason, timestamp, scope, advisory/final authority, and cited
record hashes. Output publication refuses any existing path. A reviewer name
never claims authentication, signature, consent, or approval; all four claims
must be explicitly false.

A resolution uses `kind: resolution`, `authority: final`, a selected option,
and at least two cited record paths. REEL verifies those immutable hashes,
target/scope equality, distinct reviewer keys, non-resolution record types, and
an actual disagreement (different selections or an objection). Original
findings remain untouched.

Series integration uses a separate mutable index, not a rewrite of the series
or decision records:

```yaml
schema: reel.review-index.v0.1
series_sha256: <exact-series-manifest-sha256>
episodes:
  - episode_id: S1E01
    target_sha256: <exact-reviewed-target-sha256>
    required_reviewers: [reviewer-a, reviewer-b]
    records:
      - { path: reviews/finding-a.json, sha256: <record-a-sha256> }
      - { path: reviews/finding-b.json, sha256: <record-b-sha256> }
```

```powershell
reel series-review-queue series.yaml --decision-index review-index.yaml `
  --output json
```

The queue reports only episode status (`missing`, `agreement`, `disagreement`,
or `resolved`), missing reviewer keys, record counts, explicit resolutions, and
decision release gates. It never copies decision reasons or selected options.
An explicit resolution clears only the decision gate; existing series release
readiness and other blockers remain independent. Any shareable decision summary
would require a separate intentional derivative; v0.2.13 emits none.

Neither `reel.manifest.v0.2` nor `reel.series.v0.1` changes.
