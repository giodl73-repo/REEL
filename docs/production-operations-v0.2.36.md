# Production operations contracts (v0.2.36)

REEL v0.2.36 adds provider-neutral production-operations contracts around the
existing validated production manifest. These commands plan, verify, classify,
and queue work. They do not generate pictures, synthesize voices, compose
music, publish assets, grant rights, or approve creative work.

All new input contracts use `serde(deny_unknown_fields)`. Persisted outputs use
atomic no-clobber writes. Portable reports contain hashes and stable
identifiers, not prompt text, credentials, provider secrets, private reasons,
local cache roots, or local paths.

## Generation and materialization evidence

`generation-plan` consumes `reel.generation-plan-input.v0.1`. It requires the
exact production-manifest SHA-256, tool version, unit and shot IDs, prompt hash,
named input hashes, PNG media type, and expected dimensions.

```powershell
cargo run -- generation-plan manifest.yaml generation-input.json `
  --output-path generation-plan.json --output json
```

The path-free `reel.generation-plan.v0.1` output sets
`provider_execution_requested` to `false`. It never contains prompt text.

`materialization-result` consumes a strict local result index. For every unit it
hashes the output file, checks its byte count, reads its image dimensions, and
matches those dimensions to the generation plan.

```powershell
cargo run -- materialization-result generation-plan.json result-input.json `
  --output-path materialization-result.json --output json
```

The portable result contains only unit/shot IDs, hashes, bytes, dimensions, and
media type. `provider_executed_by_reel` remains `false`.

## Append-only asset promotion

`asset-promote` writes one immutable `reel.asset-promotion-record.v0.1` file per
transition:

```text
candidate -> selected -> approved
```

Each input binds the local asset to its SHA-256 and cites one or more review
evidence hashes. `selected` requires the exact candidate record. `approved` requires the exact
selected record plus the candidate record in `prior_chain`, allowing REEL to
verify the complete candidate-to-selected-to-approved chain. Skips, reversals,
stale record hashes, malformed predecessor records, and changed asset bytes
fail.

```powershell
cargo run -- asset-promote selected-input.json `
  --output-path selected-record.json --output json
```

`selected` and `approved` are asset-ledger states only. Every record explicitly
sets `publication_approved` and `rights_approved` to `false`.

## Incremental picture planning

`picture-plan` consumes `reel.picture-plan-input.v0.1`. It validates the
production manifest and derives a deterministic recipe key for each still shot
from:

- the exact production-manifest hash;
- shot ID;
- prompt hash and named input hashes;
- recipe hash;
- output profile and purpose;
- tool version;
- review profile and disclosure.

An optional `reel.picture-cache-index.v0.1` can include machine-local paths for
verification. Those paths never appear in the report.

```powershell
cargo run -- picture-plan manifest.yaml picture-input.json `
  --prior-index prior-cache.json --output-path picture-plan.json --output json
```

The report distinguishes:

- `exact-byte-reuse`: recipe matches and the indexed local bytes, hash, and
  dimensions verify;
- `recipe-equivalent-regeneration`: recipe matches but exact local bytes are
  unavailable;
- `render`: no prior result exists;
- `stale`: manifest, recipe, dimensions, or indexed bytes no longer match;
- `missing`: the manifest shot has no exact picture-input contract.

The command never renders. `review_profile` and `disclosure` are mandatory.
`review-proxy` and `delivery` are distinct output purposes. A review proxy can
never report `delivery_ready`, even when every exact byte is reusable.
`delivery_ready` is technical cache readiness only and never implies creative,
rights, principal, publication, or release approval.

## Timecoded review and repair queue

`review-repair-queue` consumes `reel.timecoded-review-findings.v0.1`. Each
finding has a stable ID, shot ID, absolute millisecond range, severity, owner,
status, and evidence hashes. Ranges must be positive and remain inside the
exact timed shot. Unknown shots fail.

```powershell
cargo run -- review-repair-queue manifest.yaml findings.json `
  --output-path repair-queue.json --output json
```

Only `open` and `in-progress` findings enter the deterministic queue. Sorting is
severity, manifest shot order, start time, then finding ID. Resolved and waived
findings remain human records but are not repair work. The queue sets
`human_decision_required` and never infers approval.

## Portfolio and series state audit

`production-state-audit` consumes an explicit
`reel.production-state-index.v0.1` list of local manifest paths and expected
hashes. It hashes each manifest, validates every current entry with the existing
production validator, and aggregates:

- valid manifests;
- timing, generation, asset, preview, and delivery readiness counts;
- semantic blocker counts;
- stale or unreadable manifest hashes.

```powershell
cargo run -- production-state-audit production-index.json `
  --output-path production-state-audit.json --output json
```

The output uses index IDs and hashes only. It does not serialize manifest
paths. Readiness retains the existing technical meaning from
`ProductionValidationReport`.

## Voice takes and surgical retakes

`voice-take-ledger` consumes `reel.voice-take-ledger-input.v0.1`. It binds:

- the exact production-manifest hash;
- the exact voice-plan hash;
- cue and take IDs;
- exact rendered-audio hashes and byte counts;
- exact cue start/end milliseconds;
- evidence hashes and an `available` or `rejected` disposition;
- a separate explicit take-selection list.

```powershell
cargo run -- voice-take-ledger manifest.yaml voice-takes.json `
  --output-path voice-take-ledger.json --output json
```

A rejected take cannot be selected, and each cue can have at most one explicit
selection. The retake queue contains only cues with no take (`missing`) or only
rejected takes (`rejected`). Available but unselected takes are listed under
`awaiting_selection`, not silently queued or approved. REEL performs no
synthesis and sets `voice_approval_inferred` to `false`.

## Music provenance and no-score comparison

`music-provenance` consumes `reel.music-provenance-input.v0.1`. Scored variants
bind an exact score-plan hash and audio hash plus explicit source, license,
provenance, human-review status, and evidence hashes. An explicit no-score
variant must use:

- `kind: no-score`;
- `source: no-score`;
- `license: not-applicable`;
- `provenance: no-score`.

```powershell
cargo run -- music-provenance manifest.yaml music-provenance.json `
  --output-path music-provenance-report.json --output json
```

A comparison claim is accepted only when its scored and no-score hashes both
match exact declared variants. Source or license classification is evidence,
not a rights decision. The report never infers creative or rights approval.

## Sprite selector and pose coverage

`sprite-coverage` extends the layered-sprite reporting surface without changing
selector ownership. It can summarize an existing cache plan:

```powershell
cargo run -- sprite-coverage --cache-plan sprite-cache-plan.json --output json
```

or evaluate a library/profile/cast resolution:

```powershell
cargo run -- sprite-coverage --library library.yaml --profile profile.yaml `
  --cast cast.yaml --output-path selector-coverage.json --output json
```

The path-free matrix reports every character and request as `exact`,
`declared-fallback`, or `unresolved`, with binding and pose IDs only when a
resolution exists. Coverage never guesses a nearby pose. Domain vocabulary and
creative selector semantics remain owned by consuming repositories.

## Human authority boundary

These contracts provide technical evidence:

- hashes match;
- bytes and dimensions match;
- state transitions are ordered;
- timed ranges are valid;
- cache work is classified;
- review and retake queues are deterministic;
- provenance declarations are structurally complete.

They do not decide whether a picture, performance, edit, score, or release is
good; whether a person or principal approves it; whether rights are sufficient;
or whether an asset may be published or released. Those decisions remain
explicit human authority outside REEL's technical readiness reports.
